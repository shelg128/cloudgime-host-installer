using System.Security.Cryptography;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;

namespace CloudgimeHostBootstrap;

// Integrity is not publisher authenticity. A release must ALSO be Authenticode-signed.
// Shared verbatim by packaging, bootstrap and the offline regression harness.
internal static class UniversalBundleContract
{
    internal const string ManifestName = "cloudgime-host-bundle.json";
    internal static readonly string[] Required =
    [
        "host-installer.exe", "moonlight/web-server.exe", "moonlight/streamer.exe",
        "moonlight/server/config.json", "moonlight/server/data.json",
        "moonlight/system/cloudgime-runtime-agent.exe", "moonlight/static/stream.js",
        "moonlight/static/stream/transport/webrtc.js", "sunshine/sunshine.exe",
        "sentinel/SentinelLauncherSetup.exe", "host-control/SentinelHelper.exe", "host-control/cloudgime-host-bootstrap.exe"
    ];

    internal static bool IsUniversal(string root)
    {
        var file = Path.Combine(root, ManifestName);
        if (!File.Exists(file)) return false;
        var m = ParseObject(file);
        return m["bundleMode"]?.GetValue<string>() == "universal";
    }

    internal static void Validate(string root)
    {
        root = Path.GetFullPath(root);
        RejectReparse(root);
        var manifest = ParseObject(Path.Combine(root, ManifestName));
        if ((int?)manifest["schemaVersion"] != 1 || (string?)manifest["bundleMode"] != "universal"
            || (bool?)manifest["pcIdentityAssignedAfterInstall"] != true)
            throw new InvalidDataException("UNIVERSAL_MANIFEST_INVALID");
        var version = (string?)manifest["releaseVersion"];
        if (string.IsNullOrWhiteSpace(version)) throw new InvalidDataException("RELEASE_VERSION_REQUIRED");
        var entries = manifest["files"]?.AsArray() ?? throw new InvalidDataException("FILE_MANIFEST_REQUIRED");
        var declared = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        foreach (var item in entries)
        {
            var relative = (string?)item?["path"] ?? throw new InvalidDataException("FILE_PATH_REQUIRED");
            if (!IsSafeRelativePath(relative) || !declared.Add(relative)) throw new InvalidDataException("UNSAFE_OR_DUPLICATE_FILE_PATH");
            RejectMachineState(relative);
            var p = Path.Combine(root, relative.Replace('/', Path.DirectorySeparatorChar));
            RejectReparse(p);
            var expectedHash = (string?)item?["sha256"];
            if (expectedHash is null || !Regex.IsMatch(expectedHash, "\\A[0-9a-fA-F]{64}\\z")) throw new InvalidDataException("FILE_HASH_REQUIRED");
            var length = (long?)item?["size"];
            if (length is null or <= 0 || !File.Exists(p) || new FileInfo(p).Length != length)
                throw new InvalidDataException("PAYLOAD_FILE_SIZE_MISMATCH: " + relative);
            using var f = File.OpenRead(p);
            if (!Convert.ToHexString(SHA256.HashData(f)).Equals(expectedHash, StringComparison.OrdinalIgnoreCase))
                throw new InvalidDataException("PAYLOAD_FILE_HASH_MISMATCH: " + relative);
        }
        foreach (var required in Required)
            if (!declared.Contains(required)) throw new InvalidDataException("REQUIRED_PAYLOAD_MISSING: " + required);
        var actual = Walk(root).Where(p => p != ManifestName).ToHashSet(StringComparer.OrdinalIgnoreCase);
        if (!actual.SetEquals(declared)) throw new InvalidDataException("UNLISTED_PAYLOAD_FILES");
        ValidateNeutralConfig(ParseObject(Path.Combine(root,"moonlight/server/config.json")),ParseObject(Path.Combine(root,"moonlight/server/data.json")));

    }

