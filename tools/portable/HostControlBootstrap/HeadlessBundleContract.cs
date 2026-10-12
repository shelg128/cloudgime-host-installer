using System.Text.Json.Nodes;
namespace CloudgimeHostBootstrap;

// Payload integrity, not publisher authenticity or installation acceptance.
internal static class HeadlessBundleContract
{
    internal const string ManifestName="headless-node-package.json";
    internal static readonly string[] AppFiles=["cloudgime-host-bootstrap.exe"];
    private static void Exact(JsonObject o,params string[] names)
    {
        if(!o.Select(x=>x.Key).ToHashSet(StringComparer.Ordinal).SetEquals(names))
            throw new InvalidDataException("HEADLESS_MANIFEST_FIELDS_INVALID");
    }
    internal static void Validate(string root)
    {
        root=Path.GetFullPath(root);UniversalBundleContract.RejectReparse(root);
        var manifestPath=Path.Combine(root,ManifestName);
        UniversalBundleContract.RejectReparse(manifestPath);
        if(!File.Exists(manifestPath)||new FileInfo(manifestPath).Length is <1 or >4*1024*1024)
            throw new InvalidDataException("HEADLESS_MANIFEST_SIZE_INVALID");
        var m=UniversalBundleContract.ParseObject(manifestPath);
        Exact(m,"schemaVersion","kind","internalUnsigned","automaticBootReady","releaseId","files");
        var release=(string?)m["releaseId"];
        if((int?)m["schemaVersion"]!=1||(string?)m["kind"]!="cloudgime-headless-node-candidate"
            ||(bool?)m["internalUnsigned"]!=true||(bool?)m["automaticBootReady"]!=false
            ||release is null||!System.Text.RegularExpressions.Regex.IsMatch(release,@"\A[a-zA-Z0-9_.-]{1,100}\z"))
            throw new InvalidDataException("HEADLESS_PACKAGE_INVALID");
        var files=m["files"]?.AsArray()??throw new InvalidDataException("HEADLESS_INVENTORY_MISSING");
        if(files.Count is <2 or >8192)throw new InvalidDataException("HEADLESS_ENTRY_COUNT_INVALID");
        var names=new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        foreach(var item in files)
        {
            if(item is not JsonObject row)throw new InvalidDataException("HEADLESS_ENTRY_INVALID");
            Exact(row,"path","bytes","sha256");var p=(string?)row["path"]??"";
            if(!UniversalBundleContract.IsSafeRelativePath(p)||!names.Add(p))throw new InvalidDataException("HEADLESS_PATH_INVALID");
            if(!p.StartsWith("app/",StringComparison.Ordinal)&&!p.StartsWith("bundle/",StringComparison.Ordinal))throw new InvalidDataException("HEADLESS_GROUP_INVALID");
            UniversalBundleContract.RejectMachineState(p);
            if(p.Contains("host-control",StringComparison.OrdinalIgnoreCase)||p.EndsWith(".lnk",StringComparison.OrdinalIgnoreCase)
                ||p.StartsWith("app/",StringComparison.Ordinal)&&p.EndsWith(".msi",StringComparison.OrdinalIgnoreCase))
                throw new InvalidDataException("HEADLESS_GUI_PAYLOAD_DENIED");
            if(p.StartsWith("app/",StringComparison.Ordinal)&&!AppFiles.Contains(p[4..],StringComparer.Ordinal))throw new InvalidDataException("HEADLESS_APP_NOT_ALLOWED");
            var expected=(string?)row["sha256"];var length=(long?)row["bytes"];
            if(expected is null||expected.Length!=64||!expected.All(c=>char.IsAsciiHexDigit(c)&&!char.IsUpper(c))||length is null or <=0)
                throw new InvalidDataException("HEADLESS_HASH_OR_SIZE_INVALID");
            var f=Path.Combine(root,p);UniversalBundleContract.RejectReparse(f);
            if(!File.Exists(f)||new FileInfo(f).Length!=length||ManagedUpgradeTransaction.Hash(f)!=expected)
                throw new InvalidDataException("HEADLESS_PAYLOAD_MISMATCH");
        }
        if(!UniversalBundleContract.Walk(root).Where(x=>x!=ManifestName).ToHashSet(StringComparer.OrdinalIgnoreCase).SetEquals(names))throw new InvalidDataException("HEADLESS_UNLISTED_FILES");
        if(!names.Contains("app/cloudgime-host-bootstrap.exe"))throw new InvalidDataException("HEADLESS_ENGINE_MISSING");
        UniversalBundleContract.RequireWindowsExecutable(Path.Combine(root,"app/cloudgime-host-bootstrap.exe"));
        // Driver MSI files remain allowed inside bundle; the existing neutral,
        // executable and capture/encoder checks still apply in full.
        UniversalBundleContract.RequireInstallable(Path.Combine(root,"bundle"));
    }
}
