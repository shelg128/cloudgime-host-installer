using System.Text.Json.Nodes;
using System.Security.Cryptography;
using CloudgimeHostBootstrap;

internal static class BundleTests
{
    internal static int Run()
    {
        var root = Path.Combine(Path.GetTempPath(), "cg-bundle-test-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(root); int passed = 0;
        try
        {
            foreach (var relative in UniversalBundleContract.Required)
            {
                var p = Path.Combine(root, relative); Directory.CreateDirectory(Path.GetDirectoryName(p)!);
                File.WriteAllText(p, "INERT_TEST_FIXTURE_NOT_EXECUTABLE");
            }
            File.WriteAllText(Path.Combine(root,"moonlight/server/config.json"),
                "{\"web_server\":{\"bind_address\":\"127.0.0.1:18999\",\"url_path_prefix\":\"/stream\",\"default_user_id\":null,\"first_login_create_admin\":true}}");
            File.WriteAllText(Path.Combine(root,"moonlight/server/data.json"), "{\"users\":{},\"hosts\":{}}");
            JsonObject Manifest()
            {
                var entries = new JsonArray();
                foreach(var r in UniversalBundleContract.Required)
                {
                    var bytes=File.ReadAllBytes(Path.Combine(root,r));
                    entries.Add(new JsonObject { ["path"]=r,["size"]=bytes.Length,["sha256"]=Convert.ToHexString(SHA256.HashData(bytes)) });
                }
                return new JsonObject { ["schemaVersion"]=1,["bundleMode"]="universal",["pcIdentityAssignedAfterInstall"]=true,["releaseVersion"]="fixture-v1",["fixtureOnly"]=true,["files"]=entries };
            }
            void Save(JsonObject manifest) => File.WriteAllText(Path.Combine(root,UniversalBundleContract.ManifestName),manifest.ToJsonString());
            void Fails(Action mutate, string code)
            {
                var m=Manifest();Save(m);mutate();
                try { UniversalBundleContract.Validate(root); }
                catch(Exception e) when(e.Message.Contains(code,StringComparison.Ordinal)) { passed++;return; }
                throw new Exception("EXPECTED_BUNDLE_ERROR:"+code);
            }
            Save(Manifest()); UniversalBundleContract.Validate(root);passed++;
            void RejectInstall(string code) {
                try { UniversalBundleContract.RequireInstallable(root); }
                catch(InvalidDataException e) when(e.Message.Contains(code,StringComparison.Ordinal)) { passed++; return; }
                throw new Exception("INSTALLABLE_FIXTURE_ACCEPTED");
            }
            RejectInstall("FIXTURE_OR_UNCLASSIFIED_PACKAGE_NOT_INSTALLABLE");
            var noMarker=Manifest();noMarker.Remove("fixtureOnly");Save(noMarker);
            RejectInstall("FIXTURE_OR_UNCLASSIFIED_PACKAGE_NOT_INSTALLABLE");
            var falseMarker=Manifest();falseMarker["fixtureOnly"]=false;Save(falseMarker);
            RejectInstall("NON_EXECUTABLE_PAYLOAD");
            Save(Manifest());
            var file=Path.Combine(root,"moonlight/web-server.exe");var original=File.ReadAllBytes(file);
            Fails(()=>File.WriteAllBytes(file, original.Select(b=>(byte)(b^1)).ToArray()),"PAYLOAD_FILE_HASH_MISMATCH");File.WriteAllBytes(file,original);
            Fails(()=>File.Delete(file),"PAYLOAD_FILE_SIZE_MISMATCH");File.WriteAllBytes(file,original);
            Fails(()=>{var m=Manifest();m["files"]!.AsArray().RemoveAt(0);Save(m);},"REQUIRED_PAYLOAD_MISSING");
            Fails(()=>{var m=Manifest();m["files"]!.AsArray().Add(m["files"]![0]!.DeepClone());Save(m);},"UNSAFE_OR_DUPLICATE_FILE_PATH");
            Fails(()=>{var m=Manifest();m["files"]![0]!["path"]="../escape.exe";Save(m);},"UNSAFE_OR_DUPLICATE_FILE_PATH");
            Fails(()=>{var m=Manifest();m["files"]![0]!["path"]="host-installer.exe:stream";Save(m);},"UNSAFE_OR_DUPLICATE_FILE_PATH");
            Fails(()=>{var m=Manifest();m["files"]![0]!["path"]="provisioning/machine-identity.dpapi";Save(m);},"MACHINE_STATE_IN_PAYLOAD");
            var extra=Path.Combine(root,"unlisted.exe");Fails(()=>File.WriteAllText(extra,"unlisted"),"UNLISTED_PAYLOAD_FILES");File.Delete(extra);
            var config=Path.Combine(root,"moonlight/server/config.json");var neutral=File.ReadAllText(config);
            Fails(()=>{File.WriteAllText(config,neutral.Replace("127.0.0.1:18999","0.0.0.0:18999"));Save(Manifest());},"PAYLOAD_BOOTSTRAP_MUST_BIND_LOOPBACK");File.WriteAllText(config,neutral);
            Fails(()=>{File.WriteAllText(config,neutral.Replace("\"default_user_id\":null","\"default_user_id\":42"));Save(Manifest());},"PAYLOAD_USER_MUST_BE_UNASSIGNED");File.WriteAllText(config,neutral);
            Fails(()=>{File.WriteAllText(config,neutral.Replace("\"web_server\":", "\"password\":\"fixture-only\",\"web_server\":"));Save(Manifest());},"INLINE_CREDENTIAL_IN_PAYLOAD");File.WriteAllText(config,neutral);
            Fails(()=>{var m=Manifest();File.WriteAllText(Path.Combine(root,UniversalBundleContract.ManifestName),m.ToJsonString().Replace("\"schemaVersion\":1","\"schemaVersion\":1,\"schemaVersion\":1"));},"DUPLICATE_JSON_KEY");
            foreach(var bad in new[]{"/absolute","C:/path","a\\b","a//b","a/../b","nul.txt","dir/COM1.log","dir./file","dir /file"})
            {if(UniversalBundleContract.IsSafeRelativePath(bad))throw new Exception("UNSAFE_PATH_ALLOWED:"+bad);passed++;}
            foreach(var bad in new[]{"sunshine/config/sunshine_state.json","moonlight/server/shared_pair_info.json",".env.production","folder/key.pem","keeper-tunnel/data/status.json"})
            {try{UniversalBundleContract.RejectMachineState(bad);}catch(InvalidDataException){passed++;continue;}throw new Exception("MACHINE_STATE_ALLOWED");}
            return passed;
        }
        finally{Directory.Delete(root,true);}
    }
}
