using System.Security.AccessControl;
using System.Security.Cryptography;
using CloudgimeHostBootstrap;

// Real Windows file operations using the production transaction class, but NO
// services, registry, DPAPI decryption, activation API, or installed node paths.
internal static class MatchedUpgradeTests
{
    internal static int Run()
    {
        var root=Path.Combine(Path.GetTempPath(),"cg-matched-upgrade-"+Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(root);
        var assertions=0;
        void Check(bool value,string code){assertions++;if(!value)throw new InvalidOperationException(code);}
        var components=new[]{"app/Launcher.exe","app/resources/app.asar","app/resources/backend/Helper.exe","app/resources/backend/Shell.exe","broker/Broker.exe","app/resources/marker.json","policy/shell.json","policy/activation-disabled.json"};
        var preserved=new[]{"provisioning/machine-identity.dpapi","provisioning/sentinel.credential.dpapi","provisioning/host_app.credential.dpapi","provisioning/renewal.credential.dpapi","provisioning/manifest.json","profiles/user-A/save.bin","profiles/user-B/save.bin","profiles/retirement-journal.json"};
        string Hash(string file)=>Convert.ToHexString(SHA256.HashData(File.ReadAllBytes(file)));
        string Acl(string file)=>new FileInfo(file).GetAccessControl(AccessControlSections.Access|AccessControlSections.Owner|AccessControlSections.Group).GetSecurityDescriptorSddlForm(AccessControlSections.Access|AccessControlSections.Owner|AccessControlSections.Group);
        try
        {
            // Every cut point simulates a catchable failure after N real replaces.
            // This is not arbitrary process-crash/power-loss recovery qualification.
            for(var failAfter=0;failAfter<=components.Length;failAfter++)
            {
                var run=Path.Combine(root,"cut-"+failAfter);Directory.CreateDirectory(run);
                var sources=new Dictionary<string,string>();var before=new Dictionary<string,(string Hash,string? Acl)>();
                foreach(var relative in components.Concat(preserved))
                {
                    var file=Path.Combine(run,relative);Directory.CreateDirectory(Path.GetDirectoryName(file)!);
                    File.WriteAllText(file,"OLD-"+relative);before[file]=(Hash(file),OperatingSystem.IsWindows()?Acl(file):null);
                }
                foreach(var relative in components){var source=Path.Combine(run,"candidate",relative);Directory.CreateDirectory(Path.GetDirectoryName(source)!);File.WriteAllText(source,"NEW-"+relative);sources[relative]=source;}
                var transaction=new UniversalFileTransaction(Path.Combine(run,"backup"));
                try
                {
                    for(var i=0;i<components.Length;i++)
                    {
                        if(i==failAfter)throw new IOException("INJECTED_COMPONENT_FAILURE");
                        transaction.Replace(sources[components[i]],Path.Combine(run,components[i]));
                        Check(File.Exists(Path.Combine(transaction.RecoveryRoot,"files.json")),"DURABLE_INTENT_MISSING");
                    }
                    if(failAfter==components.Length)throw new IOException("INJECTED_POST_REPLACE_FAILURE");
                }
                catch(IOException e) when(e.Message.StartsWith("INJECTED_")){transaction.Rollback();}
                foreach(var (file,snapshot) in before)
                {
                    Check(Hash(file)==snapshot.Hash,"ROLLBACK_OR_PRESERVED_CONTENT_CHANGED");
                    if(snapshot.Acl is not null)Check(Acl(file)==snapshot.Acl,"ROLLBACK_OR_PRESERVED_ACL_CHANGED");
                }
                Check(File.ReadAllText(Path.Combine(transaction.RecoveryRoot,"result.txt"))=="ROLLED_BACK","ROLLBACK_RECEIPT_MISSING");
            }
            var success=Path.Combine(root,"success");Directory.CreateDirectory(success);
            var old=Path.Combine(success,"broker.exe");var next=Path.Combine(success,"new.exe");File.WriteAllText(old,"OLD");File.WriteAllText(next,"NEW");
            var committed=new UniversalFileTransaction(Path.Combine(success,"backup"));committed.Replace(next,old);committed.Commit();
            Check(File.ReadAllText(old)=="NEW","COMMIT_NOT_APPLIED");
            Check(File.ReadAllText(Path.Combine(committed.RecoveryRoot,"000000.original"))=="OLD","COMMITTED_BACKUP_NOT_RETAINED");
            committed.Rollback();Check(File.ReadAllText(old)=="OLD","EXPLICIT_POST_COMMIT_ROLLBACK_FAILED");
            committed.Rollback();Check(File.ReadAllText(old)=="OLD","REPEATED_ROLLBACK_CHANGED_DATA");
            var duplicate=new UniversalFileTransaction(Path.Combine(success,"backup"));duplicate.Replace(next,old);
            var rejected=false;try{duplicate.Replace(next,old);}catch(InvalidOperationException e) when(e.Message=="DUPLICATE_TRANSACTION_TARGET"){rejected=true;}
            Check(rejected,"DUPLICATE_TARGET_ACCEPTED");duplicate.Rollback();
            // Missing source occurs after the journal entry. Existing data must still
            // restore correctly when the production caller catches that failure.
            var missing=new UniversalFileTransaction(Path.Combine(success,"backup"));
            try{missing.Replace(Path.Combine(success,"absent.exe"),old);throw new Exception("MISSING_SOURCE_ACCEPTED");}
            catch(FileNotFoundException){missing.Rollback();}
            Check(File.ReadAllText(old)=="OLD","MISSING_SOURCE_ROLLBACK_FAILED");
            return assertions;
        }
        finally{Directory.Delete(root,true);}
    }
}
