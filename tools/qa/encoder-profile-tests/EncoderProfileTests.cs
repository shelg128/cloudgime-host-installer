using System.Reflection;
using System.Text.Json;
using DisplayPrepareHelper;

internal static class EncoderProfileTests
{
 static readonly List<object> Results=[];
 static readonly Type Helper=typeof(DisplayPrepareHelper.Program);
 static string L(int s,string message)=>$"[2026-10-07 05:30:{s:00}.000]: {message}\n";
 const string Fail="Error: Failed to create encoder [h264_nvenc]";
 const string Success="Info: Found H.264 encoder: h264_nvenc [nvenc]";
 static void Check(bool ok,string reason="assertion"){if(!ok)throw new Exception(reason);}
 static void Test(string name,Action body){try{body();Results.Add(new{name,pass=true});}catch(Exception e){Results.Add(new{name,pass=false,error=e.ToString()});}}
 static bool Veto(string config,string lines){File.WriteAllText(Path.Combine(Path.GetDirectoryName(config)!,"sunshine.log"),lines);object?[] args=[config,null];return (bool)Helper.GetMethod("TryDetectRecentSunshineNvencFailure",BindingFlags.Static|BindingFlags.NonPublic)!.Invoke(null,args)!;}
 static string Select(string config,bool nvenc){object?[] args=[new List<GpuControllerInfo>{new(){Name="NVIDIA Test"}},new List<SunshineRuntimeCandidate>{new(){Key="default",RelativeDirectory="sunshine",ConfigPath=config,AutoSelect=true}},new List<EncoderProbeResult>{new(){RuntimeKey="default",EncoderKey="nvenc",Ok=nvenc,Available=true},new(){RuntimeKey="default",EncoderKey="software",Ok=true,Available=true}},false,null,null];Helper.GetMethod("SelectRuntimeAndEncoder",BindingFlags.Static|BindingFlags.NonPublic)!.Invoke(null,args);return (string)args[4]!;}
 static bool Cache(string config,int policy,string encoder,string? nvencFailure=null){var runtime=new SunshineRuntimeCandidate{Key="default",RelativeDirectory="sunshine",ConfigPath=config,AutoSelect=true,RuntimeFingerprint="identity",FfmpegSource="bundle-local"};var profile=new HostCapabilityProfile{SelectedRuntimeDirectory="sunshine",SelectedRuntimeFingerprint="identity",ConfigPath=config,SelectedEncoder=encoder,SelectedCapture="ddx",SelectedFfmpegSource="bundle-local",NvencLogPolicyVersion=policy,EncoderProbes=nvencFailure is null?[]:[new(){RuntimeKey="default",EncoderKey="nvenc",Ok=false,Detail=nvencFailure}],RuntimeCandidates=[runtime],GpuControllers=[],AudioEndpoints=[]};object?[] a=[profile,new List<SunshineRuntimeCandidate>{runtime},new List<GpuControllerInfo>(),new List<HostAudioEndpointInfo>(),false];return (bool)Helper.GetMethod("CanReuseCachedProfile",BindingFlags.Static|BindingFlags.NonPublic)!.Invoke(null,a)!;}
 static int Main(string[] args)
 {
  var dir=Path.Combine(Path.GetTempPath(),"cg-profile-regression-"+Guid.NewGuid().ToString("N"));Directory.CreateDirectory(dir);var config=Path.Combine(dir,"sunshine.conf");File.WriteAllText(config,"encoder = nvenc\n");
  Test("recovered initialization no longer downgrades actual selector",()=>{Check(!Veto(config,L(0,Fail)+L(1,Success)));Check(Select(config,true)=="nvenc");});
  Test("latest failure still downgrades actual selector",()=>{Check(Veto(config,L(0,Success)+L(1,Fail)));Check(Select(config,false)=="software");});
  Test("failed fresh probe cannot be rescued by log",()=>{Check(!Veto(config,L(0,Success)));Check(Select(config,false)=="software");});
  Test("UI-ready is not encoder recovery",()=>Check(Veto(config,L(0,Fail)+L(1,"Info: Configuration UI available at [https://localhost]"))));
  Test("HEVC success cannot clear H264 failure",()=>Check(Veto(config,L(0,Fail)+L(1,"Info: Found HEVC encoder: hevc_nvenc [nvenc]"))));
  Test("software success is not NVENC recovery",()=>Check(Veto(config,L(0,Fail)+L(1,"Info: Found H.264 encoder: libx264 [software]"))));
  Test("creating is not successful discovery",()=>Check(Veto(config,L(0,Fail)+L(1,"Info: Creating encoder [h264_nvenc]"))));
  Test("modern milliseconds parsed",()=>Check(NvencLogEvidence.TryTimestamp(L(0,Success),out _)));
  Test("old timestamps parsed",()=>Check(NvencLogEvidence.TryTimestamp("[2026:10:07:05:30:00]: Info: x",out _)));
  Test("invalid timestamps rejected",()=>Check(!NvencLogEvidence.TryTimestamp("[2026-99-99 05:30:00]: Info: x",out _)));
  Test("undated failure retained conservatively",()=>Check(Veto(config,Fail+"\n"+L(1,Success))));
  Test("undated success never clears",()=>Check(Veto(config,L(0,Fail)+"Info: Found H.264 encoder: h264_nvenc [nvenc]\n")));
  Test("timestamp rewind retains failure",()=>Check(Veto(config,L(5,Fail)+L(1,Success))));
  Test("unrelated optional AV1 failure does not poison H264",()=>Check(!Veto(config,L(0,"Error: NvEnc: encoding format is not supported by the gpu")+L(1,Success))));
  Test("encode timeout retained after discovery without restart",()=>Check(Veto(config,L(0,"Error: encode wait timeout")+L(1,Success))));
  Test("restart alone does not clear operational failure",()=>Check(Veto(config,L(0,"Error: NVENC returned empty packet")+L(1,"Info: Sunshine version: test"))));
  Test("restart then H264 discovery clears prior operational failure",()=>Check(!Veto(config,L(0,"Error: NVENC returned empty packet")+L(1,"Info: Sunshine version: test")+L(2,Success))));
  Test("error after restart recovery still blocks",()=>Check(Veto(config,L(0,"Error: encode wait timeout")+L(1,"Info: Sunshine version: test")+L(2,Success)+L(3,Fail))));
  Test("embedded success string not a certificate",()=>Check(Veto(config,L(0,Fail)+L(1,"Error: text says Found H.264 encoder: h264_nvenc [nvenc]"))));
  Test("old software cache re-probed once",()=>{Veto(config,L(0,Success));Check(!Cache(config,0,"software"));});
  Test("old nvenc cache re-probed once",()=>Check(!Cache(config,0,"nvenc")));
  Test("current recovered NVENC cache reusable",()=>{Veto(config,L(0,Fail)+L(1,Success));Check(Cache(config,NvencLogEvidence.PolicyVersion,"nvenc"));});
  Test("cache invalidated by newer unresolved failure",()=>{Veto(config,L(0,Success)+L(1,Fail));Check(!Cache(config,NvencLogEvidence.PolicyVersion,"nvenc"));});
  Test("future policy version rejected",()=>Check(!Cache(config,999,"nvenc")));
  Test("healthy known software cache not forcibly relabeled",()=>{Veto(config,L(0,Success));Check(Cache(config,NvencLogEvidence.PolicyVersion,"software"));});
  Test("software log-veto cache reprobes after actual recovery",()=>{Veto(config,L(0,Fail)+L(1,Success));Check(!Cache(config,1,"software","sunshine_runtime_nvenc_recent_failure prior"));});
  Test("unresolved software log-veto cache not retried repeatedly",()=>{Veto(config,L(0,Fail));Check(Cache(config,1,"software","sunshine_runtime_nvenc_recent_failure prior"));});
  Test("unrelated NVENC probe failure does not force repeat",()=>{Veto(config,L(0,Success));Check(Cache(config,1,"software","nvenc_api_mismatch"));});
  Test("bounded file reader ignores partial first line",()=>{var text=new string('x',200000)+"\n"+L(0,Fail)+L(1,Success);File.WriteAllText(Path.Combine(dir,"sunshine.log"),text);var tail=NvencLogEvidence.ReadTail(Path.Combine(dir,"sunshine.log"));Check(tail.Length<=NvencLogEvidence.MaxCharacters);Check(!tail.StartsWith("x"));Check(NvencLogEvidence.LatestUnrecoveredFailure(tail)==null);});
  Test("empty log is no veto, not an encoder pass",()=>{Check(!Veto(config,""));Check(Select(config,false)=="software");});
  Test("source selectors and original profile preserved",()=>Check(File.ReadAllText(config)=="encoder = nvenc\n"));
  var failed=Results.Count(x=>!(bool)x.GetType().GetProperty("pass")!.GetValue(x)!);
  var result=new{at=DateTimeOffset.UtcNow,tests=Results.Count,passed=Results.Count-failed,failed,results=Results,fixtureDirectory=dir};File.WriteAllText(args[0],JsonSerializer.Serialize(result,new JsonSerializerOptions{WriteIndented=true}));Console.WriteLine(JsonSerializer.Serialize(new{result.tests,result.passed,result.failed}));return failed==0?0:1;
 }
}
