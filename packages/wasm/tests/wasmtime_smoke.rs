//! CI smoke test: build `l10n4x.wasm` and invoke an export under wasmtime.

use std::path::PathBuf;
use std::process::Command;

use wasmtime::*;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn ensure_wasm_built() -> PathBuf {
    let root = workspace_root();
    let wasm = root.join("target/wasm32-unknown-unknown/debug/l10n4x.wasm");
    if !wasm.exists() {
        let status = Command::new("cargo")
            .args([
                "build",
                "--target",
                "wasm32-unknown-unknown",
                "-p",
                "l10n4x-wasm",
            ])
            .current_dir(&root)
            .status()
            .expect("failed to spawn cargo build for wasm");
        assert!(status.success(), "l10n4x-wasm build failed");
    }
    wasm
}

fn link_import_stubs(linker: &mut Linker<()>, module: &Module) -> Result<()> {
    for import in module.imports() {
        if let ExternType::Func(ty) = import.ty() {
            let module_name = import.module().to_string();
            let name = import.name().to_string();
            let func_ty = ty.clone();
            linker.func_new(
                &module_name,
                &name,
                ty,
                move |caller: Caller<'_, ()>, params, results| {
                    for (i, result) in results.iter_mut().enumerate() {
                        match func_ty.results().nth(i) {
                            Some(ValType::I32) => *result = Val::I32(0),
                            Some(ValType::I64) => *result = Val::I64(0),
                            Some(ValType::F32) => *result = Val::F32(0.0_f32.to_bits()),
                            Some(ValType::F64) => *result = Val::F64(0.0_f64.to_bits()),
                            _ => {}
                        }
                    }
                    let _ = (caller, params);
                    Ok(())
                },
            )?;
        }
    }
    Ok(())
}

#[test]
fn wasmtime_load_and_call_clear() {
    let wasm_path = ensure_wasm_built();
    let engine = Engine::default();
    let module = Module::from_file(&engine, &wasm_path).expect("parse l10n4x.wasm");

    for export in [
        "l10n4x_clear",
        "l10n4x_load_namespace_bytes",
        "l10n4x_ota_reload_lpk",
        "l10n4x_ota_rollback",
        "l10n4x_ota_can_rollback",
    ] {
        let found = module.exports().any(|e| e.name() == export);
        assert!(found, "{export} export missing from wasm module");
    }

    let mut linker = Linker::new(&engine);
    link_import_stubs(&mut linker, &module).expect("link import stubs");

    let mut store = Store::new(&engine, ());
    let instance = linker
        .instantiate(&mut store, &module)
        .expect("instantiate l10n4x.wasm");

    let clear = instance
        .get_typed_func::<(), ()>(&mut store, "l10n4x_clear")
        .expect("typed l10n4x_clear");
    clear.call(&mut store, ()).expect("call l10n4x_clear");
}
