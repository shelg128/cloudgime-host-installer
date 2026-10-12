using CloudgimeHostBootstrap;
using System.Text.Json;
internal static class HeadlessSetupTests
{
    internal static int Run()
    {
        int n=0;void Check(bool value,string code){n++;if(!value)throw new Exception(code);}
        HeadlessSetupState Initial()=>new(1,Guid.NewGuid().ToString("D"),new string('a',64),"S-1-5-21-1-2-3-1001","FILES_STAGED",null,null);
        var license="unit-test-secret-license";
        var store=new Store();var platform=new Platform(store);var original=Initial();
        var complete=HeadlessSetupCoordinator.Run(store,platform,original,license.AsMemory());
        Check(complete.Stage=="ACCOUNTS_PREPARED"&&complete.Pending==null,"completion stage");
        Check(platform.Calls.SequenceEqual(HeadlessSetupCoordinator.Steps),"step order");
        Check(platform.SecretReceived.SequenceEqual(new[]{true,false,false}),"license routed only to provision");
        Check(!JsonSerializer.Serialize(store.History).Contains(license),"journal has no license");
        var status=JsonSerializer.Serialize(HeadlessSetupCoordinator.PublicStatus(complete));Check(status.Contains("\"readyForRental\":false")&&status.Contains("\"autologonArmed\":false"),"not a readiness claim");
        HeadlessSetupCoordinator.Run(store,platform,original,ReadOnlyMemory<char>.Empty);Check(platform.Calls.Count==3,"idempotent complete retry");
        foreach(var afterEffect in new[]{false,true})foreach(var step in HeadlessSetupCoordinator.Steps)
        {
            store=new Store();platform=new Platform(store){FailStep=step,FailAfter=afterEffect};original=Initial();
            try{HeadlessSetupCoordinator.Run(store,platform,original,license.AsMemory());throw new Exception("failure not raised");}
            catch(InvalidDataException e){Check(e.Message=="HEADLESS_SETUP_STEP_FAILED","exception sanitized");}
            Check(store.Current?.Pending==step,"pending retained");Check(store.Current?.OperationId==original.OperationId,"operation preserved");
            Check(!JsonSerializer.Serialize(store.History).Contains(license),"failure journal no secret");
            platform.FailStep=null;var retried=HeadlessSetupCoordinator.Run(store,platform,original,license.AsMemory());
            Check(retried.Stage=="ACCOUNTS_PREPARED","retry finished");
            Check(platform.Calls.Count(c=>c==step)==(afterEffect?1:2),"lost effect not duplicated");
        }
        store=new Store();platform=new Platform(store);original=Initial();HeadlessSetupCoordinator.Run(store,platform,original,license.AsMemory());
        foreach(var changed in new[]{original with{OperationId=Guid.NewGuid().ToString("D")},original with{OperatorSid="S-1-5-21-1-2-3-1002"},original with{PackageHash=new string('b',64)}})
        {try{HeadlessSetupCoordinator.Run(store,platform,changed,license.AsMemory());throw new Exception("binding accepted");}catch(InvalidDataException e){Check(e.Message=="HEADLESS_SETUP_BINDING_CHANGED","changed binding denied");}}
        platform.Done.Remove("PROVISION_NODE");try{HeadlessSetupCoordinator.Run(store,platform,original,license.AsMemory());throw new Exception("missing identity accepted");}catch(InvalidDataException e){Check(e.Message=="HEADLESS_SETUP_COMPLETED_STEP_CHANGED","completed step reobserved");}
        foreach(var malformed in new[]{original with{Stage="AVAILABLE"},original with{PackageHash="bad"},original with{Pending="REBOOT"},original with{Error="password=secret"}})
        {try{HeadlessSetupCoordinator.Validate(malformed);throw new Exception("invalid accepted");}catch(InvalidDataException){Check(true,"invalid journal denied");}}
        Console.WriteLine($"HEADLESS_SETUP_COORDINATOR_ASSERTIONS={n}; fixture only, no Windows account/activation/readiness mutation");return n;
    }
    private sealed class Store:IHeadlessSetupStore
    {
        internal HeadlessSetupState? Current;internal List<HeadlessSetupState> History=[];
        public HeadlessSetupState? Read()=>Current;
        public void Save(HeadlessSetupState? previous,HeadlessSetupState next){if(Current!=previous)throw new IOException("CAS");HeadlessSetupCoordinator.RequireTransition(previous,next);Current=next;History.Add(next);}
    }
    private sealed class Platform(Store store):IHeadlessSetupPlatform
    {
        internal HashSet<string> Done=[];internal List<string> Calls=[];internal List<bool> SecretReceived=[];
        internal string? FailStep;internal bool FailAfter;
        public void RequireCurrent(HeadlessSetupState s){if(store.Current!=null&&store.Current.OperationId!=s.OperationId)throw new InvalidDataException("CHANGED");}
        public bool IsComplete(string step,HeadlessSetupState s)=>Done.Contains(step);
        public void Execute(string step,HeadlessSetupState s,ReadOnlyMemory<char> license)
        {
            if(store.Current?.Pending!=step)throw new Exception("mutation before durable intent");Calls.Add(step);SecretReceived.Add(!license.IsEmpty);
            if(FailStep==step&&!FailAfter)throw new Exception("password=unit-test-secret-license");Done.Add(step);
            if(FailStep==step)throw new Exception("password=unit-test-secret-license");
        }
    }
}
