using Microsoft.Win32;
using System.Runtime.InteropServices;
using System.Security;
using System.Security.AccessControl;
using System.Diagnostics;
using System.Security.Principal;
using System.Text;
using System.Text.Json;

namespace CloudgimeHostBootstrap;

internal static class SecureAutoLogon
{
    private const string PolicyPath = @"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon";
    private const string OwnerPath = @"SOFTWARE\Cloudgime\SharedDesktop";
    private const string SecretName = "DefaultPassword";

    internal static int ConfigureInteractive()
    {
        using var operation = AcquireOperation();
        var (user, domain, sid) = ResolvePinnedConsoleIdentity();
        using var form = new Form { Text = "Cloudgime shared desktop", Width = 470, Height = 190,
            StartPosition = FormStartPosition.CenterScreen, FormBorderStyle = FormBorderStyle.FixedDialog,
            MaximizeBox = false, MinimizeBox = false };
        var message = new Label { Left = 18, Top = 15, Width = 420, Height = 38,
            Text = $"Enable automatic login for {domain}\\{user}.\r\nLSA protects storage; SYSTEM/admin can still retrieve the secret." };
        var password = new TextBox { Left = 18, Top = 62, Width = 420, UseSystemPasswordChar = true };
        var ok = new Button { Left = 265, Top = 105, Width = 80, Text = "Enable", DialogResult = DialogResult.OK };
        var cancel = new Button { Left = 358, Top = 105, Width = 80, Text = "Cancel", DialogResult = DialogResult.Cancel };
        form.Controls.AddRange([message, password, ok, cancel]); form.AcceptButton = ok; form.CancelButton = cancel;
        if (form.ShowDialog() != DialogResult.OK) return 2;
        var value = password.Text; password.Clear();
        try
        {
            if (string.IsNullOrEmpty(value) || value.Length > 1024 || value.Contains('\0'))
                throw new InvalidOperationException("AUTOLOGON_PASSWORD_INVALID");
            var identityAgain = ResolvePinnedConsoleIdentity();
            if (identityAgain != (user, domain, sid)) throw new InvalidOperationException("CONSOLE_CHANGED");
            if (!LogonUser(user, domain, value, 2, 0, out var token))
                throw new InvalidOperationException("WINDOWS_PASSWORD_VALIDATION_FAILED");
            try
            {
                using var authenticated = new WindowsIdentity(token);
                if (authenticated.User?.Value != sid) throw new InvalidOperationException("AUTHENTICATED_SID_MISMATCH");
            }
            finally { CloseHandle(token); }
            AutoLogonTransaction.Configure(new WindowsStore(), user, domain, sid, value);
            return 0;
        }
        finally { value = string.Empty; }
    }

    internal static int DisableOwned()
    {
        using var operation = AcquireOperation();
        var store = new WindowsStore(); var snapshot = store.Read();
        if (snapshot.OwnerSid is null || snapshot.User is null || snapshot.Domain is null
            || ((SecurityIdentifier)new NTAccount(snapshot.Domain, snapshot.User).Translate(typeof(SecurityIdentifier))).Value != snapshot.OwnerSid)
            throw new InvalidOperationException("AUTOLOGON_OWNER_MISMATCH");
        AutoLogonTransaction.Disable(store); return 0;
    }

