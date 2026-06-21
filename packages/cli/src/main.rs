mod config;
mod generator;
mod targets;

use clap::{Parser, Subcommand};
use config::{
    format_verify_public_key, get_encrypt_key, get_signing_key, load_config,
    parse_verify_public_key, save_config, Config, Target,
};
use generator::generate_bindings;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use axum::{
    body::Body,
    extract::{Path as AxumPath, Query, State},
    http::{header, Request, StatusCode},
    middleware::{self, Next},
    response::{
        sse::{Event, Sse},
        IntoResponse,
    },
    routing::get,
    Router,
};
use subtle::ConstantTimeEq;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use tower_http::cors::CorsLayer;

#[derive(Parser)]
#[command(name = "l10n4x")]
#[command(about = "l10n4x localization toolkit dev toolchain", version = env!("CARGO_PKG_VERSION"))]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Interactive wizard to initialize l10n4x.config.json
    Init,
    /// Build translation packages (.pak) and target bindings
    Build {
        /// Validate and report errors without writing output files
        #[arg(long)]
        dry_run: bool,
    },
    /// Validate translation keys consistency across language files
    Validate,
    /// Start the hot-reload development server and watch locales
    Dev {
        /// Flutter Web proxy mode
        #[arg(long)]
        flutter_web: bool,
        /// Port to run the server on (default: 3456)
        #[arg(long, default_value_t = 3456)]
        port: u16,
    },
    /// Generate bindings for a specific target
    Generate {
        #[arg(long)]
        target: String,
    },
}

#[derive(Clone)]
struct ServerState {
    output_dir: String,
    tx: broadcast::Sender<String>,
}

fn get_flat_keys_for_lang_dir(lang_dir: &Path) -> Result<HashSet<String>, anyhow::Error> {
    let mut merged_keys = HashSet::new();
    let entries = fs::read_dir(lang_dir)?;
    for entry in entries {
        let entry = entry?;
        let file_path = entry.path();
        if file_path.is_file() && file_path.extension().is_some_and(|ext| ext == "json") {
            let file_stem = file_path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let content = fs::read_to_string(&file_path)?;
            let parsed: serde_json::Value = serde_json::from_str(&content)?;
            let mut flat_map = HashMap::new();
            l10n4x_compiler::flatten_value(file_stem.to_string(), &parsed, &mut flat_map);
            for k in flat_map.keys() {
                merged_keys.insert(k.clone());
            }
        }
    }
    Ok(merged_keys)
}

fn validate_keys(source_dir: &str) -> Result<HashSet<String>, anyhow::Error> {
    let path = Path::new(source_dir);
    if !path.is_dir() {
        anyhow::bail!("Source directory '{}' does not exist.", source_dir);
    }
    let entries = fs::read_dir(path)?;
    let mut lang_keys = Vec::new();
    let mut all_keys = HashSet::new();

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let lang = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            let keys = get_flat_keys_for_lang_dir(&path)?;
            for k in &keys {
                all_keys.insert(k.clone());
            }
            lang_keys.push((lang, keys));
        }
    }

    if lang_keys.is_empty() {
        anyhow::bail!(
            "No language subdirectories found in source directory '{}'.",
            source_dir
        );
    }

    let mut has_mismatches = false;
    for (lang, keys) in &lang_keys {
        let missing: Vec<&String> = all_keys.iter().filter(|k| !keys.contains(*k)).collect();
        if !missing.is_empty() {
            has_mismatches = true;
            eprintln!(
                "Error: Language '{}' is missing translation keys: {:?}",
                lang, missing
            );
        }
    }

    if has_mismatches {
        anyhow::bail!("Validation failed: translation keys are inconsistent across languages.");
    }

    println!(
        "Success: Translation keys are consistent across all {} languages.",
        lang_keys.len()
    );
    Ok(all_keys)
}

