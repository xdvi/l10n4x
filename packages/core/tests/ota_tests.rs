//! OTA reload + rollback integration tests (isolated binary to avoid verify-key races).

use l10n4x_core::binary_format::{fnv1a_64, pack_l10n, RUNTIME_VERSION};
use l10n4x_core::integrity;
use l10n4x_core::loader::{load_raw_bytes, try_load_pak_bytes};
use l10n4x_core::metrics;
use l10n4x_core::ota::{ota_can_rollback, try_ota_reload_pak, try_ota_rollback};
use l10n4x_core::pak::{build_unsigned, seal};
use l10n4x_core::store::{clear_translations, translate};
use l10n4x_compiler::signing;
use std::collections::BTreeMap;
use std::sync::Mutex;

static OTA_TEST_MUTEX: Mutex<()> = Mutex::new(());

fn install_test_keys() {
    let seed = [11u8; 32];
    let _ = signing::set_signing_key(&seed);
    let pubkey = signing::signing_public_key().expect("signing key configured");
    assert!(integrity::set_verify_key(&pubkey));
}

fn text_bytecode(text: &str) -> Vec<u8> {
    let mut bc = Vec::new();
    bc.push(0x01);
    bc.extend_from_slice(&(text.len() as u32).to_be_bytes());
    bc.extend_from_slice(text.as_bytes());
    bc
}

fn make_l10n(key: &str, text: &str) -> Vec<u8> {
    let mut entries = BTreeMap::new();
    entries.insert(fnv1a_64(key.as_bytes()), text_bytecode(text));
    pack_l10n(&entries, RUNTIME_VERSION, None)
}

fn sign_l10n(l10n: &[u8]) -> Vec<u8> {
    let compressed = zstd::encode_all(l10n, 3).unwrap();
    let unsigned = build_unsigned(&compressed, None);
    let signature = signing::sign(&unsigned).unwrap();
    seal(&unsigned, &signature)
}

#[test]
fn ota_reload_and_rollback_roundtrip() {
    let _lock = OTA_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    install_test_keys();
    clear_translations();

    assert!(load_raw_bytes("en", make_l10n("greeting", "hello-v1")));
    let key = fnv1a_64(b"greeting");
    assert_eq!(translate("en", key, None, &[]), "hello-v1");

    let pak_v2 = sign_l10n(&make_l10n("greeting", "hello-v2"));
    try_ota_reload_pak("en", &pak_v2).expect("ota reload");
    assert_eq!(translate("en", key, None, &[]), "hello-v2");
    assert!(ota_can_rollback("en"));

    try_ota_rollback("en").expect("ota rollback");
    assert_eq!(translate("en", key, None, &[]), "hello-v1");
    assert!(!ota_can_rollback("en"));
}

#[test]
fn ota_verify_failure_increments_metric() {
    let _lock = OTA_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    install_test_keys();
    clear_translations();
    let before = metrics::metrics_string();
    assert!(try_ota_reload_pak("en", b"not-a-pak").is_err());
    let after = metrics::metrics_string();
    assert_ne!(before, after);
}

#[test]
fn ota_reload_via_try_load_pak_bytes_baseline() {
    let _lock = OTA_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    install_test_keys();
    clear_translations();
    let pak = sign_l10n(&make_l10n("k", "v"));
    try_load_pak_bytes("en", &pak).expect("load signed pak");
}