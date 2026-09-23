using System.Text.Json;
using System.Security.AccessControl;

namespace CloudgimeHostBootstrap;

// File-only transaction; service/task/driver changes are intentionally NOT done in an upgrade.
// All originals remain under the administrator-only recovery directory after commit.
internal sealed class UniversalFileTransaction
{
    private sealed record Entry(string Target, string Backup, bool Existed, string? Security);
    private readonly string root;
    private readonly List<Entry> entries = [];
    internal UniversalFileTransaction(string recoveryRoot)
    {
        root = Path.Combine(recoveryRoot, Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(root);
    }
    internal string RecoveryRoot => root;
    internal void Replace(string source, string target)
    {
        UniversalBundleContract.RejectReparse(source);
        UniversalBundleContract.RejectReparse(target);
        if (entries.Any(e => e.Target.Equals(target, StringComparison.OrdinalIgnoreCase))) throw new InvalidOperationException("DUPLICATE_TRANSACTION_TARGET");
        var existed = File.Exists(target);
        var backup = Path.Combine(root, entries.Count.ToString("D6") + ".original");
        var security = existed && OperatingSystem.IsWindows()
            ? new FileInfo(target).GetAccessControl(AccessControlSections.Access | AccessControlSections.Owner | AccessControlSections.Group)
                .GetSecurityDescriptorSddlForm(AccessControlSections.Access | AccessControlSections.Owner | AccessControlSections.Group) : null;
        if (existed) File.Copy(target, backup, false);
        entries.Add(new(target, backup, existed, security));
        // Durable intent precedes any replacement. Recovery never needs a credential from logs.
        using (var journal = new FileStream(Path.Combine(root, "files.json"), FileMode.Create, FileAccess.Write, FileShare.None))
        { JsonSerializer.Serialize(journal, entries); journal.Flush(true); }
        Directory.CreateDirectory(Path.GetDirectoryName(target)!);
        var next = target + ".universal-next";
        if (File.Exists(next)) throw new IOException("UNRESOLVED_UPGRADE_TEMPORARY_FILE");
        try
        {
            File.Copy(source, next, false);
            RestoreSecurity(next, security);
            File.Move(next, target, true);
        }
        finally { if (File.Exists(next)) File.Delete(next); }
    }
    internal void Rollback()
    {
        List<Exception> errors = [];
        foreach (var entry in entries.AsEnumerable().Reverse())
        {
            try
            {
                UniversalBundleContract.RejectReparse(entry.Target);
                if (entry.Existed) { File.Copy(entry.Backup, entry.Target, true); RestoreSecurity(entry.Target, entry.Security); }
                else if (File.Exists(entry.Target)) File.Delete(entry.Target);
            }
            catch (Exception e) { errors.Add(e); }
        }
        if (errors.Count > 0) throw new AggregateException("UPGRADE_ROLLBACK_INCOMPLETE: original files remain in the recovery directory", errors);
        File.WriteAllText(Path.Combine(root, "result.txt"), "ROLLED_BACK");
    }
    private static void RestoreSecurity(string file, string? sddl)
    {
        if (sddl is null || !OperatingSystem.IsWindows()) return;
        var security = new FileSecurity();
        security.SetSecurityDescriptorSddlForm(sddl, AccessControlSections.Access | AccessControlSections.Owner | AccessControlSections.Group);
        new FileInfo(file).SetAccessControl(security);
    }
    internal void Commit() => File.WriteAllText(Path.Combine(root, "result.txt"), "COMMITTED");
}
