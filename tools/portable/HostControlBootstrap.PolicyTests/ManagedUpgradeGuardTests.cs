using CloudgimeHostBootstrap;
internal static class ManagedUpgradeGuardTests
{
    internal static int Run()
    {
        var root=Path.Combine(Path.GetTempPath(),"cg-managed-guard-"+Guid.NewGuid().ToString("N"));Directory.CreateDirectory(root);int checks=0;
        void Deny(string dir){try{ManagedNodeUpgradeGuard.RequireUnmanaged(dir);}catch(InvalidOperationException e)when(e.Message.StartsWith("MANAGED_NODE_REQUIRES_COORDINATED_UPGRADE",StringComparison.Ordinal)){checks++;return;}throw new Exception("MANAGED_STATE_ACCEPTED");}
        try
        {
            ManagedNodeUpgradeGuard.RequireUnmanaged(Path.Combine(root,"missing"));checks++;
            ManagedNodeUpgradeGuard.RequireUnmanaged(root);checks++;
            int n=0;
            foreach(var p in ManagedNodeUpgradeGuard.ProtectedState)
            {
                foreach(var text in new[]{"","{}","{\"State\":{\"Stage\":\"STOPPED\"}}","{\"State\":{\"Stage\":\"RESUMED\"}}"})
                {
                    var dir=Path.Combine(root,(n++).ToString());var file=Path.Combine(dir,p);Directory.CreateDirectory(Path.GetDirectoryName(file)!);File.WriteAllText(file,text);
                    Deny(dir);if(File.ReadAllText(file)!=text)throw new Exception("GUARD_MUTATED_STATE");checks++;
                }
                var folder=Path.Combine(root,(n++).ToString());Directory.CreateDirectory(Path.Combine(folder,p));Deny(folder);
            }
            var identity=Path.Combine(root,"legacy");Directory.CreateDirectory(identity);File.WriteAllText(Path.Combine(identity,"machine-identity.dpapi"),"fixture-identity");ManagedNodeUpgradeGuard.RequireUnmanaged(identity);checks++;
            Console.WriteLine($"MANAGED_UPGRADE_GUARD_ASSERTIONS={checks}; OS_SERVICE_MUTATION=false");return checks;
        }
        finally{Directory.Delete(root,true);}
    }
}
