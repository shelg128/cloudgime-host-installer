using System.Security.Cryptography;
using System.Text.Json.Nodes;
using CloudgimeHostBootstrap;

internal static class EncoderContractTests
{
 internal static int Run()
 {
  var root=Path.Combine(Path.GetTempPath(),"cg-encoder-bundle-"+Guid.NewGuid().ToString("N"));Directory.CreateDirectory(Path.Combine(root,"moonlight/server"));
  var helper=Path.Combine(root,EncoderBundleContract.HelperPath);var receipt=Path.Combine(root,EncoderBundleContract.ReceiptPath);File.WriteAllText(helper,"INERT_BINARY_FIXTURE");int passed=0;
  JsonObject Receipt()=>new(){["schema"]=1,["contract"]="cloudgime-encoder-stability-v1",["binaryBytes"]=new FileInfo(helper).Length,["binarySha256"]=Convert.ToHexString(SHA256.HashData(File.ReadAllBytes(helper))),["sources"]=new JsonObject{["Program.cs"]=new string('a',64),["NvencLogEvidence.cs"]=new string('b',64),["EncoderRuntimeContract.cs"]=new string('c',64)},["capabilities"]=new JsonObject{["schema"]=1,["contract"]="cloudgime-encoder-stability-v1",["nvencLogPolicyVersion"]=1,["boundedLogBytes"]=160000,["orderedH264Recovery"]=true,["modernAndLegacyTimestamps"]=true,["stalePolicyCacheReprobed"]=true,["freshProbeRequired"]=true,["unresolvedFailuresPreserved"]=true,["capabilityAwareSelection"]=true,["globalFixedEncoder"]=false,["globalFixedVideoProfile"]=false,["routeOrBillingChanges"]=false,["physicalQualityAccepted"]=false}};
  void Save(JsonObject m)=>File.WriteAllText(receipt,m.ToJsonString());
  void Reject(Action action,string code){try{action();}catch(Exception e)when(e.Message.Contains(code)){passed++;return;}throw new Exception("EXPECTED_"+code);}
  try{
   Reject(()=>EncoderBundleContract.Validate(root),"ENCODER_RELEASE_RECEIPT_REQUIRED");Save(Receipt());EncoderBundleContract.Validate(root);passed++;
   var m=Receipt();m["capabilities"]!["nvencLogPolicyVersion"]=0;Save(m);Reject(()=>EncoderBundleContract.Validate(root),"ENCODER_RELEASE_CAPABILITY_INVALID");
   foreach(var field in new[]{"orderedH264Recovery","modernAndLegacyTimestamps","stalePolicyCacheReprobed","freshProbeRequired","unresolvedFailuresPreserved","capabilityAwareSelection"}){m=Receipt();m["capabilities"]![field]=false;Save(m);Reject(()=>EncoderBundleContract.Validate(root),"ENCODER_RELEASE_CAPABILITY_INVALID");}
   foreach(var field in new[]{"globalFixedEncoder","globalFixedVideoProfile","routeOrBillingChanges","physicalQualityAccepted"}){m=Receipt();m["capabilities"]![field]=true;Save(m);Reject(()=>EncoderBundleContract.Validate(root),"ENCODER_RELEASE_CAPABILITY_INVALID");}
   m=Receipt();m["binarySha256"]=new string('d',64);Save(m);Reject(()=>EncoderBundleContract.Validate(root),"ENCODER_RELEASE_BINARY_MISMATCH");
   m=Receipt();m["binaryBytes"]=1;Save(m);Reject(()=>EncoderBundleContract.Validate(root),"ENCODER_RELEASE_BINARY_MISMATCH");
   m=Receipt();m["sources"]!.AsObject().Remove("NvencLogEvidence.cs");Save(m);Reject(()=>EncoderBundleContract.Validate(root),"ENCODER_RELEASE_SOURCES_MISSING");
   m=Receipt();Save(m);File.WriteAllText(receipt,m.ToJsonString().Replace("\"schema\":1","\"schema\":1,\"schema\":1"));Reject(()=>EncoderBundleContract.Validate(root),"DUPLICATE_JSON_KEY");
   return passed;
  }finally{Directory.Delete(root,true);}
 }
}