    internal static void RequireInstallable(string root)
    {
        Validate(root);
        var manifest = ParseObject(Path.Combine(root, ManifestName));
        if ((bool?)manifest["fixtureOnly"] != false)
            throw new InvalidDataException("FIXTURE_OR_UNCLASSIFIED_PACKAGE_NOT_INSTALLABLE");
        foreach (var relative in Required.Where(p => p.EndsWith(".exe", StringComparison.OrdinalIgnoreCase)))
            RequireWindowsExecutable(Path.Combine(root, relative));
        RequireWindowsExecutable(Path.Combine(root, EncoderBundleContract.HelperPath));
        EncoderBundleContract.Validate(root);
    }

    internal static void RequireWindowsExecutable(string path)
    {
        using var file = File.OpenRead(path);
        using var reader = new BinaryReader(file);
        if (file.Length < 256 || reader.ReadUInt16() != 0x5a4d)
            throw new InvalidDataException("NON_EXECUTABLE_PAYLOAD: " + Path.GetFileName(path));
        file.Position = 0x3c;
        var header = reader.ReadInt32();
        if (header < 64 || header > file.Length - 24)
            throw new InvalidDataException("INVALID_PE_HEADER: " + Path.GetFileName(path));
        file.Position = header;
        if (reader.ReadUInt32() != 0x00004550)
            throw new InvalidDataException("INVALID_PE_SIGNATURE: " + Path.GetFileName(path));
    }

    internal static bool IsSafeRelativePath(string path) => !string.IsNullOrEmpty(path)
        && !Path.IsPathRooted(path) && !path.Contains('\\') && !path.Contains(':')
        && path.Split('/').All(p => p.Length > 0 && p != "." && p != ".." && p.TrimEnd(' ', '.') == p
            && !Regex.IsMatch(p, @"\A(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(\.|$)", RegexOptions.IgnoreCase));

    internal static void RejectMachineState(string relative)
    {
        var p = relative.ToLowerInvariant(); var name = p.Split('/')[^1];
        if (p.StartsWith("provisioning/") || p.StartsWith("keeper-tunnel/data/")
            || p.Contains("/sunshine-shared/") || p.Contains("/credentials/") || p.Contains("\\credentials\\")
            || p.Contains("managed-shared") || p.Contains("managed-sunshine")
            || name.EndsWith(".dpapi") || name.EndsWith(".pem") || name.EndsWith(".key") || name.EndsWith(".pfx") || name.EndsWith(".p12")
            || name is "pc_identity.json" or "host_activation_state.json" or "shared_pair_info.json"
                or "sunshine_state.json" or "host_provisioning_secret.xml" or "public_url.txt"
                or "host_capability_profile.json" or "selected_sunshine_runtime.txt" or "stream_display_preferences.json"
            || name.StartsWith(".env") || name.EndsWith(".env"))
            throw new InvalidDataException("MACHINE_STATE_IN_PAYLOAD: " + relative);
    }

