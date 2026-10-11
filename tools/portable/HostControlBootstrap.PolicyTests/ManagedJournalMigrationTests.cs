using System.Text.Json.Nodes;
using CloudgimeHostBootstrap;

internal static class ManagedJournalMigrationTests
{
 internal static void Run()
 {
  var root=Path.Combine(Path.GetTempPath(),"cg-journal-migration-"+Guid.NewGuid().ToString("N"));var live=Path.Combine(root,"live");var source=Path.Combine(root,"source");Directory.CreateDirectory(live);Directory.CreateDirectory(source);
  try
  {
   const string operation="11111111-1111-4111-8111-111111111111";var beforeHash=new string('a',64);var afterHash=new string('b',64);
   var journal=new JsonObject{["State"]=new JsonObject{["OperationId"]=operation,["Stage"]="STOPPED",["RequesterSid"]="S-1-5-21-fixture"},["Plan"]=new JsonObject{["BrokerHash"]=beforeHash,["HelperHash"]=beforeHash,["ShellHash"]=beforeHash}};
   var updated=(JsonObject)journal.DeepClone();foreach(var key in new[]{"BrokerHash","HelperHash","ShellHash"})updated["Plan"]![key]=afterHash;
   var old=journal.ToJsonString();var next=updated.ToJsonString();var target=Path.Combine(live,"active.json");var candidate=Path.Combine(source,"active.json");File.WriteAllText(target,old);File.WriteAllText(candidate,next);
   var worker=Path.Combine(live,"worker.exe");var newWorker=Path.Combine(source,"worker.exe");File.WriteAllText(worker,"OLD-WORKER");File.WriteAllText(newWorker,"NEW-WORKER");
   var recovery=Path.Combine(root,"recovery");var tx=ManagedUpgradeTransaction.Prepare(recovery,Guid.NewGuid().ToString("D"),[live],[(newWorker,worker),(candidate,target)]);tx.Apply(()=>{});
   var actual=JsonNode.Parse(File.ReadAllText(target))!;if((string?)actual["State"]?["OperationId"]!=operation||(string?)actual["State"]?["Stage"]!="STOPPED"||(string?)actual["Plan"]?["BrokerHash"]!=afterHash||File.ReadAllText(worker)!="NEW-WORKER")throw new Exception("MIGRATION_BINDING_CHANGED");
   ManagedUpgradeTransaction.Open(recovery,[live]).Rollback(()=>{});if(File.ReadAllText(target)!=old||File.ReadAllText(worker)!="OLD-WORKER")throw new Exception("MIGRATION_ROLLBACK_MISMATCH");
   Console.WriteLine("MANAGED_HASH_JOURNAL_MIGRATION_FIXTURE_PASS operatorIdAndStopPreserved=true originalBytesRestored=true; not a new-helper live test");
  }
  finally{Directory.Delete(root,true);}
 }
}
