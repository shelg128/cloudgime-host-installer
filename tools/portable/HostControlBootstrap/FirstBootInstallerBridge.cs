using System.Diagnostics;
using System.Security.AccessControl;
using System.Security.Cryptography;
using System.Security.Principal;
using System.Text.Json;
namespace CloudgimeHostBootstrap;

internal sealed record FirstBootInstallerPlan(int Version,string OperationId,string SentinelHelper,string SentinelHelperHash,
    string ProvisioningRoot,string OperatorSid,string PackageHash,DateTimeOffset CreatedAt,DateTimeOffset ValidUntil);
internal static class FirstBootInstallerBridge
{
    internal static void Run(string bundleRoot,string planPath)
    {
        if(!OperatingSystem.IsWindows())throw new PlatformNotSupportedException();
        RunCore(bundleRoot,planPath,false);
    }
    internal static void RunCore(string bundleRoot,string planPath,bool silent)
    {
        if(!IsAdministrator())throw new InvalidOperationException("HEADLESS_UAC_REQUIRED");
        var plan=Read(bundleRoot,planPath);
        ProtectPlan(planPath);
        plan=Read(bundleRoot,planPath);
        var helper=Path.GetFullPath(Path.Combine(bundleRoot,plan.SentinelHelper));
        var planDirectory=Path.GetDirectoryName(Path.GetFullPath(planPath))!;var directoryInfo=new DirectoryInfo(planDirectory);if(directoryInfo.LinkTarget is not null)throw new InvalidDataException("FIRST_BOOT_PLAN_REPARSE_DENIED");
        if(!helper.StartsWith(Path.GetFullPath(bundleRoot)+Path.DirectorySeparatorChar,StringComparison.OrdinalIgnoreCase)||!File.Exists(helper)
            ||!CryptographicOperations.FixedTimeEquals(Convert.FromHexString(Hash(helper)),Convert.FromHexString(plan.SentinelHelperHash)))throw new InvalidDataException("FIRST_BOOT_HELPER_HASH_INVALID");
        var expectedRoot=Path.GetFullPath(Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData),"CloudGime","Provisioning"));
        if(!string.Equals(Path.GetFullPath(plan.ProvisioningRoot),expectedRoot,StringComparison.OrdinalIgnoreCase))throw new InvalidDataException("FIRST_BOOT_ROOT_CHANGED");
        // The elevated installer may run through UAC with a linked admin token;
        // the operator SID in the plan belongs to the original desktop user.
        // Sentinel validates that SID when it provisions its owned renter state.
        var task="CloudGime-FirstBoot-"+plan.OperationId;var planHash=Hash(Path.GetFullPath(planPath));
        try
        {
            // schtasks limits /TR to 261 characters. The wrapper is necessary,
            // but has a protected SYSTEM/Administrators DACL before task create;
            // untrusted users cannot exchange its command between validation and run.
            var command=helper.EndsWith(".dll",StringComparison.OrdinalIgnoreCase)
                ?"dotnet "+Quote(helper)+" --installer-first-boot-plan "+Quote(Path.GetFullPath(planPath))+" "+planHash
                :Quote(helper)+" --installer-first-boot-plan "+Quote(Path.GetFullPath(planPath))+" "+planHash;
            var wrapper=Path.Combine(Path.GetDirectoryName(Path.GetFullPath(planPath))!,"fb-"+plan.OperationId+".cmd");
            WriteProtectedWrapper(wrapper,command);
            Execute("/Create","/TN",task,"/TR",Quote(wrapper),"/SC","ONCE","/ST","00:00","/RU","SYSTEM","/RL","HIGHEST","/F");
            Execute("/Run","/TN",task);
            // Acceptance by Task Scheduler is not evidence of a completed
            // profile/shell transaction. This receipt is written only by the
            // SYSTEM helper after provision + prepare + arm all succeed.
            var deadline=DateTimeOffset.UtcNow.AddMinutes(2);
            while(!FirstBootComplete(plan.ProvisioningRoot,plan.OperationId)&&DateTimeOffset.UtcNow<deadline)
                Thread.Sleep(250);
            if(!FirstBootComplete(plan.ProvisioningRoot,plan.OperationId))
                throw new InvalidDataException("FIRST_BOOT_TASK_NO_COMPLETION");
            Execute("/Delete","/TN",task,"/F");
            File.Delete(wrapper);
        }
        catch
        {
            // Preserve the task, immutable wrapper and exact plan for explicit
            // repair/resume rather than silently discarding forensic recovery state.
            throw;
        }
    }
    private static FirstBootInstallerPlan Read(string root,string path)
    {
        var full=Path.GetFullPath(path);var baseRoot=Path.GetFullPath(root)+Path.DirectorySeparatorChar;
        if(!full.StartsWith(baseRoot,StringComparison.OrdinalIgnoreCase)||!File.Exists(full)||new FileInfo(full).Length is 0 or >32768)throw new InvalidDataException("FIRST_BOOT_PLAN_PATH_INVALID");
        var json=File.ReadAllText(full).TrimStart('\uFEFF');var p=JsonSerializer.Deserialize<FirstBootInstallerPlan>(json,new JsonSerializerOptions{PropertyNameCaseInsensitive=true})??throw new InvalidDataException("FIRST_BOOT_PLAN_INVALID");
        if(p.Version!=1||!Guid.TryParse(p.OperationId,out _)||p.SentinelHelperHash.Length!=64||!p.SentinelHelperHash.All(Uri.IsHexDigit)||!Path.IsPathFullyQualified(p.ProvisioningRoot)
            ||p.ValidUntil<=DateTimeOffset.UtcNow||p.ValidUntil>p.CreatedAt.AddMinutes(30)||!new SecurityIdentifier(p.OperatorSid).IsAccountSid())throw new InvalidDataException("FIRST_BOOT_PLAN_INVALID");return p;
    }
    private static void ProtectPlan(string planPath)
    {
        var security=new FileSecurity();security.SetAccessRuleProtection(true,false);
        foreach(var sid in new[]{"S-1-5-18","S-1-5-32-544"})
            security.AddAccessRule(new FileSystemAccessRule(new SecurityIdentifier(sid),FileSystemRights.FullControl,AccessControlType.Allow));
        new FileInfo(planPath).SetAccessControl(security);
    }
    private static void WriteProtectedWrapper(string wrapper,string command)
    {
        File.WriteAllText(wrapper,"@echo off\r\n"+command+"\r\n",new System.Text.UTF8Encoding(false));
        var security=new FileSecurity();security.SetAccessRuleProtection(true,false);
        foreach(var sid in new[]{"S-1-5-18","S-1-5-32-544"})
            security.AddAccessRule(new FileSystemAccessRule(new SecurityIdentifier(sid),FileSystemRights.FullControl,AccessControlType.Allow));
        new FileInfo(wrapper).SetAccessControl(security);
    }
    private static bool FirstBootComplete(string root,string operation)
    {
        var path=Path.Combine(root,"first-boot-shell-private","installer-complete.json");
        if(!File.Exists(path)||new FileInfo(path).Length is 0 or >4096)return false;
        try{using var json=JsonDocument.Parse(File.ReadAllText(path));return json.RootElement.GetProperty("operationId").GetString()==operation&&json.RootElement.GetProperty("stage").GetString()=="ARMED";}catch{return false;}
    }
    private static void Execute(params string[] args)
    {
        var psi=new ProcessStartInfo("schtasks.exe"){UseShellExecute=false,CreateNoWindow=true,RedirectStandardError=true,RedirectStandardOutput=true};
        foreach(var a in args)psi.ArgumentList.Add(a);
        using var p=Process.Start(psi)??throw new InvalidDataException("FIRST_BOOT_TASK_START_FAILED");
        if(!p.WaitForExit(30000))throw new InvalidDataException("FIRST_BOOT_TASK_TIMEOUT");
        var error=p.StandardError.ReadToEnd().Trim();var output=p.StandardOutput.ReadToEnd().Trim();
        if(p.ExitCode!=0)throw new InvalidDataException("FIRST_BOOT_TASK_FAILED code="+p.ExitCode+" detail="+Clip(error.Length>0?error:output));
    }
    private static string Clip(string text)=>new string(text.Where(c=>c>=32&&c<=126).Take(180).ToArray());
    private static string Quote(string x)=>"\""+x.Replace("\"","\"\"")+"\"";
    private static string Hash(string path){using var f=File.OpenRead(path);return Convert.ToHexString(SHA256.HashData(f));}
    private static bool IsAdministrator(){using var id=WindowsIdentity.GetCurrent();return new WindowsPrincipal(id).IsInRole(WindowsBuiltInRole.Administrator);}
}
