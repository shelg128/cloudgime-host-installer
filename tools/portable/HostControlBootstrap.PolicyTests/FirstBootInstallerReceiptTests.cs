using System.Text.Json;
using System.Text.Json.Nodes;
using CloudgimeHostBootstrap;
internal static class FirstBootInstallerReceiptTests
{
    internal static void Run()
    {
        var op=Guid.NewGuid().ToString("D");var boot=Guid.NewGuid().ToString("D");const string root=@"C:\fixture\provisioning";
        var journal=JsonSerializer.SerializeToNode(new{Version=1,StartedBoot=boot,Stage="ARMED",Pending=(string?)null,
            Binding=new{OperationId=op,PackageHash=new string('a',64),HostInstallerHash=new string('b',64),OperatorSid="operator",ProvisioningRoot=root}})!;
        var receipt=JsonSerializer.SerializeToNode(new{operationId=op,packageHash=new string('a',64),hostInstallerHash=new string('b',64),operatorSid="operator",provisioningRoot=root,stage="ARMED",preparedBoot=boot})!;
        var handoff=JsonSerializer.SerializeToNode(new{Stage="ARMED",PreparedBoot=boot,Binding=new{OperationId=op,HostInstallerHash=new string('b',64),OperatorSid="operator",ProvisioningRoot=root}})!;
        bool Matches(JsonNode j,JsonNode r)=>FirstBootInstallerReceiptPolicy.Matches(j.ToJsonString(),r.ToJsonString(),handoff.ToJsonString(),op,new('a',64),new('b',64),"operator",root,boot);
        void Check(bool ok){if(!ok)throw new Exception("FIRST_BOOT_RECEIPT_TEST_FAILED");}
        Check(Matches(journal,receipt));int n=1;
        foreach(var stage in new[]{"STARTED","SHELL_PREPARED","ROLLING_BACK","ROLLED_BACK","CONFIRMED"}){var j=journal.DeepClone();j["Stage"]=stage;Check(!Matches(j,receipt));n++;}
        foreach(var field in new[]{"OperationId","PackageHash","HostInstallerHash","OperatorSid","ProvisioningRoot"}){var j=journal.DeepClone();j["Binding"]![field]="changed";Check(!Matches(j,receipt));n++;}
        var pending=journal.DeepClone();pending["Pending"]="ARM";Check(!Matches(pending,receipt));n++;
        var oldBoot=receipt.DeepClone();oldBoot["preparedBoot"]=Guid.NewGuid().ToString("D");Check(!Matches(journal,oldBoot));n++;
        var legacy=receipt.DeepClone();legacy.AsObject().Remove("preparedBoot");Check(!Matches(journal,legacy));n++;
        Check(!FirstBootInstallerReceiptPolicy.Matches("{}","{}",handoff.ToJsonString(),op,new('a',64),new('b',64),"operator",root,boot));n++;
        var duplicate=journal.ToJsonString().Insert(1,"\"Stage\":\"ARMED\",");
        Check(!FirstBootInstallerReceiptPolicy.Matches(duplicate,receipt.ToJsonString(),handoff.ToJsonString(),op,new('a',64),new('b',64),"operator",root,boot));n++;
        Check(!FirstBootInstallerReceiptPolicy.Matches(journal.ToJsonString(),receipt.ToJsonString(),handoff.ToJsonString(),op,new('a',64),new('b',64),"operator",root,Guid.NewGuid().ToString("D")));n++;
        Check(Guid.TryParse(FirstBootInstallerReceiptPolicy.ReadCurrentBoot(),out var actual)&&actual!=Guid.Empty);n++;
        handoff["Stage"]="ROLLED_BACK";Check(!Matches(journal,receipt));n++;
        var changedReceipt=receipt.DeepClone();changedReceipt["hostInstallerHash"]="changed";Check(!Matches(journal,changedReceipt));n++;
        changedReceipt=receipt.DeepClone();changedReceipt["packageHash"]="changed";Check(!Matches(journal,changedReceipt));n++;
        Console.WriteLine($"FIRST_BOOT_INSTALLER_RECEIPT_ASSERTIONS={n}");
    }
}
