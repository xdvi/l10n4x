import ctypes
import os
import sys
from typing import Dict, Optional

RELEASES_URL = "https://github.com/xdvi/l10n4x/releases/latest"

L10N4C_OK = 0
L10N4C_KEY_NOT_FOUND = 1
L10N4C_BUFFER_TOO_SMALL = 3

_FNV1A_OFFSET: int = 0xcbf29ce484222325
_FNV1A_PRIME: int = 0x100000001b3
_MASK64: int = 0xFFFFFFFFFFFFFFFF


def fnv1a_64(data: bytes) -> int:
    """FNV-1a 64‑bit hash – matches the Rust ``l10n4x_core::binary_format::fnv1a_64``."""
    h = _FNV1A_OFFSET
    for b in data:
        h ^= b
        h = (h * _FNV1A_PRIME) & _MASK64
    return h


class _L10n4cParam(ctypes.Structure):
    """Mirrors `L10n4cParam { key: *const c_char, value: *const c_char }` in l10n4c.h."""

    _fields_ = [
        ("key", ctypes.c_char_p),
        ("value", ctypes.c_char_p),
    ]

_LIB_CANDIDATES = (
    "libl10n4c.so",
    "libl10n4c-linux.so",
    "libl10n4c.dylib",
    "libl10n4c-macos.dylib",
    "l10n4c.dll",
    "l10n4c-windows.dll",
)


def resolve_lib_path(lib_path: str | None = None) -> str:
    if lib_path:
        if not os.path.isfile(lib_path):
            raise FileNotFoundError(f"l10n4c library not found at: {lib_path}")
        return lib_path

    dirs: list[str] = []
    env_dir = os.environ.get("L10N4X_LIB_DIR")
    if env_dir:
        dirs.append(env_dir)
    script_dir = os.path.dirname(os.path.abspath(__file__))
    dirs.append(os.path.join(script_dir, "..", "lib"))

    for lib_dir in dirs:
        for name in _LIB_CANDIDATES:
            candidate = os.path.join(lib_dir, name)
            if os.path.isfile(candidate):
                return candidate

    raise FileNotFoundError(
        f"l10n4c library not found. Download a release bundle from {RELEASES_URL} "
        "and extract to examples/lib/, or set L10N4X_LIB_DIR."
    )


