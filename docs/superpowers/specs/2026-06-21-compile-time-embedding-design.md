# Compile-Time Embedding of Translations (L10N Static Data)

**Status:** Design Draft
**Date:** 2026-06-21

## 1. Goal

Permitir incrustar traducciones compiladas directamente en el binario en tiempo de compilación, eliminando la necesidad de archivos `.pak` externos en runtime. El `BinaryFormatReader` ya trabaja sobre `&[u8]` y no necesita cambios.

## 2. Estrategia

Agregar un nuevo tipo `StoreData` que puede ser `Owned(Arc<Vec<u8>>)` (runtime-loaded, como hoy) o `Static(&'static [u8])` (compile-time embedded, con flag de verificación). Mantener la API existente intacta.

## 3. Cambios en `core/src/store.rs`

### Nuevo tipo:

```rust
#[derive(Clone)]
pub enum StoreData {
    Owned(Arc<Vec<u8>>),
    Static(&'static [u8]),
}

impl StoreData {
    pub fn as_slice(&self) -> &[u8] {
        match self {
            StoreData::Owned(v) => v.as_slice(),
            StoreData::Static(s) => s,
        }
    }

    /// Indica si estos datos ya fueron verificados criptográficamente.
    /// `Static` siempre devuelve `true` (la verificación ocurrió en build time).
    /// `Owned` devuelve `false` (la verificación ocurre en runtime si está configurada).
    pub fn is_verified(&self) -> bool {
        matches!(self, StoreData::Static(_))
    }

    /// Indica si los datos son estáticos (embedidos en el binario).
    pub fn is_static(&self) -> bool {
        matches!(self, StoreData::Static(_))
    }
}
```

### `TranslationStore`:

```rust
pub struct TranslationStore {
    pub locales: Arc<Vec<(String, StoreData)>>,
    pub fallback_chain: Arc<[Arc<str>]>,
}

impl TranslationStore {
    pub fn lookup(&self, locale: &str) -> Option<&[u8]> {
        let idx = self.locales.binary_search_by(|(loc, _)| loc.as_str().cmp(locale)).ok()?;
        Some(self.locales[idx].1.as_slice())
    }
}
```

### `load_raw_bytes` actualizada:

```rust
pub fn load_raw_bytes(locale_str: &str, bytes: &[u8]) -> bool {
    // misma lógica, pero entry = (locale_str.to_string(), StoreData::Owned(Arc::new(bytes.to_vec())))
}
```

### Nueva función `load_static_bytes`:

```rust
/// Carga un buffer L10N estático (embedido en el binario) en el store global.
///
/// `already_verified`: debe ser `true` si los datos ya fueron verificados
/// criptográficamente en tiempo de compilación. Si es `true`, no se repite
/// la verificación en runtime. Si es `false`, se trata como no verificado
/// (comportamiento conservador).
pub fn load_static_bytes(locale_str: &str, data: &'static [u8], already_verified: bool) -> bool {
    crate::metrics::inc_locale_loads();
    let (mut new_vec, fallback_chain) = read_store(|store| {
        ((*store.locales).clone(), alloc::sync::Arc::clone(&store.fallback_chain))
    });
    let entry = (locale_str.to_string(), StoreData::Static(data));
    match new_vec.binary_search_by(|(loc, _)| loc.as_str().cmp(locale_str)) {
        Ok(pos) => new_vec[pos] = entry,
        Err(pos) => new_vec.insert(pos, entry),
    }
    swap_store(TranslationStore {
        locales: Arc::new(new_vec),
        fallback_chain,
    });
    emit_locale_changed(locale_str);
    true
}
```

Nota: `load_static_bytes` no aloca memoria para el buffer de datos. Solo aloca el `String` del locale y el `Vec` interno del store.

## 4. Cambios en `core/src/loader.rs`

Agregar `load_static_bytes` como wrapper que delega en `store::load_static_bytes`.

## 4b. Manejo de firma en embed estático (nueva sección)

| Etapa | Acción |
|-------|--------|
| **Build time** | El build script debe ejecutar el compilador de l10n4x para generar los bytes L10N. La verificación de firma Ed25519 ocurre durante la compilación. Si falla, el build falla. |
| **Runtime (Static)** | Datos embedidos con `already_verified=true`. Nunca se repite verificación de firma. Los bytes se usan directamente. |
| **Runtime (Owned)** | Datos cargados por `load_raw_bytes` o `load_pak_bytes`. Se verifica firma en runtime si `integrity::set_verify_key` fue configurado (comportamiento actual). |

## 5. Build-time compilation (Opción A simplificada)

El `build.rs` del usuario usa la API pública del compilador de l10n4x (`l10n4x_compiler`) directamente, sin invocar un binario externo:

