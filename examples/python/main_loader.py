#!/usr/bin/env python3
import os
import sys
import ctypes
from l10n import RELEASES_URL, Translator

# 1. Definir la firma del tipo de callback para C (FFI)
# int loader_callback(const char* locale, uint8_t** out_bytes, size_t* out_len)
LOADER_CALLBACK_TYPE = ctypes.CFUNCTYPE(
    ctypes.c_int,                                    # Retorno: 0 = OK, 1 = Error
    ctypes.c_char_p,                                 # locale
    ctypes.POINTER(ctypes.POINTER(ctypes.c_uint8)),  # puntero al buffer de salida
    ctypes.POINTER(ctypes.c_size_t)                  # puntero al tamaño de salida
)

# Mantenemos las variables vivas en memoria global para que Python no limpie el garbage collector
_loader_keepalive = None
_buffer_keepalive = {}

def examples_dir() -> str:
    return os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))

def main() -> int:
    global _loader_keepalive

    pak_dir = os.path.join(examples_dir(), "dist", "locales")
    if len(sys.argv) > 1:
        pak_dir = sys.argv[1]

    try:
        tr = Translator()
    except FileNotFoundError as exc:
        print(exc, file=sys.stderr)
        print(f"hint: extract release assets to examples/lib/ — see {RELEASES_URL}", file=sys.stderr)
        return 1

    verify_hex = os.environ.get("L10N4X_VERIFY_PUBLIC_KEY")
    if verify_hex:
        tr.set_verify_key(bytes.fromhex(verify_hex))

    tr.set_fallback_locale("en")

    # 2. Definir nuestra lógica de carga en Python
    # Este callback lee el archivo .pak desde el disco de manera perezosa (bajo demanda)
    def my_python_loader(locale_bytes: bytes, out_bytes_ptr, out_len_ptr) -> int:
        locale = locale_bytes.decode("utf-8")
        file_path = os.path.join(pak_dir, f"{locale}.pak")
        
        try:
            print(f"[Python Loader Backend] Cargando '{file_path}' de manera perezosa...")
            if not os.path.isfile(file_path):
                print(f"[Python Loader Backend] Archivo no encontrado: {file_path}", file=sys.stderr)
                return 1 # Error
                
            with open(file_path, "rb") as f:
                data = f.read()
            
            # Almacenar en caché en Python para que los bytes sigan vivos durante la copia en Rust
            _buffer_keepalive[locale] = (ctypes.c_uint8 * len(data)).from_buffer_copy(data)
            
            # Asignar punteros para que Rust FFI los lea
            out_bytes_ptr[0] = ctypes.cast(_buffer_keepalive[locale], ctypes.POINTER(ctypes.c_uint8))
            out_len_ptr[0] = len(data)
            return 0 # OK
        except Exception as e:
            print(f"[Python Loader Backend] Fallo al cargar {locale}: {e}", file=sys.stderr)
            return 1 # Error

    # 3. Registrar el callback en el FFI
    try:
        # En la implementación de FFI, usaremos: l10n4c_register_loader_backend
        _loader_keepalive = LOADER_CALLBACK_TYPE(my_python_loader)
        tr._lib.l10n4c_register_loader_backend.argtypes = [LOADER_CALLBACK_TYPE]
        tr._lib.l10n4c_register_loader_backend.restype = ctypes.c_int
        
        code = tr._lib.l10n4c_register_loader_backend(_loader_keepalive)
        if code != 0:
            raise RuntimeError(f"FFI error code: {code}")
    except AttributeError:
        print("Error: Tu librería libl10n4c no tiene soporte para 'l10n4c_register_loader_backend' todavía.", file=sys.stderr)
        print("Por favor aprueba el plan de implementación para agregar este soporte en el FFI.", file=sys.stderr)
        return 1

    # 4. Traducir en caliente
    # Nota: No llamamos a tr.load_pak_directory(). Al pedir "es", Rust llamará a nuestro callback.
    es_welcome = tr.translate("es", "common.welcome")
    en_welcome = tr.translate("en", "common.welcome")
    en_greet = tr.translate("en", "common.greet", params={"name": "Diego"})

    print(f"\nResultados:")
    print(f"[es] welcome: {es_welcome}")
    print(f"[en] welcome: {en_welcome}")
    print(f"[en] greet:   {en_greet}")

    tr.clear()
    return 0

if __name__ == "__main__":
    sys.exit(main())
