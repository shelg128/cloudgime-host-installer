using System.Diagnostics;
using System.Security.AccessControl;
using System.Security.Principal;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;

namespace CloudgimeHostBootstrap;

// Shared administrator boundary for files executed by the onboarding/finalization lane.
internal static class UniversalMachineBoundary
{
    internal static string ProvisioningRoot => Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData), "CloudGime", "provisioning");

    internal static void RequireAdminConsole()
    {
        using var i = WindowsIdentity.GetCurrent();
        if (!new WindowsPrincipal(i).IsInRole(WindowsBuiltInRole.Administrator)) throw new UnauthorizedAccessException("ADMIN_REQUIRED");
        if (i.IsSystem || Process.GetCurrentProcess().SessionId <= 0 || WTSGetActiveConsoleSessionId() != (uint)Process.GetCurrentProcess().SessionId)
            throw new InvalidOperationException("ACTIVE_OPERATOR_CONSOLE_REQUIRED");
    }

    internal static void RequireProtected(string path, bool directory = false)
    {
        var full = Path.GetFullPath(path);
        var trusted = new HashSet<string>(StringComparer.Ordinal) { "S-1-5-18", "S-1-5-32-544", "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464" };
        for (FileSystemInfo? item = directory ? new DirectoryInfo(full) : new FileInfo(full); item != null; item = item is DirectoryInfo d ? d.Parent : ((FileInfo)item).Directory)
        {
            if (!item.Exists || (item.Attributes & FileAttributes.ReparsePoint) != 0) throw new InvalidOperationException("UNSAFE_INSTALL_PATH");
            FileSystemSecurity acl = item is DirectoryInfo dir ? dir.GetAccessControl() : ((FileInfo)item).GetAccessControl();
            // Parent deletion/ACL replacement can invalidate a protected child.
            var danger = FileSystemRights.DeleteSubdirectoriesAndFiles | FileSystemRights.ChangePermissions | FileSystemRights.TakeOwnership;
            if (item.FullName.Equals(full, StringComparison.OrdinalIgnoreCase))
            {
                danger |= FileSystemRights.Write | FileSystemRights.Delete;
                if (acl.GetOwner(typeof(SecurityIdentifier)) is not SecurityIdentifier owner || !trusted.Contains(owner.Value))
                    throw new InvalidOperationException("UNTRUSTED_INSTALL_OWNER");
            }
            foreach (FileSystemAccessRule rule in acl.GetAccessRules(true, true, typeof(SecurityIdentifier)))
                if (rule.AccessControlType == AccessControlType.Allow && (rule.PropagationFlags & PropagationFlags.InheritOnly) == 0
                    && !trusted.Contains(rule.IdentityReference.Value) && (rule.FileSystemRights & danger) != 0)
                    throw new InvalidOperationException("INSTALL_PATH_WRITABLE_BY_NONADMIN");
        }
    }

    internal static void RequireNoRentalActivity(bool checkProcesses = true)
    {
        // Install/upgrade may own and restart Host services, but never a live rental or Sentinel gate.
        foreach (var name in checkProcesses ? new[] { "SentinelNodeBroker", "Sentinel Launcher", "SentinelHelper", "streamer" } : Array.Empty<string>())
            foreach (var p in Process.GetProcessesByName(name))
            { using (p) { throw new InvalidOperationException("OPERATOR_MAINTENANCE_REQUIRED: running " + name); } }
        // A different administrator must not accidentally inspect their own empty profile.
        var pin = Path.Combine(ProvisioningRoot, "shared-gaming-desktop.identity.json");
        if (File.Exists(pin))
        {
            RequireProtected(pin);
            using var policy = JsonDocument.Parse(File.ReadAllBytes(pin));
            using var current = WindowsIdentity.GetCurrent();
            if (current.User?.Value != policy.RootElement.GetProperty("desktopUserSid").GetString())
                throw new InvalidOperationException("PINNED_DESKTOP_OPERATOR_REQUIRED");
        }
        var session = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "SentinelLauncher", "state", "session.json");
        if (File.Exists(session))
        {
            using var doc = JsonDocument.Parse(File.ReadAllBytes(session));
            if (doc.RootElement.ValueKind != JsonValueKind.Object) throw new InvalidOperationException("LOCAL_SESSION_INVALID");
            foreach (var p in doc.RootElement.EnumerateObject())
                if (p.Value.ValueKind != JsonValueKind.Null && !(p.Value.ValueKind == JsonValueKind.String && p.Value.GetString() == ""))
                    throw new InvalidOperationException("LOCAL_SESSION_OR_CLEANUP_PENDING");
        }
        // Encrypted journals cannot be guessed/replaced here. Only the Sentinel owner may retire them.
        var userData = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "sentinel-gaming-launcher");
        var journal = Path.Combine(userData, "renter-operation.dpapi");
        if (File.Exists(journal) && !IsIdleElectronJournal(journal, Path.Combine(userData, "Local State")))
            throw new InvalidOperationException("EXISTING_RENTER_JOURNAL: use Sentinel maintenance, not installation.");
    }

    private static bool IsIdleElectronJournal(string journalPath, string localStatePath)
    {
        try
        {
            UniversalBundleContract.RejectReparse(journalPath);
            UniversalBundleContract.RejectReparse(localStatePath);
            if (new FileInfo(journalPath).Length > 64 * 1024 || new FileInfo(localStatePath).Length > 4 * 1024 * 1024) return false;
            var journal = File.ReadAllBytes(journalPath);
            using var state = JsonDocument.Parse(File.ReadAllBytes(localStatePath));
            var wrapped = Convert.FromBase64String(state.RootElement.GetProperty("os_crypt").GetProperty("encrypted_key").GetString()!);
            if (wrapped.Length <= 5 || !wrapped.AsSpan(0, 5).SequenceEqual("DPAPI"u8)) return false;
            var encryptedKey = wrapped.AsSpan(5).ToArray();
            var key = ProtectedData.Unprotect(encryptedKey, null, DataProtectionScope.CurrentUser);
            try { return ElectronIdleJournal.IsIdle(journal, key); }
            finally { CryptographicOperations.ZeroMemory(key); CryptographicOperations.ZeroMemory(encryptedKey); }
        }
        catch { return false; }
    }

    internal static void RequireIdle(string bundleRoot)
    {
        RequireNoRentalActivity();
        RequireProtected(bundleRoot, true);
    }

    internal static void Run(string file, IEnumerable<string> args, string workingDirectory, string? input = null)
    {
        var psi = new ProcessStartInfo(file) { UseShellExecute = false, CreateNoWindow = true,
            WorkingDirectory = workingDirectory, RedirectStandardOutput = true, RedirectStandardError = true,
            RedirectStandardInput = input != null };
        foreach (var a in args) psi.ArgumentList.Add(a);
        using var child = Process.Start(psi) ?? throw new InvalidOperationException("PROCESS_START_FAILED");
        // Drain concurrently to avoid deadlock; discard output (could contain a credential).
        var stdout = child.StandardOutput.BaseStream.CopyToAsync(Stream.Null);
        var stderr = child.StandardError.BaseStream.CopyToAsync(Stream.Null);
        if (input != null) { child.StandardInput.WriteLine(input); child.StandardInput.Close(); }
        if (!child.WaitForExit(240000)) { child.Kill(true); throw new TimeoutException("FINALIZE_PROCESS_TIMEOUT"); }
        if (!Task.WaitAll([stdout, stderr], 10000)) throw new TimeoutException("FINALIZE_PIPE_TIMEOUT");
        if (child.ExitCode != 0) throw new InvalidOperationException("FINALIZE_PROCESS_FAILED: " + Path.GetFileName(file) + " exit=" + child.ExitCode);
    }
    [System.Runtime.InteropServices.DllImport("kernel32.dll")] private static extern uint WTSGetActiveConsoleSessionId();
}
