using CloudgimeHostBootstrap;
internal static class FirstBootInstallerPlanTests
{
    internal static void Run()
    {
        int count=0;var now=DateTimeOffset.UtcNow;
        var previous=new FirstBootInstallerPlan(1,Guid.NewGuid().ToString("D"),@"C:\fixture\helper.exe",new('a',64),@"C:\ProgramData\CloudGime\provisioning","S-1-5-21-1-2-3-1001",new('b',64),now.AddHours(-1),now.AddMinutes(-40));
        var requested=previous with{OperationId=Guid.NewGuid().ToString("D"),CreatedAt=now,ValidUntil=now.AddMinutes(20)};
        var result=FirstBootInstallerPlanPreparation.Renew(previous,requested);
        Check(result.OperationId==previous.OperationId&&result.CreatedAt==now&&result.ValidUntil==requested.ValidUntil);count++;
        Check(FirstBootInstallerPlanPreparation.Renew(null,requested)==requested);count++;
        foreach(var change in new[]{requested with{HostInstaller=@"C:\foreign.exe"},requested with{HostInstallerHash=new('c',64)},requested with{PackageHash=new('c',64)},requested with{OperatorSid="S-1-5-21-1-2-3-1002"},requested with{ProvisioningRoot=@"C:\foreign"},requested with{CreatedAt=now.AddHours(-2)}})
        {bool rejected=false;try{FirstBootInstallerPlanPreparation.Renew(previous,change);}catch(InvalidDataException){rejected=true;}Check(rejected);count++;}
        Check(FirstBootInstallerCommandContract.IsPrepareCommand(new[]{FirstBootInstallerCommandContract.PrepareCommand, @"C:\fixture"}));count++;
        Check(FirstBootInstallerCommandContract.IsPrepareCommand(new[]{FirstBootInstallerCommandContract.PrepareCommand, @"C:\fixture", "--silent"}));count++;
        foreach(var invalid in new[]{new string[0],new[]{FirstBootInstallerCommandContract.PrepareCommand},new[]{FirstBootInstallerCommandContract.PrepareCommand,@"C:\fixture","--unexpected"},new[]{FirstBootInstallerCommandContract.PrepareCommand,@" "}})
        {
            bool rejected=false;try{FirstBootInstallerCommandContract.ParseBundleRoot(invalid);}catch(InvalidDataException){rejected=true;}Check(rejected);count++;
        }
        var provisionValid=new[]{InstallerProvisionCommandContract.ProvisionFromEnvironmentCommand,@"C:\fixture","--silent"};
        Check(InstallerProvisionCommandContract.IsProvisionFromEnvironmentCommand(provisionValid));count++;
        Check(InstallerProvisionCommandContract.ParseBundleRoot(provisionValid)==Path.GetFullPath(@"C:\fixture"));count++;
        foreach(var invalid in new[]{new string[0],new[]{InstallerProvisionCommandContract.ProvisionFromEnvironmentCommand},new[]{InstallerProvisionCommandContract.ProvisionFromEnvironmentCommand,@"C:\fixture"},new[]{InstallerProvisionCommandContract.ProvisionFromEnvironmentCommand,@"C:\fixture","--unexpected"},new[]{InstallerProvisionCommandContract.ProvisionFromEnvironmentCommand,@" ","--silent"},new[]{"other",@"C:\fixture","--silent"}})
        {
            bool rejected=false;try{InstallerProvisionCommandContract.ParseBundleRoot(invalid);}catch(InvalidDataException){rejected=true;}Check(rejected);count++;
        }
        var fileValid=new[]{InstallerProvisionCommandContract.ProvisionFromFileCommand,@"C:\fixture",@"C:\ProgramData\CloudGime\installer-secrets\setup-token-fixture","--silent"};
        Check(InstallerProvisionCommandContract.IsProvisionFromFileCommand(fileValid));count++;
        var parsed=InstallerProvisionCommandContract.ParseFileInvocation(fileValid);
        Check(parsed.BundleRoot==Path.GetFullPath(@"C:\fixture")&&parsed.TokenFile==Path.GetFullPath(@"C:\ProgramData\CloudGime\installer-secrets\setup-token-fixture"));count++;
        foreach(var invalid in new[]{new string[0],new[]{InstallerProvisionCommandContract.ProvisionFromFileCommand},new[]{InstallerProvisionCommandContract.ProvisionFromFileCommand,@"C:\fixture"},new[]{InstallerProvisionCommandContract.ProvisionFromFileCommand,@"C:\fixture",@"C:\token"},new[]{InstallerProvisionCommandContract.ProvisionFromFileCommand,@"C:\fixture",@"C:\token","--unexpected"},new[]{InstallerProvisionCommandContract.ProvisionFromFileCommand,@" ",@"C:\token","--silent"},new[]{InstallerProvisionCommandContract.ProvisionFromFileCommand,@"C:\fixture",@" ","--silent"}})
        {
            bool rejected=false;try{InstallerProvisionCommandContract.ParseFileInvocation(invalid);}catch(InvalidDataException){rejected=true;}Check(rejected);count++;
        }
        // The installer packages two bootstrap copies. The SYSTEM bridge binds
        // its plan to the bundle copy, while prepare runs the installed app
        // copy. A release builder must keep these identical; otherwise the
        // bridge rejects the plan before first boot.
        var source=File.ReadAllBytes(typeof(FirstBootInstallerPlanTests).Assembly.Location);
        var same=Path.Combine(Path.GetTempPath(),"cg-bootstrap-same-"+Guid.NewGuid().ToString("N")+".exe");
        var different=Path.Combine(Path.GetTempPath(),"cg-bootstrap-different-"+Guid.NewGuid().ToString("N")+".exe");
        try
        {
            File.WriteAllBytes(same,source);File.WriteAllBytes(different,source.Concat(new byte[]{0}).ToArray());
            Check(Hash(source)==Hash(File.ReadAllBytes(same)));count++;
            Check(Hash(source)!=Hash(File.ReadAllBytes(different)));count++;
        }
        finally{if(File.Exists(same))File.Delete(same);if(File.Exists(different))File.Delete(different);}
        Console.WriteLine($"FIRST_BOOT_PLAN_PREPARATION_ASSERTIONS={count}");
    }
    private static string Hash(byte[] value)=>Convert.ToHexString(System.Security.Cryptography.SHA256.HashData(value));
    private static void Check(bool value){if(!value)throw new Exception("FIRST_BOOT_PLAN_RENEWAL_FAILED");}
}
