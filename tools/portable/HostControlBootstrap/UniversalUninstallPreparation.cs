using Microsoft.Win32;

namespace CloudgimeHostBootstrap;

internal static class UniversalUninstallPreparation
{
    internal static int Run(string bundleRoot)
    {
        UniversalMachineBoundary.RequireProtected(bundleRoot, true);
        // Uninstall is an explicit operator recovery action. Do not depend on the
        // renderer or delete/retire rental records; billing remains backend-owned.
        using (var machine = RegistryKey.OpenBaseKey(RegistryHive.LocalMachine, RegistryView.Registry64))
        using (var owner = machine.OpenSubKey(@"SOFTWARE\Cloudgime\SharedDesktop"))
            if (owner?.GetValue("OwnerSid") is string sid && !string.IsNullOrEmpty(sid))
                SecureAutoLogon.DisableOwned();

        var sentinelRoot = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles), "Sentinel Launcher");
        var helper = Path.Combine(sentinelRoot, "resources", "backend", "SentinelHelper.exe");
        using var services = Registry.LocalMachine.OpenSubKey(@"SYSTEM\CurrentControlSet\Services\SentinelNodeBroker");
        if (services is not null && !File.Exists(helper))
            throw new InvalidOperationException("SENTINEL_RECOVERY_HELPER_MISSING: restore Sentinel before removing the Host");
        if (File.Exists(helper))
        {
            UniversalMachineBoundary.RequireProtected(helper);
            UniversalMachineBoundary.Run(helper, ["--recover-operator-policy"], sentinelRoot);
            UniversalMachineBoundary.Run(helper, ["--remove-runtime-broker"], sentinelRoot);
            UniversalMachineBoundary.Run(helper, ["--remove-operator-policy-monitor"], sentinelRoot);
        }
        // Sentinel remains a separately uninstallable application. Its files and
        // ProgramData/CloudGime/provisioning identity are deliberately preserved.
        return 0;
    }
}