fn build_project(dry_run: bool) -> Result<(), anyhow::Error> {
    let config = load_config()?;

    // 1. Validate consistency
    let keys = validate_keys(&config.source_dir)?;

    if dry_run {
        println!(
            "Dry-run OK: {} keys validated across source '{}'.",
            keys.len(),
            config.source_dir
        );
        return Ok(());
    }

    let signing_seed = get_signing_key(&config)?;
    if !l10n4x_compiler::signing::set_signing_key(&signing_seed) {
        anyhow::bail!("Failed to configure Ed25519 signing key.");
    }

    if config.encrypt {
        let enc_key = get_encrypt_key(&config)?;
        if !l10n4x_core::encryption::set_decrypt_key(&enc_key) {
            anyhow::bail!("Failed to configure AES decrypt key.");
        }
    }

    l10n4x_compiler::compile_translations(
        Path::new(&config.source_dir),
        Path::new(&config.output_dir),
        config.encrypt,
    )
    .map_err(|e| anyhow::anyhow!("Compilation failed: {}", e))?;

    let public_key = l10n4x_compiler::signing::signing_public_key()
        .map_err(|e| anyhow::anyhow!("Failed to derive public key: {}", e))?;
    let public_key_hex = format_verify_public_key(&public_key);

    let mut config = config;
    config.verify_public_key = Some(public_key_hex.clone());
    save_config(&config)?;

    if !l10n4x_core::integrity::set_verify_key(&public_key) {
        anyhow::bail!("Failed to configure verify key.");
    }

    println!(
        "Compiled signed translation packages (.pak) at '{}'",
        config.output_dir
    );

    generate_bindings(
        &config.targets,
        &keys,
        &config.fallback,
        &config.source_dir,
        &config.output_dir,
        &public_key_hex,
        config.encrypt,
        &config.encrypt_key_env,
    )?;

    println!("Build completed successfully.");
    Ok(())
}

/// Returns `Some(s)` only if `s` is a safe single-component filename with no directory
/// traversal, no path separators, no null bytes, and no absolute path prefix.
fn sanitize_locale_filename(s: &str) -> Option<&str> {
    if s.is_empty()
        || s.contains('/')
        || s.contains('\\')
        || s.contains("..")
        || s.contains('\0')
        || s.starts_with('.')
    {
        return None;
    }
    Some(s)
}

async fn serve_locale_file(
    AxumPath(lang_pak): AxumPath<String>,
    State(state): State<ServerState>,
) -> impl IntoResponse {
    if lang_pak.ends_with(".json") {
        let locale = lang_pak.trim_end_matches(".json");
        if sanitize_locale_filename(&lang_pak).is_none() {
            return (StatusCode::BAD_REQUEST, "Invalid locale filename").into_response();
        }
        let pak_path = Path::new(&state.output_dir).join(format!("{}.pak", locale));
        if !pak_path.exists() {
            return (StatusCode::NOT_FOUND, "Locale JSON not found").into_response();
        }
        match fs::read(&pak_path) {
            Ok(bytes) => match l10n4x_core::pak::decompress_pak(&bytes) {
                Ok(decompressed) => (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "application/json")],
                    decompressed,
                )
                    .into_response(),
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Pak decompression failed: {}", e),
                )
                    .into_response(),
            },
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to read pak file: {}", e),
            )
                .into_response(),
        }
    } else if lang_pak.ends_with(".pak") {
        if sanitize_locale_filename(&lang_pak).is_none() {
            return (StatusCode::BAD_REQUEST, "Invalid locale filename").into_response();
        }
        let pak_path = Path::new(&state.output_dir).join(&lang_pak);
        if !pak_path.exists() {
            return (StatusCode::NOT_FOUND, "Locale PAK not found").into_response();
        }
        match fs::read(&pak_path) {
            Ok(bytes) => (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/octet-stream")],
                bytes,
            )
                .into_response(),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to read pak file: {}", e),
            )
                .into_response(),
        }
    } else {
        (StatusCode::BAD_REQUEST, "Invalid file format requested").into_response()
    }
}

#[derive(serde::Deserialize)]
struct EventsQuery {
    locale: Option<String>,
}

