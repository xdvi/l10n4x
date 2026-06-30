use criterion::{black_box, criterion_group, criterion_main, Criterion};
use l10n4x_compiler::signing;
use std::fs;
use std::path::PathBuf;

/// Create a realistic multi-locale test fixture in a temp directory.
/// Returns the (src_path, out_path).
fn setup_fixture(
    locale_count: usize,
    files_per_locale: usize,
    keys_per_file: usize,
) -> (PathBuf, PathBuf) {
    let tmp = std::env::temp_dir().join(format!(
        "l10n4x_bench_{}_{}_{}",
        locale_count, files_per_locale, keys_per_file
    ));
    let _ = fs::remove_dir_all(&tmp);

    let locales = [
        "en", "es", "fr", "de", "pt", "it", "nl", "pl", "sv", "da",
        "nb", "fi", "cs", "hu", "ro", "sk", "sl", "hr", "lt", "lv",
    ];

    for i in 0..locale_count {
        let locale = locales[i % locales.len()];
        let locale_dir = tmp.join(locale);
        fs::create_dir_all(&locale_dir).unwrap();

        for f in 0..files_per_locale {
            let ns = ["common", "auth", "settings", "errors", "menu"][f % 5];
            let mut entries = serde_json::Map::new();

            for k in 0..keys_per_file {
                let k_str = k.to_string();
                let (key, value) = match k % 5 {
                    0 => (format!("title_{}", k), format!("Page Title {}", k)),
                    1 => (
                        format!("greeting_{}", k),
                        // {name} is an ICU variable — escape as literal {{name}}
                        format!("Hello {{name}}! Welcome to {}", k),
                    ),
                    2 => (
                        format!("items_{}", k),
                        // ICU plural with Rust format escaping: {{ → {, }} → }, {} → placeholder
                        format!(
                            "{{count, plural, =0 {{{} items}} =1 {{{} item}} other {{{} items}}}}",
                            k_str, k_str, k_str
                        ),
                    ),
                    3 => (
                        format!("description_{}", k),
                        format!(
                            "This is a longer description with {{param1}} and {{param2}} for key {}",
                            k
                        ),
                    ),
                    4 => (
                        format!("error_{}", k),
                        format!("Error code {}: {{error_message}}", k),
                    ),
                    _ => unreachable!(),
                };
                entries.insert(key, serde_json::Value::String(value));
            }

            let json_content = serde_json::to_string(&serde_json::Value::Object(entries)).unwrap();
            fs::write(locale_dir.join(format!("{}.json", ns)), json_content).unwrap();
        }
    }

    (tmp.clone(), tmp.join("out"))
}

fn bench_compile_pipeline(c: &mut Criterion) {
    // Standard test: 8 locales × 5 files × 100 keys = 4000 total translations
    const LOCALE_COUNT: usize = 8;
    const FILES_PER_LOCALE: usize = 5;
    const KEYS_PER_FILE: usize = 100;

    let seed = [42u8; 32];
    assert!(signing::set_signing_key(&seed));

    let mut group = c.benchmark_group("compile_pipeline");
    group.sample_size(10);

    group.bench_function(
        &format!("{}loc_{}files_{}keys", LOCALE_COUNT, FILES_PER_LOCALE, KEYS_PER_FILE),
        |b| {
            b.iter_batched(
                || {
                    // Setup: create fresh fixture before each measurement
                    let (src, out) = setup_fixture(LOCALE_COUNT, FILES_PER_LOCALE, KEYS_PER_FILE);
                    (src, out)
                },
                |(src, out)| {
                    let result = l10n4x_compiler::compile_translations(
                        black_box(&src),
                        black_box(&out),
                        false,
                        6,
                    );
                    black_box(result)
                },
                criterion::BatchSize::LargeInput,
            );
        },
    );

    // Small test: 2 locales × 2 files × 10 keys = 40 translations (quick smoke)
    group.bench_function("small_2loc_2files_10keys", |b| {
        b.iter_batched(
            || setup_fixture(2, 2, 10),
            |(src, out)| {
                let result = l10n4x_compiler::compile_translations(
                    black_box(&src),
                    black_box(&out),
                    false,
                    6,
                );
                black_box(result)
            },
            criterion::BatchSize::LargeInput,
        );
    });

    group.finish();
}

criterion_group!(benches, bench_compile_pipeline);
criterion_main!(benches);
