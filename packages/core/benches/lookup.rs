use criterion::{black_box, criterion_group, criterion_main, Criterion};
use l10n4x_core::loader::load_raw_bytes;
use l10n4x_core::store::{swap_store, translate_to_writer, TranslationStore};
use std::sync::Arc;

fn build_pak_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(b"L10N");
    data.extend_from_slice(&1u32.to_be_bytes()); // version
    data.extend_from_slice(&0u32.to_be_bytes()); // index offset (filled later)
    data.extend_from_slice(&(entries.len() as u32).to_be_bytes()); // index count

    let mut sorted_entries = entries.to_vec();
    sorted_entries.sort_by_key(|e| e.0);

    let mut key_val_positions = Vec::new();

    for (key, val) in &sorted_entries {
        let key_offset = data.len();
        data.extend_from_slice(key.as_bytes());
        let key_len = key.len();

        let val_offset = data.len();
        data.extend_from_slice(val);
        let val_len = val.len();

        key_val_positions.push((key_offset, key_len, val_offset, val_len));
    }

    let index_offset = data.len();
    data[8..12].copy_from_slice(&(index_offset as u32).to_be_bytes());

    for (k_off, k_len, v_off, v_len) in key_val_positions {
        data.extend_from_slice(&(k_off as u32).to_be_bytes());
        data.extend_from_slice(&(k_len as u32).to_be_bytes());
        data.extend_from_slice(&(v_off as u32).to_be_bytes());
        data.extend_from_slice(&(v_len as u32).to_be_bytes());
    }

    data
}

fn setup_locales() {
    // 1. Simple welcome message ("Hello!")
    // 0x01 + 6 (u32 be) + "Hello!"
    let welcome_bc = [0x01, 0, 0, 0, 6, b'H', b'e', b'l', b'l', b'o', b'!'];

    // 2. Welcome name message ("Hello {name}")
    // 0x01 + 6 (u32 be) + "Hello "
    // 0x02 + 4 (u32 be) + "name"
    let name_bc = [
        0x01, 0, 0, 0, 6, b'H', b'e', b'l', b'l', b'o', b' ', 0x02, 0, 0, 0, 4, b'n', b'a', b'm',
        b'e',
    ];

    let es_pak = build_pak_bytes(&[
        ("common.welcome", &welcome_bc),
        ("common.hello_name", &name_bc),
    ]);

    let en_pak = build_pak_bytes(&[
        ("common.welcome", &welcome_bc),
        ("common.fallback_only", &welcome_bc),
    ]);

    assert!(load_raw_bytes("es", &es_pak));
    assert!(load_raw_bytes("en", &en_pak));
    l10n4x_core::store::set_fallback_locale("en");
}

fn bench_lookup(c: &mut Criterion) {
    setup_locales();

    c.bench_function("translate_hot_path", |b| {
        b.iter(|| {
            let mut buf = String::new();
            let _ = translate_to_writer(
                black_box("es"),
                black_box("common.welcome"),
                black_box(&[]),
                &mut buf,
            );
        })
    });

    c.bench_function("translate_with_params", |b| {
        b.iter(|| {
            let mut buf = String::new();
            let _ = translate_to_writer(
                black_box("es"),
                black_box("common.hello_name"),
                black_box(&[("name", "Diego")]),
                &mut buf,
            );
        })
    });

    c.bench_function("translate_fallback", |b| {
        b.iter(|| {
            let mut buf = String::new();
            // Looks up in "es", not found, falls back to "en"
            let _ = translate_to_writer(
                black_box("es"),
                black_box("common.fallback_only"),
                black_box(&[]),
                &mut buf,
            );
        })
    });

    c.bench_function("swap_store_reload", |b| {
        b.iter(|| {
            let mut store = TranslationStore::default();
            let vec = Arc::make_mut(&mut store.locales);
            vec.push(("es".to_string(), std::sync::Arc::new(vec![])));
            swap_store(black_box(store));
        })
    });

    // Pre-built data for pure swap benchmarking
    let prebuilt_locales = std::sync::Arc::new(vec![("es".to_string(), std::sync::Arc::new(vec![]))]);
    let prebuilt_chain = TranslationStore::default().fallback_chain; // cached singleton

    c.bench_function("swap_store_pure", |b| {
        b.iter(|| {
            let store = TranslationStore {
                locales: std::sync::Arc::clone(&prebuilt_locales),
                fallback_chain: std::sync::Arc::clone(&prebuilt_chain),
            };
            swap_store(black_box(store));
        })
    });
}

criterion_group!(benches, bench_lookup);
criterion_main!(benches);
