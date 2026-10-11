using System.Security.AccessControl;
using System.Security.Cryptography;
using System.Text.Json;
using System.Text.Json.Serialization;
namespace CloudgimeHostBootstrap;

internal sealed record ManagedUpgradeEntry(string Target,string Source,string? BeforeHash,string AfterHash,string? Security,bool Applied);
internal sealed record ManagedUpgradeJournal(int Version,string OperationId,string Phase,int Cursor,List<ManagedUpgradeEntry> Entries);

// Recoverable file engine. Caller supplies fixed protected target roots and an
// administrator-only operation directory; it must hold the machine-wide lock.
// No service start/stop, credential provisioning, profile cleanup, or licensing.
internal sealed class ManagedUpgradeTransaction
{
    private readonly string root;private readonly string[] allowedRoots;private ManagedUpgradeJournal state;
    private static readonly JsonSerializerOptions Json=new(){UnmappedMemberHandling=JsonUnmappedMemberHandling.Disallow};
    private string JournalPath=>Path.Combine(root,"transaction.json");
    internal string Phase=>state.Phase;
    internal IReadOnlyList<ManagedUpgradeEntry> Entries=>state.Entries;
    internal string OperationId=>state.OperationId;
    private ManagedUpgradeTransaction(string root,string[] allowed,ManagedUpgradeJournal state){this.root=Path.GetFullPath(root);allowedRoots=allowed.Select(Path.GetFullPath).ToArray();this.state=state;ValidateJournal();}
    internal static string Hash(string p){using var f=File.OpenRead(p);return Convert.ToHexString(SHA256.HashData(f)).ToLowerInvariant();}
    private static string? Sddl(string p)=>OperatingSystem.IsWindows()?new FileInfo(p).GetAccessControl(AccessControlSections.Access|AccessControlSections.Owner|AccessControlSections.Group).GetSecurityDescriptorSddlForm(AccessControlSections.Access|AccessControlSections.Owner|AccessControlSections.Group):null;
    private static void Security(string p,string? s){if(s==null||!OperatingSystem.IsWindows())return;var acl=new FileSecurity();acl.SetSecurityDescriptorSddlForm(s,AccessControlSections.Access|AccessControlSections.Owner|AccessControlSections.Group);new FileInfo(p).SetAccessControl(acl);}
    internal static ManagedUpgradeTransaction Prepare(string root,string id,string[] allowed,IEnumerable<(string Source,string Target)> input)
    {
        if(!Guid.TryParseExact(id,"D",out var op)||op==Guid.Empty)throw new InvalidDataException("UPGRADE_ID_INVALID");
        root=Path.GetFullPath(root);UniversalBundleContract.RejectReparse(root);if(File.Exists(Path.Combine(root,"transaction.json")))throw new InvalidDataException("UPGRADE_ALREADY_PREPARED");
        Directory.CreateDirectory(root);Directory.CreateDirectory(Path.Combine(root,"backup"));Directory.CreateDirectory(Path.Combine(root,"stage"));
        var tx=new ManagedUpgradeTransaction(root,allowed,new(1,id,"PREPARING",0,[]));var targets=new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        foreach(var item in input)
        {
            var target=Path.GetFullPath(item.Target);tx.RequireTarget(target);if(!targets.Add(target))throw new InvalidDataException("UPGRADE_DUPLICATE_TARGET");
            UniversalBundleContract.RejectReparse(item.Source);if(!File.Exists(item.Source)||!Directory.Exists(Path.GetDirectoryName(target)))throw new InvalidDataException("UPGRADE_INPUT_MISSING");
            var index=tx.state.Entries.Count;var source=tx.Stage(index);var backup=tx.Backup(index);
            if(File.Exists(source)||File.Exists(backup))throw new InvalidDataException("UPGRADE_STAGE_ALREADY_EXISTS");
            File.Copy(item.Source,source,false);Flush(source);var hash=Hash(source);var old=File.Exists(target)?Hash(target):null;var acl=old!=null?Sddl(target):null;
            if(old!=null){File.Copy(target,backup,false);Flush(backup);if(Hash(backup)!=old)throw new IOException("UPGRADE_BACKUP_HASH_MISMATCH");}
            tx.state.Entries.Add(new(target,source,old,hash,acl,false));
        }
        if(tx.state.Entries.Count==0||tx.state.Entries.Count>8192)throw new InvalidDataException("UPGRADE_ENTRY_COUNT_INVALID");
        tx.state=tx.state with{Phase="PREPARED"};tx.Persist();return tx;
    }
    internal static ManagedUpgradeTransaction Open(string root,string[] allowed)
    {
        UniversalBundleContract.RejectReparse(Path.Combine(root,"transaction.json"));
        if(new FileInfo(Path.Combine(root,"transaction.json")).Length>8*1024*1024)throw new InvalidDataException("UPGRADE_JOURNAL_TOO_LARGE");
        using var f=File.OpenRead(Path.Combine(root,"transaction.json"));var s=JsonSerializer.Deserialize<ManagedUpgradeJournal>(f,Json)??throw new InvalidDataException("UPGRADE_JOURNAL_MISSING");
        return new(root,allowed,s);
    }
    internal void Apply(Action beforeEach,Action<int>? afterReplace=null)
    {
        if(state.Phase=="COMMITTED"){VerifyNew();return;}
        if(state.Phase is not("PREPARED" or "APPLYING"))throw new InvalidDataException("UPGRADE_PHASE_INVALID");
        state=state with{Phase="APPLYING"};Persist();
        for(int i=0;i<state.Entries.Count;i++)
        {
            beforeEach();var e=state.Entries[i];RequireTarget(e.Target);VerifyBackup(i);if(Hash(e.Source)!=e.AfterHash)throw new IOException("UPGRADE_STAGED_FILE_CHANGED");
            var current=File.Exists(e.Target)?Hash(e.Target):null;
            if(e.Applied){if(current!=e.AfterHash)throw new IOException("UPGRADE_EXTERNAL_CHANGE");continue;}
            // Only the durable cursor permits recovery of a crash after replace but
            // before the Applied bit. Identical-before/after files are still safe.
            if(current!=e.BeforeHash&&!(state.Cursor==i&&current==e.AfterHash))throw new IOException("UPGRADE_EXTERNAL_CHANGE");
            if(e.BeforeHash!=null&&current==e.BeforeHash&&Sddl(e.Target)!=e.Security)throw new IOException("UPGRADE_ACL_EXTERNAL_CHANGE");
            state=state with{Cursor=i};Persist();
            ReplaceAtomic(e.Source,e.Target,e.Security,e.AfterHash);afterReplace?.Invoke(i);
            state.Entries[i]=e with{Applied=true};state=state with{Cursor=i+1};Persist();
        }
        beforeEach();VerifyNew();state=state with{Phase="COMMITTED"};Persist();
    }
    internal void Rollback(Action beforeEach)
    {
        if(state.Phase=="ROLLED_BACK"){VerifyOriginal();return;}
        // Preflight the WHOLE rollback before the first write. Unknown current
        // content/ACL is operator conflict, never overwritten to force a pass.
        for(int i=0;i<state.Entries.Count;i++)
        {
            var e=state.Entries[i];RequireTarget(e.Target);VerifyBackup(i);var current=File.Exists(e.Target)?Hash(e.Target):null;
            if(current!=e.BeforeHash&&current!=e.AfterHash)throw new IOException("ROLLBACK_EXTERNAL_CHANGE");
            if(current!=null&&e.Security!=null&&Sddl(e.Target)!=e.Security)throw new IOException("ROLLBACK_ACL_EXTERNAL_CHANGE");
        }
        state=state with{Phase="ROLLING_BACK"};Persist();
        for(int i=state.Entries.Count-1;i>=0;i--)
        {
            beforeEach();var e=state.Entries[i];RequireTarget(e.Target);VerifyBackup(i);
            var current=File.Exists(e.Target)?Hash(e.Target):null;if(current!=e.BeforeHash&&current!=e.AfterHash)throw new IOException("ROLLBACK_EXTERNAL_CHANGE");
            if(current==e.BeforeHash)
            {
                // Untouched/already-restored entries need no replacement. This
                // lets rollback recover the earlier entries when a later file is
                // locked against replacement but its original bytes remain intact.
                state.Entries[i]=e with{Applied=false};state=state with{Cursor=i};Persist();continue;
            }
            if(e.BeforeHash==null)
            {
                // Preserve transaction-created files for audit instead of delete.
                if(File.Exists(e.Target)){var quarantine=Path.Combine(root,"added-"+i+".retired");if(File.Exists(quarantine))throw new IOException("ROLLBACK_QUARANTINE_EXISTS");File.Move(e.Target,quarantine);}
            }
            else ReplaceAtomic(Backup(i),e.Target,e.Security,e.BeforeHash);
            state.Entries[i]=e with{Applied=false};state=state with{Cursor=i};Persist();
        }
        VerifyOriginal();state=state with{Phase="ROLLED_BACK"};Persist();
    }
    internal void VerifyNew(){foreach(var e in state.Entries){RequireTarget(e.Target);if(!File.Exists(e.Target)||Hash(e.Target)!=e.AfterHash||e.Security!=null&&Sddl(e.Target)!=e.Security)throw new IOException("UPGRADE_READBACK_FAILED");}}
    internal void VerifyOriginal(){foreach(var e in state.Entries){RequireTarget(e.Target);if(e.BeforeHash==null?File.Exists(e.Target):!File.Exists(e.Target)||Hash(e.Target)!=e.BeforeHash||Sddl(e.Target)!=e.Security)throw new IOException("ROLLBACK_READBACK_FAILED");}}
    private void VerifyBackup(int i){var e=state.Entries[i];UniversalBundleContract.RejectReparse(e.Source);UniversalBundleContract.RejectReparse(Backup(i));if(e.BeforeHash!=null&&(!File.Exists(Backup(i))||Hash(Backup(i))!=e.BeforeHash))throw new IOException("ROLLBACK_BACKUP_CHANGED");}
    private void RequireTarget(string p){if(!Path.GetFullPath(p).Equals(p,StringComparison.OrdinalIgnoreCase))throw new InvalidDataException("UPGRADE_TARGET_NOT_CANONICAL");UniversalBundleContract.RejectReparse(p);if(!allowedRoots.Any(r=>p.StartsWith(r.TrimEnd(Path.DirectorySeparatorChar)+Path.DirectorySeparatorChar,StringComparison.OrdinalIgnoreCase)))throw new InvalidDataException("UPGRADE_TARGET_OUTSIDE_ROOTS");}
    private string Stage(int i)=>Path.Combine(root,"stage",i.ToString("D6")+".next");private string Backup(int i)=>Path.Combine(root,"backup",i.ToString("D6")+".old");
    private void ValidateJournal()
    {
        if(state.Version!=1||!Guid.TryParseExact(state.OperationId,"D",out var g)||g==Guid.Empty||state.Phase is not("PREPARING" or "PREPARED" or "APPLYING" or "COMMITTED" or "ROLLING_BACK" or "ROLLED_BACK")||state.Entries.Count>8192||state.Cursor<0||state.Cursor>state.Entries.Count)throw new InvalidDataException("UPGRADE_JOURNAL_INVALID");
        var targets=new HashSet<string>(StringComparer.OrdinalIgnoreCase);for(int i=0;i<state.Entries.Count;i++){var e=state.Entries[i];RequireTarget(e.Target);if(!targets.Add(e.Target)||e.Source!=Stage(i)||!IsHash(e.AfterHash)||e.BeforeHash!=null&&!IsHash(e.BeforeHash))throw new InvalidDataException("UPGRADE_JOURNAL_ENTRY_INVALID");}
    }
    private static bool IsHash(string s)=>s.Length==64&&s.All(c=>char.IsAsciiHexDigit(c)&&!char.IsUpper(c));
    private void Persist(){ValidateJournal();var next=Path.Combine(root,"journal-"+Guid.NewGuid().ToString("N")+".tmp");using(var f=new FileStream(next,FileMode.CreateNew,FileAccess.Write,FileShare.None,4096,FileOptions.WriteThrough)){JsonSerializer.Serialize(f,state,Json);f.Flush(true);}File.Move(next,JournalPath,true);}
    private static void Flush(string p){using var f=new FileStream(p,FileMode.Open,FileAccess.ReadWrite,FileShare.Read);f.Flush(true);}
    private static void ReplaceAtomic(string source,string target,string? security,string hash)
    {
        var tmp=target+".upgrade-"+Guid.NewGuid().ToString("N");UniversalBundleContract.RejectReparse(tmp);
        try
        {
            File.Copy(source,tmp,false);Flush(tmp);Security(tmp,security);if(Hash(tmp)!=hash)throw new IOException("UPGRADE_COPY_CORRUPT");File.Move(tmp,target,true);
            if(Hash(target)!=hash)throw new IOException("UPGRADE_ATOMIC_REPLACE_FAILED");
        }
        finally
        {
            // This exact GUID temp was created by this method, not user data.
            // Remove failed staging so it cannot leak into the installed inventory.
            if(File.Exists(tmp))File.Delete(tmp);
        }
    }
}
