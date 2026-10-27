using System.Security.AccessControl;
using System.Security.Principal;
using System.Text;

namespace CloudgimeHostBootstrap;

// Protected file policy for one-time setup token consumption.
// The credential is never exposed in process argv; the file must reside in the
// administrator-protected installer-secrets directory, be owned by a trusted
// administrative identity, and be deleted immediately upon consumption.
internal static class InstallerProvisionPolicy
{
    internal static string DefaultSecretsRoot => Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData),
        "CloudGime", "installer-secrets");

    internal static void ValidateTokenFilePath(string tokenFile, string secretsRoot)
    {
        if (string.IsNullOrWhiteSpace(tokenFile) || string.IsNullOrWhiteSpace(secretsRoot))
            throw new InvalidDataException("INSTALLER_PROVISION_TOKEN_FILE_INVALID");
        var safeRoot = Path.GetFullPath(secretsRoot).TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar)
            + Path.DirectorySeparatorChar;
        var fullFile = Path.GetFullPath(tokenFile);
        if (!fullFile.StartsWith(safeRoot, StringComparison.OrdinalIgnoreCase))
            throw new InvalidDataException("INSTALLER_PROVISION_TOKEN_FILE_INVALID");
        var fileName = Path.GetFileName(fullFile);
        if (!fileName.StartsWith("setup-token-", StringComparison.OrdinalIgnoreCase) || fileName.Length < 14)
            throw new InvalidDataException("INSTALLER_PROVISION_TOKEN_FILE_INVALID");
    }

    internal static void ValidateTokenContent(string token)
    {
        if (string.IsNullOrWhiteSpace(token)
            || token.Length is < 12 or > 4096
            || token.IndexOfAny(['\r', '\n', '\0']) >= 0)
            throw new InvalidDataException("INSTALLER_PROVISION_TOKEN_REQUIRED");
    }

    internal static string ConsumeTokenFile(string tokenFile, string secretsRoot)
    {
        ValidateTokenFilePath(tokenFile, secretsRoot);
        UniversalMachineBoundary.RequireProtected(secretsRoot, true);
        UniversalMachineBoundary.RequireProtected(tokenFile);
        var info = new FileInfo(tokenFile);
        if (info.Length is < 12 or > 4096)
            throw new InvalidDataException("INSTALLER_PROVISION_TOKEN_FILE_INVALID");
        var actual = info.GetAccessControl().GetOwner(typeof(SecurityIdentifier)) as SecurityIdentifier;
        if (actual is null || actual.Value is not ("S-1-5-32-544" or "S-1-5-18"))
            throw new InvalidDataException("INSTALLER_PROVISION_TOKEN_FILE_OWNER_INVALID");
        var content = File.ReadAllText(tokenFile, new UTF8Encoding(false));
        File.Delete(tokenFile);
        ValidateTokenContent(content);
        return content;
    }
}
