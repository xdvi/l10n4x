mod config;
mod generator;
mod targets;
mod tms;

use clap::{Parser, Subcommand};
use config::{
    format_verify_public_key, get_encrypt_key, get_signing_key, load_config,
    parse_verify_public_key, save_config, Config, Target,
};
use generator::generate_bindings;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{stdin, stdout, Write};
use std::path::Path;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};
use tower_http::timeout::TimeoutLayer;

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
    Validate {
        /// Print each missing key with its expected source file path
        #[arg(long)]
        report_misses: bool,
    },
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
    /// Verify source code keys match locale files. Exits non-zero on any mismatch (CI gate).
    Check {
        /// Glob patterns for source files (defaults same as extract).
        #[arg(long, value_name = "GLOB")]
        src: Vec<String>,
        /// Output results as JSON to stdout.
        #[arg(long)]
        json: bool,
    },
    /// Generate a pseudolocale for layout and overflow testing.
    Pseudo {
        /// Source locale to transform (defaults to config fallback).
        #[arg(long, value_name = "LOCALE")]
        locale: Option<String>,
        /// Output directory for pseudolocale JSON files (defaults to sourceDir/pseudo).
        #[arg(long, value_name = "DIR")]
        out: Option<String>,
    },
    /// Show translation coverage statistics for all locales.
    Stats {
        /// Output results as JSON.
        #[arg(long)]
        json: bool,
        /// Show the list of missing keys for each locale.
        #[arg(long)]
        verbose: bool,
    },
    /// Scan source files for translation key usages and add missing keys to locale JSON files.
    Extract {
        /// Glob pattern(s) for source files to scan (e.g. "src/**/*.ts").
        /// If omitted, reads `extractPatterns` from l10n4x.config.json.
        #[arg(long, value_name = "GLOB")]
        src: Vec<String>,
        /// Print what would change without writing any files.
        #[arg(long)]
        dry_run: bool,
    },
    /// TMS exchange: export/import locale JSON or push signed paks to a webhook.
    Sync {
        /// Provider: `file`, `webhook`, or `crowdin`.
        #[arg(long, default_value = "file")]
        provider: String,
        /// Direction: `export`, `import`, or `push`.
        #[arg(long)]
        direction: String,
        /// Output directory for export (default: `tms-export` or `tms-crowdin`).
        #[arg(long, value_name = "DIR")]
        out: Option<String>,
        /// Input directory for import.
        #[arg(long, value_name = "DIR")]
        from: Option<String>,
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

fn source_file_for_key(key: &str) -> &str {
    key.split('.').next().unwrap_or("unknown")
}

fn validate_keys(source_dir: &str, report_misses: bool) -> Result<HashSet<String>, anyhow::Error> {
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
            if report_misses {
                eprintln!("Error: Language '{}' is missing keys:", lang);
                for k in &missing {
                    eprintln!(
                        "  '{}' → expected in '{}/{}.json'",
                        k,
                        lang,
                        source_file_for_key(k)
                    );
                }
            } else {
                eprintln!(
                    "Error: Language '{}' is missing translation keys: {:?}",
                    lang, missing
                );
            }
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
    let keys = validate_keys(&config.source_dir, false)?;

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

    let bundle_mode = match config.bundles.mode.as_str() {
        "modular" => l10n4x_compiler::BundleMode::Modular,
        _ => l10n4x_compiler::BundleMode::Monolith,
    };

    let compile_options = {
        #[cfg(feature = "debug-keys")]
        {
            l10n4x_compiler::CompileOptions {
                encrypt: config.encrypt,
                compression_level: config.compression_level,
                bundle_mode,
                preload: config.bundles.preload.clone(),
                embed_debug_keys: config.debug_keys,
            }
        }
        #[cfg(not(feature = "debug-keys"))]
        {
            l10n4x_compiler::CompileOptions {
                encrypt: config.encrypt,
                compression_level: config.compression_level,
                bundle_mode,
                preload: config.bundles.preload.clone(),
            }
        }
    };

    if config.debug_keys {
        #[cfg(not(feature = "debug-keys"))]
        eprintln!(
            "Warning: debugKeys is enabled in config but this CLI build lacks the debug-keys feature."
        );
    }

    l10n4x_compiler::compile_with_options(
        Path::new(&config.source_dir),
        Path::new(&config.output_dir),
        compile_options,
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

    tms::maybe_push_webhook_after_build(&config)?;

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
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    if lang_pak.len() > 512 {
        return (StatusCode::BAD_REQUEST, "Path too long").into_response();
    }
    if lang_pak.ends_with(".json") {
        let locale = lang_pak.trim_end_matches(".json");
        if sanitize_locale_filename(locale).is_none() {
            return (StatusCode::BAD_REQUEST, "Invalid locale filename").into_response();
        }
        let pak_path = Path::new(&state.output_dir).join(format!("{}.pak", locale));
        if !pak_path.exists() {
            return (StatusCode::NOT_FOUND, "Locale JSON not found").into_response();
        }
        match fs::read(&pak_path) {
            Ok(bytes) => match l10n4x_core::pak::decompress_pak(&bytes) {
                Ok(decompressed) => {
                    let etag = fnv1a_hex(&bytes);
                    let etag_header = format!("\"{}\"", etag);
                    let client_etag = headers
                        .get(axum::http::header::IF_NONE_MATCH)
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("");
                    if client_etag == etag_header {
                        return StatusCode::NOT_MODIFIED.into_response();
                    }
                    (
                        StatusCode::OK,
                        [
                            (header::CONTENT_TYPE, "application/json"),
                            (header::ETAG, etag_header.as_str()),
                            (header::CACHE_CONTROL, "no-cache"),
                        ],
                        decompressed,
                    )
                        .into_response()
                }
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
            Ok(bytes) => {
                let etag = fnv1a_hex(&bytes);
                let etag_header = format!("\"{}\"", etag);
                let client_etag = headers
                    .get(axum::http::header::IF_NONE_MATCH)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                if client_etag == etag_header {
                    return StatusCode::NOT_MODIFIED.into_response();
                }
                (
                    StatusCode::OK,
                    [
                        (header::CONTENT_TYPE, "application/octet-stream"),
                        (header::ETAG, etag_header.as_str()),
                        (header::CACHE_CONTROL, "no-cache"),
                    ],
                    bytes,
                )
                    .into_response()
            }
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
        if locale.len() > 128
            || locale.contains('\n')
            || locale.contains('\r')
            || locale.contains(':')
        {
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

struct RateLimiter {
    state: Mutex<HashMap<String, (usize, Instant)>>,
    max_per_second: usize,
}

impl RateLimiter {
    fn new(max_per_second: usize) -> Self {
        Self {
            state: Mutex::new(HashMap::new()),
            max_per_second,
        }
    }

    fn check(&self, ip: &str) -> bool {
        let mut state = self.state.lock().unwrap();
        let now = Instant::now();
        let entry = state.entry(ip.to_string()).or_insert((0, now));
        if now.duration_since(entry.1).as_secs() >= 1 {
            *entry = (1, now);
            true
        } else if entry.0 < self.max_per_second {
            entry.0 += 1;
            true
        } else {
            false
        }
    }
}

static RATE_LIMITER: LazyLock<RateLimiter> = LazyLock::new(|| RateLimiter::new(100));

async fn rate_limit_middleware(
    req: Request<Body>,
    next: Next,
) -> Result<impl IntoResponse, StatusCode> {
    let ip = req
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|s| s.trim().to_string())
        .or_else(|| {
            req.extensions()
                .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
                .map(|ci| ci.0.ip().to_string())
        })
        .unwrap_or_else(|| "unknown".to_string());

    if !RATE_LIMITER.check(&ip) {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }
    Ok(next.run(req).await)
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
        .layer(middleware::from_fn(auth_middleware))
        .layer(middleware::from_fn(rate_limit_middleware));

    let app = Router::new()
        .merge(protected_routes)
        .layer(cors_layer)
        .layer(TimeoutLayer::new(Duration::from_secs(30)))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    let actual_port = listener.local_addr()?.port();
    println!(
        "l10n4x dev server running at http://localhost:{}",
        actual_port
    );
    if flutter_web {
        println!("Flutter Web proxy mode active.");
    }
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;
    Ok(())
}

/// Inserts a dot-separated key path into a nested JSON object.
fn insert_nested_key(obj: &mut serde_json::Value, key_path: &str, value: &str) {
    let parts: Vec<&str> = key_path.splitn(2, '.').collect();
    if let serde_json::Value::Object(map) = obj {
        if parts.len() == 1 {
            map.entry(parts[0])
                .or_insert(serde_json::Value::String(value.to_string()));
        } else {
            let child = map
                .entry(parts[0])
                .or_insert(serde_json::Value::Object(serde_json::Map::new()));
            insert_nested_key(child, parts[1], value);
        }
    }
}

// ── Task 6: Check (CI gate) ────────────────────────────────────────────────

#[derive(Debug)]
struct CheckReport {
    missing_in_locale: Vec<String>,
    unused_in_code: Vec<String>,
}

fn check_report(code_keys: &[String], locale_keys: &[String]) -> CheckReport {
    let code_set: std::collections::HashSet<_> = code_keys.iter().collect();
    let locale_set: std::collections::HashSet<_> = locale_keys.iter().collect();

    let mut missing_in_locale: Vec<String> = code_set
        .difference(&locale_set)
        .map(|s| (*s).clone())
        .collect();
    missing_in_locale.sort();

    let mut unused_in_code: Vec<String> = locale_set
        .difference(&code_set)
        .map(|s| (*s).clone())
        .collect();
    unused_in_code.sort();

    CheckReport {
        missing_in_locale,
        unused_in_code,
    }
}

fn check_command(src_globs: Vec<String>, json_output: bool) -> i32 {
    let config = match load_config() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: {}", e);
            return 1;
        }
    };

    let globs = if src_globs.is_empty() {
        vec![
            "src/**/*.ts".to_string(),
            "src/**/*.tsx".to_string(),
            "src/**/*.js".to_string(),
            "lib/**/*.go".to_string(),
            "lib/**/*.py".to_string(),
        ]
    } else {
        src_globs
    };

    let mut code_keys_set: std::collections::HashSet<String> = std::collections::HashSet::new();
    for pattern in &globs {
        if let Ok(entries) = glob::glob(pattern) {
            for entry in entries.flatten() {
                if let Ok(content) = std::fs::read_to_string(&entry) {
                    for k in extract_keys_from_source(&content) {
                        code_keys_set.insert(k);
                    }
                }
            }
        }
    }
    let mut code_keys: Vec<String> = code_keys_set.into_iter().collect();
    code_keys.sort();

    let ref_locale_path = std::path::Path::new(&config.source_dir).join(&config.fallback);
    let locale_keys: Vec<String> = if ref_locale_path.is_dir() {
        match get_flat_keys_for_lang_dir(&ref_locale_path) {
            Ok(keys) => {
                let mut v: Vec<_> = keys.into_iter().collect();
                v.sort();
                v
            }
            Err(e) => {
                eprintln!("Error reading locale keys: {}", e);
                return 1;
            }
        }
    } else {
        match validate_keys(&config.source_dir, false) {
            Ok(keys) => {
                let mut v: Vec<_> = keys.into_iter().collect();
                v.sort();
                v
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                return 1;
            }
        }
    };

    let report = check_report(&code_keys, &locale_keys);
    let has_issues = !report.missing_in_locale.is_empty() || !report.unused_in_code.is_empty();

    if json_output {
        println!("{{");
        println!("  \"missing\": {:?},", report.missing_in_locale);
        println!("  \"unused\": {:?}", report.unused_in_code);
        println!("}}");
    } else {
        if report.missing_in_locale.is_empty() && report.unused_in_code.is_empty() {
            println!(
                "✓ All {} code keys are present in locale files.",
                code_keys.len()
            );
        } else {
            if !report.missing_in_locale.is_empty() {
                eprintln!(
                    "✗ {} key(s) used in code but missing from locales:",
                    report.missing_in_locale.len()
                );
                for k in &report.missing_in_locale {
                    eprintln!("    - {}", k);
                }
            }
            if !report.unused_in_code.is_empty() {
                println!(
                    "⚠ {} key(s) in locales not used in code:",
                    report.unused_in_code.len()
                );
                for k in &report.unused_in_code {
                    println!("    - {}", k);
                }
            }
        }
    }

    if has_issues {
        1
    } else {
        0
    }
}

// ── Task 7: Pseudo (pseudolocalization) ────────────────────────────────────

fn pseudolocalize_string(s: &str) -> String {
    const SUBSTITUTIONS: &[(char, char)] = &[
        ('a', 'á'),
        ('b', 'ƀ'),
        ('c', 'ć'),
        ('d', 'ď'),
        ('e', 'é'),
        ('f', 'ƒ'),
        ('g', 'ĝ'),
        ('h', 'ĥ'),
        ('i', 'í'),
        ('j', 'ĵ'),
        ('k', 'ķ'),
        ('l', 'ĺ'),
        ('m', 'm'),
        ('n', 'ń'),
        ('o', 'ö'),
        ('p', 'þ'),
        ('q', 'q'),
        ('r', 'ŕ'),
        ('s', 'š'),
        ('t', 'ţ'),
        ('u', 'ü'),
        ('v', 'v'),
        ('w', 'ŵ'),
        ('x', 'x'),
        ('y', 'ŷ'),
        ('z', 'ž'),
        ('A', 'Á'),
        ('B', 'Ɓ'),
        ('C', 'Ć'),
        ('D', 'Ď'),
        ('E', 'É'),
        ('F', 'F'),
        ('G', 'Ĝ'),
        ('H', 'Ĥ'),
        ('I', 'Í'),
        ('J', 'Ĵ'),
        ('K', 'Ķ'),
        ('L', 'Ĺ'),
        ('M', 'M'),
        ('N', 'Ń'),
        ('O', 'Ö'),
        ('P', 'Þ'),
        ('Q', 'Q'),
        ('R', 'Ŕ'),
        ('S', 'Š'),
        ('T', 'Ţ'),
        ('U', 'Ü'),
        ('V', 'V'),
        ('W', 'Ŵ'),
        ('X', 'X'),
        ('Y', 'Ŷ'),
        ('Z', 'Ž'),
    ];

    let mut result = String::with_capacity(s.len() * 2 + 2);
    result.push('[');

    let mut chars = s.chars().peekable();
    let mut visible_chars = 0usize;

    while let Some(c) = chars.next() {
        if c == '{' {
            result.push(c);
            for inner in chars.by_ref() {
                result.push(inner);
                if inner == '}' {
                    break;
                }
            }
        } else {
            let sub = SUBSTITUTIONS
                .iter()
                .find(|(from, _)| *from == c)
                .map(|(_, to)| *to)
                .unwrap_or(c);
            result.push(sub);
            visible_chars += 1;
        }
    }

    let target_extra = (visible_chars * 2) / 5 + 1;
    for _ in 0..target_extra {
        result.push('~');
    }

    result.push(']');
    result
}

fn pseudo_transform_json(value: &serde_json::Value, count: &mut usize) -> serde_json::Value {
    match value {
        serde_json::Value::String(s) => {
            *count += 1;
            serde_json::Value::String(pseudolocalize_string(s))
        }
        serde_json::Value::Object(map) => {
            let new_map: serde_json::Map<String, serde_json::Value> = map
                .iter()
                .map(|(k, v)| (k.clone(), pseudo_transform_json(v, count)))
                .collect();
            serde_json::Value::Object(new_map)
        }
        other => other.clone(),
    }
}

fn pseudo_command(
    locale_opt: Option<String>,
    out_opt: Option<String>,
) -> Result<(), anyhow::Error> {
    let config = load_config()?;
    let source_locale = locale_opt.unwrap_or_else(|| config.fallback.clone());
    let source_path = std::path::Path::new(&config.source_dir).join(&source_locale);

    if !source_path.is_dir() {
        anyhow::bail!(
            "Source locale '{}' not found at '{}'",
            source_locale,
            source_path.display()
        );
    }

    let out_dir_str = out_opt.unwrap_or_else(|| {
        std::path::Path::new(&config.source_dir)
            .join("pseudo")
            .to_string_lossy()
            .into_owned()
    });
    let out_dir = std::path::Path::new(&out_dir_str);
    std::fs::create_dir_all(out_dir)?;

    let mut total_keys = 0usize;

    for file_entry in std::fs::read_dir(&source_path)? {
        let file_entry = file_entry?;
        let fpath = file_entry.path();
        let is_json = matches!(fpath.extension().and_then(|e| e.to_str()), Some("json"));
        if !fpath.is_file() || !is_json {
            continue;
        }

        let content = std::fs::read_to_string(&fpath)?;
        let json: serde_json::Value = serde_json::from_str(&content)?;

        let pseudo_json = pseudo_transform_json(&json, &mut total_keys);
        let file_name = fpath.file_name().unwrap();
        let out_file = out_dir.join(file_name);
        std::fs::write(&out_file, serde_json::to_string_pretty(&pseudo_json)?)?;
        println!("Wrote {}", out_file.display());
    }

    println!("Pseudolocale generated: {} keys transformed.", total_keys);
    Ok(())
}

// ── Task 8: Stats (coverage report) ─────────────────────────────────────────

struct LocaleCoverage {
    locale: String,
    total_keys: usize,
    missing_count: usize,
    extra_count: usize,
    percent: u8,
}

fn compute_coverage(
    locale: &str,
    locale_keys: &std::collections::HashSet<String>,
    reference_keys: &std::collections::HashSet<String>,
) -> LocaleCoverage {
    if reference_keys.is_empty() {
        return LocaleCoverage {
            locale: locale.to_string(),
            total_keys: 0,
            missing_count: 0,
            extra_count: locale_keys.len(),
            percent: 100,
        };
    }
    let missing_count = reference_keys.difference(locale_keys).count();
    let extra_count = locale_keys.difference(reference_keys).count();
    let translated = reference_keys.len() - missing_count;
    let percent = (translated * 100 / reference_keys.len()) as u8;

    LocaleCoverage {
        locale: locale.to_string(),
        total_keys: reference_keys.len(),
        missing_count,
        extra_count,
        percent,
    }
}

fn stats_command(json_output: bool, verbose: bool) -> Result<(), anyhow::Error> {
    let config = load_config()?;
    let src_path = std::path::Path::new(&config.source_dir);

    let ref_path = src_path.join(&config.fallback);
    let ref_keys: std::collections::HashSet<String> = if ref_path.is_dir() {
        get_flat_keys_for_lang_dir(&ref_path)?.into_iter().collect()
    } else {
        validate_keys(&config.source_dir, false)?.into_iter().collect()
    };

    let mut coverages: Vec<LocaleCoverage> = Vec::new();

    for lang_entry in std::fs::read_dir(src_path)? {
        let lang_entry = lang_entry?;
        if !lang_entry.path().is_dir() {
            continue;
        }
        let lang = lang_entry.file_name().to_string_lossy().to_string();
        let locale_keys: std::collections::HashSet<String> =
            get_flat_keys_for_lang_dir(&lang_entry.path())?
                .into_iter()
                .collect();
        coverages.push(compute_coverage(&lang, &locale_keys, &ref_keys));
    }

    coverages.sort_by_key(|c| c.percent);

    if json_output {
        println!("[");
        for (i, cov) in coverages.iter().enumerate() {
            let comma = if i < coverages.len() - 1 { "," } else { "" };
            println!(
                "  {{\"locale\":\"{}\",\"percent\":{},\"total\":{},\"missing\":{},\"extra\":{}}}{}",
                cov.locale, cov.percent, cov.total_keys, cov.missing_count, cov.extra_count, comma
            );
        }
        println!("]");
    } else {
        println!(
            "\n{:<12} {:>8} {:>8} {:>8}  Bar",
            "Locale", "Coverage", "Missing", "Total"
        );
        println!("{}", "─".repeat(60));
        for cov in &coverages {
            let bar_filled = (cov.percent as usize * 20) / 100;
            let bar: String = "█".repeat(bar_filled) + &"░".repeat(20 - bar_filled);
            let status = if cov.percent == 100 {
                "✓"
            } else if cov.percent >= 80 {
                "~"
            } else {
                "✗"
            };
            println!(
                "{:<12} {:>7}%  {:>7}  {:>7}  {} {}",
                cov.locale, cov.percent, cov.missing_count, cov.total_keys, bar, status
            );
            if verbose && cov.missing_count > 0 {
                let locale_path = src_path.join(&cov.locale);
                if let Ok(locale_keys) = get_flat_keys_for_lang_dir(&locale_path) {
                    let locale_set: std::collections::HashSet<_> =
                        locale_keys.into_iter().collect();
                    let mut missing: Vec<_> = ref_keys.difference(&locale_set).collect();
                    missing.sort();
                    for k in missing {
                        println!("    missing: {}", k);
                    }
                }
            }
        }
        let total_locales = coverages.len();
        let avg = coverages
            .iter()
            .map(|c| c.percent as usize)
            .sum::<usize>()
            .checked_div(total_locales)
            .unwrap_or(0);
        println!("{}", "─".repeat(60));
        println!(
            "Average coverage: {}% across {} locale(s)",
            avg, total_locales
        );
    }

    Ok(())
}

// ── Task 11: ETag helper ───────────────────────────────────────────────────

/// Computes FNV-1a 64-bit hash of `data` and returns it as a 16-char lowercase hex string.
fn fnv1a_hex(data: &[u8]) -> String {
    format!("{:016x}", l10n4x_core::binary_format::fnv1a_64(data))
}

// ── Existing extract command ────────────────────────────────────────────────

fn extract_command(src_globs: Vec<String>, dry_run: bool) -> Result<(), anyhow::Error> {
    let config = load_config()?;

    let globs = if src_globs.is_empty() {
        vec![
            "src/**/*.ts".to_string(),
            "src/**/*.tsx".to_string(),
            "src/**/*.js".to_string(),
            "lib/**/*.go".to_string(),
            "lib/**/*.py".to_string(),
        ]
    } else {
        src_globs
    };

    let mut all_keys: std::collections::HashSet<String> = std::collections::HashSet::new();
    for pattern in &globs {
        for entry in glob::glob(pattern)
            .map_err(|e| anyhow::anyhow!("Invalid glob '{}': {}", pattern, e))?
            .flatten()
        {
            if let Ok(content) = std::fs::read_to_string(&entry) {
                for key in extract_keys_from_source(&content) {
                    all_keys.insert(key);
                }
            }
        }
    }

    if all_keys.is_empty() {
        println!("No translation keys found. Check your --src globs.");
        return Ok(());
    }

    let src_path = std::path::Path::new(&config.source_dir);
    let mut total_added = 0usize;

    for lang_entry in std::fs::read_dir(src_path)? {
        let lang_entry = lang_entry?;
        if !lang_entry.path().is_dir() {
            continue;
        }
        let lang = lang_entry.file_name().to_string_lossy().to_string();

        let mut existing_keys: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut namespaces: std::collections::HashMap<String, serde_json::Value> =
            std::collections::HashMap::new();

        for file_entry in std::fs::read_dir(lang_entry.path())? {
            let file_entry = file_entry?;
            let fpath = file_entry.path();
            let is_json = matches!(fpath.extension().and_then(|e| e.to_str()), Some("json"));
            if !fpath.is_file() || !is_json {
                continue;
            }
            let ns = fpath.file_stem().unwrap().to_string_lossy().to_string();
            let content = std::fs::read_to_string(&fpath)?;
            let obj: serde_json::Value = serde_json::from_str(&content)
                .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

            let mut flat: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            l10n4x_compiler::flatten_value(ns.clone(), &obj, &mut flat);
            for k in flat.keys() {
                existing_keys.insert(k.clone());
            }
            namespaces.insert(ns, obj);
        }

        let missing: Vec<&str> = all_keys
            .iter()
            .filter(|k| !existing_keys.contains(*k))
            .map(|k| k.as_str())
            .collect();

        if missing.is_empty() {
            continue;
        }

        let mut by_namespace: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        for key in &missing {
            let ns = key.split('.').next().unwrap_or("common").to_string();
            by_namespace.entry(ns).or_default().push(key.to_string());
        }

        for (ns, keys) in by_namespace {
            let file_path = lang_entry.path().join(format!("{}.json", ns));
            let mut obj = namespaces
                .get(&ns)
                .cloned()
                .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

            for key in &keys {
                let rest = key.strip_prefix(&format!("{}.", ns)).unwrap_or(key);
                insert_nested_key(&mut obj, rest, "");
                total_added += 1;
                if dry_run {
                    println!("[DRY RUN] [{}] Would add key: {}", lang, key);
                } else {
                    println!("[{}] Added missing key: {}", lang, key);
                }
            }

            if !dry_run {
                let content = serde_json::to_string_pretty(&obj)?;
                std::fs::write(&file_path, content)?;
            }
        }
    }

    if dry_run {
        println!(
            "Dry-run: {} key(s) would be added across all locales.",
            total_added
        );
    } else {
        println!("Extract complete: {} key(s) added.", total_added);
    }
    Ok(())
}

fn detect_project_type() -> Vec<String> {
    let mut targets = Vec::new();
    if Path::new("package.json").exists() {
        targets.push("typescript".to_string());
        if let Ok(content) = std::fs::read_to_string("package.json") {
            if content.contains("\"react\"") || content.contains("\"react-dom\"") {
                targets.push("react".to_string());
            }
            if content.contains("\"vue\"") {
                targets.push("vue".to_string());
            }
            if content.contains("\"svelte\"") {
                targets.push("svelte".to_string());
            }
        }
    }
    if Path::new("go.mod").exists() {
        targets.push("go".to_string());
    }
    if Path::new("pubspec.yaml").exists() {
        targets.push("flutter".to_string());
    }
    if Path::new("Cargo.toml").exists() {
        targets.push("c".to_string());
    }
    targets
}

fn init_wizard() -> Result<(), anyhow::Error> {
    println!("Initializing l10n4x configuration...");

    let path = Path::new("l10n4x.config.json");
    if path.exists() {
        anyhow::bail!("l10n4x.config.json already exists in this directory.");
    }

    let detected = detect_project_type();
    let mut targets = Vec::new();

    if detected.is_empty() {
        println!("No standard project files detected.");
    }

    for t in &detected {
        print!("Add '{}' target? [Y/n]: ", t);
        stdout().flush()?;
        let mut input = String::new();
        stdin().read_line(&mut input)?;
        let input = input.trim().to_lowercase();
        if input.is_empty() || input == "y" || input == "yes" {
            let target = match t.as_str() {
                "go" => Target {
                    r#type: "go".to_string(),
                    out_dir: "./backend/pkg/i18n".to_string(),
                    options: serde_json::json!({ "package": "i18n" }),
                },
                "react" => Target {
                    r#type: "typescript".to_string(),
                    out_dir: "./frontend/src/i18n".to_string(),
                    options: serde_json::json!({ "flavor": "react", "strictTypes": true }),
                },
                "typescript" => Target {
                    r#type: "typescript".to_string(),
                    out_dir: "./frontend/src/i18n".to_string(),
                    options: serde_json::json!({ "flavor": "react", "strictTypes": true }),
                },
                "vue" => Target {
                    r#type: "vue".to_string(),
                    out_dir: "./src/i18n".to_string(),
                    options: serde_json::json!({}),
                },
                "svelte" => Target {
                    r#type: "svelte".to_string(),
                    out_dir: "./src/i18n".to_string(),
                    options: serde_json::json!({}),
                },
                "flutter" => Target {
                    r#type: "flutter".to_string(),
                    out_dir: "./mobile/lib/generated".to_string(),
                    options: serde_json::json!({
                        "package": "app",
                        "useFfi": true,
                        "strictNullSafety": true,
                        "generateHelpers": true
                    }),
                },
                "c" => Target {
                    r#type: "c".to_string(),
                    out_dir: "./src/i18n".to_string(),
                    options: serde_json::json!({}),
                },
                _ => Target {
                    r#type: t.clone(),
                    out_dir: "./src/i18n".to_string(),
                    options: serde_json::json!({}),
                },
            };
            println!("  Added '{}' target.", t);
            targets.push(target);
        } else {
            println!("  Skipped '{}' target.", t);
        }
    }

    if targets.is_empty() {
        println!("Adding default targets.");
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
        targets.push(Target {
            r#type: "vue".to_string(),
            out_dir: "./i18n/vue".to_string(),
            options: serde_json::json!({}),
        });
        targets.push(Target {
            r#type: "svelte".to_string(),
            out_dir: "./i18n/svelte".to_string(),
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
        compression_level: 8,
        encrypt_key_env: "L10N4X_ENCRYPT_KEY".to_string(),
        cors_origins: None,
        debug_keys: false,
        bundles: config::BundlesConfig::default(),
        tms: None,
        targets,
    };

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

/// Extracts string-literal translation keys from source text.
fn extract_keys_from_source(src: &str) -> Vec<String> {
    use std::collections::HashSet;
    let mut found: HashSet<String> = HashSet::new();

    let patterns: &[&str] = &[r#"t\(["']([^"']+)["']\)"#, r#"\.T\(["']([^"']+)["']\)"#];
    for pattern in patterns {
        if let Ok(re) = regex_lite::Regex::new(pattern) {
            for cap in re.captures_iter(src) {
                if let Some(m) = cap.get(1) {
                    found.insert(m.as_str().to_string());
                }
            }
        }
    }

    if let Ok(re) = regex_lite::Regex::new(r"LocaleKey\.([A-Z0-9_]+)") {
        for cap in re.captures_iter(src) {
            if let Some(m) = cap.get(1) {
                let key = m.as_str().to_lowercase().replace('_', ".");
                found.insert(key);
            }
        }
    }

    let mut result: Vec<String> = found.into_iter().collect();
    result.sort();
    result
}

#[cfg(test)]
mod extract_tests {
    use super::extract_keys_from_source;

    #[test]
    fn finds_ts_double_quoted_keys() {
        let src = r#"const a = t("welcome.message"); const b = t("user.name");"#;
        let keys = extract_keys_from_source(src);
        assert!(
            keys.iter().any(|k| k == "welcome.message"),
            "should find double-quoted key"
        );
        assert!(
            keys.iter().any(|k| k == "user.name"),
            "should find second key"
        );
    }

    #[test]
    fn finds_ts_single_quoted_keys() {
        let src = r#"t('settings.title')"#;
        let keys = extract_keys_from_source(src);
        assert!(keys.iter().any(|k| k == "settings.title"));
    }

    #[test]
    fn finds_go_t_keys() {
        let src = r#"msg := i18n.T("errors.not_found")"#;
        let keys = extract_keys_from_source(src);
        assert!(keys.iter().any(|k| k == "errors.not_found"));
    }

    #[test]
    fn deduplicates_keys() {
        let src = r#"t("menu.home"); t("menu.home");"#;
        let keys = extract_keys_from_source(src);
        let count = keys.iter().filter(|k| *k == "menu.home").count();
        assert_eq!(count, 1);
    }

    #[test]
    fn ignores_dynamic_keys() {
        let src = r#"t(keyVar)"#;
        let keys = extract_keys_from_source(src);
        assert!(keys.is_empty());
    }
}

#[cfg(test)]
mod check_tests {
    use super::check_report;

    #[test]
    fn detects_key_missing_from_locale() {
        let code_keys = vec!["common.title".to_string(), "common.missing_key".to_string()];
        let locale_keys = vec!["common.title".to_string()];
        let report = check_report(&code_keys, &locale_keys);
        assert!(report
            .missing_in_locale
            .contains(&"common.missing_key".to_string()));
        assert!(report.unused_in_code.is_empty());
    }

    #[test]
    fn detects_locale_key_unused_in_code() {
        let code_keys = vec!["common.title".to_string()];
        let locale_keys = vec!["common.title".to_string(), "common.orphan".to_string()];
        let report = check_report(&code_keys, &locale_keys);
        assert!(report.unused_in_code.contains(&"common.orphan".to_string()));
        assert!(report.missing_in_locale.is_empty());
    }

    #[test]
    fn clean_check_returns_empty_report() {
        let keys = vec!["a.b".to_string(), "c.d".to_string()];
        let report = check_report(&keys, &keys);
        assert!(report.missing_in_locale.is_empty());
        assert!(report.unused_in_code.is_empty());
    }
}

#[cfg(test)]
mod pseudo_tests {
    use super::pseudolocalize_string;

    #[test]
    fn wraps_in_brackets() {
        let result = pseudolocalize_string("Hello");
        assert!(result.starts_with('['));
        assert!(result.ends_with(']'));
    }

    #[test]
    fn substitutes_latin_chars() {
        let result = pseudolocalize_string("Hello");
        assert!(result.contains('é') || result.contains('ĥ') || result.contains('ö'));
    }

    #[test]
    fn preserves_icu_placeholders() {
        let result = pseudolocalize_string("Hello {name}, you have {count} items.");
        assert!(result.contains("{name}"));
        assert!(result.contains("{count}"));
    }

    #[test]
    fn pads_to_longer_length() {
        let original = "Short text";
        let result = pseudolocalize_string(original);
        let inner = &result[1..result.len() - 1];
        assert!(
            inner.len() >= original.len(),
            "pseudolocalized string should be at least as long as original"
        );
    }
}

#[cfg(test)]
mod stats_tests {
    use super::compute_coverage;
    use std::collections::HashSet;

    fn make_keys(keys: &[&str]) -> HashSet<String> {
        keys.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn full_coverage() {
        let ref_keys = make_keys(&["a", "b", "c"]);
        let locale_keys = make_keys(&["a", "b", "c"]);
        let cov = compute_coverage("en", &locale_keys, &ref_keys);
        assert_eq!(cov.percent, 100);
        assert_eq!(cov.missing_count, 0);
    }

    #[test]
    fn partial_coverage() {
        let ref_keys = make_keys(&["a", "b", "c", "d"]);
        let locale_keys = make_keys(&["a", "b"]);
        let cov = compute_coverage("fr", &locale_keys, &ref_keys);
        assert_eq!(cov.percent, 50);
        assert_eq!(cov.missing_count, 2);
    }

    #[test]
    fn zero_keys_in_locale() {
        let ref_keys = make_keys(&["a"]);
        let cov = compute_coverage("de", &HashSet::new(), &ref_keys);
        assert_eq!(cov.percent, 0);
    }
}

#[cfg(test)]
mod etag_tests {
    use super::fnv1a_hex;

    #[test]
    fn same_input_produces_same_etag() {
        let data = b"hello world";
        assert_eq!(fnv1a_hex(data), fnv1a_hex(data));
    }

    #[test]
    fn different_input_produces_different_etag() {
        assert_ne!(fnv1a_hex(b"hello"), fnv1a_hex(b"world"));
    }

    #[test]
    fn etag_is_hex_string() {
        let tag = fnv1a_hex(b"test");
        assert!(tag.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(tag.len(), 16, "FNV-1a 64-bit = 16 hex chars");
    }
}

#[cfg(test)]
mod path_safety_tests {
    use super::sanitize_locale_filename;

    #[test]
    fn accepts_simple_locale_pak() {
        assert!(sanitize_locale_filename("en.pak").is_some());
        assert!(sanitize_locale_filename("zh-CN.pak").is_some());
        assert!(sanitize_locale_filename("en").is_some());
        assert!(sanitize_locale_filename("pt_BR").is_some());
    }

    #[test]
    fn rejects_locale_stem_with_path_traversal() {
        assert!(sanitize_locale_filename("../en").is_none());
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
        Commands::Validate { report_misses } => {
            let config = load_config()?;
            validate_keys(&config.source_dir, report_misses)?;
        }
        Commands::Dev { flutter_web, port } => {
            run_dev_server(port, flutter_web).await?;
        }
        Commands::Check { src, json } => {
            std::process::exit(check_command(src, json));
        }
        Commands::Pseudo { locale, out } => {
            if let Err(e) = pseudo_command(locale, out) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Stats { json, verbose } => {
            if let Err(e) = stats_command(json, verbose) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Extract { src, dry_run } => {
            let result = extract_command(src, dry_run);
            if let Err(e) = result {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Sync {
            provider,
            direction,
            out,
            from,
        } => {
            let config = load_config()?;
            let dir = tms::SyncDirection::parse(&direction)?;
            if let Err(e) = tms::run_sync(&config, &provider, dir, out.as_deref(), from.as_deref())
            {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Generate { target } => {
            let normalized = if target == "dart" {
                "flutter"
            } else {
                target.as_str()
            };
            if !targets::SUPPORTED_TARGETS.contains(&normalized) {
                anyhow::bail!(
                    "Unsupported target '{}'. Supported targets are: {}.",
                    target,
                    targets::SUPPORTED_TARGETS.join(", ")
                );
            }
            let config = load_config()?;
            let keys = validate_keys(&config.source_dir, false)?;
            let filtered: Vec<Target> = config
                .targets
                .into_iter()
                .filter(|t| {
                    let cfg_type = if t.r#type == "dart" {
                        "flutter"
                    } else {
                        t.r#type.as_str()
                    };
                    cfg_type == normalized
                })
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
