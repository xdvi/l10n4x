const wasm = require('./pkg/l10n4x.js');

try {
    // Attempt loading empty bytes, which should fail decompression
    wasm.l10n4x_load_lpk_bytes(new Uint8Array(0), "es");
    console.error("FAIL: Expected exception was not thrown!");
    process.exit(1);
} catch (err) {
    const errMsg = err.message || err;
    console.log("OK: Caught expected exception:", errMsg);
    if (errMsg.includes("Invalid format or decompression failed")) {
        console.log("PASS: Error message matched successfully!");
        process.exit(0);
    } else {
        console.error("FAIL: Unexpected error message:", errMsg);
        process.exit(1);
    }
}
