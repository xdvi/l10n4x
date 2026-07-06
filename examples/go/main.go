package main

import (
	"fmt"
	"os"
	"path/filepath"
)

const releasesURL = "https://github.com/xdvi/l10n4x/releases/latest"

func main() {
	lpkDir := filepath.Join("..", "dist", "locales")
	if len(os.Args) > 1 {
		lpkDir = os.Args[1]
	}

	// Prerequisites:
	//   1. Download the platform bundle from GitHub Releases and extract to examples/lib/
	//      Linux:   l10n4x-linux-amd64.tar.gz   → lib/libl10n4c.so, lib/l10n4x, lib/l10n4c.h
	//      macOS:   l10n4x-macos-universal.tar.gz
	//      Windows: l10n4x-windows-amd64.zip
	//   2. l10n4x build  (CLI from the same release bundle)
	//   3. export L10N4X_VERIFY_PUBLIC_KEY=<verifyPublicKey from l10n4x.config.json>
	//   4. (optional) export L10N4X_ENCRYPT_KEY=<32-byte key> when encrypt: true

	tr, err := NewTranslator("es", lpkDir)
	if err != nil {
		fmt.Fprintf(os.Stderr, "hint: extract release assets to examples/lib/ — see %s\n", releasesURL)
		panic(err)
	}
	defer tr.Close()

	fmt.Println(tr.Translate("es", "common.welcome"))
	fmt.Println(tr.Translate("en", "common.welcome"))
}