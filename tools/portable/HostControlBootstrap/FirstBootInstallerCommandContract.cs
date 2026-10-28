namespace CloudgimeHostBootstrap;

// Keep the CLI shape used by both the Rust host installer and the headless
// fresh-install lane in one testable contract. Preparing a plan is distinct
// from executing the SYSTEM helper bridge.
internal static class FirstBootInstallerCommandContract
{
    internal const string PrepareCommand = "prepare-installer-first-boot";

    internal static bool IsPrepareCommand(string[] args) =>
        args.Length > 0 && string.Equals(args[0], PrepareCommand, StringComparison.Ordinal);

    internal static bool IsValidInvocation(string[] args)
    {
        if (args.Length < 2 || !string.Equals(args[0], PrepareCommand, StringComparison.Ordinal))
            return false;
        if (args.Length is 2 or 3)
            return !string.IsNullOrWhiteSpace(args[1]) && (args.Length == 2 || string.Equals(args[2], "--silent", StringComparison.Ordinal));
        return string.Equals(args[^1], "--silent", StringComparison.Ordinal)
            && !string.IsNullOrWhiteSpace(string.Join(" ", args[1..^1]));
    }

    internal static string ParseBundleRoot(string[] args)
    {
        if (!IsValidInvocation(args)) throw new InvalidDataException("FIRST_BOOT_COMMAND_INVALID");
        var raw = (args.Length > 3 && string.Equals(args[^1], "--silent", StringComparison.Ordinal))
            ? string.Join(" ", args[1..^1])
            : args[1];
        return Path.GetFullPath(raw);
    }
}
