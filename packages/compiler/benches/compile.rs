use criterion::{black_box, criterion_group, criterion_main, Criterion};
use l10n4x_compiler::signing;
use std::fs;
use std::path::PathBuf;

/// Create a realistic multi-locale test fixture.
fn create_fixture(
    base: &PathBuf,
    locale_count: usize,
    files_per_locale: usize,
    keys_per_file: usize,
) {
    let locales = ["en", "es", "fr", "de", "pt", "it", "nl", "pl", "sv"];

    for i in 0..locale_count {
        let locale = locales[i % locales.len()];
        let locale_dir = base.join(locale);
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
                        format!("Hello {{name}}! Welcome to {}", k),
                    ),
                    2 => (
                        format!("items_{}", k),
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
}

fn bench_compile_pipeline(c: &mut Criterion) {
    let seed = [42u8; 32];
    assert!(signing::set_signing_key(&seed));

    // --- Medium test: 8 locales x 5 files x 50 keys = 2000 translations ---
    let medium_src = std::env::temp_dir().join("l10n4x_bench_medium");
    let _ = fs::remove_dir_all(&medium_src);
    create_fixture(&medium_src, 8, 5, 50);

    let mut group = c.benchmark_group("compile_pipeline");
    group.sample_size(30);
    group.measurement_time(std::time::Duration::from_secs(20));

    group.bench_function("medium_8loc_5files_50keys", |b| {
        b.iter(|| {
            let out = std::env::temp_dir().join("l10n4x_bench_out_med");
            let _ = fs::remove_dir_all(&out);
            let result = l10n4x_compiler::compile_translations(
                black_box(&medium_src),
                black_box(&out),
                false,
                6,
            );
            let _ = fs::remove_dir_all(&out);
            black_box(result)
        });
    });

    // --- Small test: 2 locales x 2 files x 10 keys ---
    let small_src = std::env::temp_dir().join("l10n4x_bench_small");
    let _ = fs::remove_dir_all(&small_src);
    create_fixture(&small_src, 2, 2, 10);

    group.bench_function("small_2loc_2files_10keys", |b| {
        b.iter(|| {
            let out = std::env::temp_dir().join("l10n4x_bench_out_small");
            let _ = fs::remove_dir_all(&out);
            let result = l10n4x_compiler::compile_translations(
                black_box(&small_src),
                black_box(&out),
                false,
                6,
            );
            let _ = fs::remove_dir_all(&out);
            black_box(result)
        });
    });

    group.finish();
}

criterion_group!(benches, bench_compile_pipeline);
criterion_main!(benches);
