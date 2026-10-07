using System.Security.Cryptography;
using System.Text.Json.Nodes;
using CloudgimeHostBootstrap;

internal static class InstallableEncoderTests
{
 internal static int Run()
 {
  var root=Path.Combine(Path.GetTempPath(),"cg-installable-encoder-"+Guid.NewGuid().ToString("N"));Directory.CreateDirectory(root);int passed=0;
  var pe=new byte[512];pe[0]=0x4d;pe[1]=0x5a;BitConverter.GetBytes(128).CopyTo(pe,60);pe[128]=0x50;pe[129]=0x45;
  void Write(string p,string text){p=Path.Combine(root,p);Directory.CreateDirectory(Path.GetDirectoryName(p)!);File.WriteAllText(p,text);}
  void Seal(){var entries=new JsonArray();foreach(var p in UniversalBundleContract.Walk(root).Where(x=>x!=UniversalBundleContract.ManifestName).Order(StringComparer.Ordinal)){var b=File.ReadAllBytes(Path.Combine(root,p));entries.Add(new JsonObject{["path"]=p,["size"]=b.Length,["sha256"]=Convert.ToHexString(SHA256.HashData(b))});}Write(UniversalBundleContract.ManifestName,new JsonObject{["schemaVersion"]=1,["bundleMode"]="universal",["pcIdentityAssignedAfterInstall"]=true,["releaseVersion"]="test-only",["fixtureOnly"]=false,["files"]=entries}.ToJsonString());}
  void Reject(string code){try{UniversalBundleContract.RequireInstallable(root);}catch(Exception e)when(e.Message.Contains(code)){passed++;return;}throw new Exception("EXPECTED_INSTALLABLE_"+code);}
  try{
   foreach(var p in UniversalBundleContract.Required){Directory.CreateDirectory(Path.GetDirectoryName(Path.Combine(root,p))!);if(p.EndsWith(".exe"))File.WriteAllBytes(Path.Combine(root,p),pe);else Write(p,"fixture");}
   Write("moonlight/server/config.json","{\"webrtc\":{\"ice_servers\":[{\"urls\":[\"stun:test.invalid:3478\"]}]},\"web_server\":{\"bind_address\":\"127.0.0.1:18999\",\"url_path_prefix\":\"/stream\",\"default_user_id\":null,\"first_login_create_admin\":true}}");Write("moonlight/server/data.json","{\"users\":{},\"hosts\":{}}");
   File.WriteAllBytes(Path.Combine(root,EncoderBundleContract.HelperPath),pe);Seal();Reject("ENCODER_RELEASE_RECEIPT_REQUIRED");
   var c=new JsonObject{["schema"]=1,["contract"]="cloudgime-encoder-stability-v1",["nvencLogPolicyVersion"]=1,["boundedLogBytes"]=160000,["orderedH264Recovery"]=true,["modernAndLegacyTimestamps"]=true,["stalePolicyCacheReprobed"]=true,["freshProbeRequired"]=true,["unresolvedFailuresPreserved"]=true,["capabilityAwareSelection"]=true,["globalFixedEncoder"]=false,["globalFixedVideoProfile"]=false,["routeOrBillingChanges"]=false,["physicalQualityAccepted"]=false};
   var m=new JsonObject{["schema"]=1,["contract"]="cloudgime-encoder-stability-v1",["binaryBytes"]=pe.Length,["binarySha256"]=Convert.ToHexString(SHA256.HashData(pe)),["sources"]=new JsonObject{["Program.cs"]=new string('a',64),["NvencLogEvidence.cs"]=new string('b',64),["EncoderRuntimeContract.cs"]=new string('c',64)},["capabilities"]=c};
   Write(EncoderBundleContract.ReceiptPath,m.ToJsonString());Seal();UniversalBundleContract.RequireInstallable(root);passed++;
   // Even if someone regenerates the enclosing bundle manifest, helper identity must match its receipt.
   pe[511]=1;File.WriteAllBytes(Path.Combine(root,EncoderBundleContract.HelperPath),pe);Seal();Reject("ENCODER_RELEASE_BINARY_MISMATCH");pe[511]=0;File.WriteAllBytes(Path.Combine(root,EncoderBundleContract.HelperPath),pe);
   c["nvencLogPolicyVersion"]=0;Write(EncoderBundleContract.ReceiptPath,m.ToJsonString());Seal();Reject("ENCODER_RELEASE_CAPABILITY_INVALID");
   return passed;
  }finally{Directory.Delete(root,true);}
 }
}
