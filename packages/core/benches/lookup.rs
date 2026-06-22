use criterion::{black_box, criterion_group, criterion_main, Criterion};
use l10n4x_core::binary_format::fnv1a_64;
use l10n4x_core::integrity;
use l10n4x_core::loader::{load_raw_bytes, try_load_pak_lazy};
use l10n4x_core::pak::{build_unsigned, seal};
use l10n4x_core::store::{
    clear_translations, key_exists, set_fallback_locale, swap_store, translate,
    translate_to_writer, translate_to_writer_with_status, StoreData, TranslationStore,
};
use l10n4x_compiler::signing;
use std::sync::{Arc, OnceLock};

fn make_binary_with_keys(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"L10N");
    buf.extend_from_slice(&1u32.to_be_bytes());
    buf.extend_from_slice(&0u32.to_be_bytes());
    buf.extend_from_slice(&(entries.len() as u32).to_be_bytes());

    let mut sorted = entries.to_vec();
    sorted.sort_by_key(|(key, _)| fnv1a_64(key.as_bytes()));

    let mut index_records = Vec::with_capacity(sorted.len());
    for (key, val) in sorted {
        let val_offset = buf.len() as u32;
        buf.extend_from_slice(val);
        index_records.push((fnv1a_64(key.as_bytes()), val_offset, val.len() as u32));
    }

    let index_offset = buf.len() as u32;
    buf[8..12].copy_from_slice(&index_offset.to_be_bytes());
    for (hash, val_offset, val_len) in index_records {
        buf.extend_from_slice(&hash.to_be_bytes());
        buf.extend_from_slice(&val_offset.to_be_bytes());
        buf.extend_from_slice(&val_len.to_be_bytes());
    }
    buf
}

fn setup_locales() {
    clear_translations();

    let welcome_bc = [0x01, 0, 0, 0, 6, b'H', b'e', b'l', b'l', b'o', b'!'];
    let name_bc = [
        0x01, 0, 0, 0, 6, b'H', b'e', b'l', b'l', b'o', b' ', 0x02, 0, 0, 0, 4, b'n', b'a', b'm',
        b'e',
    ];

    assert!(load_raw_bytes(
        "es",
        make_binary_with_keys(&[
            ("common.welcome", &welcome_bc),
            ("common.hello_name", &name_bc),
        ]),
    ));
    assert!(load_raw_bytes(
        "en",
        make_binary_with_keys(&[
            ("common.welcome", &welcome_bc),
            ("common.fallback_only", &welcome_bc),
        ]),
    ));
    set_fallback_locale("en");
}

fn lazy_pak_bytes() -> &'static [u8] {
    static PAK: OnceLock<Vec<u8>> = OnceLock::new();
    PAK.get_or_init(|| {
        let seed = [42u8; 32];
        assert!(signing::set_signing_key(&seed));
        let pubkey = signing::signing_public_key().unwrap();
        assert!(integrity::set_verify_key(&pubkey));
        let welcome_bc = [0x01, 0, 0, 0, 6, b'H', b'e', b'l', b'l', b'o', b'!'];
        let l10n = make_binary_with_keys(&[("common.welcome", &welcome_bc)]);
        let compressed = zstd::encode_all(l10n.as_slice(), 3).unwrap();
        let unsigned = build_unsigned(&compressed, None);
        let signature = signing::sign(&unsigned).unwrap();
        seal(&unsigned, &signature)
    })
    .as_slice()
}

fn setup_lazy_locale() -> u64 {
    clear_translations();
    assert!(try_load_pak_lazy("lazy", lazy_pak_bytes()).is_ok());
    fnv1a_64(b"common.welcome")
}

