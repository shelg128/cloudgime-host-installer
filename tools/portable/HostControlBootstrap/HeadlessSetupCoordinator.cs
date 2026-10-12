using System.Security.Cryptography;
using System.Text.Json;

namespace CloudgimeHostBootstrap;

internal sealed record HeadlessSetupState(int Version,string OperationId,string PackageHash,string OperatorSid,
    string Stage,string? Pending,string? Error);
internal interface IHeadlessSetupStore
{
    HeadlessSetupState? Read();
    void Save(HeadlessSetupState? expected,HeadlessSetupState next);
}
internal interface IHeadlessSetupPlatform
{
    void RequireCurrent(HeadlessSetupState state);
    // Re-observe/retry the exact step after a lost response; never mint identity anew.
    bool IsComplete(string step,HeadlessSetupState state);
    void Execute(string step,HeadlessSetupState state,ReadOnlyMemory<char> activationLicense);
}

// Milestone 1 deliberately stops at disabled accounts. This is not a claim of
// armed autologon, first-boot confirmation or public rental readiness.
internal static class HeadlessSetupCoordinator
{
    internal static readonly string[] Steps=["PROVISION_NODE","PIN_OPERATOR","CREATE_DISABLED_ACCOUNTS"];
    private static readonly string[] Stages=["FILES_STAGED","NODE_PROVISIONED","OPERATOR_PINNED","ACCOUNTS_PREPARED"];
    internal static void Validate(HeadlessSetupState s)
    {
        if(s is null||s.Version!=1||!Guid.TryParseExact(s.OperationId,"D",out var id)||id==Guid.Empty
          ||s.PackageHash is null||s.PackageHash.Length!=64||!s.PackageHash.All(c=>char.IsAsciiHexDigit(c)&&!char.IsUpper(c))
          ||s.OperatorSid is null||!System.Text.RegularExpressions.Regex.IsMatch(s.OperatorSid,@"\AS-1-5-21-(\d{1,10}-){3}\d{1,10}\z")
          ||!Stages.Contains(s.Stage)||s.Pending!=null&&(!Steps.Contains(s.Pending)||Array.IndexOf(Steps,s.Pending)!=Array.IndexOf(Stages,s.Stage))
          ||s.Error!=null&&(s.Pending==null||!System.Text.RegularExpressions.Regex.IsMatch(s.Error,@"\A[A-Z0-9_]{1,100}\z")))
            throw new InvalidDataException("HEADLESS_SETUP_JOURNAL_INVALID");
    }
    internal static HeadlessSetupState Run(IHeadlessSetupStore store,IHeadlessSetupPlatform os,HeadlessSetupState binding,ReadOnlyMemory<char> license)
    {
        Validate(binding);if(binding.Stage!="FILES_STAGED"||binding.Pending!=null||binding.Error!=null)throw new InvalidDataException("HEADLESS_SETUP_INITIAL_STATE_INVALID");
        var s=store.Read();
        if(s==null){os.RequireCurrent(binding);store.Save(null,binding);s=binding;}
        Validate(s);RequireBinding(s,binding);os.RequireCurrent(s);
        var index=Array.IndexOf(Stages,s.Stage);
        // A READY-looking local journal alone never bypasses revalidation.
        for(var completed=0;completed<index;completed++)if(!os.IsComplete(Steps[completed],s))throw new InvalidDataException("HEADLESS_SETUP_COMPLETED_STEP_CHANGED");
        for(var i=index;i<Steps.Length;i++)
        {
            var step=Steps[i];os.RequireCurrent(s);
            var pending=s with{Pending=step,Error=null};store.Save(s,pending);s=pending;
            try
            {
                if(!os.IsComplete(step,s))os.Execute(step,s,step=="PROVISION_NODE"?license:ReadOnlyMemory<char>.Empty);
                os.RequireCurrent(s);if(!os.IsComplete(step,s))throw new InvalidDataException("HEADLESS_SETUP_STEP_UNVERIFIED");
                var next=s with{Stage=Stages[i+1],Pending=null,Error=null};store.Save(s,next);s=next;
            }
            catch(Exception error)
            {
                // Fixed diagnostic only; native errors can contain secrets/URLs.
                var code=error is InvalidDataException&&System.Text.RegularExpressions.Regex.IsMatch(error.Message,@"\A[A-Z0-9_]{1,100}\z")?error.Message:"HEADLESS_SETUP_STEP_FAILED";
                var failed=s with{Error=code};store.Save(s,failed);throw new InvalidDataException(code);
            }
        }
        return s;
    }
    internal static void RequireTransition(HeadlessSetupState? before,HeadlessSetupState after)
    {
        Validate(after);
        if(before==null)
        {
            if(after.Stage!="FILES_STAGED"||after.Pending!=null||after.Error!=null)
                throw new InvalidDataException("HEADLESS_SETUP_TRANSITION_INVALID");
            return;
        }
        Validate(before);RequireBinding(before,after);
        var i=Array.IndexOf(Stages,before.Stage);
        bool allowed=before==after
            ||i<Steps.Length&&after.Stage==before.Stage&&after.Pending==Steps[i]
                &&(before.Pending==null?after.Error==null:before.Pending==after.Pending)
            ||i<Steps.Length&&before.Pending==Steps[i]&&before.Error==null
                &&after.Stage==Stages[i+1]&&after.Pending==null&&after.Error==null;
        if(!allowed)throw new InvalidDataException("HEADLESS_SETUP_TRANSITION_INVALID");
    }
    internal static void RequireBinding(HeadlessSetupState a,HeadlessSetupState b)
    {
        if(a.OperationId!=b.OperationId||a.PackageHash!=b.PackageHash||a.OperatorSid!=b.OperatorSid)throw new InvalidDataException("HEADLESS_SETUP_BINDING_CHANGED");
    }
    internal static object PublicStatus(HeadlessSetupState s)
    {
        Validate(s);return new{operationId=s.OperationId,stage=s.Stage,pending=s.Pending,error=s.Error,
          accountsPrepared=s.Stage=="ACCOUNTS_PREPARED",autologonArmed=false,restartRequested=false,
          readyForRental=false,nextRequired="FIRST_BOOT_GATE_AND_AUTOMATIC_SETUP_INTEGRATION"};
    }
}
