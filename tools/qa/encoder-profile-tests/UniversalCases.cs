using System.Text.Json;
using DisplayPrepareHelper;

internal static partial class EncoderProfileTests
{
 static string SelectHardware(string gpu, Dictionary<string,bool> healthy, bool force=false, bool allow=true)
 {
  object?[] args=[new List<GpuControllerInfo>{new(){Name=gpu}},new List<SunshineRuntimeCandidate>{new(){Key="default",RelativeDirectory="sunshine",AutoSelect=allow}},healthy.Select(x=>new EncoderProbeResult{RuntimeKey="default",EncoderKey=x.Key,Ok=x.Value,Available=true}).ToList(),force,null,null];
  Helper.GetMethod("SelectRuntimeAndEncoder",System.Reflection.BindingFlags.Static|System.Reflection.BindingFlags.NonPublic)!.Invoke(null,args);return (string)args[4]!;
 }
 static void UniversalCases()
 {
  Test("Intel-only retains proven QuickSync",()=>Check(SelectHardware("Intel UHD",new(){["nvenc"]=false,["quicksync"]=true,["software"]=true})=="quicksync"));
  Test("AMD-only retains proven AMF",()=>Check(SelectHardware("AMD Radeon",new(){["nvenc"]=false,["amdvce"]=true,["software"]=true})=="amdvce"));
  Test("unknown GPU never invents NVENC health",()=>Check(SelectHardware("unknown display",new(){["nvenc"]=false,["software"]=true})=="software"));
  Test("NVIDIA unavailable may use proven alternate hardware",()=>Check(SelectHardware("NVIDIA and Intel",new(){["nvenc"]=false,["quicksync"]=true,["software"]=true})=="quicksync"));
  Test("hardware failure remains valid software fallback",()=>Check(SelectHardware("NVIDIA",new(){["nvenc"]=false,["quicksync"]=false,["software"]=true})=="software"));
  Test("no successful probe never advertises healthy NVENC",()=>Check(SelectHardware("NVIDIA",new(){["nvenc"]=false,["software"]=false})=="auto"));
  Test("manual-only runtime not promoted by automatic selection",()=>Check(SelectHardware("NVIDIA",new(){["nvenc"]=true,["software"]=true},allow:false)=="auto"));
  Test("hardware default does not force RMX device geometry",()=>{var c=JsonDocument.Parse(EncoderRuntimeContract.Json()).RootElement;Check(c.GetProperty("globalFixedEncoder").GetBoolean()==false&&c.GetProperty("globalFixedVideoProfile").GetBoolean()==false);});
  Test("contract never claims physical quality acceptance",()=>{var c=JsonDocument.Parse(EncoderRuntimeContract.Json()).RootElement;Check(c.GetProperty("physicalQualityAccepted").GetBoolean()==false&&c.GetProperty("routeOrBillingChanges").GetBoolean()==false);});
  Test("contract version tracks cache policy version",()=>Check(JsonDocument.Parse(EncoderRuntimeContract.Json()).RootElement.GetProperty("nvencLogPolicyVersion").GetInt32()==NvencLogEvidence.PolicyVersion));
  Test("empty input does not alter software fallback",()=>Check(SelectHardware("generic",new(){["software"]=true})=="software"));
 }
}
