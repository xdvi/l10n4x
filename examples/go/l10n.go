package main

/*
#cgo CFLAGS: -I${SRCDIR}/../lib -I${SRCDIR}/../../packages/ffi
#cgo linux LDFLAGS: -L${SRCDIR}/../lib -Wl,-rpath,${SRCDIR}/../lib -ll10n4c -ldl -lpthread
#cgo darwin LDFLAGS: -L${SRCDIR}/../lib -Wl,-rpath,${SRCDIR}/../lib -ll10n4c -lpthread
#cgo windows LDFLAGS: -L${SRCDIR}/../lib -ll10n4c
#include <stdlib.h>
#include <stdint.h>
#include "l10n4c.h"
*/
import "C"
import (
	"encoding/hex"
	"fmt"
	"os"
	"sync"
	"unsafe"
)

// Translator wraps the FFI library and caches locale C-strings to avoid cgo overhead.
type Translator struct {
	fallback    string
	pakDir      string
	localeCache map[string]*C.char
	mu          sync.RWMutex
}

func installRuntimeKeys() error {
	verifyHex := os.Getenv("L10N4X_VERIFY_PUBLIC_KEY")
	if verifyHex == "" {
		return fmt.Errorf("L10N4X_VERIFY_PUBLIC_KEY is not set (64-char hex from l10n4x.config.json verifyPublicKey)")
	}
	verifyBytes, err := hex.DecodeString(verifyHex)
	if err != nil || len(verifyBytes) != 32 {
		return fmt.Errorf("L10N4X_VERIFY_PUBLIC_KEY must be 64 hex characters (32 bytes)")
	}
	if C.l10n4c_set_verify_key((*C.uchar)(&verifyBytes[0]), C.size_t(32)) != C.L10N4C_OK {
		return fmt.Errorf("l10n4c: invalid verify public key")
	}

	if encRaw := os.Getenv("L10N4X_ENCRYPT_KEY"); encRaw != "" {
		if len(encRaw) != 32 {
			return fmt.Errorf("L10N4X_ENCRYPT_KEY must be exactly 32 bytes when set")
		}
		encKey := []byte(encRaw)
		if C.l10n4c_set_decrypt_key((*C.uchar)(&encKey[0]), C.size_t(32)) != C.L10N4C_OK {
			return fmt.Errorf("l10n4c: invalid decrypt key")
		}
	}

	return nil
}

// NewTranslator loads all .pak files from pakDir.
// Requires L10N4X_VERIFY_PUBLIC_KEY; set L10N4X_ENCRYPT_KEY only when encrypt is enabled in config.
func NewTranslator(fallbackLocale, pakDir string) (*Translator, error) {
	if err := installRuntimeKeys(); err != nil {
		return nil, err
	}

	tr := &Translator{
		fallback:    fallbackLocale,
		pakDir:      pakDir,
		localeCache: make(map[string]*C.char),
	}

	cFallback := C.CString(fallbackLocale)
	defer C.free(unsafe.Pointer(cFallback))
	if C.l10n4c_set_fallback_locale(cFallback) != C.L10N4C_OK {
		return nil, fmt.Errorf("l10n4c: failed to set fallback locale")
	}

	cDir := C.CString(pakDir)
	defer C.free(unsafe.Pointer(cDir))
	if C.l10n4c_load_pak_directory(cDir) != C.L10N4C_OK {
		return nil, fmt.Errorf("l10n4c: failed to load pak directory: %s", pakDir)
	}

	return tr, nil
}

func (t *Translator) getLocaleCString(locale string) *C.char {
	t.mu.RLock()
	if c, ok := t.localeCache[locale]; ok {
		t.mu.RUnlock()
		return c
	}
	t.mu.RUnlock()

	cLocale := C.CString(locale)
	t.mu.Lock()
	defer t.mu.Unlock()
	if existing, ok := t.localeCache[locale]; ok {
		C.free(unsafe.Pointer(cLocale))
		return existing
	}
	t.localeCache[locale] = cLocale
	return cLocale
}

// Translate resolves a key for the given locale using the alloc API.
func (t *Translator) Translate(locale, key string) string {
	cLocale := t.getLocaleCString(locale)
	cKey := C.CString(key)
	defer C.free(unsafe.Pointer(cKey))

	ptr := C.l10n4c_translate_alloc(cLocale, cKey)
	if ptr == nil {
		return key
	}
	defer C.l10n4c_free_string(ptr)
	return C.GoString(ptr)
}

// Close releases cached C strings and clears loaded translations.
func (t *Translator) Close() {
	t.mu.Lock()
	defer t.mu.Unlock()
	for _, c := range t.localeCache {
		C.free(unsafe.Pointer(c))
	}
	t.localeCache = make(map[string]*C.char)
	C.l10n4c_clear()
}