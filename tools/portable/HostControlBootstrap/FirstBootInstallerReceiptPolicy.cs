using System.Text.Json;
using System.Runtime.InteropServices;
namespace CloudgimeHostBootstrap;

internal static class FirstBootInstallerReceiptPolicy
{
    // The former receipt is not sufficient after a rollback. A matching durable
    // transaction must still be ARMED on the recorded boot, with no pending step.
    internal static bool Matches(string journalJson,string receiptJson,string handoffJson,string operation,string packageHash,string helperHash,string operatorSid,string root,string currentBoot)
    {
        try
        {
            using var journal=JsonDocument.Parse(journalJson,new JsonDocumentOptions{MaxDepth=6});
            using var receipt=JsonDocument.Parse(receiptJson,new JsonDocumentOptions{MaxDepth=4});
            using var handoff=JsonDocument.Parse(handoffJson,new JsonDocumentOptions{MaxDepth=6});
            if(!Unique(journal.RootElement)||!Unique(receipt.RootElement)||!Unique(handoff.RootElement))return false;
            var j=journal.RootElement;var r=receipt.RootElement;var b=j.GetProperty("Binding");var h=handoff.RootElement;var hb=h.GetProperty("Binding");
            return j.GetProperty("Version").GetInt32()==1&&j.GetProperty("Stage").GetString()=="ARMED"
                &&j.GetProperty("StartedBoot").GetString()==currentBoot
                &&j.GetProperty("Pending").ValueKind==JsonValueKind.Null
                &&b.GetProperty("OperationId").GetString()==operation&&b.GetProperty("PackageHash").GetString()==packageHash
                &&b.GetProperty("HostInstallerHash").GetString()==helperHash&&b.GetProperty("OperatorSid").GetString()==operatorSid
                &&string.Equals(b.GetProperty("ProvisioningRoot").GetString(),root,StringComparison.OrdinalIgnoreCase)
                &&h.GetProperty("Stage").GetString()=="ARMED"&&h.GetProperty("PreparedBoot").GetString()==j.GetProperty("StartedBoot").GetString()
                &&hb.GetProperty("OperationId").GetString()==operation&&hb.GetProperty("HostInstallerHash").GetString()==helperHash
                &&hb.GetProperty("OperatorSid").GetString()==operatorSid
                &&r.GetProperty("operationId").GetString()==operation&&r.GetProperty("packageHash").GetString()==packageHash
                &&r.GetProperty("hostInstallerHash").GetString()==helperHash&&r.GetProperty("operatorSid").GetString()==operatorSid
                &&string.Equals(r.GetProperty("provisioningRoot").GetString(),root,StringComparison.OrdinalIgnoreCase)
                &&r.GetProperty("stage").GetString()=="ARMED"
                &&r.GetProperty("preparedBoot").GetString()==j.GetProperty("StartedBoot").GetString()
                &&Guid.TryParseExact(j.GetProperty("StartedBoot").GetString(),"D",out var boot)&&boot!=Guid.Empty;
        }
        catch(Exception e)when(e is JsonException or KeyNotFoundException or InvalidOperationException or FormatException){return false;}
    }
    internal static string ReadCurrentBoot()
    {
        var buffer=Marshal.AllocHGlobal(64);
        try
        {
            if(NtQuerySystemInformation(90,buffer,64,out var length)!=0||length<16||length>64)throw new InvalidDataException("FIRST_BOOT_ID_UNAVAILABLE");
            var environment=Marshal.PtrToStructure<Guid>(buffer);if(environment==Guid.Empty)throw new InvalidDataException("FIRST_BOOT_ID_UNAVAILABLE");
            if(NtQuerySystemInformation(3,buffer,48,out length)!=0||length!=48)throw new InvalidDataException("FIRST_BOOT_ID_UNAVAILABLE");
            var boot=Marshal.ReadInt64(buffer);if(boot<=0)throw new InvalidDataException("FIRST_BOOT_ID_UNAVAILABLE");
            var bytes=System.Text.Encoding.UTF8.GetBytes("CloudGime kernel boot v2\0"+environment.ToString("D")+"\0"+boot.ToString(System.Globalization.CultureInfo.InvariantCulture));
            var digest=System.Security.Cryptography.SHA256.HashData(bytes);return new Guid(digest.AsSpan(0,16)).ToString("D");
        }
        finally{Marshal.FreeHGlobal(buffer);}
    }
    [DllImport("ntdll.dll")]private static extern int NtQuerySystemInformation(int informationClass,IntPtr buffer,uint length,out uint returned);
    private static bool Unique(JsonElement value)
    {
        if(value.ValueKind!=JsonValueKind.Object)return false;
        var names=new HashSet<string>(StringComparer.Ordinal);
        foreach(var field in value.EnumerateObject())
            if(!names.Add(field.Name)||field.Value.ValueKind==JsonValueKind.Object&&!Unique(field.Value))return false;
        return true;
    }
}
