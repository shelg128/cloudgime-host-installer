using System.Security.AccessControl;
using System.Security.Cryptography;
using System.Security.Principal;
using System.Text.Json;
namespace CloudgimeHostBootstrap;

// Installer-owned plan outside immutable bundle inventory. A lease renewal
// changes only timestamps; the setup operation survives network/process retry.
internal static class FirstBootInstallerPlanPreparation
{
    internal static void Run(string bundleRoot)
    {
        using var identity=WindowsIdentity.GetCurrent();
        var operatorSid=ResolveOperatorSid(identity);
        var bundle=Path.GetFullPath(bundleRoot);UniversalMachineBoundary.RequireProtected(bundle,true);
        var helper=Path.Combine(bundle,"host-installer.exe");
        var bootstrap=Path.Combine(bundle,"host-control","cloudgime-host-bootstrap.exe");
        UniversalMachineBoundary.RequireProtected(helper);UniversalMachineBoundary.RequireProtected(bootstrap);
        var root=UniversalMachineBoundary.ProvisioningRoot;UniversalMachineBoundary.RequireProtected(root,true);
        var directory=Path.Combine(root,"first-boot-plans");
        if(!Directory.Exists(directory))
        {
            var acl=new DirectorySecurity();acl.SetAccessRuleProtection(true,false);acl.SetOwner(new SecurityIdentifier("S-1-5-32-544"));
            foreach(var sid in new[]{"S-1-5-18","S-1-5-32-544"})acl.AddAccessRule(new FileSystemAccessRule(new SecurityIdentifier(sid),FileSystemRights.FullControl,InheritanceFlags.ContainerInherit|InheritanceFlags.ObjectInherit,PropagationFlags.None,AccessControlType.Allow));
            new DirectoryInfo(directory).Create(acl);
        }
        UniversalMachineBoundary.RequireProtected(directory,true);
        var planPath=Path.Combine(directory,"plan.json");var lockPath=Path.Combine(directory,"plan.lock");
        if(File.Exists(lockPath))UniversalMachineBoundary.RequireProtected(lockPath);
        using(var lease=new FileStream(lockPath,FileMode.OpenOrCreate,FileAccess.ReadWrite,FileShare.None))
        {
            var now=DateTimeOffset.UtcNow;
            var requested=new FirstBootInstallerPlan(1,Guid.NewGuid().ToString("D"),helper,Hash(helper),root,operatorSid,Hash(bootstrap),now,now.AddMinutes(20));
            FirstBootInstallerPlan? previous=null;
            if(File.Exists(planPath))
            {
                UniversalMachineBoundary.RequireProtected(planPath);
                var json=UniversalBundleContract.ParseObject(planPath);
                previous=json.Deserialize<FirstBootInstallerPlan>(new JsonSerializerOptions{UnmappedMemberHandling=System.Text.Json.Serialization.JsonUnmappedMemberHandling.Disallow})
                    ??throw new InvalidDataException("FIRST_BOOT_PLAN_INVALID");
            }
            var next=Renew(previous,requested);
            var temp=Path.Combine(directory,Guid.NewGuid().ToString("N")+".next");
            using(var output=new FileStream(temp,FileMode.CreateNew,FileAccess.Write,FileShare.None,4096,FileOptions.WriteThrough))
            {JsonSerializer.Serialize(output,next);output.Flush(true);}
            UniversalMachineBoundary.RequireProtected(temp);File.Move(temp,planPath,true);
        }
        // Bridge waits only for this operation's verified same-boot ARMED receipt.
        FirstBootInstallerBridge.Run(bundle,planPath);
    }
    private static string ResolveOperatorSid(WindowsIdentity identity)
    {
        if(!identity.IsSystem)
        {
            if(!new WindowsPrincipal(identity).IsInRole(WindowsBuiltInRole.Administrator)
                ||identity.User?.IsAccountSid()!=true)throw new InvalidDataException("FIRST_BOOT_OPERATOR_REQUIRED");
            return identity.User.Value;
        }
        var sharedPin=Path.Combine(UniversalMachineBoundary.ProvisioningRoot,"shared-gaming-desktop.identity.json");
        if(File.Exists(sharedPin))
        {
            UniversalMachineBoundary.RequireProtected(sharedPin);
            var doc=UniversalBundleContract.ParseObject(sharedPin);
            var pinned=(string?)doc["desktopUserSid"];
            if(!string.IsNullOrWhiteSpace(pinned)&&System.Text.RegularExpressions.Regex.IsMatch(pinned,@"\AS-1-5-21-(\d{1,10}-){3}\d{1,10}\z"))
                return pinned;
        }
        foreach(var candidate in new[]{"admin"})
        {
            try
            {
                var sid=((SecurityIdentifier)new NTAccount(Environment.MachineName,candidate).Translate(typeof(SecurityIdentifier))).Value;
                if(System.Text.RegularExpressions.Regex.IsMatch(sid,@"\AS-1-5-21-(\d{1,10}-){3}\d{1,10}\z"))
                    return sid;
            }
            catch { }
        }
        throw new InvalidDataException("FIRST_BOOT_OPERATOR_REQUIRED");
    }
    internal static FirstBootInstallerPlan Renew(FirstBootInstallerPlan? previous,FirstBootInstallerPlan requested)
    {
        if(previous==null)return requested;
        if(!Guid.TryParseExact(previous.OperationId,"D",out var id)||id==Guid.Empty||previous.Version!=requested.Version
            ||previous.HostInstaller!=requested.HostInstaller||previous.HostInstallerHash!=requested.HostInstallerHash
            ||previous.ProvisioningRoot!=requested.ProvisioningRoot||previous.OperatorSid!=requested.OperatorSid||previous.PackageHash!=requested.PackageHash
            ||previous.CreatedAt>requested.CreatedAt||previous.ValidUntil>requested.ValidUntil)
            throw new InvalidDataException("FIRST_BOOT_PLAN_BINDING_CHANGED");
        return requested with{OperationId=previous.OperationId};
    }
    private static string Hash(string path){using var file=new FileStream(path,FileMode.Open,FileAccess.Read,FileShare.Read);return Convert.ToHexString(SHA256.HashData(file)).ToLowerInvariant();}
}
