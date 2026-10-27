using System.Diagnostics;
using System.Security.Cryptography;
using System.Text;
namespace CloudgimeHostBootstrap;
internal static class HeadlessPairingTask
{
    internal static void Stage(string bundleRoot)
    {
        var source=Path.Combine(bundleRoot,"auto-pair-host.ps1");UniversalMachineBoundary.RequireProtected(source);
        var hash=Hash(source);var root=Path.Combine(UniversalMachineBoundary.ProvisioningRoot,"pairing-private");
        if(!Directory.Exists(root))
        {
            var acl=new System.Security.AccessControl.DirectorySecurity();acl.SetAccessRuleProtection(true,false);acl.SetOwner(new System.Security.Principal.SecurityIdentifier("S-1-5-32-544"));
            foreach(var sid in new[]{"S-1-5-18","S-1-5-32-544"})acl.AddAccessRule(new System.Security.AccessControl.FileSystemAccessRule(new System.Security.Principal.SecurityIdentifier(sid),System.Security.AccessControl.FileSystemRights.FullControl,System.Security.AccessControl.InheritanceFlags.ContainerInherit|System.Security.AccessControl.InheritanceFlags.ObjectInherit,System.Security.AccessControl.PropagationFlags.None,System.Security.AccessControl.AccessControlType.Allow));
            new DirectoryInfo(root).Create(acl);
        }
        UniversalMachineBoundary.RequireProtected(root,true);
        static string Q(string s)=>"'"+s.Replace("'","''")+"'";
        var content="$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'\r\n"+
            "$source="+Q(source)+";$bundle="+Q(bundleRoot)+";$root="+Q(root)+"\r\n"+
            "if((Get-FileHash $source).Hash.ToLowerInvariant()-ne'"+hash+"'){throw 'PAIRING_SOURCE_HASH_CHANGED'}\r\n"+
            "for($i=0;$i-lt 60;$i++){try{& powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $source -BundleRoot $bundle -FirstBootOperation ((Get-Content (Join-Path (Split-Path $root) 'first-boot-shell-private/handoff.json') -Raw|ConvertFrom-Json).Binding.OperationId) *> $null;if($LASTEXITCODE-eq 0){@{version=1;stage='PAIRED';at=[datetime]::UtcNow.ToString('o')}|ConvertTo-Json|Set-Content (Join-Path $root 'paired.json');exit 0}}catch{};Start-Sleep -Seconds 5};@{version=1;stage='PAIRING_RETRY_REQUIRED';at=[datetime]::UtcNow.ToString('o')}|ConvertTo-Json|Set-Content (Join-Path $root 'retry.json');exit 1\r\n";
        var script=Path.Combine(root,"pair-after-logon.ps1");
        if(File.Exists(script)){UniversalMachineBoundary.RequireProtected(script);if(File.ReadAllText(script)!=content)throw new InvalidDataException("HEADLESS_PAIRING_TASK_CHANGED");}
        else File.WriteAllText(script,content,new UTF8Encoding(false));
        var psi=new ProcessStartInfo(Path.Combine(Environment.SystemDirectory,"schtasks.exe")){UseShellExecute=false,CreateNoWindow=true,RedirectStandardOutput=true,RedirectStandardError=true};
        foreach(var arg in new[]{"/Create","/TN","CloudGime-FirstBootPairing","/TR","powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File \""+script+"\"","/SC","ONLOGON","/RU","SYSTEM","/RL","HIGHEST","/F"})psi.ArgumentList.Add(arg);
        using var process=Process.Start(psi)??throw new InvalidDataException("HEADLESS_PAIRING_TASK_FAILED");
        var output=process.StandardOutput.ReadToEndAsync();var errors=process.StandardError.ReadToEndAsync();
        if(!process.WaitForExit(30000)){process.Kill(true);throw new InvalidDataException("HEADLESS_PAIRING_TASK_TIMEOUT");}Task.WaitAll(output,errors);
        if(process.ExitCode!=0)throw new InvalidDataException("HEADLESS_PAIRING_TASK_FAILED");
        StageSupervisor(bundleRoot,root);
    }
    private static void StageSupervisor(string bundleRoot,string root)
    {
        var image=Path.Combine(bundleRoot,"moonlight","system","cloudgime-runtime-agent.exe");UniversalMachineBoundary.RequireProtected(image);
        static string Q(string s)=>"'"+s.Replace("'","''")+"'";
        var script=Path.Combine(root,"stage-supervisor.ps1");
        var text="$ErrorActionPreference='Stop';$a=New-ScheduledTaskAction -Execute "+Q(image)+" -Argument "+Q("--bundle-root \""+bundleRoot+"\" run-daemon")+
            ";$triggers=@((New-ScheduledTaskTrigger -AtStartup),(New-ScheduledTaskTrigger -AtLogOn));$principal=New-ScheduledTaskPrincipal -UserId 'SYSTEM' -RunLevel Highest;"+
            "$settings=New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Seconds 0) -MultipleInstances IgnoreNew -StartWhenAvailable -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries;"+
            "Register-ScheduledTask -TaskName 'CloudgimeHostUser-Host' -Action $a -Trigger $triggers -Principal $principal -Settings $settings -Force|Out-Null;"+
            "$transport="+Q(Path.Combine(bundleRoot,"keeper-tunnel","cloudgimehosttunnel.exe"))+";"+
            "$ta=New-ScheduledTaskAction -Execute $transport -WorkingDirectory "+Q(Path.Combine(bundleRoot,"keeper-tunnel"))+";"+
            "Register-ScheduledTask -TaskName 'CloudGime-HostTransport' -Action $ta -Trigger $triggers -Principal $principal -Settings $settings -Force|Out-Null;Start-ScheduledTask -TaskName 'CloudGime-HostTransport'";
        if(File.Exists(script)){UniversalMachineBoundary.RequireProtected(script);if(File.ReadAllText(script)!=text)throw new InvalidDataException("HEADLESS_SUPERVISOR_TASK_CHANGED");}
        else File.WriteAllText(script,text,new UTF8Encoding(false));
        var psi=new ProcessStartInfo(Path.Combine(Environment.SystemDirectory,"WindowsPowerShell","v1.0","powershell.exe")){UseShellExecute=false,CreateNoWindow=true,RedirectStandardOutput=true,RedirectStandardError=true};
        foreach(var arg in new[]{"-NoProfile","-NonInteractive","-ExecutionPolicy","Bypass","-File",script})psi.ArgumentList.Add(arg);
        using var process=Process.Start(psi)??throw new InvalidDataException("HEADLESS_SUPERVISOR_TASK_FAILED");var output=process.StandardOutput.ReadToEndAsync();var error=process.StandardError.ReadToEndAsync();
        if(!process.WaitForExit(30000)){process.Kill(true);throw new InvalidDataException("HEADLESS_SUPERVISOR_TASK_TIMEOUT");}Task.WaitAll(output,error);if(process.ExitCode!=0)throw new InvalidDataException("HEADLESS_SUPERVISOR_TASK_FAILED");
    }
    private static string Hash(string file){using var input=File.OpenRead(file);return Convert.ToHexString(SHA256.HashData(input)).ToLowerInvariant();}
}
