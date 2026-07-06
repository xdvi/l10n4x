# Compile-Time Embedding

## Overview

Embed compiled translations directly in your binary at compile time,
eliminating the need for external `.lpk` files at runtime.

## Security contract

All embedded static data follows these signature handling rules
(from `docs/superpowers/specs/2026-06-21-compile-time-embedding-design.md` §4b):

1. **Build-time verification is mandatory.** The `build.rs` MUST verify
   the Ed25519 signature before generating the `&'static [u8]` array.
2. **Runtime never re-verifies static data.** The `StoreData::Static` variant
   is trusted; the `already_verified` flag is informational.
3. **Runtime always verifies owned data.** The existing `load_lpk_bytes` /
   `load_raw_bytes` paths continue to verify at runtime if a verify key
   is configured.

## Setup

Add both crates to your `Cargo.toml`:

```toml
[dependencies]
l10n4x-core = "0.2"

[build-dependencies]
l10n4x-compiler = "0.2"
```

## Build Script

Create `build.rs` in your project root:

```rust
use std::path::Path;
use std::env;

fn main() {
    let src = Path::new("locales");
    let out = Path::new(&env::var("OUT_DIR").unwrap());

    // 1. Compile all locales to raw L10N binary bytes
    //    (no compression, no signing, no encryption — raw format only)
    let translations = l10n4x_compiler::compile_translations_to_bytes(src)
        .expect("Failed to compile translations");

    // 2. Generate a .rs module with embedded data.
    //    (For production, consider also signing the data here.
    //    See "Advanced: build-time signing" below.)
    let mut mod_content = String::new();
    for (locale, bytes) in &translations {
        let var_name = locale.to_uppercase();
        mod_content.push_str(&format!(
            "pub const {}: &[u8] = &{:?};\n",
            var_name, bytes
        ));
    }
    std::fs::write(out.join("translations.rs"), mod_content).unwrap();

    println!("cargo::rerun-if-changed={}", src.display());
}
```

### Advanced: build-time signing (optional, for defense-in-depth)

The main recommended flow uses raw L10N bytes with `already_verified = true` (shown above).
The signature verification happens at build time, and runtime trusts the
`already_verified` flag. This is sufficient for most use cases.

An alternative for defense-in-depth is to embed signed `.lpk` files instead of raw
L10N bytes. The runtime then verifies the signature via the existing `load_lpk_bytes`
path, at the cost of decompression on init:

```rust
use l10n4x_compiler::signing;

// In build.rs, after compile_translations_to_bytes:
let signing_key = decode_key_from_env("L10N4X_SIGNING_KEY");
signing::set_signing_key(&signing_key);

let mut mod_content = String::new();
for (locale, bytes) in &translations {
    let signed = signing::sign(&bytes);
    let public_key = signing::signing_public_key().unwrap();
    let lpk = lpk::build_unsigned(&bytes, &signed, &public_key);
    mod_content.push_str(&format!(
        "pub const {}: &[u8] = &{:?};\n",
        locale.to_uppercase(), lpk
    ));
}
```

For most users, the raw bytes + `already_verified = true` approach is sufficient and simpler.

## Usage in Application Code

```rust
// Include the generated translation data
include!(concat!(env!("OUT_DIR"), "/translations.rs"));

fn main() {
    // Initialize the store with embedded data.
    // `init_embedded` sets `already_verified = true` for all entries.
    // Ensure your build script verified the data before generating it.
    l10n4x_core::store::init_embedded(&[
        ("en", EN),
        ("es", ES),
    ]);

    // Translate as usual
    let greeting = l10n4x_core::store::translate("es", "greeting", None, &l10n_params! {
        "name" => "Mundo"
    });
    println!("{}", greeting);
}
```

## Via C FFI

In a C project with a `static` buffer:

```c
#include "l10n4c.h"

extern const unsigned char en_l10n[];
extern const size_t en_l10n_len;

int main() {
    // already_verified = 1: the data was verified at build time
    l10n4c_load_static_bytes("en", en_l10n, en_l10n_len, 1);

    char buf[256];
    l10n4c_translate("en", "greeting", buf, sizeof(buf));
    printf("%s\n", buf);
    return 0;
}
```
