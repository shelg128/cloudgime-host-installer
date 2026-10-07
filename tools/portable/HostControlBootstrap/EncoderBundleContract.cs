using System.Security.Cryptography;
using System.Text.Json.Nodes;

namespace CloudgimeHostBootstrap;

// Integrity/feature check, not a substitute for the enclosing publisher signature.
internal static class EncoderBundleContract
{
    internal const string HelperPath="moonlight/server/display-prepare-helper.exe";
    internal const string ReceiptPath=HelperPath+".encoder-contract.json";
    internal static void Validate(string root)
    {
        var path=Path.Combine(root,ReceiptPath.Replace('/',Path.DirectorySeparatorChar));
        if(!File.Exists(path)||new FileInfo(path).Length>65536)throw new InvalidDataException("ENCODER_RELEASE_RECEIPT_REQUIRED");
        var m=UniversalBundleContract.ParseObject(path);var c=m["capabilities"]?.AsObject();
        bool True(string k)=>c?[k]?.GetValue<bool>()==true;
        bool False(string k)=>c?[k]?.GetValue<bool>()==false;
        if((int?)m["schema"]!=1||(string?)m["contract"]!="cloudgime-encoder-stability-v1"||c is null||
           (int?)c["schema"]!=1||(string?)c["contract"]!="cloudgime-encoder-stability-v1"||(int?)c["nvencLogPolicyVersion"]!=1||
           (int?)c["boundedLogBytes"]!=160000||!True("orderedH264Recovery")||!True("modernAndLegacyTimestamps")||
           !True("stalePolicyCacheReprobed")||!True("freshProbeRequired")||!True("unresolvedFailuresPreserved")||!True("capabilityAwareSelection")||
           !False("globalFixedEncoder")||!False("globalFixedVideoProfile")||!False("routeOrBillingChanges")||!False("physicalQualityAccepted"))
            throw new InvalidDataException("ENCODER_RELEASE_CAPABILITY_INVALID");
        var helper=Path.Combine(root,HelperPath.Replace('/',Path.DirectorySeparatorChar));UniversalBundleContract.RejectReparse(helper);
        if(!File.Exists(helper)||(long?)m["binaryBytes"]!=new FileInfo(helper).Length)throw new InvalidDataException("ENCODER_RELEASE_BINARY_MISMATCH");
        using var f=File.OpenRead(helper);var digest=Convert.ToHexString(SHA256.HashData(f));
        if(!digest.Equals((string?)m["binarySha256"],StringComparison.OrdinalIgnoreCase))throw new InvalidDataException("ENCODER_RELEASE_BINARY_MISMATCH");
        var sources=m["sources"]?.AsObject()??throw new InvalidDataException("ENCODER_RELEASE_SOURCES_MISSING");
        foreach(var name in new[]{"Program.cs","NvencLogEvidence.cs","EncoderRuntimeContract.cs"})
            if(sources[name]?.GetValue<string>() is not string hash||hash.Length!=64||!hash.All(Uri.IsHexDigit))throw new InvalidDataException("ENCODER_RELEASE_SOURCES_MISSING");
    }
}
