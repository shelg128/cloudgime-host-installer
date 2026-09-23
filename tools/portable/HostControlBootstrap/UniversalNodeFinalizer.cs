using Microsoft.Win32;
using System.Diagnostics;
using System.Text.Json;

namespace CloudgimeHostBootstrap;

internal static class UniversalNodeFinalizer
{
    internal static int Run(string bundleRoot, string sentinelRoot, bool enableAutoLogon)
    {
        bundleRoot = Path.GetFullPath(bundleRoot);
        sentinelRoot = Path.GetFullPath(sentinelRoot);
        UniversalMachineBoundary.RequireAdminConsole();
        UniversalMachineBoundary.RequireIdle(bundleRoot);
        UniversalMachineBoundary.RequireProtected(sentinelRoot, true);
        RequireProvisionedNode();
        var helper = Path.Combine(sentinelRoot, "resources", "backend", "SentinelHelper.exe");
        var pairing = Path.Combine(bundleRoot, "auto-pair-host.ps1");
        foreach (var path in new[] { helper, pairing, Path.Combine(bundleRoot, "host-installer.exe"),
            Path.Combine(sentinelRoot, "Sentinel Launcher.exe"), Path.Combine(sentinelRoot, "resources", "app.asar") })
            UniversalMachineBoundary.RequireProtected(path);
        var powershell = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.System),
            "WindowsPowerShell", "v1.0", "powershell.exe");
        // Initial onboarding only. Never stop a running broker, retire an encrypted journal,
        // or guess whether an active/settling rental is safe to modify.
        try
        {
            Run(helper, ["--pin-shared-gaming-desktop"], sentinelRoot);
            Run(helper, ["--install-runtime-broker-deferred", sentinelRoot], sentinelRoot);
            Run(powershell, ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", pairing, "-BundleRoot", bundleRoot], bundleRoot);
            RunSc("config", "SentinelNodeBroker", "start=", "auto");
            RunSc("failure", "SentinelNodeBroker", "reset=", "86400", "actions=", "restart/5000/restart/15000/restart/30000");
            RunSc("start", "SentinelNodeBroker");
            WaitServiceRunning();
            UniversalDesktopProbe.Verify(helper);
        }
        catch
        {
            // Failed onboarding must not strand the operator behind an unverified gate.
            // Credentials and desktop pin are retained for explicit retry/recovery.
            try { RunSc("config", "SentinelNodeBroker", "start=", "disabled"); } catch { }
            try { RunSc("stop", "SentinelNodeBroker"); } catch { }
            throw;
        }
        if (enableAutoLogon && SecureAutoLogon.ConfigureInteractive() != 0)
            throw new InvalidOperationException("AUTOLOGON_CANCELLED: Node setup completed; automatic Windows login was not enabled.");
        return 0;
    }

    static void RequireProvisionedNode()
    {
        var root = UniversalMachineBoundary.ProvisioningRoot;
        var manifest = Path.Combine(root, "manifest.json");
        foreach (var file in new[] { manifest, Path.Combine(root, "host_app.credential.dpapi"),
            Path.Combine(root, "sentinel.credential.dpapi"), Path.Combine(root, "machine-identity.dpapi") })
            UniversalMachineBoundary.RequireProtected(file);
        using var document = JsonDocument.Parse(File.ReadAllBytes(manifest));
        if (document.RootElement.GetProperty("status").GetString() != "PROVISIONED_WAITING_HEARTBEATS")
            throw new InvalidOperationException("PROVISIONING_STATUS_INVALID");
    }

    static void Run(string file, IEnumerable<string> args, string cwd)
        => UniversalMachineBoundary.Run(file, args, cwd);
    static void RunSc(params string[] args)
        => Run(Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.System), "sc.exe"), args, Environment.SystemDirectory);
    static void WaitServiceRunning()
    {
        for (var i = 0; i < 60; i++)
        {
            using var query = new System.Management.ManagementObjectSearcher("SELECT State FROM Win32_Service WHERE Name='SentinelNodeBroker'");
            if (query.Get().Cast<System.Management.ManagementObject>().Any(s => (string?)s["State"] == "Running")) return;
            Thread.Sleep(500);
        }
        throw new TimeoutException("BROKER_START_TIMEOUT");
    }
}
