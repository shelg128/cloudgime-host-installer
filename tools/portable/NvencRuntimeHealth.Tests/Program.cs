using DisplayPrepareHelper;
var tests=new (string,Action)[]{
 ("healthy NVENC startup is not runtime failure",()=>Eq(NvencRuntimeHealth.FailureLines("Info: Trying encoder [nvenc]\nInfo: NvEnc: created encoder H.264 P1\nInfo: Found H.264 encoder: h264_nvenc [nvenc]"),"")),
 ("unavailable AV1 does not invalidate successful H264",()=>Eq(NvencRuntimeHealth.FailureLines("Info: Creating encoder [av1_nvenc]\nError: NvEnc: encoding format is not supported by the gpu\nInfo: Found H.264 encoder: h264_nvenc [nvenc]"),"")),
 ("real encode timeout stays a failure",()=>Eq(NvencRuntimeHealth.FailureLines("startup\nError: encode wait timeout\nhealthy"),"Error: encode wait timeout")),
 ("real H264 open failure stays a failure",()=>Eq(NvencRuntimeHealth.FailureLines("Error: Could not open codec [h264_nvenc]"),"Error: Could not open codec [h264_nvenc]")),
 ("case-insensitive and multiple failures",()=>Eq(NvencRuntimeHealth.FailureLines(" NVENC RETURNED EMPTY PACKET\r\nFailed to create encoder D3D11 device "),"NVENC RETURNED EMPTY PACKET | Failed to create encoder D3D11 device")),
 ("unrelated log cannot become a failure",()=>Eq(NvencRuntimeHealth.FailureLines("Sunshine version: cg-capture1\nInfo: Capture size 720x1570"),"")),
 ("empty log has no failure evidence",()=>Eq(NvencRuntimeHealth.FailureLines(""),"")),
};
int failed=0;foreach(var(name,action)in tests){try{action();Console.WriteLine("PASS "+name);}catch(Exception e){failed++;Console.WriteLine("FAIL "+name+": "+e.Message);}}Console.WriteLine($"{tests.Length-failed}/{tests.Length} passed");Environment.ExitCode=failed==0?0:1;
static void Eq(string actual,string expected){if(actual!=expected)throw new Exception($"Expected [{expected}], received [{actual}]");}
