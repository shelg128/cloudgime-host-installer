using System.Security.AccessControl;
using System.Security.Principal;
using System.Text.Json;
using System.Text.Json.Serialization;
namespace CloudgimeHostBootstrap;

internal sealed class HeadlessSetupJournal : IHeadlessSetupStore,IDisposable
{
    private readonly string _file;private readonly FileStream _lease;
    private static readonly JsonSerializerOptions Json=new(){UnmappedMemberHandling=JsonUnmappedMemberHandling.Disallow,MaxDepth=4};
    internal HeadlessSetupJournal(string operationRoot)
    {
        UniversalMachineBoundary.RequireProtected(operationRoot,true);_file=Path.Combine(operationRoot,"setup.json");
        var gate=Path.Combine(operationRoot,"setup.lock");UniversalBundleContract.RejectReparse(gate);
        _lease=new FileStream(gate,FileMode.OpenOrCreate,FileAccess.ReadWrite,FileShare.None);
    }
    private void RequireLease()
    {
        if(_lease.SafeFileHandle.IsClosed||_lease.SafeFileHandle.IsInvalid)throw new IOException("HEADLESS_SETUP_LEASE_CLOSED");
        UniversalMachineBoundary.RequireProtected(Path.GetDirectoryName(_file)!,true);
    }
    public HeadlessSetupState? Read()
    {
        RequireLease();
        UniversalBundleContract.RejectReparse(_file);
        try
        {
            using var input=new FileStream(_file,FileMode.Open,FileAccess.Read,FileShare.Read);
            UniversalMachineBoundary.RequireProtected(_file);
            if(input.Length is <=0 or >16384)throw new InvalidDataException("HEADLESS_SETUP_JOURNAL_INVALID");
            var data=new byte[checked((int)input.Length)];input.ReadExactly(data);
            using var doc=JsonDocument.Parse(data,new JsonDocumentOptions{MaxDepth=4});
            if(doc.RootElement.ValueKind!=JsonValueKind.Object)throw new InvalidDataException("HEADLESS_SETUP_JOURNAL_INVALID");
            var fields=new HashSet<string>(StringComparer.Ordinal);
            foreach(var item in doc.RootElement.EnumerateObject())if(!fields.Add(item.Name))throw new InvalidDataException("HEADLESS_SETUP_JOURNAL_INVALID");
            if(!fields.SetEquals(new[]{"Version","OperationId","PackageHash","OperatorSid","Stage","Pending","Error"}))throw new InvalidDataException("HEADLESS_SETUP_JOURNAL_INVALID");
            var s=doc.RootElement.Deserialize<HeadlessSetupState>(Json)??throw new InvalidDataException("HEADLESS_SETUP_JOURNAL_INVALID");
            HeadlessSetupCoordinator.Validate(s);return s;
        }
        catch(FileNotFoundException){return null;}
        catch(JsonException){throw new InvalidDataException("HEADLESS_SETUP_JOURNAL_INVALID");}
    }
    public void Save(HeadlessSetupState? expected,HeadlessSetupState next)
    {
        if(Read()!=expected)throw new IOException("HEADLESS_SETUP_JOURNAL_CHANGED");
        HeadlessSetupCoordinator.RequireTransition(expected,next);
        var temp=Path.Combine(Path.GetDirectoryName(_file)!,"setup-"+Guid.NewGuid().ToString("N")+".tmp");
        using(var output=new FileStream(temp,FileMode.CreateNew,FileAccess.Write,FileShare.None,4096,FileOptions.WriteThrough)){JsonSerializer.Serialize(output,next,Json);output.Flush(true);}
        File.Move(temp,_file,true);if(Read()!=next)throw new IOException("HEADLESS_SETUP_JOURNAL_READBACK_FAILED");
    }
    public void Dispose()=>_lease.Dispose();
}
