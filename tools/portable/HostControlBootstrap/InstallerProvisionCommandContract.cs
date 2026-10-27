namespace CloudgimeHostBootstrap;

// A silent NSIS install runs elevated, so its UAC process cannot rely on a
// caller's inherited environment.  The automation credential is therefore a
// short-lived, protected file reference (never an argv secret). Bootstrap
// checks its location/ACL, consumes it once, and deletes it before spawning
// host-installer. Interactive setup still uses the private installer process
// environment and this same stdin-only child transport.
internal static class InstallerProvisionCommandContract
{
    internal const string ProvisionFromEnvironmentCommand = "provision-installer-license-env";
    internal const string ProvisionFromFileCommand = "provision-installer-license-file";

    internal static bool IsProvisionFromEnvironmentCommand(string[] args) =>
        args.Length > 0 && string.Equals(args[0], ProvisionFromEnvironmentCommand, StringComparison.Ordinal);

    internal static bool IsProvisionFromFileCommand(string[] args) =>
        args.Length > 0 && string.Equals(args[0], ProvisionFromFileCommand, StringComparison.Ordinal);

    internal static bool IsValidInvocation(string[] args) =>
        args.Length == 3
        && string.Equals(args[0], ProvisionFromEnvironmentCommand, StringComparison.Ordinal)
        && !string.IsNullOrWhiteSpace(args[1])
        && string.Equals(args[2], "--silent", StringComparison.Ordinal);

    internal static bool IsValidFileInvocation(string[] args) =>
        args.Length == 4
        && string.Equals(args[0], ProvisionFromFileCommand, StringComparison.Ordinal)
        && !string.IsNullOrWhiteSpace(args[1])
        && !string.IsNullOrWhiteSpace(args[2])
        && string.Equals(args[3], "--silent", StringComparison.Ordinal);

    internal static string ParseBundleRoot(string[] args)
    {
        if (!IsValidInvocation(args)) throw new InvalidDataException("INSTALLER_PROVISION_COMMAND_INVALID");
        return Path.GetFullPath(args[1]);
    }

    internal static (string BundleRoot, string TokenFile) ParseFileInvocation(string[] args)
    {
        if (!IsValidFileInvocation(args)) throw new InvalidDataException("INSTALLER_PROVISION_FILE_COMMAND_INVALID");
        return (Path.GetFullPath(args[1]), Path.GetFullPath(args[2]));
    }
}