```rust
// build.rs
use std::path::Path;
use std::env;

fn main() {
    let src = Path::new("locales");   // directorio con JSON por locale
    let out = Path::new(&env::var("OUT_DIR").unwrap());

    // 1. Compilar los JSON a HashMap<String, Vec<MessageNode>>
    let translations = l10n4x_compiler::compile_translations_to_map(src)
        .expect("Failed to compile translations");

    // 2. Serializar cada locale a binary L10N
    for (locale, nodes) in &translations {
        let bytes = l10n4x_compiler::binary_writer::write_binary_format(nodes);
        // 3. Opcional: comprimir y firmar

        // 4. Escribir un archivo .rs con los bytes embedidos
        let dest = out.join(format!("l10n_{}.rs", locale));
        std::fs::write(&dest, format!(
            "pub const {}: &[u8] = &{:?};",
            locale.to_uppercase(),
            bytes
        )).unwrap();
    }

    // 5. Generar módulo que include! todos los locales
    let mod_file = locales.iter().map(|(locale, _)| {
        format!("pub mod {} {{ include!(concat!(env!(\"OUT_DIR\"), \"/l10n_{}.rs\")); }}", locale, locale)
    }).collect::<Vec<_>>().join("\n");
    std::fs::write(out.join("translations.rs"), mod_file).unwrap();
}
```

Uso en el crate del usuario:

```rust
// src/main.rs
include!(concat!(env!("OUT_DIR"), "/translations.rs"));

fn main() {
    l10n4x_core::store::init_embedded(&[
        ("es", es::ES),
        ("en", en::EN),
    ]);

    let saludo = l10n4x_core::store::translate("es", "greeting", None, &[]);
    println!("{}", saludo);
}
```

Esta Opción A puede usarse tanto para embedir datos comprimidos (.pak, descomprimiendo en init) como para embedir datos ya procesados (L10N raw, cero overhead). Es decisión del build.rs qué formato generar.

## 6. `init_embedded`

```rust
pub fn init_embedded(locales: &[(&str, &'static [u8])]) {
    for (locale, data) in locales {
        load_static_bytes(locale, data, true);
    }
}
```

## 7. FFI

1. `l10n4c_load_static_bytes(locale: *const c_char, data: *const u8, len: usize, verified: bool) -> i32` — nueva función C que recibe puntero + longitud sin tomar ownership.
2. La FFI existente (`l10n4c_load_pak_locale`, `l10n4c_load_pak_directory`) sigue funcionando sin cambios.
3. Para datos estáticos embedidos, el usuario debe llamar `l10n4c_load_static_bytes` antes de cualquier llamada a translate/init. Esto permite pasar data desde secciones `.rodata` del binario C.

## 8. WASM

Similar: el binding WASM ya acepta `Uint8Array` vía `l10n4x_load_pak_bytes`. Para embedding estático, se puede agregar una función que reciba `&'static [u8]` directamente.

## 9. no_std

`StoreData::Static(&'static [u8])` es compatible con `no_std + alloc` (solo requiere `core` + `alloc` para el `Vec` del store y el `String` del locale).

`StoreData::Owned(Arc<Vec<u8>>)` requiere `alloc` (para `Arc` y `Vec`).

Ambos variantes son compatibles con `no_std` (el crate ya tiene `#![no_std]` condicional).

## 10. Pruebas

- Test de `StoreData::as_slice()` con ambas variantes.
- Test de `StoreData::is_verified()`: `Static` → `true`, `Owned` → `false`.
- Test de `load_static_bytes` + `translate` con un locale embedido.
- Verificar que `load_static_bytes` con `already_verified=true` no ejecuta verificación de firma en runtime.
- Confirmar que `load_raw_bytes` (runtime) y `load_static_bytes` pueden coexistir en el mismo `TranslationStore` sin corrupción de datos.
- Test de `init_embedded` con múltiples locales.

## 11. Archivos a modificar

| Archivo | Cambio |
|---------|--------|
| `packages/core/src/store.rs` | Agregar `StoreData`, actualizar `TranslationStore`, `load_raw_bytes`, agregar `load_static_bytes`, `init_embedded` |
| `packages/core/src/loader.rs` | Agregar `load_static_bytes` (wrapper) |
| `packages/core/src/lib.rs` | Exportar `StoreData`, `init_embedded`, `load_static_bytes` |
| `packages/compiler/src/lib.rs` | Exponer API pública estable para uso desde build.rs (`crate-type = ["lib", "bin"]` ya debería estar) |
| `packages/ffi/src/lib.rs` | Agregar `l10n4c_load_static_bytes` |

## 12. Herramienta recomendada para usuarios (opcional)

En el futuro, se puede crear un crate `l10n4x_build` que simplifique el build.rs:

```rust
// Cargo.toml (dev-dependency)
[build-dependencies]
l10n4x_build = "0.1"

// build.rs
l10n4x_build::embed("locales", "translations").unwrap();
```

Esto es opcional y puede implementarse después del diseño base. La Opción A (build.rs manual con la librería del compilador) cubre todos los casos de uso.