    private static FileStream AcquireOperation()
    {
        using var identity = WindowsIdentity.GetCurrent();
        if (!new WindowsPrincipal(identity).IsInRole(WindowsBuiltInRole.Administrator))
            throw new UnauthorizedAccessException("ADMIN_REQUIRED");
        // Local file lock shared across sessions. The root must already be protected by provisioning.
        var directory = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData), "CloudGime", "provisioning");
        RequireProtectedPath(directory, true);
        var lockPath = Path.Combine(directory, "autologon.lock");
        if (File.Exists(lockPath)) RequireProtectedPath(lockPath, false);
        return new FileStream(lockPath, FileMode.OpenOrCreate, FileAccess.ReadWrite, FileShare.None);
    }

    [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool LogonUser(string user, string domain, string password, int type, int provider, out IntPtr token);
    [DllImport("kernel32.dll")] private static extern bool CloseHandle(IntPtr handle);
    [DllImport("kernel32.dll")] private static extern uint WTSGetActiveConsoleSessionId();

    private static void RequireProtectedPath(string path, bool directory)
    {
        var trusted = new HashSet<string>(StringComparer.Ordinal) { "S-1-5-18", "S-1-5-32-544" };
        for (FileSystemInfo? item = directory ? new DirectoryInfo(path) : new FileInfo(path); item is not null;)
        {
            if (!item.Exists || (item.Attributes & FileAttributes.ReparsePoint) != 0)
                throw new InvalidOperationException("AUTOLOGON_UNSAFE_POLICY_PATH");
            var acl = item is DirectoryInfo d ? (FileSystemSecurity)d.GetAccessControl() : ((FileInfo)item).GetAccessControl();
            var owner = acl.GetOwner(typeof(SecurityIdentifier)) as SecurityIdentifier;
            if (item.FullName == path && (owner is null || !trusted.Contains(owner.Value)))
                throw new InvalidOperationException("AUTOLOGON_UNTRUSTED_POLICY_OWNER");
            var dangerous = FileSystemRights.ChangePermissions | FileSystemRights.TakeOwnership | FileSystemRights.DeleteSubdirectoriesAndFiles;
            if (item.FullName == path) dangerous |= FileSystemRights.Write | FileSystemRights.Delete;
            foreach (FileSystemAccessRule rule in acl.GetAccessRules(true, true, typeof(SecurityIdentifier)))
                if (rule.AccessControlType == AccessControlType.Allow && !trusted.Contains(rule.IdentityReference.Value)
                    && (rule.PropagationFlags & PropagationFlags.InheritOnly) == 0 && (rule.FileSystemRights & dangerous) != 0)
                    throw new InvalidOperationException("AUTOLOGON_POLICY_WRITABLE_BY_NONADMIN");
            item = item switch { DirectoryInfo dir => dir.Parent, FileInfo file => file.Directory, _ => null };
        }
    }

    private static (string User, string Domain, string Sid) ResolvePinnedConsoleIdentity()
    {
        using var current = WindowsIdentity.GetCurrent();
        if (current.IsSystem || current.User is null || Process.GetCurrentProcess().SessionId <= 0
            || WTSGetActiveConsoleSessionId() != (uint)Process.GetCurrentProcess().SessionId)
            throw new InvalidOperationException("PINNED_CONSOLE_REQUIRED");
        using var system = new System.Management.ManagementObjectSearcher("SELECT UserName FROM Win32_ComputerSystem");
        var qualified = system.Get().Cast<System.Management.ManagementObject>()
            .Select(x => x["UserName"] as string).FirstOrDefault(x => !string.IsNullOrWhiteSpace(x))
            ?? throw new InvalidOperationException("ACTIVE_CONSOLE_REQUIRED");
        var account = new NTAccount(qualified);
        var sid = ((SecurityIdentifier)account.Translate(typeof(SecurityIdentifier))).Value;
        var parts = qualified.Split('\\', 2); if (parts.Length != 2 || !parts[0].Equals(Environment.MachineName, StringComparison.OrdinalIgnoreCase) || current.User.Value != sid) throw new InvalidOperationException("SAME_LOCAL_CONSOLE_ACCOUNT_REQUIRED");
        var manifestPath = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData), "CloudGime", "provisioning", "shared-gaming-desktop.identity.json");
        if (!File.Exists(manifestPath)) throw new InvalidOperationException("SHARED_DESKTOP_NOT_PINNED");
        RequireProtectedPath(manifestPath, false);
        using var document = JsonDocument.Parse(File.ReadAllBytes(manifestPath));
        if (document.RootElement.GetProperty("schemaVersion").GetInt32() != 1)
            throw new InvalidOperationException("SHARED_DESKTOP_SCHEMA_INVALID");
        using var services = RegistryKey.OpenBaseKey(RegistryHive.LocalMachine, RegistryView.Registry64).OpenSubKey(@"SYSTEM\CurrentControlSet\Services\SentinelNodeBroker");
        if (services?.GetValue("Start") is not int start || start != 2)
            throw new InvalidOperationException("AUTOMATIC_SENTINEL_BROKER_REQUIRED");
        var expected = document.RootElement.GetProperty("desktopUserSid").GetString();
        if (!string.Equals(expected, sid, StringComparison.Ordinal)) throw new InvalidOperationException("SHARED_DESKTOP_SID_MISMATCH");
        return (parts[1], parts[0], sid);
    }

    private sealed class WindowsStore : IAutoLogonStore
    {
        public AutoLogonSnapshot Read()
        {
            using var winlogon = RegistryKey.OpenBaseKey(RegistryHive.LocalMachine, RegistryView.Registry64).OpenSubKey(PolicyPath);
            using var owner = RegistryKey.OpenBaseKey(RegistryHive.LocalMachine, RegistryView.Registry64).OpenSubKey(OwnerPath);
            return new((string?)winlogon?.GetValue("AutoAdminLogon"), (string?)winlogon?.GetValue("DefaultUserName"),
                (string?)winlogon?.GetValue("DefaultDomainName"), (string?)owner?.GetValue("OwnerSid"),
                LsaSecret.Read(SecretName), winlogon?.GetValue("DefaultPassword") is not null);
        }
        public void SetEnabled(string? value)
        {
            using var key = RegistryKey.OpenBaseKey(RegistryHive.LocalMachine, RegistryView.Registry64).CreateSubKey(PolicyPath, true)!;
            if (key.GetValue("DefaultPassword") is not null) throw new InvalidOperationException("AUTOLOGON_EXTERNALLY_MODIFIED");
            if (value is null) key.DeleteValue("AutoAdminLogon", false); else key.SetValue("AutoAdminLogon", value, RegistryValueKind.String);
        }
        public void SetSecret(string? value) => LsaSecret.Write(SecretName, value);
        public void SetIdentity(string? user, string? domain, string? sid)
        {
            using var root = RegistryKey.OpenBaseKey(RegistryHive.LocalMachine, RegistryView.Registry64);
            using var winlogon = root.CreateSubKey(PolicyPath, true)!; using var owner = root.CreateSubKey(OwnerPath, true)!;
            foreach (var (name, value) in new[] { ("DefaultUserName", user), ("DefaultDomainName", domain) })
                if (value is null) winlogon.DeleteValue(name, false); else winlogon.SetValue(name, value, RegistryValueKind.String);
            if (sid is null) owner.DeleteValue("OwnerSid", false); else owner.SetValue("OwnerSid", sid, RegistryValueKind.String);
        }
    }

    private static class LsaSecret
    {
        [StructLayout(LayoutKind.Sequential)] private struct LSA_UNICODE_STRING { public ushort Length, MaximumLength; public IntPtr Buffer; }
        [StructLayout(LayoutKind.Sequential)] private struct LSA_OBJECT_ATTRIBUTES { public uint Length; public IntPtr RootDirectory, ObjectName; public uint Attributes; public IntPtr SecurityDescriptor, SecurityQualityOfService; }
        [DllImport("advapi32.dll")] static extern uint LsaOpenPolicy(IntPtr system, ref LSA_OBJECT_ATTRIBUTES attr, uint access, out IntPtr handle);
        [DllImport("advapi32.dll")] static extern uint LsaStorePrivateData(IntPtr handle, ref LSA_UNICODE_STRING key, IntPtr data);
        [DllImport("advapi32.dll")] static extern uint LsaRetrievePrivateData(IntPtr handle, ref LSA_UNICODE_STRING key, out IntPtr data);
        [DllImport("advapi32.dll")] static extern uint LsaClose(IntPtr handle);
        [DllImport("advapi32.dll")] static extern uint LsaFreeMemory(IntPtr buffer);
        const uint POLICY_CREATE_SECRET = 0x20, POLICY_GET_PRIVATE_INFORMATION = 0x4;
        static LSA_UNICODE_STRING U(string text, out IntPtr ptr) { ptr = Marshal.StringToHGlobalUni(text); return new() { Buffer = ptr, Length = (ushort)(text.Length * 2), MaximumLength = (ushort)((text.Length + 1) * 2) }; }
        static IntPtr Open(uint access) { var a = new LSA_OBJECT_ATTRIBUTES { Length = (uint)Marshal.SizeOf<LSA_OBJECT_ATTRIBUTES>() }; if (LsaOpenPolicy(IntPtr.Zero, ref a, access, out var h) != 0) throw new InvalidOperationException("LSA_OPEN_FAILED"); return h; }
        internal static string? Read(string name) { var h = Open(POLICY_GET_PRIVATE_INFORMATION); var k = U(name, out var kp); try { var status = LsaRetrievePrivateData(h, ref k, out var p); if (status == 0xC0000034) return null; if (status != 0) throw new InvalidOperationException("LSA_READ_FAILED"); try { var s = Marshal.PtrToStructure<LSA_UNICODE_STRING>(p); return s.Length == 0 ? "" : Marshal.PtrToStringUni(s.Buffer, s.Length / 2); } finally { var secret = Marshal.PtrToStructure<LSA_UNICODE_STRING>(p); if (secret.Buffer != IntPtr.Zero) for (var i = 0; i < secret.Length; i++) Marshal.WriteByte(secret.Buffer, i, 0); LsaFreeMemory(p); } } finally { Marshal.FreeHGlobal(kp); LsaClose(h); } }
        internal static void Write(string name, string? value) { var h = Open(POLICY_CREATE_SECRET); var k = U(name, out var kp); IntPtr vp = IntPtr.Zero, structure = IntPtr.Zero; try { if (value is null) { if (LsaStorePrivateData(h, ref k, IntPtr.Zero) != 0) throw new InvalidOperationException("LSA_DELETE_FAILED"); return; } var v = U(value, out vp); structure = Marshal.AllocHGlobal(Marshal.SizeOf<LSA_UNICODE_STRING>()); Marshal.StructureToPtr(v, structure, false); if (LsaStorePrivateData(h, ref k, structure) != 0) throw new InvalidOperationException("LSA_STORE_FAILED"); } finally { if (structure != IntPtr.Zero) Marshal.FreeHGlobal(structure); if (vp != IntPtr.Zero) Marshal.ZeroFreeGlobalAllocUnicode(vp); Marshal.FreeHGlobal(kp); LsaClose(h); } }
    }
}
