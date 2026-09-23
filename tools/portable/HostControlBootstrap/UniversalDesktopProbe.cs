using System.Security.Principal;
using System.Text;

namespace CloudgimeHostBootstrap;

internal static class UniversalDesktopProbe
{
    // Task Scheduler launches the fixed trusted helper with the existing user's limited token.
    // No password, logon, desktop switch, or elevated renter process is created.
    internal static void Verify(string helper)
    {
        UniversalMachineBoundary.RequireAdminConsole();
        UniversalMachineBoundary.RequireProtected(helper);
        using var identity = WindowsIdentity.GetCurrent();
        var sid = identity.User?.Value ?? throw new InvalidOperationException("CONSOLE_SID_REQUIRED");
        var task = "CloudGime-DesktopProbe-" + Guid.NewGuid().ToString("N");
        var escapedHelper = helper.Replace("'", "''", StringComparison.Ordinal);
        var script = $$"""
$ErrorActionPreference='Stop'; $ProgressPreference='SilentlyContinue'
$name='{{task}}'
try {
  $action=New-ScheduledTaskAction -Execute '{{escapedHelper}}' -Argument '--verify-interactive-rental-desktop'
  $principal=New-ScheduledTaskPrincipal -UserId '{{sid}}' -LogonType Interactive -RunLevel Limited
  $settings=New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Seconds 45) -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries
  Register-ScheduledTask -TaskName $name -Action $action -Principal $principal -Settings $settings | Out-Null
  Start-ScheduledTask -TaskName $name
  $deadline=[DateTime]::UtcNow.AddSeconds(60)
  do {
    Start-Sleep -Milliseconds 250
    $state=Get-ScheduledTask -TaskName $name
    $info=Get-ScheduledTaskInfo -TaskName $name
    if($info.LastRunTime.Year -gt 2000 -and $state.State -ne 'Running' -and $info.LastTaskResult -ne 267009 -and $info.LastTaskResult -ne 267011) {
      if($info.LastTaskResult -ne 0){throw 'SHARED_DESKTOP_PROBE_REJECTED'}
      exit 0
    }
  } while([DateTime]::UtcNow -lt $deadline)
  throw 'SHARED_DESKTOP_PROBE_TIMEOUT'
} finally {
  Stop-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue
  Unregister-ScheduledTask -TaskName $name -Confirm:$false -ErrorAction SilentlyContinue
}
""";
        var powershell = Path.Combine(Environment.SystemDirectory, "WindowsPowerShell", "v1.0", "powershell.exe");
        UniversalMachineBoundary.Run(powershell, ["-NoLogo", "-NoProfile", "-NonInteractive", "-EncodedCommand",
            Convert.ToBase64String(Encoding.Unicode.GetBytes(script))], Path.GetDirectoryName(helper)!);
    }
}
