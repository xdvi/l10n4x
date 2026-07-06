using System.Runtime.InteropServices;
using Xunit;

namespace L10nTests;

public class SmokeTests : IDisposable
{
    private const string LibName = "l10n4c";
    private const int L10N4C_OK = 0;
    private const int L10N4C_KEY_NOT_FOUND = 1;

    // FNV-1a 64‑bit constants — must match l10n4x_core::binary_format::fnv1a_64
    private const ulong Fnv1aOffset = 0xcbf29ce484222325;
    private const ulong Fnv1aPrime  = 0x100000001b3;

    private static readonly string LpkDir;
    private static readonly bool HasKey;

    static SmokeTests()
    {
        HasKey = !string.IsNullOrEmpty(
            Environment.GetEnvironmentVariable("L10N4X_VERIFY_PUBLIC_KEY"));
        var rootDir = Path.GetFullPath(Path.Combine(
            AppContext.BaseDirectory, "..", "..", "..", "..", ".."));
        LpkDir = Environment.GetEnvironmentVariable("L10N4X_LPK_DIR")
            ?? Path.Combine(rootDir, "dist", "locales");
    }

    public SmokeTests()
    {
        if (!HasKey) return;
        var verifyHex = Environment.GetEnvironmentVariable("L10N4X_VERIFY_PUBLIC_KEY")!;
        var verifyKey = HexToBytes(verifyHex);
        if (l10n4c_set_verify_key(verifyKey, 32) != L10N4C_OK)
            throw new InvalidOperationException("invalid verify key");
        if (l10n4c_set_fallback_locale("es") != L10N4C_OK)
            throw new InvalidOperationException("set_fallback_locale failed");
        if (l10n4c_load_lpk_directory(LpkDir) != L10N4C_OK)
            throw new InvalidOperationException($"load_lpk_directory failed: {LpkDir}");
    }

    public void Dispose()
    {
        if (HasKey) l10n4c_clear();
    }

    [Fact]
    public void SpanishWelcome()
    {
        if (!HasKey) return;
        var result = Translate("es", "common.welcome");
        Assert.Contains("Bienvenido", result);
    }

    [Fact]
    public void EnglishWelcomeBuffered()
    {
        if (!HasKey) return;
        var result = TranslateBuffered("en", "common.welcome");
        Assert.Contains("Welcome", result);
    }

    [Fact]
    public void FallbackToSpanish()
    {
        if (!HasKey) return;
        var result = Translate("xx", "common.welcome");
        Assert.Contains("Bienvenido", result);
    }

    [Fact]
    public void MissingKeyReturnsHash()
    {
        if (!HasKey) return;
        var result = Translate("en", "nonexistent.key");
        Assert.StartsWith("0x", result); // key-hash fallback for unknown keys
    }

    // ── FFI declarations (subset of L10n.cs) ────────────────────────────

    [DllImport(LibName, CallingConvention = CallingConvention.Cdecl)]
    private static extern int l10n4c_set_verify_key(byte[] key, nuint keyLen);

    [DllImport(LibName, CallingConvention = CallingConvention.Cdecl)]
    private static extern int l10n4c_set_fallback_locale(
        [MarshalAs(UnmanagedType.LPUTF8Str)] string locale);

    [DllImport(LibName, CallingConvention = CallingConvention.Cdecl)]
    private static extern int l10n4c_load_lpk_directory(
        [MarshalAs(UnmanagedType.LPUTF8Str)] string dirPath);

    [DllImport(LibName, CallingConvention = CallingConvention.Cdecl)]
    private static extern int l10n4c_translate_required_size(
        [MarshalAs(UnmanagedType.LPUTF8Str)] string locale,
        ulong keyHash,
        out nuint outSize);

    [DllImport(LibName, CallingConvention = CallingConvention.Cdecl)]
    private static extern int l10n4c_translate(
        [MarshalAs(UnmanagedType.LPUTF8Str)] string locale,
        ulong keyHash,
        byte[] buf, nuint maxLen);

    [DllImport(LibName, CallingConvention = CallingConvention.Cdecl)]
    private static extern nint l10n4c_translate_alloc(
        [MarshalAs(UnmanagedType.LPUTF8Str)] string locale,
        ulong keyHash);

    [DllImport(LibName, CallingConvention = CallingConvention.Cdecl)]
    private static extern void l10n4c_free_string(nint ptr);

    [DllImport(LibName, CallingConvention = CallingConvention.Cdecl)]
    private static extern void l10n4c_clear();

    // ── Helpers ─────────────────────────────────────────────────────────

    private static string Translate(string locale, string key)
    {
        var keyHash = Fnv1a_64(key);
        var ptr = l10n4c_translate_alloc(locale, keyHash);
        if (ptr == nint.Zero) return key;
        try { return Marshal.PtrToStringUTF8(ptr) ?? key; }
        finally { l10n4c_free_string(ptr); }
    }

    private static string TranslateBuffered(string locale, string key)
    {
        var keyHash = Fnv1a_64(key);
        var code = l10n4c_translate_required_size(locale, keyHash, out var size);
        if (code != L10N4C_OK && code != L10N4C_KEY_NOT_FOUND) return key;
        var buf = new byte[size];
        l10n4c_translate(locale, keyHash, buf, size);
        return System.Text.Encoding.UTF8.GetString(buf, 0, (int)size - 1);
    }

    private static byte[] HexToBytes(string hex)
    {
        if (hex.Length != 64)
            throw new ArgumentException("Expected 64 hex chars", nameof(hex));
        var bytes = new byte[32];
        for (var i = 0; i < 32; i++)
            bytes[i] = Convert.ToByte(hex.Substring(i * 2, 2), 16);
        return bytes;
    }

    /// <summary>FNV-1a 64‑bit hash — must match l10n4x_core::binary_format::fnv1a_64.</summary>
    private static ulong Fnv1a_64(string key)
    {
        var hash = Fnv1aOffset;
        foreach (var b in System.Text.Encoding.UTF8.GetBytes(key))
        {
            hash ^= b;
            hash *= Fnv1aPrime;
        }
        return hash;
    }
}
