use l10n4x_compiler::signing;
use l10n4x_core::binary_format::{fnv1a_64, pack_l10n, RUNTIME_VERSION};
use l10n4x_core::integrity;
use l10n4x_core::loader::try_load_static_bytes_for_store;
use l10n4x_core::pak::{build_unsigned, seal};
use l10n4x_core::store::{clear_translations_for_store, translate, translate_for_store};
use l10n4x_core::store_registry::{create_store, destroy_store, StoreHandle};
use std::collections::BTreeMap;
use std::sync::Mutex;

static OTA_SCOPED_TEST_MUTEX: Mutex<()> = Mutex::new(());

fn pak_with_text(hash: u64, text: &[u8]) -> Vec<u8> {
    let mut entries = BTreeMap::new();
    entries.insert(hash, text.to_vec());
    pack_l10n(&entries, RUNTIME_VERSION, 1, None)
}

fn install_test_signing_keys() {
    let seed = [22u8; 32];
    let _ = signing::set_signing_key(&seed);
    let pubkey = signing::signing_public_key().expect("signing key configured");
    assert!(integrity::set_verify_key(&pubkey));
}

fn signed_pak_with_text(hash: u64, text: &[u8]) -> Vec<u8> {
    let l10n = pak_with_text(hash, text);
    let compressed = zstd::encode_all(l10n.as_slice(), 3).unwrap();
    let unsigned = build_unsigned(&compressed, None);
    let signature = signing::sign(&unsigned).expect("sign pak");
    seal(&unsigned, &signature)
}

#[test]
fn two_stores_are_isolated() {
    let a = create_store().expect("create a");
    let b = create_store().expect("create b");

    l10n4x_core::store::set_fallback_chain_for_store(a, &["en"]).unwrap();
    l10n4x_core::store::set_fallback_chain_for_store(b, &["fr"]).unwrap();

    assert_eq!(
        l10n4x_core::store::get_fallback_chain_for_store(a)
            .first()
            .map(|s| s.as_ref()),
        Some("en")
    );
    assert_eq!(
        l10n4x_core::store::get_fallback_chain_for_store(b)
            .first()
            .map(|s| s.as_ref()),
        Some("fr")
    );

    destroy_store(a).unwrap();
    destroy_store(b).unwrap();
}

#[test]
fn destroy_invalid_handle_errors() {
    let fake = StoreHandle::from_raw(999_999).unwrap();
    assert!(destroy_store(fake).is_err());
}

#[test]
fn scoped_translate_uses_only_that_store() {
    let h = create_store().unwrap();
    let key = fnv1a_64(b"hello");
    let pak = pak_with_text(key, b"scoped");
    try_load_static_bytes_for_store(Some(h), "en", &pak, true).unwrap();

    let global = translate("en", key, None, &[]);
    assert_ne!(global, "scoped");

    let scoped = translate_for_store(Some(h), "en", key, None, &[]);
    assert_eq!(scoped, "scoped");

    clear_translations_for_store(Some(h)).unwrap();
    destroy_store(h).unwrap();
}

#[test]
fn ota_reload_only_affects_target_store() {
    let _lock = OTA_SCOPED_TEST_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    install_test_signing_keys();

    let a = create_store().unwrap();
    let b = create_store().unwrap();
    let key = fnv1a_64(b"k");
    let pak_v1 = pak_with_text(key, b"v1");
    let pak_v2 = signed_pak_with_text(key, b"v2");

    try_load_static_bytes_for_store(Some(a), "en", &pak_v1, true).unwrap();
    try_load_static_bytes_for_store(Some(b), "en", &pak_v1, true).unwrap();

    l10n4x_core::ota::try_ota_reload_pak_for_store(Some(a), "en", &pak_v2).unwrap();
    assert_eq!(translate_for_store(Some(a), "en", key, None, &[]), "v2");
    assert_eq!(translate_for_store(Some(b), "en", key, None, &[]), "v1");

    destroy_store(a).unwrap();
    destroy_store(b).unwrap();
}
