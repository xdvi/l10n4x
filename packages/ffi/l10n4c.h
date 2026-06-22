#ifndef L10N4C_H
#define L10N4C_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ── Status codes ─────────────────────────────────────────────────────────── */

#define L10N4C_OK                  0  /* Success                                    */
#define L10N4C_KEY_NOT_FOUND       1  /* Key missing in requested + fallback locale  */
#define L10N4C_LOCALE_NOT_LOADED   2  /* Locale not loaded — call load_pak_* first   */
#define L10N4C_BUFFER_TOO_SMALL    3  /* Buffer too small — call _required_size      */
#define L10N4C_INVALID_PARAMS      4  /* Null pointer or invalid UTF-8               */
#define L10N4C_INTERNAL_ERROR      5  /* Unexpected internal error                   */
#define L10N4C_INVALID_ENCODING    6  /* Parameter contains invalid UTF-8 encoding   */
#define L10N4C_IO_ERROR            7  /* File/directory I/O failure                  */
#define L10N4C_SIGNATURE_INVALID   8  /* Ed25519 signature mismatch (tampered pak)   */
#define L10N4C_VERIFY_KEY_NOT_SET  9  /* Call l10n4c_set_verify_key first            */
#define L10N4C_NOT_INITIALIZED    10  /* Call l10n4c_load_pak_directory first        */
#define L10N4C_DECRYPT_KEY_NOT_SET 11 /* Call l10n4c_set_decrypt_key first (L10E)    */
#define L10N4C_BUFFER_OVERFLOW     12 /* Operation resulted in buffer overflow        */
#define L10N4C_RUNTIME_TOO_OLD      13 /* Pak requires a newer runtime than this build  */

/* ── Types ────────────────────────────────────────────────────────────────── */

typedef struct {
    const char *key;
    const char *value;
} L10n4cParam;

/* ── Configuration ────────────────────────────────────────────────────────── */

int32_t l10n4c_set_verify_key(const uint8_t *key, size_t key_len);
int32_t l10n4c_set_decrypt_key(const uint8_t *key, size_t key_len);
int32_t l10n4c_set_fallback_locale(const char *locale);

/**
 * Sets the ordered fallback locale chain (first match wins).
 * @param locales  Array of null-terminated locale strings.
 * @param count    Number of entries in the array (capped at 16).
 * @return L10N4C_OK or L10N4C_INVALID_PARAMS.
 */
int32_t l10n4c_set_fallback_chain(const char **locales, size_t count);

/* ── Loading (runtime only — compile with `l10n4x build` CLI) ─────────── */

int32_t l10n4c_load_pak_locale(const char *locale, const char *file_path);
int32_t l10n4c_load_namespace(const char *locale, const char *namespace,
                                const char *file_path);
int32_t l10n4c_init_modular(const char *base_dir, const char *locale);
int32_t l10n4c_load_pak_directory(const char *dir_path);
int32_t l10n4c_load_static_bytes(const char *locale, const uint8_t *data,
                                   size_t data_len, int32_t already_verified);
void    l10n4c_clear(void);

/* ── Translation (buffer-based) ───────────────────────────────────────────── */

int32_t l10n4c_translate_required_size(const char *locale, uint64_t key_hash, size_t *out_size);
int32_t l10n4c_translate(const char *locale, uint64_t key_hash, uint8_t *buf, size_t max_len);

int32_t l10n4c_translate_with_params_required_size(
    const char *locale, uint64_t key_hash, const L10n4cParam *params, size_t param_count,
    size_t *out_size);
int32_t l10n4c_translate_with_params(
    const char *locale, uint64_t key_hash, const L10n4cParam *params, size_t param_count,
    uint8_t *buf, size_t max_len);

/* ── Translation (alloc-based — caller must free with l10n4c_free_string) ─ */

char *l10n4c_translate_alloc(const char *locale, uint64_t key_hash);
char *l10n4c_translate_with_params_alloc(
    const char *locale, uint64_t key_hash, const L10n4cParam *params, size_t param_count);

int32_t l10n4c_translate_with_context_required_size(
    const char *locale, uint64_t key_hash, uint64_t context_hash, size_t *out_size);
int32_t l10n4c_translate_with_context(
    const char *locale, uint64_t key_hash, uint64_t context_hash,
    uint8_t *buf, size_t max_len);
char *l10n4c_translate_with_context_alloc(
    const char *locale, uint64_t key_hash, uint64_t context_hash);

int32_t l10n4c_translate_with_context_and_params_required_size(
    const char *locale, uint64_t key_hash, uint64_t context_hash,
    const L10n4cParam *params, size_t param_count, size_t *out_size);
int32_t l10n4c_translate_with_context_and_params(
    const char *locale, uint64_t key_hash, uint64_t context_hash,
    const L10n4cParam *params, size_t param_count, uint8_t *buf, size_t max_len);
char *l10n4c_translate_with_context_and_params_alloc(
    const char *locale, uint64_t key_hash, uint64_t context_hash,
    const L10n4cParam *params, size_t param_count);

void  l10n4c_free_string(char *ptr);

/* ── Custom Formatters ─────────────────────────────────────────────────────── */

/** Custom formatter function type: takes value, locale, and options, returns allocated string. */
typedef char* (*l10n4c_custom_formatter_fn)(const char *value, const char *locale, const char *options);

/**
 * Registers a custom formatter with the given name.
 * The formatter is called for ICU message syntax like `{var, formatterName}`.
 * @param name  Formatter name (e.g. "uppercase", "rot13")
 * @param formatter  Function pointer, or NULL to unregister.
 * @return L10N4C_OK or error code.
 */
int32_t l10n4c_register_formatter(const char *name, l10n4c_custom_formatter_fn formatter);

/* ── Callbacks ─────────────────────────────────────────────────────────────── */

/** Callback type for missing translation key events. */
typedef void (*l10n4c_missing_key_fn)(const char *locale, uint64_t key_hash);

/**
 * Registers a callback invoked when a key is not found in any locale or fallback.
 * Pass NULL to unregister. Thread-safe for concurrent translate calls.
 */
void l10n4c_set_missing_key_handler(l10n4c_missing_key_fn handler);

/**
 * Writes comma-separated loaded locale codes into out_buf (up to out_len bytes).
 * Returns the number of bytes written (excluding null terminator),
 * or L10N4C_BUFFER_TOO_SMALL if the buffer is too small.
 */
int32_t l10n4c_get_loaded_locales(uint8_t *out_buf, size_t out_len);

/**
 * Returns comma-separated metrics counters: total translations, cache hits,
 * cache misses, locale loads, format errors — as a UTF-8 string.
 * Returns the number of bytes written, or L10N4C_BUFFER_TOO_SMALL.
 */
int32_t l10n4c_get_metrics(uint8_t *out_buf, size_t out_len);

/* ── Version ─────────────────────────────────────────────────────────────────── */

/**
 * Returns the library version string (e.g. "0.2.0").
 * The returned string is owned by the caller and must be freed with l10n4c_free_string.
 */
char *l10n4c_get_version(void);

#ifdef __cplusplus
}
#endif

#endif /* L10N4C_H */