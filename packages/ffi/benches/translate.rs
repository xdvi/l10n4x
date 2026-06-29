use criterion::{black_box, criterion_group, criterion_main, Criterion};
use l10n4c::{
    l10n4c_clear, l10n4c_translate, l10n4c_translate_required_size, l10n4c_translate_with_params,
    l10n4c_translate_with_params_required_size, L10n4cParam, L10N4C_OK,
};
use l10n4x_core::binary_format::fnv1a_64;
use l10n4x_core::loader::load_raw_bytes;
use std::ffi::CString;

fn make_binary_with_key(key: &str, val: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"L10N");
    buf.extend_from_slice(&1u32.to_be_bytes());
    let val_offset: u32 = 16;
    let index_offset: u32 = val_offset + val.len() as u32;
    buf.extend_from_slice(&index_offset.to_be_bytes());
    buf.extend_from_slice(&1u32.to_be_bytes());
    buf.extend_from_slice(val);
    let hash = fnv1a_64(key.as_bytes());
    buf.extend_from_slice(&hash.to_be_bytes());
    buf.extend_from_slice(&val_offset.to_be_bytes());
    buf.extend_from_slice(&(val.len() as u32).to_be_bytes());
    buf
}

fn setup() -> (CString, u64) {
    l10n4c_clear();
    let welcome_bc = [0x01, 0, 0, 0, 6, b'H', b'e', b'l', b'l', b'o', b'!'];
    assert!(load_raw_bytes(
        "en",
        make_binary_with_key("common.welcome", &welcome_bc),
    ));
    let locale = CString::new("en").unwrap();
    let key_hash = fnv1a_64(b"common.welcome");
    (locale, key_hash)
}

fn bench_ffi_translate(c: &mut Criterion) {
    let (locale, key_hash) = setup();
    let mut buf = vec![0u8; 64];

    c.bench_function("ffi_required_size_cold", |b| {
        b.iter(|| {
            let mut out_size = 0usize;
            let status = l10n4c_translate_required_size(
                black_box(locale.as_ptr()),
                black_box(key_hash),
                black_box(&mut out_size),
            );
            black_box(status);
            black_box(out_size);
        });
    });

    // Prime thread-local cache (simulates required_size before translate).
    let mut out_size = 0usize;
    assert_eq!(
        l10n4c_translate_required_size(locale.as_ptr(), key_hash, &mut out_size),
        L10N4C_OK
    );

    c.bench_function("ffi_translate_cache_hit", |b| {
        b.iter(|| {
            let status = l10n4c_translate(
                black_box(locale.as_ptr()),
                black_box(key_hash),
                black_box(buf.as_mut_ptr()),
                black_box(buf.len()),
            );
            black_box(status);
        });
    });

    c.bench_function("ffi_two_phase_full", |b| {
        b.iter(|| {
            let mut out_size = 0usize;
            let _ = l10n4c_translate_required_size(
                black_box(locale.as_ptr()),
                black_box(key_hash),
                black_box(&mut out_size),
            );
            let status = l10n4c_translate(
                black_box(locale.as_ptr()),
                black_box(key_hash),
                black_box(buf.as_mut_ptr()),
                black_box(buf.len()),
            );
            black_box(status);
        });
    });

    let name_bc = [
        0x01, 0, 0, 0, 6, b'H', b'e', b'l', b'l', b'o', b' ', 0x02, 0, 0, 0, 4, b'n', b'a', b'm',
        b'e',
    ];
    l10n4c_clear();
    assert!(load_raw_bytes(
        "en",
        make_binary_with_key("common.hello_name", &name_bc),
    ));
    let hello_name_hash = fnv1a_64(b"common.hello_name");
    let key_c = CString::new("name").unwrap();
    let val_c = CString::new("Diego").unwrap();
    let param = L10n4cParam {
        key: key_c.as_ptr(),
        value: val_c.as_ptr(),
    };
    let mut params_buf = vec![0u8; 64];

    c.bench_function("ffi_translate_with_params", |b| {
        b.iter(|| {
            let status = l10n4c_translate_with_params(
                black_box(locale.as_ptr()),
                black_box(hello_name_hash),
                black_box(&param),
                black_box(1),
                black_box(params_buf.as_mut_ptr()),
                black_box(params_buf.len()),
            );
            black_box(status);
        });
    });

    let mut out_size = 0usize;
    assert_eq!(
        l10n4c_translate_with_params_required_size(
            locale.as_ptr(),
            hello_name_hash,
            &param,
            1,
            &mut out_size,
        ),
        L10N4C_OK
    );

    c.bench_function("ffi_translate_with_params_cache_hit", |b| {
        b.iter(|| {
            let status = l10n4c_translate_with_params(
                black_box(locale.as_ptr()),
                black_box(hello_name_hash),
                black_box(&param),
                black_box(1),
                black_box(params_buf.as_mut_ptr()),
                black_box(params_buf.len()),
            );
            black_box(status);
        });
    });
}

criterion_group!(benches, bench_ffi_translate);
criterion_main!(benches);
