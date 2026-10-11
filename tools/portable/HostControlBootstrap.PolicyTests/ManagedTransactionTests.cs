using CloudgimeHostBootstrap;
internal static class ManagedTransactionTests
{
 internal static int Run()
 {
  var root=Path.Combine(Path.GetTempPath(),"cg-managed-tx-"+Guid.NewGuid().ToString("N"));Directory.CreateDirectory(root);int checks=0;
  void Check(bool b,string n){checks++;if(!b)throw new Exception(n);}
  try{
   for(int cut=0;cut<4;cut++){
    var dir=Path.Combine(root,"cut"+cut);var live=Path.Combine(dir,"live");var src=Path.Combine(dir,"source");Directory.CreateDirectory(live);Directory.CreateDirectory(src);var pairs=new List<(string Source,string Target)>();
    for(int i=0;i<4;i++){var a=Path.Combine(src,i+".bin");var b=Path.Combine(live,i+".bin");File.WriteAllText(a,"NEW"+i);File.WriteAllText(b,"OLD"+i);pairs.Add((a,b));}
    var recovery=Path.Combine(dir,"recovery");var tx=ManagedUpgradeTransaction.Prepare(recovery,Guid.NewGuid().ToString("D"),[live],pairs);
    try{tx.Apply(()=>{},i=>{if(i==cut)throw new IOException("SIMULATED_PROCESS_EXIT");});throw new Exception("NO_FAULT");}catch(IOException e)when(e.Message=="SIMULATED_PROCESS_EXIT"){}
    // New object/new journal load, no reliance on in-memory entries.
    var resumed=ManagedUpgradeTransaction.Open(recovery,[live]);resumed.Rollback(()=>{});Check(resumed.Phase=="ROLLED_BACK","phase");for(int i=0;i<4;i++)Check(File.ReadAllText(pairs[i].Target)=="OLD"+i,"original restored");resumed.Rollback(()=>{});checks++;
   }
   var live2=Path.Combine(root,"full");Directory.CreateDirectory(live2);var source=Path.Combine(root,"new.bin");File.WriteAllText(source,"NEW");var target=Path.Combine(live2,"app.bin");File.WriteAllText(target,"OLD");var rec=Path.Combine(root,"rec");var ok=ManagedUpgradeTransaction.Prepare(rec,Guid.NewGuid().ToString("D"),[live2],[(source,target)]);ok.Apply(()=>{});Check(ok.Phase=="COMMITTED","commit");ManagedUpgradeTransaction.Open(rec,[live2]).Rollback(()=>{});Check(File.ReadAllText(target)=="OLD","post commit rollback");
   rec=Path.Combine(root,"conflict");var conflict=ManagedUpgradeTransaction.Prepare(rec,Guid.NewGuid().ToString("D"),[live2],[(source,target)]);conflict.Apply(()=>{});File.WriteAllText(target,"FOREIGN");try{ManagedUpgradeTransaction.Open(rec,[live2]).Rollback(()=>{});throw new Exception("FOREIGN_OVERWRITTEN");}catch(IOException e)when(e.Message=="ROLLBACK_EXTERNAL_CHANGE"){checks++;}Check(File.ReadAllText(target)=="FOREIGN","foreign untouched");
   var added=Path.Combine(live2,"added.bin");rec=Path.Combine(root,"add");var add=ManagedUpgradeTransaction.Prepare(rec,Guid.NewGuid().ToString("D"),[live2],[(source,added)]);add.Apply(()=>{});ManagedUpgradeTransaction.Open(rec,[live2]).Rollback(()=>{});Check(!File.Exists(added)&&File.ReadAllText(Path.Combine(rec,"added-0.retired"))=="NEW","new file quarantined");
   try{ManagedUpgradeTransaction.Prepare(Path.Combine(root,"bad"),Guid.NewGuid().ToString("D"),[live2],[(source,Path.Combine(root,"escape"))]);throw new Exception("ESCAPE_ALLOWED");}catch(InvalidDataException e)when(e.Message=="UPGRADE_TARGET_OUTSIDE_ROOTS"){checks++;}
   rec=Path.Combine(root,"crashresume");File.WriteAllText(target,"OLD");var cr=ManagedUpgradeTransaction.Prepare(rec,Guid.NewGuid().ToString("D"),[live2],[(source,target)]);try{cr.Apply(()=>{},_=>throw new IOException("CRASH"));}catch(IOException){}var re=ManagedUpgradeTransaction.Open(rec,[live2]);re.Apply(()=>{});Check(re.Phase=="COMMITTED"&&File.ReadAllText(target)=="NEW","resume after replace-before-journal");
   var journal=Path.Combine(rec,"transaction.json");var text=File.ReadAllText(journal);File.WriteAllText(journal,text.Replace("app.bin","../../escape.bin"));try{ManagedUpgradeTransaction.Open(rec,[live2]);throw new Exception("TAMPER_ACCEPTED");}catch(InvalidDataException){checks++;}
   // Real sharing violation: first target changes, second cannot be replaced.
   var lockedRoot=Path.Combine(root,"locked");Directory.CreateDirectory(lockedRoot);var first=Path.Combine(lockedRoot,"first.bin");var locked=Path.Combine(lockedRoot,"locked.bin");File.WriteAllText(first,"OLD-FIRST");File.WriteAllText(locked,"OLD-LOCKED");
   rec=Path.Combine(root,"locked-rec");var lockTx=ManagedUpgradeTransaction.Prepare(rec,Guid.NewGuid().ToString("D"),[lockedRoot],[(source,first),(source,locked)]);
   using(var held=new FileStream(locked,FileMode.Open,FileAccess.Read,FileShare.Read)){
    try{lockTx.Apply(()=>{});throw new Exception("LOCK_DID_NOT_FAIL");}catch(Exception e)when(e is IOException or UnauthorizedAccessException){checks++;ManagedUpgradeTransaction.Open(rec,[lockedRoot]).Rollback(()=>{});}
    Check(File.ReadAllText(first)=="OLD-FIRST"&&File.ReadAllText(locked)=="OLD-LOCKED","sharing-failure rollback restores earlier write");
    Check(Directory.GetFiles(lockedRoot).Length==2,"failed replace leaves no staging file");
   }
   var corrupt=Path.Combine(root,"corrupt-rec");File.WriteAllText(target,"OLD");var corruptTx=ManagedUpgradeTransaction.Prepare(corrupt,Guid.NewGuid().ToString("D"),[live2],[(source,target)]);corruptTx.Apply(()=>{});File.WriteAllText(Path.Combine(corrupt,"backup","000000.old"),"CORRUPT");
   try{ManagedUpgradeTransaction.Open(corrupt,[live2]).Rollback(()=>{});throw new Exception("CORRUPT_BACKUP_ACCEPTED");}catch(IOException e)when(e.Message=="ROLLBACK_BACKUP_CHANGED"){checks++;}Check(File.ReadAllText(target)=="NEW","corrupt backup does not overwrite live");
   Console.WriteLine($"MANAGED_DURABLE_TRANSACTION_ASSERTIONS={checks}; real temp filesystem/new-object recovery; no production install");return checks;
  }finally{Directory.Delete(root,true);}
 }
}
