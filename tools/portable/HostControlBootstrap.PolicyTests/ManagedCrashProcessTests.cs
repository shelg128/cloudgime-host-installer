using CloudgimeHostBootstrap;
using System.Diagnostics;
using System.Security.Cryptography;
internal static class ManagedCrashProcessTests
{
 internal static bool Worker(string[] args)
 {
  if(args.Length==0||args[0]!="--managed-upgrade-crash-fixture")return false;
  if(args.Length!=3)throw new InvalidDataException("FIXTURE_ARGUMENTS");var root=Path.GetFullPath(args[1]);var temp=Path.GetFullPath(Path.GetTempPath());
  if(!root.StartsWith(temp,StringComparison.OrdinalIgnoreCase)||!Path.GetFileName(root).StartsWith("cg-upgrade-process-",StringComparison.Ordinal)||!File.Exists(Path.Combine(root,"FIXTURE-ONLY")))throw new InvalidDataException("FIXTURE_PATH");
  var live=Path.Combine(root,"live");var source=Path.Combine(root,"source");var op=Path.Combine(root,"recovery");
  if(args[2]=="crash"){
   var pairs=Enumerable.Range(0,3).Select(i=>(Path.Combine(source,i+".bin"),Path.Combine(live,i+".bin")));
   var tx=ManagedUpgradeTransaction.Prepare(op,Guid.NewGuid().ToString("D"),[live],pairs);
   tx.Apply(()=>{},i=>{if(i==1)Environment.Exit(73);});throw new Exception("CRASH_NOT_INJECTED");
  }
  if(args[2]=="rollback"){ManagedUpgradeTransaction.Open(op,[live]).Rollback(()=>{});return true;}
  throw new InvalidDataException("FIXTURE_ACTION");
 }
 internal static void Run()
 {
  var root=Path.Combine(Path.GetTempPath(),"cg-upgrade-process-"+Guid.NewGuid().ToString("N"));var live=Path.Combine(root,"live");var source=Path.Combine(root,"source");Directory.CreateDirectory(live);Directory.CreateDirectory(source);File.WriteAllText(Path.Combine(root,"FIXTURE-ONLY"),"no services or real node paths");
  try{
   for(int i=0;i<3;i++){File.WriteAllText(Path.Combine(live,i+".bin"),"OLD-"+i);File.WriteAllText(Path.Combine(source,i+".bin"),"NEW-"+i);}
   int Invoke(string action){var psi=new ProcessStartInfo(Environment.ProcessPath!){UseShellExecute=false,CreateNoWindow=true,RedirectStandardOutput=true,RedirectStandardError=true};foreach(var a in new[]{"--managed-upgrade-crash-fixture",root,action})psi.ArgumentList.Add(a);using var p=Process.Start(psi)!;var a1=p.StandardOutput.ReadToEndAsync();var a2=p.StandardError.ReadToEndAsync();if(!p.WaitForExit(15000)){p.Kill();throw new Exception("CHILD_TIMEOUT");}Task.WaitAll(a1,a2);return p.ExitCode;}
   if(Invoke("crash")!=73)throw new Exception("CHILD_DID_NOT_EXIT_AT_CUT");
   if(File.ReadAllText(Path.Combine(live,"0.bin"))!="NEW-0"||File.ReadAllText(Path.Combine(live,"1.bin"))!="NEW-1"||File.ReadAllText(Path.Combine(live,"2.bin"))!="OLD-2")throw new Exception("CUT_STATE_WRONG");
   if(Invoke("rollback")!=0)throw new Exception("FRESH_PROCESS_RECOVERY_FAILED");
   for(int i=0;i<3;i++)if(File.ReadAllText(Path.Combine(live,i+".bin"))!="OLD-"+i)throw new Exception("FRESH_PROCESS_RESTORE_WRONG");
   Console.WriteLine("MANAGED_REAL_PROCESS_EXIT_RECOVERY_PASS exit=73 freshRollbackProcess=true fileCount=3 scope=temporary-fixture-not-PC02");
  }finally{Directory.Delete(root,true);}
 }
}
