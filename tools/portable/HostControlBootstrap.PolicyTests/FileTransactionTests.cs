using CloudgimeHostBootstrap;

internal static class FileTransactionTests
{
    internal static int Run()
    {
        var root=Path.Combine(Path.GetTempPath(),"cg-upgrade-test-"+Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(root);
        try
        {
            var source=Path.Combine(root,"new.txt");var target=Path.Combine(root,"old.txt");var added=Path.Combine(root,"added.txt");
            File.WriteAllText(source,"NEW");File.WriteAllText(target,"OLD");
            var t=new UniversalFileTransaction(Path.Combine(root,"backup"));
            t.Replace(source,target);t.Replace(source,added);
            if(File.ReadAllText(target)!="NEW"||File.ReadAllText(added)!="NEW")throw new Exception("UPGRADE_NOT_APPLIED");
            t.Rollback();
            if(File.ReadAllText(target)!="OLD"||File.Exists(added))throw new Exception("ROLLBACK_DID_NOT_RESTORE");
            var t2=new UniversalFileTransaction(Path.Combine(root,"backup"));t2.Replace(source,target);t2.Commit();
            if(File.ReadAllText(target)!="NEW"||File.ReadAllText(Path.Combine(t2.RecoveryRoot,"000000.original"))!="OLD")throw new Exception("COMMIT_BACKUP_MISSING");
            return 3;
        }
        finally {Directory.Delete(root,true);}
    }
}
