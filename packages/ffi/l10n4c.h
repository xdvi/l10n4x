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
#define L10N4C_DECRYPT_KEY_NOT_SET 11 /* Call l10n4c_set_decrypt_key first (L10E)    */
#define L10N4C_BUFFER_OVERFLOW     12 /* Operation resulted in buffer overflow        */

/* ── Types ────────────────────────────────────────────────────────────────── */

typedef struct {
    const char *key;
    const char *value;
} L10n4cParam;

/* ── Configuration ────────────────────────────────────────────────────────── */

int32_t l10n4c_set_verify_key(const uint8_t *key, size_t key_len);
int32_t l10n4c_set_decrypt_key(const uint8_t *key, size_t key_len);
int32_t l10n4c_set_fallback_locale(const char *locale);

/* ── Loading (runtime only — compile with `l10n4x build` CLI) ─────────── */

int32_t l10n4c_load_pak_locale(const char *locale, const char *file_path);
int32_t l10n4c_load_pak_directory(const char *dir_path);
void    l10n4c_clear(void);

/* ── Translation (buffer-based) ───────────────────────────────────────────── */

int32_t l10n4c_translate_required_size(const char *locale, const char *key, size_t *out_size);
int32_t l10n4c_translate(const char *locale, const char *key, uint8_t *buf, size_t max_len);

int32_t l10n4c_translate_with_params_required_size(
    const char *locale, const char *key, const L10n4cParam *params, size_t param_count,
    size_t *out_size);
int32_t l10n4c_translate_with_params(
    const char *locale, const char *key, const L10n4cParam *params, size_t param_count,
    uint8_t *buf, size_t max_len);

/* ── Translation (alloc-based — caller must free with l10n4c_free_string) ─ */

char *l10n4c_translate_alloc(const char *locale, const char *key);
char *l10n4c_translate_with_params_alloc(
    const char *locale, const char *key, const L10n4cParam *params, size_t param_count);
void  l10n4c_free_string(char *ptr);

#ifdef __cplusplus
}
#endif

#endif /* L10N4C_H */