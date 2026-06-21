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
    extract::{Path as AxumPath, State},
    http::{header, StatusCode},
    response::{
        sse::{Event, Sse},
        IntoResponse,
    },
    routing::get,
    Router,
};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use tower_http::cors::CorsLayer;

#[derive(Parser)]
#[command(name = "l10n4x")]
#[command(about = "l10n4x localization toolkit dev toolchain", version = "0.1.0")]
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
    if !l10n4x_core::integrity::set_signing_key(&signing_seed) {
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

    let public_key = l10n4x_core::integrity::signing_public_key()
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
        &config.output_dir,
        &public_key_hex,
        config.encrypt,
        &config.encrypt_key_env,
    )?;

    println!("Build completed successfully.");
    Ok(())
}

async fn serve_locale_file(
    AxumPath(lang_pak): AxumPath<String>,
    State(state): State<ServerState>,
) -> impl IntoResponse {
    if lang_pak.ends_with(".json") {
        let locale = lang_pak.trim_end_matches(".json");
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

async fn handle_events(
    State(state): State<ServerState>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let rx = state.tx.subscribe();
    let stream = BroadcastStream::new(rx).map(|msg| {
        let data = msg.unwrap_or_else(|_| "change".to_string());
        Ok(Event::default().data(data))
    });
    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default())
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

    let app = Router::new()
        .route("/locales/:lang_pak", get(serve_locale_file))
        .route("/events", get(handle_events))
        .layer(CorsLayer::permissive())
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
                &config.output_dir,
                pubkey_hex,
                config.encrypt,
                &config.encrypt_key_env,
            )?;
        }
    }

    Ok(())
}
