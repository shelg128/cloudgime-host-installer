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
            var operatorSid = policy.RootElement.GetProperty("desktopUserSid").GetString();
            var accepted = new HashSet<string>(StringComparer.Ordinal) { operatorSid ?? string.Empty };
            var dedicated = Path.Combine(ProvisioningRoot, "rental-desktop.identity.json");
            if (File.Exists(dedicated))
            {
                RequireProtected(dedicated);
                using var renter = JsonDocument.Parse(File.ReadAllBytes(dedicated));
                var renterSid = renter.RootElement.GetProperty("rentalUserSid").GetString();
                var recordedOperator = renter.RootElement.GetProperty("operatorUserSid").GetString();
                if (recordedOperator != operatorSid || string.IsNullOrWhiteSpace(renterSid))
                    throw new InvalidOperationException("DESKTOP_POLICY_BINDING_INVALID");
                accepted.Add(renterSid);
            }
            var activeSid = ActiveConsoleUserSid();
            if (!accepted.Contains(activeSid))
                throw new InvalidOperationException("PINNED_DESKTOP_CONSOLE_REQUIRED");
            VerifyActiveDesktopIdle(activeSid);
        }
        else
        {
            // Legacy installs without a machine desktop pin may inspect only the
            // current caller profile. Provisioned Nodes always use the two-token path.
            RequireCurrentProfileIdle();
        }
    }

    internal static int RequireActiveDesktopIdleAgent()
    {
        using var current = WindowsIdentity.GetCurrent();
        var active = WTSGetActiveConsoleSessionId();
        if (current.IsSystem || current.User is null || Process.GetCurrentProcess().SessionId <= 0
            || active == uint.MaxValue || active != (uint)Process.GetCurrentProcess().SessionId)
            throw new InvalidOperationException("ACTIVE_DESKTOP_AGENT_REQUIRED");
        var pin = Path.Combine(ProvisioningRoot, "shared-gaming-desktop.identity.json");
        RequireProtected(pin);
        using var shared = JsonDocument.Parse(File.ReadAllBytes(pin));
        var accepted = new HashSet<string>(StringComparer.Ordinal)
            { shared.RootElement.GetProperty("desktopUserSid").GetString() ?? string.Empty };
        var dedicated = Path.Combine(ProvisioningRoot, "rental-desktop.identity.json");
        if (File.Exists(dedicated))
        {
            RequireProtected(dedicated);
            using var renter = JsonDocument.Parse(File.ReadAllBytes(dedicated));
            if (renter.RootElement.GetProperty("operatorUserSid").GetString() != accepted.First())
                throw new InvalidOperationException("DESKTOP_POLICY_BINDING_INVALID");
            accepted.Add(renter.RootElement.GetProperty("rentalUserSid").GetString() ?? string.Empty);
        }
        if (!accepted.Contains(current.User.Value))
            throw new InvalidOperationException("ACTIVE_DESKTOP_AGENT_SID_INVALID");
        RequireCurrentProfileIdle();
        return 0;
    }

    private static void RequireCurrentProfileIdle()
    {
        var session = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "SentinelLauncher", "state", "session.json");
        if (File.Exists(session))
        {
            using var doc = JsonDocument.Parse(File.ReadAllBytes(session));
            if (doc.RootElement.ValueKind != JsonValueKind.Object) throw new InvalidOperationException("LOCAL_SESSION_INVALID");
            foreach (var p in doc.RootElement.EnumerateObject())
                if (p.Value.ValueKind != JsonValueKind.Null && !(p.Value.ValueKind == JsonValueKind.String && p.Value.GetString() == ""))
                    throw new InvalidOperationException("LOCAL_SESSION_OR_CLEANUP_PENDING");
        }
        var userData = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "sentinel-gaming-launcher");
        var localState = Path.Combine(userData, "Local State");
        var journal = Path.Combine(userData, "renter-operation.dpapi");
        if (File.Exists(journal) && !IsIdleElectronJournal(journal, localState))
            throw new InvalidOperationException("EXISTING_RENTER_JOURNAL: use Sentinel maintenance, not installation.");
        var cleanup = Path.Combine(userData, "cleanup-operation.dpapi");
        if (File.Exists(cleanup) && !IsDoneElectronCleanup(cleanup, localState))
            throw new InvalidOperationException("EXISTING_CLEANUP_OUTBOX: use Sentinel maintenance, not installation.");
    }

    private static bool IsDoneElectronCleanup(string outboxPath, string localStatePath)
    {
        try { var (encrypted,key)=ReadElectronState(outboxPath,localStatePath,128*1024);try{return ElectronIdleJournal.IsCleanupDone(encrypted,key);}finally{CryptographicOperations.ZeroMemory(encrypted);CryptographicOperations.ZeroMemory(key);} }
        catch{return false;}
    }
    private static bool IsIdleElectronJournal(string journalPath, string localStatePath)
    {
        try { var (encrypted,key)=ReadElectronState(journalPath,localStatePath,64*1024);try{return ElectronIdleJournal.IsIdle(encrypted,key);}finally{CryptographicOperations.ZeroMemory(encrypted);CryptographicOperations.ZeroMemory(key);} }
        catch{return false;}
    }
    private static (byte[] Encrypted,byte[] Key) ReadElectronState(string encryptedPath,string localStatePath,int maxBytes)
    {
        UniversalBundleContract.RejectReparse(encryptedPath);UniversalBundleContract.RejectReparse(localStatePath);
        if(new FileInfo(encryptedPath).Length>maxBytes||new FileInfo(localStatePath).Length>4*1024*1024)throw new InvalidDataException("ELECTRON_STATE_SIZE_INVALID");
        var encrypted=File.ReadAllBytes(encryptedPath);using var state=JsonDocument.Parse(File.ReadAllBytes(localStatePath));
        var wrapped=Convert.FromBase64String(state.RootElement.GetProperty("os_crypt").GetProperty("encrypted_key").GetString()!);
        if(wrapped.Length<=5||!wrapped.AsSpan(0,5).SequenceEqual("DPAPI"u8))throw new InvalidDataException("ELECTRON_KEY_INVALID");
        var protectedKey=wrapped.AsSpan(5).ToArray();try{return(encrypted,ProtectedData.Unprotect(protectedKey,null,DataProtectionScope.CurrentUser));}
        finally{CryptographicOperations.ZeroMemory(protectedKey);CryptographicOperations.ZeroMemory(wrapped);}
    }
    private static void VerifyActiveDesktopIdle(string activeSid)
    {
        var executable=Environment.ProcessPath??throw new InvalidOperationException("BOOTSTRAP_PATH_REQUIRED");RequireProtected(executable);
        var task="CloudGime-UpgradeIdleProbe-"+Guid.NewGuid().ToString("N");
        var script="""
$ErrorActionPreference='Stop';$name='__TASK__'
try{$a=New-ScheduledTaskAction -Execute '__EXECUTABLE__' -Argument 'verify-active-desktop-idle --silent';$p=New-ScheduledTaskPrincipal -UserId '__SID__' -LogonType Interactive -RunLevel Limited;Register-ScheduledTask $name -Action $a -Principal $p -Settings (New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Seconds 45))|Out-Null;Start-ScheduledTask $name;$end=[DateTime]::UtcNow.AddSeconds(60);do{$s=Get-ScheduledTask $name;$i=Get-ScheduledTaskInfo $name;if($i.LastRunTime.Year-gt2000-and$s.State-ne'Running'-and$i.LastTaskResult-notin@(267009,267011)){if($i.LastTaskResult-ne0){throw 'ACTIVE_DESKTOP_IDLE_REJECTED'};exit 0};Start-Sleep -Milliseconds 250}while([DateTime]::UtcNow-lt$end);throw 'ACTIVE_DESKTOP_IDLE_TIMEOUT'}finally{Stop-ScheduledTask $name -ErrorAction SilentlyContinue;Unregister-ScheduledTask $name -Confirm:$false -ErrorAction SilentlyContinue}
""".Replace("__TASK__",task,StringComparison.Ordinal)
    .Replace("__EXECUTABLE__",executable.Replace("'","''",StringComparison.Ordinal),StringComparison.Ordinal)
    .Replace("__SID__",activeSid,StringComparison.Ordinal);
        Run(Path.Combine(Environment.SystemDirectory,"WindowsPowerShell","v1.0","powershell.exe"),["-NoLogo","-NoProfile","-NonInteractive","-EncodedCommand",Convert.ToBase64String(Encoding.Unicode.GetBytes(script))],Path.GetDirectoryName(executable)!);
    }
    private static string ActiveConsoleUserSid()
    {
        var active=WTSGetActiveConsoleSessionId();if(active==uint.MaxValue||active==0)throw new InvalidOperationException("ACTIVE_CONSOLE_REQUIRED");
        string Query(int info)
        {
            if(!WTSQuerySessionInformationW(IntPtr.Zero,active,info,out var buffer,out var bytes)||buffer==IntPtr.Zero)
                throw new InvalidOperationException("ACTIVE_CONSOLE_QUERY_FAILED");
            try{return System.Runtime.InteropServices.Marshal.PtrToStringUni(buffer)??string.Empty;}finally{WTSFreeMemory(buffer);}
        }
        var user=Query(5);var domain=Query(7);if(string.IsNullOrWhiteSpace(user)||string.IsNullOrWhiteSpace(domain))throw new InvalidOperationException("ACTIVE_CONSOLE_ACCOUNT_MISSING");
        var sid=((SecurityIdentifier)new NTAccount(domain,user).Translate(typeof(SecurityIdentifier))).Value;
        if(WTSGetActiveConsoleSessionId()!=active)throw new InvalidOperationException("ACTIVE_CONSOLE_CHANGED");
        return sid;
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
    [System.Runtime.InteropServices.DllImport("wtsapi32.dll",CharSet=System.Runtime.InteropServices.CharSet.Unicode,ExactSpelling=true,SetLastError=true)]
    [return:System.Runtime.InteropServices.MarshalAs(System.Runtime.InteropServices.UnmanagedType.Bool)]
    private static extern bool WTSQuerySessionInformationW(IntPtr server,uint sessionId,int info,out IntPtr buffer,out uint bytes);
    [System.Runtime.InteropServices.DllImport("wtsapi32.dll")] private static extern void WTSFreeMemory(IntPtr buffer);
    [System.Runtime.InteropServices.DllImport("kernel32.dll")] private static extern uint WTSGetActiveConsoleSessionId();
}
