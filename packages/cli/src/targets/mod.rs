pub mod c;
pub mod flutter;
pub mod go;
pub mod python;
pub mod typescript;

/// Shared runtime/build settings passed to binding generators.
pub struct GenerateContext<'a> {
    pub fallback: &'a str,
    pub output_dir: &'a str,
    pub source_dir: &'a str,
    pub verify_key_bytes: &'a str,
    pub verify_public_key_hex: &'a str,
    pub encrypt: bool,
    pub encrypt_key_env: &'a str,
}
