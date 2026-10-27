using System.Diagnostics;
namespace CloudgimeHostBootstrap;
internal static class HeadlessServiceRecovery
{
 internal static readonly string[][] Commands=
 [
  ["config","CloudgimeHost-Host","start=","delayed-auto"],
  ["failure","CloudgimeHost-Host","reset=","86400","actions=","restart/15000/restart/30000/restart/60000"],
  ["failureflag","CloudgimeHost-Host","1"]
 ];
 internal static void Configure()
 {
  foreach(var args in Commands)
  {
   var psi=new ProcessStartInfo(Path.Combine(Environment.SystemDirectory,"sc.exe")){UseShellExecute=false,CreateNoWindow=true,RedirectStandardOutput=true,RedirectStandardError=true};foreach(var a in args)psi.ArgumentList.Add(a);
   using var process=Process.Start(psi)??throw new InvalidDataException("HEADLESS_SERVICE_RECOVERY_FAILED");
   var output=process.StandardOutput.ReadToEndAsync();var errors=process.StandardError.ReadToEndAsync();
   if(!process.WaitForExit(30000)){process.Kill(true);throw new InvalidDataException("HEADLESS_SERVICE_RECOVERY_TIMEOUT");}Task.WaitAll(output,errors);
   if(process.ExitCode!=0)throw new InvalidDataException("HEADLESS_SERVICE_RECOVERY_FAILED");
  }
 }
}
