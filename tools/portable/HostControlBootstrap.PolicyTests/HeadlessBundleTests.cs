using System.Security.Cryptography;
using System.Text.Json.Nodes;
using CloudgimeHostBootstrap;

internal static class HeadlessBundleTests
{
    internal static int Run()
    {
        var root=Path.Combine(Path.GetTempPath(),"cg-headless-bundle-"+Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(root);var passed=0;
        string FileAt(string name)=>Path.Combine(root,name);
        void Write(string name,byte[] data){var file=FileAt(name);Directory.CreateDirectory(Path.GetDirectoryName(file)!);File.WriteAllBytes(file,data);}
        void Text(string name,string text)=>Write(name,System.Text.Encoding.UTF8.GetBytes(text));
        var pe=new byte[512];pe[0]=0x4d;pe[1]=0x5a;BitConverter.GetBytes(128).CopyTo(pe,60);pe[128]=0x50;pe[129]=0x45;
        string Hash(byte[] b)=>Convert.ToHexString(SHA256.HashData(b)).ToLowerInvariant();
        JsonObject Outer()
        {
            var files=new JsonArray();foreach(var p in UniversalBundleContract.Walk(root).Where(p=>p!=HeadlessBundleContract.ManifestName).Order(StringComparer.Ordinal))
            {var data=File.ReadAllBytes(FileAt(p));files.Add(new JsonObject{["path"]=p,["bytes"]=data.Length,["sha256"]=Hash(data)});}
            return new(){["schemaVersion"]=1,["kind"]="cloudgime-headless-node-candidate",["releaseId"]="fixture-only",["internalUnsigned"]=true,["automaticBootReady"]=false,["files"]=files};
        }
        void SealOuter()=>Text(HeadlessBundleContract.ManifestName,Outer().ToJsonString());
        void Reject(string code){try{HeadlessBundleContract.Validate(root);}catch(InvalidDataException e)when(e.Message.Contains(code,StringComparison.Ordinal)){passed++;return;}throw new Exception("HEADLESS_EXPECTED_"+code);}
        try
        {
            Write("app/cloudgime-host-bootstrap.exe",pe);
            foreach(var p in UniversalBundleContract.Required){if(p.EndsWith(".exe"))Write("bundle/"+p,pe);else Text("bundle/"+p,"inert fixture");}
            Text("bundle/moonlight/server/config.json","{\"web_server\":{\"bind_address\":\"127.0.0.1:18999\",\"url_path_prefix\":\"/stream\",\"default_user_id\":null,\"first_login_create_admin\":true},\"webrtc\":{\"ice_servers\":[{\"urls\":[\"stun:fixture.invalid:3478\"]}]}}");
            Text("bundle/moonlight/server/data.json","{\"users\":{},\"hosts\":{}}");Write("bundle/"+EncoderBundleContract.HelperPath,pe);
            Text("bundle/drivers/allowed-driver.msi","inert fixture, never executed");
            var capabilities=new JsonObject{["schema"]=1,["contract"]="cloudgime-encoder-stability-v1",["nvencLogPolicyVersion"]=1,["boundedLogBytes"]=160000,["orderedH264Recovery"]=true,["modernAndLegacyTimestamps"]=true,["stalePolicyCacheReprobed"]=true,["freshProbeRequired"]=true,["unresolvedFailuresPreserved"]=true,["capabilityAwareSelection"]=true,["globalFixedEncoder"]=false,["globalFixedVideoProfile"]=false,["routeOrBillingChanges"]=false,["physicalQualityAccepted"]=false};
            Text("bundle/"+EncoderBundleContract.ReceiptPath,new JsonObject{["schema"]=1,["contract"]="cloudgime-encoder-stability-v1",["binaryBytes"]=pe.Length,["binarySha256"]=Hash(pe),["sources"]=new JsonObject{["Program.cs"]=new string('a',64),["NvencLogEvidence.cs"]=new string('b',64),["EncoderRuntimeContract.cs"]=new string('c',64)},["capabilities"]=capabilities}.ToJsonString());
            var innerFiles=new JsonArray();foreach(var p in UniversalBundleContract.Walk(FileAt("bundle"))){var b=File.ReadAllBytes(FileAt("bundle/"+p));innerFiles.Add(new JsonObject{["path"]=p,["size"]=b.Length,["sha256"]=Hash(b)});}
            Text("bundle/"+UniversalBundleContract.ManifestName,new JsonObject{["schemaVersion"]=1,["bundleMode"]="universal",["pcIdentityAssignedAfterInstall"]=true,["releaseVersion"]="fixture-only",["fixtureOnly"]=false,["files"]=innerFiles}.ToJsonString());
            SealOuter();HeadlessBundleContract.Validate(root);passed++;
            foreach(var name in new[]{"app/cloudgime-host-control.exe","app/open-host-control.exe","app/start.lnk","app/ui.msi","app/arbitrary.exe","bundle/provisioning/machine-identity.dpapi","bundle/.env","bundle/token.key"})
            {Write(name,pe);SealOuter();Reject(name.Contains(".dpapi")||name.Contains(".env")||name.EndsWith(".key")?"MACHINE_STATE_IN_PAYLOAD":name=="app/arbitrary.exe"?"HEADLESS_APP_NOT_ALLOWED":"HEADLESS_GUI_PAYLOAD_DENIED");File.Delete(FileAt(name));}
            SealOuter();Text("bundle/unlisted.txt","not listed");Reject("HEADLESS_UNLISTED_FILES");File.Delete(FileAt("bundle/unlisted.txt"));
            var altered=(byte[])pe.Clone();altered[511]=1;Write("app/cloudgime-host-bootstrap.exe",altered);Reject("HEADLESS_PAYLOAD_MISMATCH");Write("app/cloudgime-host-bootstrap.exe",pe);
            foreach(var prop in new[]{"automaticBootReady","internalUnsigned"}){var m=Outer();m[prop]=prop=="automaticBootReady";Text(HeadlessBundleContract.ManifestName,m.ToJsonString());Reject("HEADLESS_PACKAGE_INVALID");}
            foreach(var bad in new[]{"../escape.exe","bundle/../../escape","C:/absolute.exe","bundle/x:secret","app//bad","bundle/NUL.exe"}){var m=Outer();m["files"]![0]!["path"]=bad;Text(HeadlessBundleContract.ManifestName,m.ToJsonString());Reject("HEADLESS_PATH_INVALID");}
            var duplicate=Outer();duplicate["files"]!.AsArray().Add(duplicate["files"]![0]!.DeepClone());Text(HeadlessBundleContract.ManifestName,duplicate.ToJsonString());Reject("HEADLESS_PATH_INVALID");
            var extra=Outer();extra["password"]="fixture";Text(HeadlessBundleContract.ManifestName,extra.ToJsonString());Reject("HEADLESS_MANIFEST_FIELDS_INVALID");
            var wrongKind=Outer();wrongKind["kind"]="cloudgime-headless-production";Text(HeadlessBundleContract.ManifestName,wrongKind.ToJsonString());Reject("HEADLESS_PACKAGE_INVALID");
            var size=Outer();size["files"]![0]!["bytes"]=0;Text(HeadlessBundleContract.ManifestName,size.ToJsonString());Reject("HEADLESS_HASH_OR_SIZE_INVALID");
            SealOuter();HeadlessBundleContract.Validate(root);passed++;
            Console.WriteLine($"HEADLESS_BUNDLE_ASSERTIONS={passed}; fixture bytes only, no installation");return passed;
        }
        finally{Directory.Delete(root,true);}
    }
}
