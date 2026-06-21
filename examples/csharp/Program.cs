// Prerequisites:
//   1. Download platform bundle from GitHub Releases → examples/lib/
//   2. l10n4x build  (CLI from the same release bundle)
//   3. export L10N4X_VERIFY_PUBLIC_KEY=<verifyPublicKey from l10n4x.config.json>
//   4. (optional) export L10N4X_ENCRYPT_KEY when encrypt: true
//
// Releases: https://github.com/xdvi/l10n4x/releases/latest

try
{
    L10n.InstallRuntimeKeys();
}
catch (Exception ex)
{
    Console.WriteLine(ex.Message);
    return 1;
}

if (L10n.l10n4c_set_fallback_locale("es") != L10n.L10N4C_OK)
{
    Console.WriteLine("Failed to set fallback locale.");
    return 1;
}

var examplesDir = Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, "..", "..", "..", ".."));
var pakDir = Path.Combine(examplesDir, "dist", "locales");

if (L10n.l10n4c_load_pak_directory(pakDir) != L10n.L10N4C_OK)
{
    Console.WriteLine("Failed to load pak directory.");
    return 1;
}

Console.WriteLine(L10n.Translate("es", "common.welcome"));
Console.WriteLine(L10n.TranslateBuffered("en", "common.welcome"));

L10n.l10n4c_clear();
return 0;