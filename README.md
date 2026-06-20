# l10n4x

[![Language](https://img.shields.io/badge/language-Rust-orange.svg)](https://www.rust-lang.org/)
[![C-FFI](https://img.shields.io/badge/C--FFI-Go%20%7C%20C%2B%2B%20%7C%20C%23%20%7C%20Dart-blue.svg)](#)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](#)

> *Modern, Dynamic, and Type-Safe Localization (l10n) Engine and Toolkit in Rust.*

`l10n4x` is a unified, full-lifecycle internationalization and localization (i18n/l10n) workspace. It compiles translation bundles into encrypted `.pak` files, loads them dynamically in any runtime with sub-nanosecond lookups, and automatically generates **type-safe bindings** for client targets (Go, TypeScript/React, C/C++, Python, Flutter/Dart). It supports a high-performance subset of ICU MessageFormat-style messages (plurals, selects, variables) compiled to optimized bytecode.

---

## Workspace Structure

The project is organized as a Cargo virtual workspace:

```text
l10n4x/
├── Cargo.toml (Workspace Root configuration)
└── packages/
    ├── core/ (l10n4x-core)
    │   └── src/lib.rs (in-memory store, AES-GCM decryption, JSON parser)
    ├── compiler/ (l10n4x-compiler)
    │   └── src/lib.rs (key merging, flattening, and pak compilation)
    ├── ffi/ (l10n4c)
    │   └── src/lib.rs (public C FFI wrappers exporting l10n4c_*)
    ├── cli/ (l10n4x-toolkit)
    │   └── src/main.rs (CLI toolkit with dev server, watcher, and generators)
    └── wasm/ (l10n4x-wasm)
        └── src/lib.rs (WebAssembly bindings exposing JS-compatible API)
```

---

## The Developer Toolkit: `l10n4x-toolkit`

The `l10n4x` binary (toolkit) acts as a **development toolchain** connecting your translation JSON files with consumer target codebases.

### 1. Installation
To build the toolkit binary:
```bash
cargo build --release --bin l10n4x
```
The compiled CLI will be located at `target/release/l10n4x`.

### 2. Configuration: `l10n4x.config.json`
Define your directories, fallback locale, target languages, and code generation directories:
```json
{
  "project": "my_project",
  "sourceDir": "./locales",
  "outputDir": "./dist/locales",
  "keyEnv": "L10N4X_KEY",
  "fallback": "en",
  "targets": [
    {
      "type": "go",
      "outDir": "./backend/pkg/i18n"
    },
    {
      "type": "typescript",
      "outDir": "./frontend/src/i18n"
    }
  ]
}
```

### 3. CLI Commands

| Command | Description |
|---------|-------------|
| `l10n4x init` | Interactive wizard that detects project type and generates initial `l10n4x.config.json`. |
| `l10n4x validate` | Validates key consistency across all target locales. |
| `l10n4x build` | Validates keys, compiles encrypted `.pak` files, and generates type-safe code bindings. |
| `l10n4x dev` | Starts local dev server (Axum) with file watch triggers (Notify) for live hot-reloads. |

### 4. JSON Structure & Array Flattening
Nested JSON localization files are flattened automatically using dot-notated namespaces. JSON arrays are also flattened using their 0-indexed position as the key subscript:
```json
{
  "menu": {
    "items": ["Home", "Settings"]
  }
}
```
Flattens to the following translation lookup keys:
- `menu.items.0` -> `Home`
- `menu.items.1` -> `Settings`

---

## C-FFI Compatibility Layer: `l10n4c`

The `l10n4c` package (`packages/ffi`) compiles to C-compatible static (`.a`) and dynamic (`.so`) libraries, exposing the following API:

### API Reference
* `l10n4c_set_encryption_key(key: *const u8, key_len: usize) -> bool`  
  Configures the 32-byte key used for AES-GCM decryption/encryption.
* `l10n4c_set_fallback_locale(locale: *const c_char) -> bool`  
  Sets the global fallback language (defaults to `"en"`).
* `l10n4c_compile(src_dir: *const c_char, out_dir: *const c_char) -> bool`  
  Compiles directories of raw JSON files into GCM-encrypted `.pak` files.
* `l10n4c_load_pak_directory(dir_path: *const c_char) -> bool`  
  Scans a directory and automatically decrypts/loads all `.pak` files in memory.
* `l10n4c_load_pak_locale(locale: *const c_char, file_path: *const c_char) -> bool`  
  Decrypts and loads a single `.pak` file for a locale.
* `l10n4c_load_locale(locale: *const c_char, json: *const c_char, prefix: *const c_char) -> bool`  
  Loads a raw, unencrypted JSON string into memory.
* `l10n4c_translate(locale: *const c_char, key: *const c_char) -> *mut c_char`  
  Resolves a key in memory. Falls back to default locale if missing. Returns allocated C string.
* `l10n4c_free_string(ptr: *mut c_char)`  
  Safely drops the C string allocation returned by `l10n4c_translate`.
* `l10n4c_clear()`  
  Clears all loaded translations.

---

## Go CGO Integration Example

Below is a complete implementation showing how to load the static archive (`libl10n4c.a`) and perform lookups in Go:

```go
package main

/*
#cgo LDFLAGS: -L${SRCDIR}/path/to/release -ll10n4c -ldl -lpthread
#include <stdlib.h>
#include <stdbool.h>

bool l10n4c_set_encryption_key(const unsigned char* key, size_t key_len);
bool l10n4c_set_fallback_locale(const char* locale);
bool l10n4c_load_pak_directory(const char* dir_path);
char* l10n4c_translate(const char* locale, const char* key);
void l10n4c_free_string(char* ptr);
*/
import "C"
import (
	"fmt"
	"unsafe"
)

func main() {
	// 1. Set the 32-byte encryption key
	key := []byte("polyglot-default-key-32-bytes!!!")
	C.l10n4c_set_encryption_key((*C.uchar)(&key[0]), C.size_t(len(key)))

	// 2. Set default fallback locale
	cFallback := C.CString("es")
	defer C.free(unsafe.Pointer(cFallback))
	C.l10n4c_set_fallback_locale(cFallback)

	// 3. Load the encrypted pak files from the directory
	cDir := C.CString("./dist/locales")
	defer C.free(unsafe.Pointer(cDir))
	if !C.l10n4c_load_pak_directory(cDir) {
		panic("Failed to load translation paks")
	}

	// 4. Perform translations
	cLocale := C.CString("es")
	cKey := C.CString("common.welcome")
	defer C.free(unsafe.Pointer(cLocale))
	defer C.free(unsafe.Pointer(cKey))

	cRes := C.l10n4c_translate(cLocale, cKey)
	if cRes != nil {
		defer C.l10n4c_free_string(cRes) // CRITICAL: Free memory
		translation := C.GoString(cRes)
		fmt.Printf("Translated: %s\n", translation)
	}
}
```

---

## License

This project is licensed under the MIT License.
