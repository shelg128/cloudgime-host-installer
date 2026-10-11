using System.Diagnostics;
using System.Management;
using System.Security.AccessControl;
using System.Security.Principal;
using System.Text.Json;
using System.Text.Json.Nodes;
using Microsoft.Win32;
namespace CloudgimeHostBootstrap;

// First supported lane: already STOPPED managed Sentinel, unchanged live Host
// runtime. It is a separate transaction, NOT a skip-guard switch for legacy NSIS.
internal static class ManagedStoppedUpgrade
{
 private static readonly string Data=Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData),"CloudGime");
 private static readonly string Provision=Path.Combine(Data,"provisioning");
 private static readonly string App=Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles),"Sentinel Launcher");
 private static readonly string HostApp=Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles),"Cloudgime Host");
 private static readonly string Recovery=Path.Combine(Data,"ManagedUpgradeRecovery");
 private static readonly string[] Allowed=[App,HostApp,Path.Combine(Data,"NodeBroker"),Path.Combine(Data,"RentalShell"),Provision];
 private sealed record Preserved(string Path,string Hash,string Sddl);
 private sealed record Context(string OperationId,string PackageHash,string OperatorId,string WindowsUserSid,string UserShell,List<Preserved> Files,string WindowsState,List<Preserved> Profiles);
 internal static int Stage(string id,string source,string expectedHash)
 {
  RequireAdmin();if(!Guid.TryParseExact(id,"D",out var g)||g==Guid.Empty||expectedHash.Length!=64||!expectedHash.All(Uri.IsHexDigit))throw new InvalidDataException("UPGRADE_STAGE_ID_OR_HASH_INVALID");
  EnsurePrivateRecovery();var root=Path.Combine(Recovery,id);if(Directory.Exists(root))throw new InvalidDataException("UPGRADE_OPERATION_EXISTS");
  source=Path.GetFullPath(source);UniversalBundleContract.RejectReparse(source);
  if(ManagedUpgradeTransaction.Hash(Path.Combine(source,"managed-upgrade.json"))!=expectedHash)throw new InvalidDataException("UPGRADE_STAGE_MANIFEST_CHANGED");
  var manifest=UniversalBundleContract.ParseObject(Path.Combine(source,"managed-upgrade.json"));var entries=manifest["files"]!.AsArray();var names=new HashSet<string>(StringComparer.OrdinalIgnoreCase);
  foreach(var item in entries){var name=(string)item!["path"]!;if(!UniversalBundleContract.IsSafeRelativePath(name)||!names.Add(name))throw new InvalidDataException("UPGRADE_STAGE_PATH_INVALID");}
  if(!UniversalBundleContract.Walk(source).Where(x=>x!="managed-upgrade.json").ToHashSet(StringComparer.OrdinalIgnoreCase).SetEquals(names))throw new InvalidDataException("UPGRADE_STAGE_INVENTORY_CHANGED");
  new DirectoryInfo(root).Create(PrivateAcl());var package=Path.Combine(root,"payload");Directory.CreateDirectory(package);
  foreach(var item in entries){var name=(string)item!["path"]!;var target=Path.Combine(package,name);Directory.CreateDirectory(Path.GetDirectoryName(target)!);File.Copy(Path.Combine(source,name),target,false);if(ManagedUpgradeTransaction.Hash(target)!=(string?)item["sha256"])throw new InvalidDataException("UPGRADE_STAGE_COPY_HASH_CHANGED");}
  File.Copy(Path.Combine(source,"managed-upgrade.json"),Path.Combine(package,"managed-upgrade.json"));if(ManagedUpgradeTransaction.Hash(Path.Combine(package,"managed-upgrade.json"))!=expectedHash)throw new InvalidDataException("UPGRADE_STAGE_MANIFEST_CHANGED");
  File.Copy(Environment.ProcessPath!,Path.Combine(root,"engine.exe"));UniversalMachineBoundary.RequireProtected(package,true);return 0;
 }
 internal static int Run(string action,string id,string? packageRoot)
 {
  RequireAdmin();if(!Guid.TryParseExact(id,"D",out var guid)||guid==Guid.Empty)throw new InvalidDataException("UPGRADE_ID_INVALID");
  if(action is not("prepare" or "apply" or "rollback" or "status" or "watch"))throw new InvalidDataException("UPGRADE_ACTION_INVALID");
  EnsurePrivateRecovery();var op=Path.Combine(Recovery,id);UniversalBundleContract.RejectReparse(op);
  if(action=="prepare"&&!Directory.Exists(op))new DirectoryInfo(op).Create(PrivateAcl());
  UniversalMachineBoundary.RequireProtected(op,true);
  if(action=="watch")
  {
   using(var me=WindowsIdentity.GetCurrent())if(!me.IsSystem||Process.GetCurrentProcess().SessionId!=0)throw new UnauthorizedAccessException("UPGRADE_WATCH_SYSTEM_REQUIRED");
   var deadline=DateTimeOffset.UtcNow.AddMinutes(12);while(DateTimeOffset.UtcNow<deadline){if(File.Exists(Path.Combine(op,"installed.json"))||File.Exists(Path.Combine(op,"rollback.json")))return 0;Thread.Sleep(1000);}
   // Parent may be stalled holding the machine lock. Retry boundedly; do not
   // disable the independent recovery after one sharing violation.
   for(int attempt=0;attempt<120;attempt++){try{return Run("rollback",id,null);}catch(IOException){Thread.Sleep(1000);}}throw new IOException("UPGRADE_RECOVERY_LOCK_TIMEOUT");
  }
  using var machineLock=new FileStream(Path.Combine(Recovery,"upgrade.lock"),FileMode.OpenOrCreate,FileAccess.ReadWrite,FileShare.None);
  var emergencyLock=Path.Combine(Provision,"operator-emergency","operation.lock");UniversalMachineBoundary.RequireProtected(emergencyLock);
  using var operatorLock=new FileStream(emergencyLock,FileMode.Open,FileAccess.ReadWrite,FileShare.None);
  if(action=="status"){Console.WriteLine(ManagedUpgradeTransaction.Open(op,Allowed).Phase);return 0;}
  if(action=="prepare")Prepare(op,id,packageRoot??throw new InvalidDataException("UPGRADE_PACKAGE_REQUIRED"));
  else{
   var context=JsonSerializer.Deserialize<Context>(File.ReadAllBytes(Path.Combine(op,"context.json")))??throw new InvalidDataException("UPGRADE_CONTEXT_MISSING");
   if(context.OperationId!=id||context.Files.Count<5||context.Files.Any(f=>!Allowed.Concat(new[]{Path.Combine(Data,"Host")}).Any(a=>Path.GetFullPath(f.Path).StartsWith(a+Path.DirectorySeparatorChar,StringComparison.OrdinalIgnoreCase))))throw new InvalidDataException("UPGRADE_CONTEXT_CHANGED");var tx=ManagedUpgradeTransaction.Open(op,Allowed);
   void Current(){RequireStopped(context.OperatorId,context.UserShell);VerifyPreserved(context,false);}
   Current();
   if(action=="rollback"){tx.Rollback(Current);Current();VerifyPreserved(context,true);WriteReceipt(op,"rollback",context);return 0;}
   using var authority=new ManagedUpgradeAuthority(Provision);authority.Verify(id,context.PackageHash);ArmRecovery(op,id);
   try{
    tx.Apply(()=>{Current();authority.Verify(id,context.PackageHash);});Current();VerifyPreserved(context,true);WriteReceipt(op,"installed",context);
   }catch{try{tx.Rollback(Current);VerifyPreserved(context,true);WriteReceipt(op,"rollback",context);}catch{throw new InvalidOperationException("MANAGED_UPGRADE_ROLLBACK_INCOMPLETE");}throw;}
   finally{if(ManagedUpgradeTransaction.Open(op,Allowed).Phase is "COMMITTED" or "ROLLED_BACK")TryDeleteTask(id);}
  }
  return 0;
 }
 private static void Prepare(string op,string id,string packageRoot)
 {
  if(File.Exists(Path.Combine(op,"transaction.json")))throw new InvalidDataException("UPGRADE_PREPARED_ALREADY");
  var manifest=UniversalBundleContract.ParseObject(Path.Combine(packageRoot,"managed-upgrade.json"));
  if((int?)manifest["version"]!=1||(string?)manifest["kind"]!="cloudgime-stopped-managed-upgrade"||(bool?)manifest["internalUnsigned"]!=true)throw new InvalidDataException("UPGRADE_PACKAGE_FORMAT_INVALID");
  var packageHash=ManagedUpgradeTransaction.Hash(Path.Combine(packageRoot,"managed-upgrade.json"));
  UniversalMachineBoundary.RequireProtected(packageRoot,true);var entries=manifest["files"]!.AsArray();var declared=new HashSet<string>(StringComparer.OrdinalIgnoreCase);
  foreach(var item in entries){var p=(string)item!["path"]!;if(!UniversalBundleContract.IsSafeRelativePath(p)||!declared.Add(p))throw new InvalidDataException("UPGRADE_PAYLOAD_PATH_INVALID");var f=Path.Combine(packageRoot,p);UniversalBundleContract.RejectReparse(f);if(ManagedUpgradeTransaction.Hash(f)!=(string?)item["sha256"])throw new InvalidDataException("UPGRADE_PAYLOAD_HASH_MISMATCH");}
  var actual=UniversalBundleContract.Walk(packageRoot).Where(p=>p!="managed-upgrade.json").ToHashSet(StringComparer.OrdinalIgnoreCase);if(!actual.SetEquals(declared))throw new InvalidDataException("UPGRADE_UNLISTED_FILES");
  var operatorFile=Path.Combine(Provision,"operator-emergency","active.json");var journal=UniversalBundleContract.ParseObject(operatorFile);var state=journal["State"]!.AsObject();var oldPlan=journal["Plan"]!.AsObject();var operatorId=(string)state["OperationId"]!;var sid=(string)oldPlan["WindowsUserSid"]!;
  RequireStopped(operatorId,"explorer.exe");
  using var authority=new ManagedUpgradeAuthority(Provision);authority.Verify(id,packageHash);
  if((string?)oldPlan["PcId"]!=authority.PcId||(string?)oldPlan["ActivationId"]!=authority.ActivationId)throw new InvalidDataException("UPGRADE_OPERATOR_BINDING_CHANGED");
  foreach(var pair in new[]{("BrokerPath","BrokerHash"),("HelperPath","HelperHash"),("ShellPath","ShellHash"),("LauncherPath","LauncherHash")})
   if(ManagedUpgradeTransaction.Hash((string)oldPlan[pair.Item1]!)!=(string?)oldPlan[pair.Item2])throw new InvalidDataException("UPGRADE_OLD_PLAN_HASH_CHANGED");
  // Host runtime/config files are validated unchanged, never stopped/replaced in
  // this lane. Future changing-host updates need their own coordinated lifecycle.
  foreach(var item in entries.Where(x=>((string)x!["path"]!).StartsWith("host/",StringComparison.Ordinal)))
  {var relative=((string)item!["path"]!)[5..];if(relative.EndsWith("config.json",StringComparison.OrdinalIgnoreCase)||relative.EndsWith(".conf",StringComparison.OrdinalIgnoreCase)||relative.Contains("provisioning"))throw new InvalidDataException("UPGRADE_HOST_CONFIG_PAYLOAD_DENIED");var live=Path.Combine(Data,"Host",relative);if(!File.Exists(live)||ManagedUpgradeTransaction.Hash(live)!=(string?)item["sha256"])throw new InvalidDataException("UPGRADE_HOST_RUNTIME_CHANGE_NOT_SUPPORTED");}
  var replacements=new List<(string Source,string Target)>();
  foreach(var item in entries){var p=(string)item!["path"]!;if(p.StartsWith("sentinel/")){var rel=p[9..];if(rel.EndsWith(".dpapi",StringComparison.OrdinalIgnoreCase)||rel.Contains("provisioning",StringComparison.OrdinalIgnoreCase))throw new InvalidDataException("UPGRADE_MACHINE_STATE_PAYLOAD_DENIED");replacements.Add((Path.Combine(packageRoot,p),Path.Combine(App,rel)));}
   else if(p.StartsWith("host-app/")){var rel=p[9..];if(rel is not("cloudgime-host-bootstrap.exe" or "cloudgime-host-control.exe" or "open-host-control.exe" or "uninstaller-cloudgime.exe"))throw new InvalidDataException("UPGRADE_HOST_APP_PAYLOAD_DENIED");replacements.Add((Path.Combine(packageRoot,p),Path.Combine(HostApp,rel)));}
   else if(!p.StartsWith("host/"))throw new InvalidDataException("UPGRADE_PAYLOAD_GROUP_DENIED");}
  var helper=Path.Combine(packageRoot,"sentinel","resources","backend","SentinelHelper.exe");var shell=Path.Combine(packageRoot,"sentinel","resources","backend","CloudGimeRentalShell.exe");var launcher=Path.Combine(packageRoot,"sentinel","Sentinel Launcher.exe");
  foreach(var exe in new[]{helper,shell,launcher})UniversalBundleContract.RequireWindowsExecutable(exe);
  var marker=UniversalBundleContract.ParseObject(Path.Combine(packageRoot,"sentinel","resources","sentinel-runtime-broker-v1.json"));if((string?)marker["launcherSha256"]!=ManagedUpgradeTransaction.Hash(launcher)||(string?)marker["appAsarSha256"]!=ManagedUpgradeTransaction.Hash(Path.Combine(packageRoot,"sentinel","resources","app.asar")))throw new InvalidDataException("UPGRADE_MARKER_MISMATCH");
  replacements.Add((helper,(string)oldPlan["BrokerPath"]!));replacements.Add((shell,(string)oldPlan["ShellPath"]!));
  var worker=Path.Combine(Provision,"operator-emergency","worker-"+operatorId+".exe");if(!File.Exists(worker)||ManagedUpgradeTransaction.Hash(worker)!=(string?)oldPlan["HelperHash"])throw new InvalidDataException("UPGRADE_OLD_RESUME_WORKER_CHANGED");replacements.Add((helper,worker));
  foreach(var file in new[]{"rental-reset-shell.json","rental-reset-activation.json"}){
   var doc=UniversalBundleContract.ParseObject(Path.Combine(Provision,file));doc["BrokerSha256"]=ManagedUpgradeTransaction.Hash(helper);if(doc.ContainsKey("ShellSha256"))doc["ShellSha256"]=ManagedUpgradeTransaction.Hash(shell);var source=Path.Combine(op,file);File.WriteAllText(source,doc.ToJsonString());replacements.Add((source,Path.Combine(Provision,file)));}
  var shellJournalPath=Path.Combine(Provision,"rental-reset-shell-private","shell-configuration.json");
  var shellJournal=UniversalBundleContract.ParseObject(shellJournalPath);
  if((string?)shellJournal["Stage"]!="PREPARED"||(string?)shellJournal["Binding"]?["ExecutableSha256"]!=(string?)oldPlan["ShellHash"]||(string?)shellJournal["Binding"]?["RentalUserSid"]!=sid)throw new InvalidDataException("UPGRADE_SHELL_JOURNAL_CHANGED");
  shellJournal["Binding"]!["ExecutableSha256"]=ManagedUpgradeTransaction.Hash(shell);
  var shellJournalNew=Path.Combine(op,"shell-configuration-next.json");File.WriteAllText(shellJournalNew,shellJournal.ToJsonString());replacements.Add((shellJournalNew,shellJournalPath));
  oldPlan["BrokerHash"]=ManagedUpgradeTransaction.Hash(helper);oldPlan["HelperHash"]=ManagedUpgradeTransaction.Hash(helper);oldPlan["ShellHash"]=ManagedUpgradeTransaction.Hash(shell);oldPlan["LauncherHash"]=ManagedUpgradeTransaction.Hash(launcher);
  var newJournal=Path.Combine(op,"operator-next.json");File.WriteAllText(newJournal,journal.ToJsonString());replacements.Add((newJournal,operatorFile));
  var preserved=new List<Preserved>();foreach(var f in Directory.EnumerateFiles(Provision,"*.dpapi").Concat(new[]{Path.Combine(Provision,"manifest.json"),Path.Combine(Provision,"rental-turnover.json"),Path.Combine(Data,"Host","moonlight","server","config.json"),Path.Combine(Data,"Host","sunshine","config","sunshine.conf")}))
  {UniversalMachineBoundary.RequireProtected(f);preserved.Add(new(f,ManagedUpgradeTransaction.Hash(f),FileSddl(f)));}
  var packageInventory=entries.Select(x=>(string)x!["path"]!).ToHashSet(StringComparer.OrdinalIgnoreCase);
  foreach(var required in new[]{"sentinel/Sentinel Launcher.exe","sentinel/resources/app.asar","sentinel/resources/backend/SentinelHelper.exe","sentinel/resources/backend/CloudGimeRentalShell.exe","sentinel/resources/sentinel-runtime-broker-v1.json","host-app/cloudgime-host-bootstrap.exe","host/moonlight/web-server.exe","host/moonlight/streamer.exe","host/moonlight/system/cloudgime-runtime-agent.exe"})
   if(!packageInventory.Contains(required))throw new InvalidDataException("UPGRADE_REQUIRED_COMPONENT_MISSING");
  foreach(var componentPath in replacements.Select(x=>x.Target))UniversalMachineBoundary.RequireProtected(Path.GetDirectoryName(componentPath)!,true);
  var unchanged=replacements.Where(x=>File.Exists(x.Target)&&ManagedUpgradeTransaction.Hash(x.Source)==ManagedUpgradeTransaction.Hash(x.Target)).ToArray();
  foreach(var item in unchanged)if(!preserved.Any(x=>x.Path.Equals(item.Target,StringComparison.OrdinalIgnoreCase)))preserved.Add(new(item.Target,ManagedUpgradeTransaction.Hash(item.Target),FileSddl(item.Target)));
  var changed=replacements.Except(unchanged).ToList();
  if(changed.Count==0)throw new InvalidDataException("UPGRADE_NO_CHANGED_FILES");
  var context=new Context(id,packageHash,operatorId,sid,"explorer.exe",preserved,WindowsState(sid),ProfileInventory());WriteDurable(Path.Combine(op,"context.json"),JsonSerializer.Serialize(context));
  var recoveryRunner=Path.Combine(op,"recovery.exe");File.Copy(Environment.ProcessPath!,recoveryRunner,false);
  using(var flush=new FileStream(recoveryRunner,FileMode.Open,FileAccess.ReadWrite,FileShare.Read))flush.Flush(true);
  if(ManagedUpgradeTransaction.Hash(recoveryRunner)!=ManagedUpgradeTransaction.Hash(Environment.ProcessPath!))throw new InvalidDataException("UPGRADE_RECOVERY_BINARY_CHANGED");
  ManagedUpgradeTransaction.Prepare(op,id,Allowed,changed);VerifyPreserved(context,true);WriteReceipt(op,"prepared",context);
 }
 private static void RequireStopped(string operatorId,string userShell)
 {
  var j=UniversalBundleContract.ParseObject(Path.Combine(Provision,"operator-emergency","active.json"));if((string?)j["State"]?["OperationId"]!=operatorId||(string?)j["State"]?["Stage"]!="STOPPED")throw new InvalidDataException("UPGRADE_OPERATOR_STOP_CHANGED");
  foreach(var name in new[]{"SentinelNodeBroker","Sentinel Launcher","SentinelHelper","CloudGimeRentalShell","streamer"})if(Process.GetProcessesByName(name).Length!=0)throw new InvalidDataException("UPGRADE_RUNTIME_ACTIVE");
  using var search=new ManagementObjectSearcher("SELECT State,StartMode FROM Win32_Service WHERE Name='SentinelNodeBroker'");var rows=search.Get().Cast<ManagementObject>().ToArray();if(rows.Length!=1||(string?)rows[0]["State"]!="Stopped"||(string?)rows[0]["StartMode"]!="Disabled")throw new InvalidDataException("UPGRADE_STOP_SERVICE_CHANGED");
  using var users=RegistryKey.OpenBaseKey(RegistryHive.Users,RegistryView.Registry64);using var key=users.OpenSubKey((string)j["Plan"]!["WindowsUserSid"]!+@"\Software\Microsoft\Windows\CurrentVersion\Policies\System");if((string?)key?.GetValue("Shell")!=userShell)throw new InvalidDataException("UPGRADE_USER_SHELL_CHANGED");
  var activation=UniversalBundleContract.ParseObject(Path.Combine(Provision,"rental-reset-activation.json"));if((bool?)activation["Enabled"]!=false||File.Exists(Path.Combine(Provision,"rental-reset-private","journal","reset-journal.json")))throw new InvalidDataException("UPGRADE_RESET_OR_PILOT_ACTIVE");
 }
 private static string FileSddl(string path)=>new FileInfo(path).GetAccessControl(AccessControlSections.Access|AccessControlSections.Owner|AccessControlSections.Group).GetSecurityDescriptorSddlForm(AccessControlSections.Access|AccessControlSections.Owner|AccessControlSections.Group);
 private static void VerifyPreserved(Context c,bool profiles){foreach(var p in c.Files){UniversalMachineBoundary.RequireProtected(p.Path);if(ManagedUpgradeTransaction.Hash(p.Path)!=p.Hash||FileSddl(p.Path)!=p.Sddl)throw new InvalidDataException("UPGRADE_PRESERVED_DATA_CHANGED");}if(WindowsState(c.WindowsUserSid)!=c.WindowsState)throw new InvalidDataException("UPGRADE_WINDOWS_STATE_CHANGED");if(profiles&&!ProfileInventory().SequenceEqual(c.Profiles))throw new InvalidDataException("UPGRADE_PROFILE_CONTENT_OR_ACL_CHANGED");}
 private static List<Preserved> ProfileInventory()
 {
  var root=Path.Combine(Data,"RentalProfiles");var result=new List<Preserved>();if(!Directory.Exists(root))return result;
  var queue=new Queue<string>();queue.Enqueue(root);while(queue.Count>0){var dir=queue.Dequeue();UniversalBundleContract.RejectReparse(dir);foreach(var p in Directory.EnumerateFileSystemEntries(dir).Order(StringComparer.Ordinal)){UniversalBundleContract.RejectReparse(p);if(Directory.Exists(p)){queue.Enqueue(p);continue;}if(result.Count>=20000)throw new InvalidDataException("UPGRADE_PROFILE_INVENTORY_LIMIT");result.Add(new(p,ManagedUpgradeTransaction.Hash(p),FileSddl(p)));}}return result.OrderBy(x=>x.Path,StringComparer.Ordinal).ToList();
 }
 private static string WindowsState(string sid)
 {
  using var m=RegistryKey.OpenBaseKey(RegistryHive.LocalMachine,RegistryView.Registry64);using var user=RegistryKey.OpenBaseKey(RegistryHive.Users,RegistryView.Registry64);
  var values=new SortedDictionary<string,string?>(StringComparer.Ordinal);
  foreach(var keyPath in new[]{@"SYSTEM\CurrentControlSet\Services\SentinelNodeBroker",@"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon"}){using var key=m.OpenSubKey(keyPath)??throw new InvalidDataException("UPGRADE_REGISTRY_MISSING");foreach(var name in key.GetValueNames().Order(StringComparer.Ordinal)){if(name=="DefaultPassword"){values[keyPath+":"+name]="PRESENT";continue;}var value=key.GetValue(name);values[keyPath+":"+name]=JsonSerializer.Serialize(value);}}
  using var shell=user.OpenSubKey(sid+@"\Software\Microsoft\Windows\CurrentVersion\Policies\System")??throw new InvalidDataException("UPGRADE_USER_POLICY_MISSING");values["UserShell"]=(string?)shell.GetValue("Shell");
  foreach(var service in new[]{"CloudgimeHost-Host","sshd","StarDeskService","ZeroTierOneService"}){using var query=new ManagementObjectSearcher("SELECT State,ProcessId FROM Win32_Service WHERE Name='"+service+"'");foreach(ManagementObject row in query.Get())values[service]=row["State"]+":"+row["ProcessId"];}
  return JsonSerializer.Serialize(values);
 }
 private static void WriteDurable(string path,string content){using var f=new FileStream(path,FileMode.CreateNew,FileAccess.Write,FileShare.None,4096,FileOptions.WriteThrough);var bytes=System.Text.Encoding.UTF8.GetBytes(content);f.Write(bytes);f.Flush(true);}
 private static void WriteReceipt(string op,string stage,Context c){var p=Path.Combine(op,stage+".json");var value=JsonSerializer.Serialize(new{stage,operationId=c.OperationId,operatorId=c.OperatorId,at=DateTimeOffset.UtcNow,operatorState="STOPPED",runtimeStarted=false,licenseClaimed=false,profilesTouched=false});if(!File.Exists(p))WriteDurable(p,value);}
 private static void RequireAdmin(){using var me=WindowsIdentity.GetCurrent();using var imp=WindowsIdentity.GetCurrent(true);if(imp!=null||!new WindowsPrincipal(me).IsInRole(WindowsBuiltInRole.Administrator))throw new UnauthorizedAccessException("UPGRADE_ADMIN_REQUIRED");}
 private static DirectorySecurity PrivateAcl(){var a=new DirectorySecurity();a.SetAccessRuleProtection(true,false);a.SetOwner(new SecurityIdentifier("S-1-5-32-544"));foreach(var sid in new[]{"S-1-5-18","S-1-5-32-544"})a.AddAccessRule(new(new SecurityIdentifier(sid),FileSystemRights.FullControl,InheritanceFlags.ContainerInherit|InheritanceFlags.ObjectInherit,PropagationFlags.None,AccessControlType.Allow));return a;}
 private static void EnsurePrivateRecovery(){UniversalMachineBoundary.RequireProtected(Data,true);UniversalBundleContract.RejectReparse(Recovery);if(!Directory.Exists(Recovery))new DirectoryInfo(Recovery).Create(PrivateAcl());UniversalMachineBoundary.RequireProtected(Recovery,true);}
 private static void ArmRecovery(string op,string id)
 {
  var source=Environment.ProcessPath!;var worker=Path.Combine(op,"recovery.exe");if(!File.Exists(worker))File.Copy(source,worker,false);if(ManagedUpgradeTransaction.Hash(source)!=ManagedUpgradeTransaction.Hash(worker))throw new InvalidDataException("UPGRADE_RECOVERY_BINARY_CHANGED");
  var xml=Path.Combine(op,"watch.xml");var command=System.Security.SecurityElement.Escape(worker);File.WriteAllText(xml,"<?xml version=\"1.0\" encoding=\"UTF-16\"?><Task version=\"1.2\" xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\"><Triggers/><Principals><Principal id=\"System\"><UserId>S-1-5-18</UserId><RunLevel>HighestAvailable</RunLevel></Principal></Principals><Settings><ExecutionTimeLimit>PT30M</ExecutionTimeLimit></Settings><Actions Context=\"System\"><Exec><Command>"+command+"</Command><Arguments>managed-upgrade watch "+id+" --silent</Arguments></Exec></Actions></Task>",System.Text.Encoding.Unicode);
  TaskTool(["/Create","/TN","CloudGime-ManagedUpgrade-"+id,"/XML",xml,"/F"]);TaskTool(["/Run","/TN","CloudGime-ManagedUpgrade-"+id]);
 }
 private static void TryDeleteTask(string id){try{TaskTool(["/Delete","/TN","CloudGime-ManagedUpgrade-"+id,"/F"]);}catch{}}
 private static void TaskTool(string[] args){var start=new ProcessStartInfo(Path.Combine(Environment.SystemDirectory,"schtasks.exe")){UseShellExecute=false,CreateNoWindow=true,RedirectStandardOutput=true,RedirectStandardError=true};foreach(var a in args)start.ArgumentList.Add(a);using var p=Process.Start(start)!;var a1=p.StandardOutput.ReadToEndAsync();var a2=p.StandardError.ReadToEndAsync();if(!p.WaitForExit(15000))throw new TimeoutException("UPGRADE_RECOVERY_TASK_TIMEOUT");Task.WaitAll(a1,a2);if(p.ExitCode!=0)throw new IOException("UPGRADE_RECOVERY_TASK_FAILED");}
}
