using System.Security.AccessControl;
using System.Security.Cryptography;
using System.Security.Principal;
using System.Text.Json;
using Microsoft.Win32;
namespace CloudgimeHostBootstrap;

internal sealed record HeadlessFreshJournal(int Version,string OperationId,string PackageHash,string OperatorSid,
    string Stage,string? Pending,string? Error);

// Fresh-install lane, separate from the managed upgrade engine. Every durable
// effect has a preceding intent; a retry may use only this package/operator.
// Never deletes prior managed state, claims READY, or restarts Windows.
internal static class HeadlessFreshInstall
{
    internal static readonly string[] Stages=["STAGING","FILES_STAGED","NODE_PROVISIONED","RUNTIME_INSTALLED","ARMED"];
    private const string JournalName="fresh-install.json";
    private static readonly JsonSerializerOptions Json=new(){UnmappedMemberHandling=System.Text.Json.Serialization.JsonUnmappedMemberHandling.Disallow};
    internal static void Run(string package,ReadOnlyMemory<char> license)
    {
        RequireOperator();using var me=WindowsIdentity.GetCurrent();var owner=me.User!.Value;
        package=Path.GetFullPath(package);HeadlessBundleContract.Validate(package);
        var packageHash=Hash(Path.Combine(package,HeadlessBundleContract.ManifestName));
        var currentImage=Environment.ProcessPath??throw new InvalidDataException("HEADLESS_ENGINE_PATH_INVALID");
        if(Hash(currentImage)!=Hash(Path.Combine(package,"app","cloudgime-host-bootstrap.exe")))throw new InvalidDataException("HEADLESS_ENGINE_MISMATCH");
        var parent=Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData),"CloudGime");
        var operationRoot=Path.Combine(parent,"fresh-install-private");
        var target=Path.Combine(parent,"Host");
        var app=Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles),"Cloudgime Host");
        EnsureDirectory(parent,false);EnsureDirectory(operationRoot,true);
        using var gate=new FileStream(Path.Combine(operationRoot,"install.lock"),FileMode.OpenOrCreate,FileAccess.ReadWrite,FileShare.None);
        var journal=Read(operationRoot);
        if(journal==null)
        {
            // A new journal cannot legitimize an existing installation.
            foreach(var path in new[]{target,app,Path.Combine(parent,"NodeBroker"),Path.Combine(parent,"RentalShell"),UniversalMachineBoundary.ProvisioningRoot})
                if(Directory.Exists(path)||File.Exists(path))throw new InvalidDataException("HEADLESS_FRESH_STATE_REQUIRED");
            if(Directory.Exists(Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles),"Sentinel Launcher")))throw new InvalidDataException("HEADLESS_FRESH_STATE_REQUIRED");
            foreach(var name in new[]{"CloudgimeHost-Host","CloudgimeRuntime-Host","SentinelNodeBroker"})
                if(ServiceExists(name))throw new InvalidDataException("HEADLESS_FRESH_STATE_REQUIRED");
            journal=new(1,Guid.NewGuid().ToString("D"),packageHash,owner,"STAGING",null,null);Save(operationRoot,null,journal);
        }
        Validate(journal);
        if(journal.PackageHash!=packageHash||journal.OperatorSid!=owner)throw new InvalidDataException("HEADLESS_FRESH_BINDING_CHANGED");
        var state=journal;
        void Persist(HeadlessFreshJournal next){Save(operationRoot,state,next);state=next;}
        void Effect(string action,string next,Action effect)
        {
            Persist(state with{Pending=action,Error=null});
            try{RequireOperator();effect();Persist(state with{Stage=next,Pending=null,Error=null});}
            catch(Exception ex)
            {
                var code=ex is InvalidDataException&&System.Text.RegularExpressions.Regex.IsMatch(ex.Message,@"\A[A-Z0-9_]{1,100}\z")?ex.Message:"HEADLESS_FRESH_STEP_FAILED";
                Persist(state with{Error=code});throw new InvalidDataException(code);
            }
        }
        if(state.Stage=="STAGING")Effect("STAGE","FILES_STAGED",()=>
        {
            EnsureDirectory(target,false);EnsureDirectory(app,false);
            foreach(var relative in UniversalBundleContract.Walk(Path.Combine(package,"bundle")))
                StageFile(Path.Combine(package,"bundle",relative),Path.Combine(target,relative));
            StageFile(Path.Combine(package,"app","cloudgime-host-bootstrap.exe"),Path.Combine(app,"cloudgime-host-bootstrap.exe"));
            UniversalBundleContract.RequireInstallable(target);
        });
        var installer=Path.Combine(target,"host-installer.exe");
        var helper=Path.Combine(target,"host-control","SentinelHelper.exe");
        var bootstrap=Path.Combine(target,"host-control","cloudgime-host-bootstrap.exe");
        // Pin immutable executables while running each stage. Mutable runtime
        // config is produced only after FILES_STAGED; never re-adopt it as payload.
        foreach(var relative in new[]{"host-installer.exe","host-control/SentinelHelper.exe","host-control/cloudgime-host-bootstrap.exe"})
        {
            var path=Path.Combine(target,relative);UniversalMachineBoundary.RequireProtected(path);
            if(Hash(path)!=Hash(Path.Combine(package,"bundle",relative)))throw new InvalidDataException("HEADLESS_INSTALLED_IMAGE_CHANGED");
        }
        if(state.Stage=="FILES_STAGED")Effect("ACTIVATE","NODE_PROVISIONED",()=>
        {
            EnsureDirectory(UniversalMachineBoundary.ProvisioningRoot,true);
            // The token travels only through stdin. This is the same atomic
            // installer issuer used by the NSIS silent path; it owns claim,
            // component credentials and its replay receipt.
            Child(installer,["--bundle-root",target,"provision","--activation-only","--profile","always_on_host","--api-base-url","https://cloudgime.my.id"],target,license);
            VerifyProvisioning();
            HeadlessPublicRoute.Configure(target);
        });
        VerifyProvisioning();
        if(state.Stage=="NODE_PROVISIONED")Effect("RUNTIME","RUNTIME_INSTALLED",()=>
        {
            // Ordinary Sentinel installer has a defer flag; no gate may start
            // before the owned disabled account + first-boot policy exist.
            Child(Path.Combine(target,"sentinel","SentinelLauncherSetup.exe"),["/S","/DEFERBROKERSTART"],target);
            var installedResources=Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles),"Sentinel Launcher","resources","backend");
            var installedHelper=Path.Combine(installedResources,"SentinelHelper.exe");
            var shellSource=Path.Combine(installedResources,"CloudGimeRentalShell.exe");
            UniversalMachineBoundary.RequireProtected(shellSource);
            EnsureDirectory(Path.Combine(parent,"RentalShell"),false);
            StageFile(shellSource,Path.Combine(parent,"RentalShell","CloudGimeRentalShell.exe"));
            if(Hash(installedHelper)!=Hash(helper))throw new InvalidDataException("HEADLESS_INSTALLED_HELPER_MISMATCH");
            Child(installer,["--bundle-root",target,"prepare-install"],target);
            Child(installer,["--bundle-root",target,"install-service"],target);
            HeadlessServiceRecovery.Configure();
            // Pairing needs the new renter's interactive desktop. Stage its
            // authenticated source for next logon, not in the SSH/admin session.
            HeadlessPairingTask.Stage(target);
            Child(installer,["--bundle-root",target,"start-service"],target);
        });
        if(state.Stage=="RUNTIME_INSTALLED")Effect("FIRST_BOOT","ARMED",()=>
            Child(bootstrap,["prepare-installer-first-boot",target,"--silent"],target));
        // Re-observe even a completed outer journal. A stale ARMED record never
        // bypasses the bound inner helper/shell/autologon verification.
        if(journal.Stage=="ARMED")Child(bootstrap,["prepare-installer-first-boot",target,"--silent"],target);
        // The caller gets RESTART_REQUIRED; no fabricated health or AVAILABLE.
        File.WriteAllText(Path.Combine(operationRoot,"result.json"),JsonSerializer.Serialize(new{operationId=state.OperationId,stage="ARMED",rentalReady=false,restartRequested=false,requiresOperatorConfirmedRestart=true}));
        Console.WriteLine("{\"ok\":true,\"stage\":\"ARMED\",\"rentalReady\":false,\"restartRequested\":false,\"requiresOperatorConfirmedRestart\":true}");
    }
    internal static void VerifyProvisioning()
    {
        var root=UniversalMachineBoundary.ProvisioningRoot;
        foreach(var name in new[]{"manifest.json","machine-identity.dpapi","host_app.credential.dpapi","sentinel.credential.dpapi","installer_renewal.credential.dpapi"})UniversalMachineBoundary.RequireProtected(Path.Combine(root,name));
        var manifest=UniversalBundleContract.ParseObject(Path.Combine(root,"manifest.json"));
        if((int?)manifest["schemaVersion"]!=2||(string?)manifest["status"]!="PROVISIONED_WAITING_HEARTBEATS"
            ||(string?)manifest["controlPlaneUrl"]!="https://cloudgime.my.id"||string.IsNullOrWhiteSpace((string?)manifest["binding"]?["pcId"]))
            throw new InvalidDataException("HEADLESS_PROVISIONING_UNVERIFIED");
    }
    private static void Child(string image,string[] args,string cwd,ReadOnlyMemory<char> secret=default)
    {UniversalMachineBoundary.RequireProtected(image);_=HeadlessSetupProcess.Run(image,args,cwd,secret,TimeSpan.FromMinutes(4));}
    private static bool ServiceExists(string name)
    {using var key=Registry.LocalMachine.OpenSubKey(@"SYSTEM\CurrentControlSet\Services\"+name);return key!=null;}
    private static void RequireOperator()
    {
        using var impersonation=WindowsIdentity.GetCurrent(true);using var me=WindowsIdentity.GetCurrent();
        if(impersonation!=null||me.IsSystem||me.User==null||!new WindowsPrincipal(me).IsInRole(WindowsBuiltInRole.Administrator)
            ||!System.Text.RegularExpressions.Regex.IsMatch(me.User.Value,@"\AS-1-5-21-(\d{1,10}-){3}\d{1,10}\z"))
            throw new InvalidDataException("HEADLESS_ELEVATED_OPERATOR_REQUIRED");
    }
    private static void EnsureDirectory(string path,bool privateAccess)
    {
        UniversalBundleContract.RejectReparse(path);
        if(Directory.Exists(path)){UniversalMachineBoundary.RequireProtected(path,true);return;}
        var parent=Path.GetDirectoryName(path)!;
        // ProgramData may allow creating unrelated siblings. Only replacement,
        // deletion or ACL/owner control of this boundary is dangerous here.
        for(var directory=new DirectoryInfo(parent);directory!=null;directory=directory.Parent)
        {
            UniversalBundleContract.RejectReparse(directory.FullName);
            foreach(FileSystemAccessRule rule in directory.GetAccessControl().GetAccessRules(true,true,typeof(SecurityIdentifier)))
                if(rule.AccessControlType==AccessControlType.Allow&&(rule.PropagationFlags&PropagationFlags.InheritOnly)==0
                    &&rule.IdentityReference.Value is not("S-1-5-18" or "S-1-5-32-544" or "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464")
                    &&(rule.FileSystemRights&(FileSystemRights.Delete|FileSystemRights.DeleteSubdirectoriesAndFiles|FileSystemRights.ChangePermissions|FileSystemRights.TakeOwnership))!=0)
                    throw new InvalidDataException("HEADLESS_PARENT_WRITABLE");
        }
        var acl=new DirectorySecurity();acl.SetAccessRuleProtection(true,false);acl.SetOwner(new SecurityIdentifier("S-1-5-32-544"));
        foreach(var sid in new[]{"S-1-5-18","S-1-5-32-544"})acl.AddAccessRule(new FileSystemAccessRule(new SecurityIdentifier(sid),FileSystemRights.FullControl,InheritanceFlags.ContainerInherit|InheritanceFlags.ObjectInherit,PropagationFlags.None,AccessControlType.Allow));
        if(!privateAccess)acl.AddAccessRule(new FileSystemAccessRule(new SecurityIdentifier("S-1-5-32-545"),FileSystemRights.ReadAndExecute,InheritanceFlags.ContainerInherit|InheritanceFlags.ObjectInherit,PropagationFlags.None,AccessControlType.Allow));
        new DirectoryInfo(path).Create(acl);UniversalMachineBoundary.RequireProtected(path,true);
    }
    private static void StageFile(string source,string target)
    {
        UniversalBundleContract.RejectReparse(source);UniversalBundleContract.RejectReparse(target);
        var parent=Path.GetDirectoryName(target)!;
        if(!Directory.Exists(parent)){EnsureParents(parent);}
        if(File.Exists(target)){UniversalMachineBoundary.RequireProtected(target);if(Hash(source)!=Hash(target))throw new InvalidDataException("HEADLESS_STAGE_FILE_CONFLICT");return;}
        var pending=target+".headless-next";
        if(File.Exists(pending)){if(Hash(source)!=Hash(pending))throw new InvalidDataException("HEADLESS_STAGE_FILE_CONFLICT");}
        else{using var input=File.OpenRead(source);using var output=new FileStream(pending,FileMode.CreateNew,FileAccess.Write,FileShare.None,65536,FileOptions.WriteThrough);input.CopyTo(output);output.Flush(true);}
        UniversalMachineBoundary.RequireProtected(pending);if(Hash(source)!=Hash(pending))throw new InvalidDataException("HEADLESS_STAGE_COPY_UNVERIFIED");File.Move(pending,target,false);
    }
    private static void EnsureParents(string path){if(!Directory.Exists(Path.GetDirectoryName(path)!))EnsureParents(Path.GetDirectoryName(path)!);EnsureDirectory(path,false);}
    private static string Hash(string path){using var input=new FileStream(path,FileMode.Open,FileAccess.Read,FileShare.Read);return Convert.ToHexString(SHA256.HashData(input)).ToLowerInvariant();}
    internal static void Validate(HeadlessFreshJournal j)
    {
        if(j==null||j.Version!=1||!Guid.TryParseExact(j.OperationId,"D",out var id)||id==Guid.Empty||j.PackageHash is not{Length:64}||!j.PackageHash.All(Uri.IsHexDigit)
            ||!System.Text.RegularExpressions.Regex.IsMatch(j.OperatorSid??"",@"\AS-1-5-21-(\d{1,10}-){3}\d{1,10}\z")||!Stages.Contains(j.Stage)
            ||j.Pending!=null&&(Array.IndexOf(Stages,j.Stage)>=4||j.Pending!=new[]{"STAGE","ACTIVATE","RUNTIME","FIRST_BOOT"}[Array.IndexOf(Stages,j.Stage)])
            ||j.Error!=null&&(j.Pending==null||!System.Text.RegularExpressions.Regex.IsMatch(j.Error,@"\A[A-Z0-9_]{1,100}\z")))throw new InvalidDataException("HEADLESS_FRESH_JOURNAL_INVALID");
    }
    internal static void RequireTransition(HeadlessFreshJournal? previous,HeadlessFreshJournal next)
    {
        Validate(next);
        if(previous==null){if(next.Stage!="STAGING"||next.Pending!=null||next.Error!=null)throw new InvalidDataException("HEADLESS_FRESH_TRANSITION_INVALID");return;}
        Validate(previous);
        if(previous.OperationId!=next.OperationId||previous.PackageHash!=next.PackageHash||previous.OperatorSid!=next.OperatorSid)throw new InvalidDataException("HEADLESS_FRESH_BINDING_CHANGED");
        int index=Array.IndexOf(Stages,previous.Stage);
        bool allowed=previous==next||index<4&&previous.Stage==next.Stage&&next.Pending==new[]{"STAGE","ACTIVATE","RUNTIME","FIRST_BOOT"}[index]
            ||index<4&&previous.Pending!=null&&previous.Error==null&&next.Stage==Stages[index+1]&&next.Pending==null&&next.Error==null;
        if(!allowed)throw new InvalidDataException("HEADLESS_FRESH_TRANSITION_INVALID");
    }
    private static HeadlessFreshJournal? Read(string root)
    {
        var path=Path.Combine(root,JournalName);if(!File.Exists(path))return null;UniversalMachineBoundary.RequireProtected(path);
        var json=UniversalBundleContract.ParseObject(path);
        if(!json.Select(x=>x.Key).ToHashSet(StringComparer.Ordinal).SetEquals(new[]{"Version","OperationId","PackageHash","OperatorSid","Stage","Pending","Error"}))throw new InvalidDataException("HEADLESS_FRESH_JOURNAL_INVALID");
        var j=json.Deserialize<HeadlessFreshJournal>(Json)??throw new InvalidDataException("HEADLESS_FRESH_JOURNAL_INVALID");Validate(j);return j;
    }
    private static void Save(string root,HeadlessFreshJournal? expected,HeadlessFreshJournal next)
    {
        RequireTransition(expected,next);if(Read(root)!=expected)throw new InvalidDataException("HEADLESS_FRESH_JOURNAL_CHANGED");
        var temp=Path.Combine(root,Guid.NewGuid().ToString("N")+".next");using(var output=new FileStream(temp,FileMode.CreateNew,FileAccess.Write,FileShare.None,4096,FileOptions.WriteThrough)){JsonSerializer.Serialize(output,next);output.Flush(true);}
        File.Move(temp,Path.Combine(root,JournalName),true);if(Read(root)!=next)throw new IOException("HEADLESS_FRESH_JOURNAL_UNVERIFIED");
    }
}
