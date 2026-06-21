// This test should fail to compile because signing capabilities have been removed from l10n4x-core.
fn main() {
    let _ = l10n4x_core::integrity::set_signing_key(b"seed");
}