    internal static void ValidateNeutralConfig(JsonObject config, JsonObject data)
    {
        var web = config["web_server"]?.AsObject() ?? throw new InvalidDataException("WEB_CONFIG_REQUIRED");
        if (web["default_user_id"] is not null || (bool?)web["first_login_create_admin"] != true)
            throw new InvalidDataException("PAYLOAD_USER_MUST_BE_UNASSIGNED");
        var address = (string?)web["bind_address"];
        if (address is null || !Uri.TryCreate("http://" + address, UriKind.Absolute, out var local)
            || !local.IsLoopback || local.UserInfo.Length != 0 || local.AbsolutePath != "/" || local.Query.Length != 0)
            throw new InvalidDataException("PAYLOAD_BOOTSTRAP_MUST_BIND_LOOPBACK");
        var prefix = (string?)web["url_path_prefix"];
        if (prefix is null || !Regex.IsMatch(prefix, @"\A/[A-Za-z0-9_-]+\z"))
            throw new InvalidDataException("LOCAL_PREFIX_INVALID");
        foreach (var key in new[] { "users", "hosts" })
            if (data[key] is not JsonObject o || o.Count != 0) throw new InvalidDataException("PAYLOAD_PAIRING_STATE_NOT_EMPTY");
        var ice = config["webrtc"]?["ice_servers"]?.AsArray() ?? throw new InvalidDataException("ICE_SERVERS_REQUIRED");
        foreach (var server in ice)
        {
            if (server is not JsonObject item || item["urls"] is not JsonArray urls || urls.Count == 0)
                throw new InvalidDataException("ICE_SERVER_INVALID");
            if (item["username"] is not null || item["credential"] is not null)
                throw new InvalidDataException("TURN_CREDENTIAL_FORBIDDEN_DIRECT_ONLY");
            foreach (var url in urls.Select(v => v?.GetValue<string>() ?? string.Empty))
                if (!url.StartsWith("stun:", StringComparison.OrdinalIgnoreCase))
                    throw new InvalidDataException("NON_STUN_ICE_FORBIDDEN_DIRECT_ONLY");
        }
        RejectInlineCredentials(config);
        RejectInlineCredentials(data);
    }

    private static void RejectInlineCredentials(JsonNode? node)
    {
        if (node is JsonObject o)
            foreach (var (key, value) in o)
            {
                if (Regex.IsMatch(key, "password|credential|token|secret|private.?key", RegexOptions.IgnoreCase)
                    && value is JsonValue scalar && scalar.TryGetValue<string>(out var text) && !string.IsNullOrEmpty(text))
                    throw new InvalidDataException("INLINE_CREDENTIAL_IN_PAYLOAD");
                RejectInlineCredentials(value);
            }
        else if (node is JsonArray a) foreach (var v in a) RejectInlineCredentials(v);
    }

    internal static IEnumerable<string> Walk(string root, string prefix = "")
    {
        foreach (var p in Directory.EnumerateFileSystemEntries(Path.Combine(root, prefix)))
        {
            RejectReparse(p);
            var relative = prefix.Length == 0 ? Path.GetFileName(p) : prefix + "/" + Path.GetFileName(p);
            if (Directory.Exists(p)) foreach (var nested in Walk(root, relative)) yield return nested;
            else yield return relative;
        }
    }

    internal static void RejectReparse(string path)
    {
        for (var p = Path.GetFullPath(path); p is not null; p = Path.GetDirectoryName(p))
            if ((File.Exists(p) || Directory.Exists(p)) && (File.GetAttributes(p) & FileAttributes.ReparsePoint) != 0)
                throw new InvalidDataException("REPARSE_PAYLOAD_PATH");
    }

    internal static JsonObject ParseObject(string path)
    {
        RejectReparse(path);
        if (new FileInfo(path).Length > 16 * 1024 * 1024) throw new InvalidDataException("MANIFEST_TOO_LARGE");
        var bytes = File.ReadAllBytes(path);
        var span = bytes.AsSpan();
        if (span.Length >= 3 && span[0] == 0xEF && span[1] == 0xBB && span[2] == 0xBF)
            span = span[3..];
        using var document = JsonDocument.Parse(span.ToArray());
        RejectDuplicateKeys(document.RootElement);
        return JsonNode.Parse(document.RootElement.GetRawText())?.AsObject() ?? throw new InvalidDataException("JSON_OBJECT_REQUIRED");
    }
    private static void RejectDuplicateKeys(JsonElement value)
    {
        if (value.ValueKind == JsonValueKind.Object)
        {
            var names = new HashSet<string>(StringComparer.Ordinal);
            foreach (var p in value.EnumerateObject())
            { if (!names.Add(p.Name)) throw new InvalidDataException("DUPLICATE_JSON_KEY"); RejectDuplicateKeys(p.Value); }
        }
        else if (value.ValueKind == JsonValueKind.Array) foreach (var v in value.EnumerateArray()) RejectDuplicateKeys(v);
    }
}
