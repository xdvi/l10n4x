package main

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func pakDir() string {
	if d := os.Getenv("L10N4X_PAK_DIR"); d != "" {
		return d
	}
	return filepath.Join("..", "dist", "locales")
}

func skipIfNoKey(t *testing.T) {
	t.Helper()
	if os.Getenv("L10N4X_VERIFY_PUBLIC_KEY") == "" {
		t.Skip("L10N4X_VERIFY_PUBLIC_KEY not set")
	}
}

func TestSpanishWelcome(t *testing.T) {
	skipIfNoKey(t)
	tr, err := NewTranslator("es", pakDir())
	if err != nil {
		t.Fatal(err)
	}
	defer tr.Close()
	result := tr.Translate("es", "common.welcome")
	if !strings.Contains(result, "Bienvenido") {
		t.Fatalf("expected Bienvenido in result, got: %s", result)
	}
}

func TestEnglishWelcome(t *testing.T) {
	skipIfNoKey(t)
	tr, err := NewTranslator("es", pakDir())
	if err != nil {
		t.Fatal(err)
	}
	defer tr.Close()
	result := tr.Translate("en", "common.welcome")
	if !strings.Contains(result, "Welcome") {
		t.Fatalf("expected Welcome in result, got: %s", result)
	}
}

func TestFallbackToSpanish(t *testing.T) {
	skipIfNoKey(t)
	tr, err := NewTranslator("es", pakDir())
	if err != nil {
		t.Fatal(err)
	}
	defer tr.Close()
	result := tr.Translate("xx", "common.welcome")
	if !strings.Contains(result, "Bienvenido") {
		t.Fatalf("expected fallback ¡Bienvenido! in result, got: %s", result)
	}
}

func TestMissingKeyReturnsRaw(t *testing.T) {
	skipIfNoKey(t)
	tr, err := NewTranslator("es", pakDir())
	if err != nil {
		t.Fatal(err)
	}
	defer tr.Close()
	result := tr.Translate("en", "nonexistent.key")
	if !strings.HasPrefix(result, "0x") {
		t.Fatalf("expected hex key-hash fallback, got: %s", result)
	}
}