async fn handle_events(
    Query(query): Query<EventsQuery>,
    State(state): State<ServerState>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, axum::Error>>>, StatusCode> {
    if let Some(locale) = &query.locale {
        if locale.contains('\n') || locale.contains('\r') || locale.contains(':') {
            return Err(StatusCode::BAD_REQUEST);
        }
    }
    let rx = state.tx.subscribe();
    let stream = BroadcastStream::new(rx).map(|msg| {
        let data = msg.map_err(axum::Error::new)?;
        if data.contains('\n') || data.contains('\r') {
            return Err(axum::Error::new("Payload contains invalid newlines"));
        }
        Ok(Event::default().data(data))
    });
    Ok(Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default()))
}

fn extract_token(req: &Request<Body>) -> Option<String> {
    if let Some(auth_header) = req.headers().get(axum::http::header::AUTHORIZATION) {
        if let Ok(auth_str) = auth_header.to_str() {
            if let Some(token) = auth_str.strip_prefix("Bearer ") {
                return Some(token.to_string());
            }
        }
    }
    if let Some(query) = req.uri().query() {
        for pair in query.split('&') {
            let mut parts = pair.splitn(2, '=');
            if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
                if k == "token" {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

async fn auth_middleware(req: Request<Body>, next: Next) -> Result<impl IntoResponse, StatusCode> {
    let expected_token = match std::env::var("L10N4X_DEV_TOKEN") {
        Ok(t) if !t.is_empty() => t,
        _ => return Ok(next.run(req).await),
    };

    if let Some(token) = extract_token(&req) {
        if token.len() <= 128 {
            let expected = expected_token.as_bytes();
            let incoming = token.as_bytes();
            let match_choice = if expected.len() == incoming.len() {
                expected.ct_eq(incoming)
            } else {
                expected.ct_eq(expected) & subtle::Choice::from(0)
            };
            if match_choice.unwrap_u8() == 1 {
                return Ok(next.run(req).await);
            }
        }
    }
    Err(StatusCode::UNAUTHORIZED)
}

async fn run_dev_server(port: u16, flutter_web: bool) -> Result<(), anyhow::Error> {
    let config = load_config()?;
    if let Some(hex) = &config.verify_public_key {
        let pk = parse_verify_public_key(hex)?;
        l10n4x_core::integrity::set_verify_key(&pk);
    }
    if config.encrypt {
        if let Ok(enc_key) = get_encrypt_key(&config) {
            l10n4x_core::encryption::set_decrypt_key(&enc_key);
        }
    }

    if let Err(e) = build_project(false) {
        eprintln!("Initial build failed: {}. Dev server will start anyway.", e);
    }

    let (tx, _) = broadcast::channel(16);
    let state = ServerState {
        output_dir: config.output_dir.clone(),
        tx: tx.clone(),
    };

    let source_dir = config.source_dir.clone();
    let watcher_tx = tx.clone();
    tokio::task::spawn_blocking(move || {
        use notify::{RecursiveMode, Watcher};
        let (event_tx, event_rx) = std::sync::mpsc::channel();
        let mut watcher = match notify::recommended_watcher(event_tx) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("Failed to initialize watcher: {}", e);
                return;
            }
        };

        if let Err(e) = watcher.watch(Path::new(&source_dir), RecursiveMode::Recursive) {
            eprintln!("Failed to watch source directory '{}': {}", source_dir, e);
            return;
        }

        println!("Watching for changes in '{}'...", source_dir);

        loop {
            match event_rx.recv() {
                Ok(Ok(event)) => {
                    let has_json_changes = event
                        .paths
                        .iter()
                        .any(|p| p.extension().is_some_and(|ext| ext == "json"));
                    if has_json_changes {
                        // 300ms trailing-edge debounce
                        std::thread::sleep(std::time::Duration::from_millis(300));
                        // Drain subsequent events
                        while let Ok(Ok(_)) = event_rx.try_recv() {}

                        println!("Translation file changed. Rebuilding...");
                        match build_project(false) {
                            Ok(_) => {
                                let lang = event
                                    .paths
                                    .first()
                                    .and_then(|p| p.parent())
                                    .and_then(|p| p.file_name())
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("unknown");
                                if lang.contains('\n') || lang.contains('\r') || lang.contains(':')
                                {
                                    eprintln!(
                                        "Skipping invalid locale code in change payload: {}",
                                        lang
                                    );
                                    continue;
                                }
                                let sse_payload =
                                    format!("{{\"type\": \"change\", \"lang\": \"{}\"}}", lang);
                                let _ = watcher_tx.send(sse_payload);
                            }
                            Err(e) => {
                                eprintln!("Rebuild failed: {}", e);
                            }
                        }
                    }
                }
                Ok(Err(e)) => eprintln!("Watcher error: {}", e),
                Err(_) => break,
            }
        }
    });

    let cors_layer = if let Some(origins) = &config.cors_origins {
        let mut parsed_origins = Vec::new();
        for origin_str in origins {
            let s = origin_str.as_str();
            if (s.starts_with("http://") || s.starts_with("https://"))
                && !s.contains('\n')
                && !s.contains('\r')
            {
                if let Ok(origin_val) = axum::http::HeaderValue::from_str(origin_str) {
                    parsed_origins.push(origin_val);
                } else {
                    anyhow::bail!("Invalid CORS origin configured: {}", origin_str);
                }
            } else {
                anyhow::bail!("Invalid CORS origin configured: {}", origin_str);
            }
        }
        CorsLayer::new()
            .allow_origin(tower_http::cors::AllowOrigin::list(parsed_origins))
            .allow_methods(tower_http::cors::Any)
            .allow_headers(tower_http::cors::Any)
    } else {
        let allowed_origin = tower_http::cors::AllowOrigin::predicate(|origin, _parts| {
            let bytes = origin.as_bytes();
            if bytes == b"null" {
                return false;
            }
            if let Ok(origin_str) = std::str::from_utf8(bytes) {
                if origin_str.starts_with("http://localhost:")
                    || origin_str.starts_with("http://127.0.0.1:")
                    || origin_str == "http://localhost"
                    || origin_str == "http://127.0.0.1"
                {
                    return true;
                }
            }
            false
        });
        CorsLayer::new()
            .allow_origin(allowed_origin)
            .allow_methods(tower_http::cors::Any)
            .allow_headers(tower_http::cors::Any)
    };

    let protected_routes = Router::new()
        .route("/locales/:lang_pak", get(serve_locale_file))
        .route("/events", get(handle_events))
        .layer(middleware::from_fn(auth_middleware));

    let app = Router::new()
        .merge(protected_routes)
        .layer(cors_layer)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    println!("l10n4x dev server running at http://localhost:{}", port);
    if flutter_web {
        println!("Flutter Web proxy mode active.");
    }
    axum::serve(listener, app).await?;
    Ok(())
}

fn init_wizard() -> Result<(), anyhow::Error> {
    println!("Initializing l10n4x configuration...");

    let mut targets = Vec::new();

    if Path::new("go.mod").exists() {
        println!("Detected Go project. Adding 'go' target.");
        targets.push(Target {
            r#type: "go".to_string(),
            out_dir: "./backend/pkg/i18n".to_string(),
            options: serde_json::json!({ "package": "i18n" }),
        });
    }

    if Path::new("package.json").exists() {
        println!("Detected Node/JS project. Adding 'typescript' target.");
        targets.push(Target {
            r#type: "typescript".to_string(),
            out_dir: "./frontend/src/i18n".to_string(),
            options: serde_json::json!({ "flavor": "react", "strictTypes": true }),
        });
    }

    if Path::new("pubspec.yaml").exists() {
        println!("Detected Flutter project. Adding 'flutter' target.");
        targets.push(Target {
            r#type: "flutter".to_string(),
            out_dir: "./mobile/lib/generated".to_string(),
            options: serde_json::json!({
                "package": "app",
                "useFfi": true,
                "strictNullSafety": true,
                "generateHelpers": true
            }),
        });
    }

    if targets.is_empty() {
        println!(
            "No standard project files detected. Adding default 'go' and 'typescript' targets."
        );
        targets.push(Target {
            r#type: "go".to_string(),
            out_dir: "./i18n/go".to_string(),
            options: serde_json::json!({}),
        });
        targets.push(Target {
            r#type: "typescript".to_string(),
            out_dir: "./i18n/ts".to_string(),
            options: serde_json::json!({}),
        });
    }

    let config = Config {
        project: "l10n4x_project".to_string(),
        source_dir: "./locales".to_string(),
        output_dir: "./examples/dist/locales".to_string(),
        fallback: "en".to_string(),
        signing_key_env: "L10N4X_SIGNING_KEY".to_string(),
        verify_public_key: None,
        encrypt: false,
        encrypt_key_env: "L10N4X_ENCRYPT_KEY".to_string(),
        cors_origins: None,
        targets,
    };

    let path = Path::new("l10n4x.config.json");
    if path.exists() {
        anyhow::bail!("l10n4x.config.json already exists in this directory.");
    }

    let content = serde_json::to_string_pretty(&config)?;
    fs::write(path, content)?;
    fs::create_dir_all(&config.source_dir)?;
    fs::create_dir_all(Path::new(&config.source_dir).join("en"))?;
    fs::write(
        Path::new(&config.source_dir).join("en").join("common.json"),
        "{\n  \"welcome\": \"Welcome!\"\n}\n",
    )?;

    println!("Created l10n4x.config.json and initial locales directory successfully!");
    println!("Set L10N4X_SIGNING_KEY to a 32-byte seed, then run `l10n4x build`.");
    println!("Generate a seed: head -c 32 /dev/urandom | base64");
    Ok(())
}

#[cfg(test)]
mod path_safety_tests {
    use super::sanitize_locale_filename;

    #[test]
    fn accepts_simple_locale_pak() {
        assert!(sanitize_locale_filename("en.pak").is_some());
        assert!(sanitize_locale_filename("zh-CN.pak").is_some());
        assert!(sanitize_locale_filename("pt_BR.json").is_some());
    }

    #[test]
    fn rejects_path_traversal() {
        assert!(sanitize_locale_filename("../../etc/passwd").is_none());
        assert!(sanitize_locale_filename("../en.pak").is_none());
        assert!(sanitize_locale_filename("..\\en.pak").is_none());
    }

    #[test]
    fn rejects_absolute_paths() {
        assert!(sanitize_locale_filename("/etc/passwd").is_none());
        assert!(sanitize_locale_filename("\\windows\\system.pak").is_none());
    }

    #[test]
    fn rejects_directory_separators() {
        assert!(sanitize_locale_filename("sub/dir/en.pak").is_none());
        assert!(sanitize_locale_filename("sub\\en.pak").is_none());
    }

    #[test]
    fn rejects_null_bytes() {
        assert!(sanitize_locale_filename("en\0.pak").is_none());
    }
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init => {
            init_wizard()?;
        }
        Commands::Build { dry_run } => {
            build_project(dry_run)?;
        }
        Commands::Validate => {
            let config = load_config()?;
            validate_keys(&config.source_dir)?;
        }
        Commands::Dev { flutter_web, port } => {
            run_dev_server(port, flutter_web).await?;
        }
        Commands::Generate { target } => {
            if !["go", "typescript", "python", "c", "flutter", "dart"].contains(&target.as_str()) {
                anyhow::bail!(
                    "Unsupported target '{}'. Supported targets are: go, typescript, python, c, flutter, dart.",
                    target
                );
            }
            let config = load_config()?;
            let keys = validate_keys(&config.source_dir)?;
            let filtered: Vec<Target> = config
                .targets
                .into_iter()
                .filter(|t| t.r#type == target)
                .collect();
            if filtered.is_empty() {
                anyhow::bail!("No target matching '{}' found in configuration.", target);
            }
            let pubkey_hex = config.verify_public_key.as_deref().ok_or_else(|| {
                anyhow::anyhow!("verifyPublicKey missing — run `l10n4x build` first.")
            })?;
            let pubkey = parse_verify_public_key(pubkey_hex)?;
            l10n4x_core::integrity::set_verify_key(&pubkey);
            generate_bindings(
                &filtered,
                &keys,
                &config.fallback,
                &config.source_dir,
                &config.output_dir,
                pubkey_hex,
                config.encrypt,
                &config.encrypt_key_env,
            )?;
        }
    }

    Ok(())
}