class Translator:
    """Python wrapper around the l10n4c C-FFI library."""

    def __init__(self, lib_path: str | None = None):
        self._lib = ctypes.CDLL(resolve_lib_path(lib_path))
        self._locale_cache: Dict[str, bytes] = {}
        self._setup_ffi()

    def _setup_ffi(self):
        self._lib.l10n4c_set_verify_key.argtypes = [
            ctypes.POINTER(ctypes.c_uint8),
            ctypes.c_size_t,
        ]
        self._lib.l10n4c_set_verify_key.restype = ctypes.c_int

        self._lib.l10n4c_set_decrypt_key.argtypes = [
            ctypes.POINTER(ctypes.c_uint8),
            ctypes.c_size_t,
        ]
        self._lib.l10n4c_set_decrypt_key.restype = ctypes.c_int

        self._lib.l10n4c_set_fallback_locale.argtypes = [ctypes.c_char_p]
        self._lib.l10n4c_set_fallback_locale.restype = ctypes.c_int

        self._lib.l10n4c_load_lpk_directory.argtypes = [ctypes.c_char_p]
        self._lib.l10n4c_load_lpk_directory.restype = ctypes.c_int

        self._lib.l10n4c_translate_required_size.argtypes = [
            ctypes.c_char_p,
            ctypes.c_uint64,
            ctypes.POINTER(ctypes.c_size_t),
        ]
        self._lib.l10n4c_translate_required_size.restype = ctypes.c_int

        self._lib.l10n4c_translate.argtypes = [
            ctypes.c_char_p,
            ctypes.c_uint64,
            ctypes.c_char_p,
            ctypes.c_size_t,
        ]
        self._lib.l10n4c_translate.restype = ctypes.c_int

        self._lib.l10n4c_translate_alloc.argtypes = [ctypes.c_char_p, ctypes.c_uint64]
        self._lib.l10n4c_translate_alloc.restype = ctypes.c_void_p

        self._lib.l10n4c_translate_with_params_alloc.argtypes = [
            ctypes.c_char_p,
            ctypes.c_uint64,
            ctypes.POINTER(_L10n4cParam),
            ctypes.c_size_t,
        ]
        self._lib.l10n4c_translate_with_params_alloc.restype = ctypes.c_void_p

        self._lib.l10n4c_free_string.argtypes = [ctypes.c_void_p]
        self._lib.l10n4c_free_string.restype = None

        self._lib.l10n4c_clear.argtypes = []
        self._lib.l10n4c_clear.restype = None

    def set_verify_key(self, verify_key: bytes):
        if len(verify_key) != 32:
            raise ValueError("Verify key must be exactly 32 bytes")
        buf = (ctypes.c_uint8 * 32).from_buffer_copy(verify_key)
        if self._lib.l10n4c_set_verify_key(buf, 32) != L10N4C_OK:
            raise RuntimeError("l10n4c: failed to set verify key")

    def set_decrypt_key(self, decrypt_key: bytes):
        if len(decrypt_key) != 32:
            raise ValueError("Decrypt key must be exactly 32 bytes")
        buf = (ctypes.c_uint8 * 32).from_buffer_copy(decrypt_key)
        if self._lib.l10n4c_set_decrypt_key(buf, 32) != L10N4C_OK:
            raise RuntimeError("l10n4c: failed to set decrypt key")

    def set_fallback_locale(self, locale: str):
        c_locale = locale.encode("utf-8")
        if self._lib.l10n4c_set_fallback_locale(c_locale) != L10N4C_OK:
            raise RuntimeError("l10n4c: failed to set fallback locale")

    def load_lpk_directory(self, dir_path: str):
        c_path = dir_path.encode("utf-8")
        if self._lib.l10n4c_load_lpk_directory(c_path) != L10N4C_OK:
            raise RuntimeError(f"l10n4c: failed to load lpk directory: {dir_path}")

    def _get_cached_locale(self, locale: str) -> bytes:
        if locale not in self._locale_cache:
            self._locale_cache[locale] = locale.encode("utf-8")
        return self._locale_cache[locale]

    def translate(
        self,
        locale: str,
        key: str,
        params: Optional[Dict[str, str]] = None,
    ) -> str:
        """Translate *key* for *locale*, optionally interpolating *params*."""
        c_locale = self._get_cached_locale(locale)
        key_hash: int = fnv1a_64(key.encode("utf-8"))

        if params:
            arr = (_L10n4cParam * len(params))()
            keep_alive = []
            for i, (k, v) in enumerate(params.items()):
                kb = k.encode("utf-8")
                vb = v.encode("utf-8")
                keep_alive.extend([kb, vb])
                arr[i].key = kb
                arr[i].value = vb
            ptr = self._lib.l10n4c_translate_with_params_alloc(
                c_locale, ctypes.c_uint64(key_hash), arr, len(params)
            )
        else:
            ptr = self._lib.l10n4c_translate_alloc(c_locale, ctypes.c_uint64(key_hash))

        if not ptr:
            return key

        try:
            val = ctypes.cast(ptr, ctypes.c_char_p).value
            return val.decode("utf-8") if val is not None else key
        finally:
            self._lib.l10n4c_free_string(ptr)

    def translate_buffered(self, locale: str, key: str) -> str:
        c_locale = self._get_cached_locale(locale)
        key_hash: int = fnv1a_64(key.encode("utf-8"))

        out_size = ctypes.c_size_t(0)
        code = self._lib.l10n4c_translate_required_size(
            c_locale, ctypes.c_uint64(key_hash), ctypes.byref(out_size)
        )
        if code != L10N4C_OK and code != L10N4C_KEY_NOT_FOUND:
            return key

        size = out_size.value
        buf = ctypes.create_string_buffer(size)
        self._lib.l10n4c_translate(c_locale, ctypes.c_uint64(key_hash), buf, size)
        return buf.raw[: size - 1].decode("utf-8")

    def clear(self):
        self._lib.l10n4c_clear()