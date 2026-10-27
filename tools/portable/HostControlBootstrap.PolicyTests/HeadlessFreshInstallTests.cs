using CloudgimeHostBootstrap;
internal static class HeadlessFreshInstallTests
{
    internal static void Run()
    {
        int n=0;
        void Check(bool ok){n++;if(!ok)throw new Exception("FRESH_INSTALL_TEST_FAILED");}
        void Reject(Action action){n++;try{action();}catch(InvalidDataException){return;}throw new Exception("FRESH_INSTALL_REJECTION_MISSING");}
        var state=new HeadlessFreshJournal(1,Guid.NewGuid().ToString("D"),new('a',64),"S-1-5-21-1-2-3-1001","STAGING",null,null);
        Check(HeadlessSetupCommands.Handles(["validate-universal-bundle","--silent","--bundle-root","C:\\fixture"]));
        // Command dispatch is covered here; package-integrity behavior is exercised
        // by BundleTests with a complete inert fixture.
        Check(!HeadlessSetupCommands.Handles(["unknown-command"]));
        HeadlessFreshInstall.RequireTransition(null,state);Check(true);
        var actions=new[]{"STAGE","ACTIVATE","RUNTIME","FIRST_BOOT"};
        for(int i=0;i<4;i++)
        {
            var pending=state with{Pending=actions[i]};HeadlessFreshInstall.RequireTransition(state,pending);Check(true);
            var failed=pending with{Error="FIXTURE_INTERRUPTED"};HeadlessFreshInstall.RequireTransition(pending,failed);Check(true);
            HeadlessFreshInstall.RequireTransition(failed,pending);Check(true);
            var done=pending with{Stage=HeadlessFreshInstall.Stages[i+1],Pending=null};HeadlessFreshInstall.RequireTransition(pending,done);Check(true);
            Reject(()=>HeadlessFreshInstall.RequireTransition(state,done));
            Reject(()=>HeadlessFreshInstall.RequireTransition(pending,done with{PackageHash=new('b',64)}));
            Reject(()=>HeadlessFreshInstall.RequireTransition(pending,done with{OperatorSid="S-1-5-21-1-2-3-1002"}));
            state=done;
        }
        Reject(()=>HeadlessFreshInstall.Validate(state with{Pending="FIRST_BOOT"}));
        Reject(()=>HeadlessFreshInstall.RequireTransition(state,state with{Stage="FILES_STAGED"}));
        Reject(()=>HeadlessFreshInstall.Validate(state with{OperationId=Guid.Empty.ToString("D")}));
        Reject(()=>HeadlessFreshInstall.Validate(state with{Error="raw secret in error"}));
        Check(HeadlessPublicRoute.Build("https://cloudgime.my.id","2")=="https://cloudgime.my.id/api/v1/application-proxy/pc/2/host/stream/");
        foreach(var id in new[]{"../2","2/host","","2?x=1"})Reject(()=>HeadlessPublicRoute.Build("https://cloudgime.my.id",id));
        Reject(()=>HeadlessPublicRoute.Build("http://cloudgime.my.id","2"));
        Console.WriteLine($"HEADLESS_FRESH_INSTALL_JOURNAL_ASSERTIONS={n}");
    }
}
