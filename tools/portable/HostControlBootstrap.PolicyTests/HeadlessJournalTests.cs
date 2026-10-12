using System.Diagnostics;
using System.Security.AccessControl;
using System.Security.Principal;
using System.Text.Json;
using CloudgimeHostBootstrap;

internal static class HeadlessJournalTests
{
    private static string Root(string value)
    {
        var root=Path.GetFullPath(value);var parent=Path.GetFullPath(Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData)).TrimEnd(Path.DirectorySeparatorChar)+Path.DirectorySeparatorChar;
        if(!string.Equals(Path.GetDirectoryName(root)+Path.DirectorySeparatorChar,parent,StringComparison.OrdinalIgnoreCase)||!System.Text.RegularExpressions.Regex.IsMatch(Path.GetFileName(root),@"\Acg-headless-journal-[a-f0-9]{32}\z"))throw new InvalidDataException("HEADLESS_FIXTURE_ROOT_INVALID");
        return root;
    }
    internal static bool Worker(string[] args)
    {
        if(args.Length==0||args[0]!="--headless-journal-fixture")return false;
        if(args.Length!=3)throw new InvalidDataException("HEADLESS_FIXTURE_ARGS");
        using var journal=new HeadlessSetupJournal(Root(args[1]));
        var current=journal.Read()??throw new InvalidDataException("HEADLESS_FIXTURE_MISSING");
        if(current.OperationId!=args[2]||current.Stage!="FILES_STAGED"||current.Pending!="PROVISION_NODE")throw new InvalidDataException("HEADLESS_FIXTURE_STATE_INVALID");
        journal.Save(current,current with{Stage="NODE_PROVISIONED",Pending=null});
        Console.WriteLine("HEADLESS_JOURNAL_CHILD_ADVANCED");return true;
    }
    internal static int Run()
    {
        var root=Root(Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData),"cg-headless-journal-"+Guid.NewGuid().ToString("N")));
        var acl=new DirectorySecurity();acl.SetAccessRuleProtection(true,false);acl.SetOwner(new SecurityIdentifier("S-1-5-32-544"));
        foreach(var sid in new[]{"S-1-5-18","S-1-5-32-544"})acl.AddAccessRule(new FileSystemAccessRule(new SecurityIdentifier(sid),FileSystemRights.FullControl,InheritanceFlags.ContainerInherit|InheritanceFlags.ObjectInherit,PropagationFlags.None,AccessControlType.Allow));
        new DirectoryInfo(root).Create(acl);int count=0;void Check(bool ok,string message){count++;if(!ok)throw new Exception(message);}
        var first=new HeadlessSetupState(1,Guid.NewGuid().ToString("D"),new string('a',64),"S-1-5-21-1-2-3-1001","FILES_STAGED",null,null);
        try
        {
            using(var j=new HeadlessSetupJournal(root))
            {
                Check(j.Read()==null,"new empty journal");j.Save(null,first);Check(j.Read()==first,"persist initial");
                try{using var conflict=new HeadlessSetupJournal(root);throw new Exception("concurrent lock accepted");}catch(IOException){count++;}
                try{j.Save(first,first with{Stage="ACCOUNTS_PREPARED"});throw new Exception("skipped stages accepted");}catch(InvalidDataException){count++;}
                j.Save(first,first with{Pending="PROVISION_NODE"});
            }
            var executable=Environment.ProcessPath!;var args=new List<string>();if(Path.GetFileNameWithoutExtension(executable).Equals("dotnet",StringComparison.OrdinalIgnoreCase))args.Add(typeof(HeadlessJournalTests).Assembly.Location);
            args.AddRange(["--headless-journal-fixture",root,first.OperationId]);
            var output=HeadlessSetupProcess.Run(executable,args,Environment.CurrentDirectory,ReadOnlyMemory<char>.Empty,TimeSpan.FromSeconds(10));
            Check(output.Contains("HEADLESS_JOURNAL_CHILD_ADVANCED"),"fresh process read existing journal");
            using(var j=new HeadlessSetupJournal(root))
            {
                Check(j.Read()!.Stage=="NODE_PROVISIONED","parent reads child progress");
                try{j.Save(first,first);throw new Exception("stale write accepted");}catch(IOException){count++;}
            }
            var file=Path.Combine(root,"setup.json");var original=File.ReadAllText(file);
            foreach(var corrupt in new[]{"{}",original.Replace("\"Version\":1","\"Version\":1,\"Version\":1"),original.Replace("\"NODE_PROVISIONED\"","\"RENTABLE\""),original[..^1]+",\"password\":\"fixture\"}"})
            {
                File.WriteAllText(file,corrupt);using var j=new HeadlessSetupJournal(root);
                try{j.Read();throw new Exception("bad journal accepted");}catch(InvalidDataException error){Check(error.Message=="HEADLESS_SETUP_JOURNAL_INVALID","malformed journal denied");}
            }
            File.WriteAllText(file,original);using(var j=new HeadlessSetupJournal(root))Check(j.Read()!.OperationId==first.OperationId,"identity unchanged");
            var publicRule=new FileSystemAccessRule(new SecurityIdentifier("S-1-5-32-545"),FileSystemRights.Write,AccessControlType.Allow);var bad=new FileInfo(file).GetAccessControl();bad.AddAccessRule(publicRule);new FileInfo(file).SetAccessControl(bad);
            using(var j=new HeadlessSetupJournal(root)){try{j.Read();throw new Exception("writable journal accepted");}catch(InvalidOperationException error){Check(error.Message=="INSTALL_PATH_WRITABLE_BY_NONADMIN","writable journal denied");}}
            Console.WriteLine($"HEADLESS_DURABLE_JOURNAL_ASSERTIONS={count}; protected disposable ProgramData fixture and fresh child process; no installed CloudGime/account/service changes");return count;
        }
        finally{Directory.Delete(root,true);}
    }
}
