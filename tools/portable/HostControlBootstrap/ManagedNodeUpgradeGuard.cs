namespace CloudgimeHostBootstrap;

// The ordinary NSIS installer is NOT a coordinated managed-rental transaction.
// File presence is a denial signal, never permission inferred from JSON content.
// A stopped EmergencyKill node must not be adopted as a clean/idle installation.
internal static class ManagedNodeUpgradeGuard
{
    internal static readonly string[] ProtectedState =
    [
        "rental-turnover.json",
        "rental-reset-shell.json",
        "rental-reset-private/journal/reset-journal.json",
        "operator-emergency/active.json"
    ];

    internal static void RequireUnmanaged(string provisioningRoot)
    {
        foreach(var relative in ProtectedState)
        {
            var candidate=Path.Combine(provisioningRoot,relative.Replace('/',Path.DirectorySeparatorChar));
            UniversalBundleContract.RejectReparse(candidate);
            // File.Exists returns false on access errors. Use GetAttributes so an
            // unreadable path fails closed rather than looking like first install.
            try
            {
                _=File.GetAttributes(candidate);
                throw new InvalidOperationException("MANAGED_NODE_REQUIRES_COORDINATED_UPGRADE: preserve operator stop, identity, rental profiles and policies; do not remove state to bypass this guard.");
            }
            catch(FileNotFoundException) { }
            catch(DirectoryNotFoundException) { }
        }
    }
}
