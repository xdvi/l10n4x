using System.Runtime.InteropServices;
using System.Text;

internal static class L10n
{
    private const string LibName = "l10n4c";
    private const string ReleasesUrl = "https://github.com/xdvi/l10n4x/releases/latest";

    public const int L10N4C_OK = 0;
    public const int L10N4C_KEY_NOT_FOUND = 1;
    public const int L10N4C_BUFFER_TOO_SMALL = 3;

    static L10n()
    {
        var handle = LoadNativeLibrary();
        NativeLibrary.SetDllImportResolver(typeof(L10n).Assembly, (name, _, _) =>
            name == LibName ? handle : nint.Zero);
    }

    [DllImport(LibName, CallingConvention = CallingConvention.Cdecl)]
    public static extern int l10n4c_set_verify_key(byte[] key, nuint keyLen);

    [DllImport(LibName, CallingConvention = CallingConvention.Cdecl)]
    public static extern int l10n4c_set_decrypt_key(byte[] key, nuint keyLen);

    [DllImport(LibName, CallingConvention = CallingConvention.Cdecl)]
    public static extern int l10n4c_set_fallback_locale(
        [MarshalAs(UnmanagedType.LPUTF8Str)] string locale);

    [DllImport(LibName, CallingConvention = CallingConvention.Cdecl)]
    public static extern int l10n4c_load_pak_directory(
        [MarshalAs(UnmanagedType.LPUTF8Str)] string dirPath);

    [DllImport(LibName, CallingConvention = CallingConvention.Cdecl)]
    public static extern int l10n4c_translate_required_size(
        [MarshalAs(UnmanagedType.LPUTF8Str)] string locale,
        [MarshalAs(UnmanagedType.LPUTF8Str)] string key,
        out nuint outSize);

    [DllImport(LibName, CallingConvention = CallingConvention.Cdecl)]
    public static extern int l10n4c_translate(
        [MarshalAs(UnmanagedType.LPUTF8Str)] string locale,
        [MarshalAs(UnmanagedType.LPUTF8Str)] string key,
        byte[] buf,
        nuint maxLen);

    [DllImport(LibName, CallingConvention = CallingConvention.Cdecl)]
    public static extern nint l10n4c_translate_alloc(
        [MarshalAs(UnmanagedType.LPUTF8Str)] string locale,
        [MarshalAs(UnmanagedType.LPUTF8Str)] string key);

    [DllImport(LibName, CallingConvention = CallingConvention.Cdecl)]
    public static extern void l10n4c_free_string(nint ptr);

    [DllImport(LibName, CallingConvention = CallingConvention.Cdecl)]
    public static extern void l10n4c_clear();

    private static nint LoadNativeLibrary()
    {
        var libDir = Environment.GetEnvironmentVariable("L10N4X_LIB_DIR");
        if (string.IsNullOrEmpty(libDir))
        {
            libDir = Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, "..", "..", "..", "..", "lib"));
        }

        var candidates = new[]
        {
            "libl10n4c.so",
            "libl10n4c-linux.so",
            "libl10n4c.dylib",
            "libl10n4c-macos.dylib",
            "l10n4c.dll",
            "l10n4c-windows.dll",
        };

        foreach (var name in candidates)
        {
            var path = Path.Combine(libDir, name);
            if (File.Exists(path))
            {
                return NativeLibrary.Load(path);
            }
        }

        throw new FileNotFoundException(
            $"l10n4c native library not found in '{libDir}'. " +
            $"Download a release bundle from {ReleasesUrl} and extract to examples/lib/, " +
            "or set L10N4X_LIB_DIR.");
    }

    public static void InstallRuntimeKeys()
    {
        var verifyHex = Environment.GetEnvironmentVariable("L10N4X_VERIFY_PUBLIC_KEY");
        if (string.IsNullOrEmpty(verifyHex))
        {
            throw new InvalidOperationException(
                "L10N4X_VERIFY_PUBLIC_KEY is not set (64-char hex from l10n4x.config.json verifyPublicKey)");
        }

        var verifyKey = HexToBytes(verifyHex);
        if (l10n4c_set_verify_key(verifyKey, 32) != L10N4C_OK)
        {
            throw new InvalidOperationException("l10n4c: invalid verify public key");
        }

        var encRaw = Environment.GetEnvironmentVariable("L10N4X_ENCRYPT_KEY");
        if (!string.IsNullOrEmpty(encRaw))
        {
            if (encRaw.Length != 32)
            {
                throw new InvalidOperationException("L10N4X_ENCRYPT_KEY must be exactly 32 bytes when set");
            }
            var encKey = Encoding.Latin1.GetBytes(encRaw);
            if (l10n4c_set_decrypt_key(encKey, 32) != L10N4C_OK)
            {
                throw new InvalidOperationException("l10n4c: invalid decrypt key");
            }
        }
    }

    private static byte[] HexToBytes(string hex)
    {
        if (hex.Length != 64)
        {
            throw new ArgumentException("Expected 64 hex characters (32 bytes)", nameof(hex));
        }
        var bytes = new byte[32];
        for (var i = 0; i < 32; i++)
        {
            bytes[i] = Convert.ToByte(hex.Substring(i * 2, 2), 16);
        }
        return bytes;
    }

    public static string Translate(string locale, string key)
    {
        var ptr = l10n4c_translate_alloc(locale, key);
        if (ptr == nint.Zero)
        {
            return key;
        }
        try
        {
            return Marshal.PtrToStringUTF8(ptr) ?? key;
        }
        finally
        {
            l10n4c_free_string(ptr);
        }
    }

    public static string TranslateBuffered(string locale, string key)
    {
        var code = l10n4c_translate_required_size(locale, key, out var size);
        if (code != L10N4C_OK && code != L10N4C_KEY_NOT_FOUND)
        {
            return key;
        }
        var buf = new byte[size];
        l10n4c_translate(locale, key, buf, size);
        return Encoding.UTF8.GetString(buf, 0, (int)size - 1);
    }
}