fn bench_lookup(c: &mut Criterion) {
    setup_locales();

    let welcome_hash = fnv1a_64(b"common.welcome");
    let hello_name_hash = fnv1a_64(b"common.hello_name");
    let fallback_hash = fnv1a_64(b"common.fallback_only");

    c.bench_function("translate_to_writer_hit", |b| {
        b.iter(|| {
            let mut buf = String::new();
            let _ = translate_to_writer(
                black_box("es"),
                black_box(welcome_hash),
                None,
                black_box(&[]),
                &mut buf,
            );
        });
    });

    c.bench_function("translate_alloc_cache_hit", |b| {
        b.iter(|| {
            let _ = translate(black_box("es"), black_box(welcome_hash), None, black_box(&[]));
        });
    });

    c.bench_function("translate_with_params", |b| {
        b.iter(|| {
            let mut buf = String::new();
            let _ = translate_to_writer(
                black_box("es"),
                black_box(hello_name_hash),
                None,
                black_box(&[("name", "Diego")]),
                &mut buf,
            );
        });
    });

    let params = [("name", "Diego")];
    let _ = translate("es", hello_name_hash, None, &params);

    c.bench_function("translate_with_params_cache_hit", |b| {
        b.iter(|| {
            let _ = translate(
                black_box("es"),
                black_box(hello_name_hash),
                None,
                black_box(&params),
            );
        });
    });

    c.bench_function("translate_fallback", |b| {
        b.iter(|| {
            let mut buf = String::new();
            let _ = translate_to_writer(
                black_box("es"),
                black_box(fallback_hash),
                None,
                black_box(&[]),
                &mut buf,
            );
        });
    });

    c.bench_function("translate_to_writer_with_status", |b| {
        b.iter(|| {
            let mut buf = String::new();
            let _ = translate_to_writer_with_status(
                black_box("es"),
                black_box(welcome_hash),
                None,
                black_box(&[]),
                &mut buf,
            );
        });
    });

    c.bench_function("key_exists", |b| {
        b.iter(|| {
            let _ = key_exists(black_box("es"), black_box(welcome_hash), None);
        });
    });

    c.bench_function("ffi_two_phase_pattern", |b| {
        b.iter(|| {
            let mut buf = String::new();
            let status = translate_to_writer_with_status(
                black_box("es"),
                black_box(welcome_hash),
                None,
                black_box(&[]),
                &mut buf,
            )
            .unwrap();
            black_box(status.key_found);
            black_box(buf.len());
        });
    });

    let lazy_hash = setup_lazy_locale();
    c.bench_function("lazy_cold_first_translate", |b| {
        b.iter_batched(
            setup_lazy_locale,
            |key_hash| {
                let _ = translate(black_box("lazy"), black_box(key_hash), None, black_box(&[]));
            },
            criterion::BatchSize::SmallInput,
        );
    });

    // Warm lazy decompression cache once, then measure steady-state.
    let _ = translate("lazy", lazy_hash, None, &[]);
    c.bench_function("lazy_steady_translate", |b| {
        b.iter(|| {
            let _ = translate(black_box("lazy"), black_box(lazy_hash), None, black_box(&[]));
        });
    });

    c.bench_function("swap_store_reload", |b| {
        b.iter(|| {
            let mut store = TranslationStore::default();
            let vec = Arc::make_mut(&mut store.locales);
            vec.push((
                "es".to_string(),
                StoreData::Owned(Arc::new(Vec::new())),
            ));
            swap_store(black_box(store));
        })
    });

    let prebuilt_locales = Arc::new(vec![(
        "es".to_string(),
        StoreData::Owned(Arc::new(Vec::new())),
    )]);
    let prebuilt_chain = TranslationStore::default().fallback_chain;

    c.bench_function("swap_store_pure", |b| {
        b.iter(|| {
            let store = TranslationStore {
                locales: Arc::clone(&prebuilt_locales),
                fallback_chain: Arc::clone(&prebuilt_chain),
                #[cfg(feature = "std")]
                lazy_cache: None,
                #[cfg(feature = "std")]
                offset_maps: None,
                #[cfg(feature = "std")]
                loaded_namespaces: None,
            };
            swap_store(black_box(store));
        });
    });

    let welcome_bc = [0x01, 0, 0, 0, 6, b'H', b'e', b'l', b'l', b'o', b'!'];
    let reload_bytes = make_binary_with_keys(&[("common.welcome", &welcome_bc)]);
    setup_locales();
    c.bench_function("load_raw_bytes_reload", |b| {
        b.iter(|| {
            let _ = load_raw_bytes(black_box("es"), black_box(reload_bytes.clone()));
        });
    });
}

criterion_group!(benches, bench_lookup);
criterion_main!(benches);