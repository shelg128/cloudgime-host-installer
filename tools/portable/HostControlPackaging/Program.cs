using System.Diagnostics;
using System.Net.Http.Headers;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;

namespace HostControlPackaging;

internal static class Program
{
    private static readonly UTF8Encoding Utf8NoBom = new(false);
    private const string ReleaseAllInOneRoot = @"C:\1\realease all in one";

    private static async Task<int> Main(string[] args)
    {
        try
        {
            var parsed = ParsedArguments.Parse(args);
            switch (parsed.Command)
            {
                case "publish-release":
                    PublishRelease(parsed);
                    break;
                case "prepare-nsis-payload":
                    PrepareNsisPayload(parsed);
                    break;
                case "build-nsis":
                    BuildNsis(parsed);
                    break;
                case "validate-end-to-end":
                    await ValidateEndToEndAsync(parsed);
                    break;
                default:
                    throw new InvalidOperationException($"Unknown command '{parsed.Command}'.");
            }

            return 0;
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine(ex.Message);
            return 1;
        }
    }

    private static void PublishRelease(ParsedArguments args)
    {
        var portableRoot = ResolvePortableRoot(args);
        var hostControlRoot = ResolveHostControlRoot(args, portableRoot);
        var repoRoot = ResolveRepoRoot(portableRoot);
        var installedRoot = ResolveInstalledHostRoot(args.GetValue("installed-root"));
        var releaseRoot = Path.Combine(hostControlRoot, "release");
        var bundleReleaseRoot = Path.Combine(releaseRoot, "bundle");
        var tauriTargetRoot = ResolveTauriTargetRoot(hostControlRoot);
        var msiRoot = Path.Combine(tauriTargetRoot, "release", "bundle", "msi");
        var tauriExePath = Path.Combine(tauriTargetRoot, "release", "hostcontrolapptauri.exe");
        var bundleRoot = NormalizeFullPath(args.GetValue("bundle-root") ?? Path.Combine(repoRoot, "export", "mon1"));
        BuildTauriApp(hostControlRoot);
        var bootstrapExe = PublishBootstrap(portableRoot);
        var emergencyUninstallerExe = PublishEmergencyUninstaller(portableRoot);
        var launcherExe = PublishLauncher(portableRoot);
        var keeperTunnelAgentExe = PublishKeeperTunnelAgent(args, repoRoot, installedRoot);
        var appInstallerPath = GetLatestMsiPath(msiRoot)
            ?? throw new InvalidOperationException($"Tauri MSI not found in {msiRoot}. Run 'npm run tauri build' first.");
        if (!File.Exists(tauriExePath))
        {
            throw new InvalidOperationException($"Tauri release executable not found: {tauriExePath}");
        }

        EnsureDirectoryDeleted(releaseRoot);
        Directory.CreateDirectory(releaseRoot);
        Directory.CreateDirectory(bundleReleaseRoot);

        CopyFileWithRetry(tauriExePath, Path.Combine(releaseRoot, "cloudgime-host-control.exe"));
        CopyFileWithRetry(launcherExe, Path.Combine(releaseRoot, "open-host-control.exe"));
        CopyFileWithRetry(bootstrapExe, Path.Combine(releaseRoot, "cloudgime-host-bootstrap.exe"));
        CopyFileWithRetry(bootstrapExe, Path.Combine(bundleReleaseRoot, "cloudgime-host-bootstrap.exe"));
        CopyFileWithRetry(emergencyUninstallerExe, Path.Combine(releaseRoot, "uninstaller-cloudgime.exe"));
        CopyFileWithRetry(appInstallerPath, Path.Combine(releaseRoot, "cloudgime-host-control.msi"));

        CopyManagedBundleSeed(portableRoot, releaseRoot);

        if (!Directory.Exists(bundleRoot))
        {
            throw new InvalidOperationException($"Bundle root not found: {bundleRoot}");
        }

        CopyDirectoryContents(bundleRoot, bundleReleaseRoot);
        OverlayLatestRuntimeStatic(repoRoot, bundleReleaseRoot);
        OverlayDriverSeed(repoRoot, bundleReleaseRoot);
        CopyFileWithRetry(keeperTunnelAgentExe, Path.Combine(bundleReleaseRoot, "keeper-tunnel", "KeeperTunnelAgent.exe"));
        StripLegacyFrpArtifacts(bundleReleaseRoot);
        RemoveStalePowerShellFiles(bundleReleaseRoot);
        WriteReleaseWrappers(releaseRoot, bundleReleaseRoot);
        WriteManagedBundleNotes(bundleReleaseRoot);

        Console.WriteLine($"Release prepared at {releaseRoot}");
        Console.WriteLine($"Executable: {Path.Combine(releaseRoot, "cloudgime-host-control.exe")}");
        Console.WriteLine($"Bootstrap:  {Path.Combine(releaseRoot, "cloudgime-host-bootstrap.exe")}");
        Console.WriteLine($"Uninstaller:{Path.Combine(releaseRoot, "uninstaller-cloudgime.exe")}");
        Console.WriteLine($"MSI:        {Path.Combine(releaseRoot, "cloudgime-host-control.msi")}");

        SyncHostArtifactsToAllInOne(releaseRoot, null);
    }

    private static void PrepareNsisPayload(ParsedArguments args)
    {
        var portableRoot = ResolvePortableRoot(args);
        var hostControlRoot = ResolveHostControlRoot(args, portableRoot);
        var repoRoot = ResolveRepoRoot(portableRoot);
        var installedRoot = ResolveInstalledHostRoot(args.GetValue("installed-root"));
        var bundleRoot = NormalizeFullPath(args.GetValue("bundle-root")
            ?? installedRoot
            ?? Path.Combine(repoRoot, "export", "mon1-template"));
        var payloadRoot = NormalizeFullPath(args.GetValue("payload-root") ?? Path.Combine(hostControlRoot, "installer", "payload"));
        var tauriTargetRoot = ResolveTauriTargetRoot(hostControlRoot);
        var msiRoot = Path.Combine(tauriTargetRoot, "release", "bundle", "msi");
        var appInstallerPath = GetLatestMsiPath(msiRoot);
        var releaseExecutablePath = Path.Combine(hostControlRoot, "release", "cloudgime-host-control.exe");
        var installedExecutablePath = installedRoot is not null
            ? Path.Combine(installedRoot, "cloudgime-host-control.exe")
            : string.Empty;
        var appExecutablePath = File.Exists(releaseExecutablePath)
            ? releaseExecutablePath
            : installedExecutablePath;
        var emergencyUninstallerPath = Path.Combine(hostControlRoot, "release", "uninstaller-cloudgime.exe");
        if (!File.Exists(appExecutablePath))
        {
            throw new InvalidOperationException(
                $"Cloudgime Host Control executable was not found in release or installed root. Checked: {releaseExecutablePath}; {installedExecutablePath}");
        }

        var bootstrapExe = PublishBootstrap(portableRoot);
        var launcherExe = PublishLauncher(portableRoot);
        var keeperTunnelAgentExe = PublishKeeperTunnelAgent(args, repoRoot, installedRoot);
        var installedLauncherPath = installedRoot is not null
            ? Path.Combine(installedRoot, "open-host-control.exe")
            : string.Empty;

        if (!Directory.Exists(bundleRoot))
        {
            throw new InvalidOperationException($"Bundle root not found: {bundleRoot}");
        }

        EnsureDirectoryDeleted(payloadRoot);
        var appPayload = Path.Combine(payloadRoot, "app");
        var bundlePayload = Path.Combine(payloadRoot, "bundle");
        Directory.CreateDirectory(appPayload);
        Directory.CreateDirectory(bundlePayload);

        CopyOptionalFile(appInstallerPath, Path.Combine(appPayload, "cloudgime-host-control.msi"));
        CopyFileWithRetry(appExecutablePath, Path.Combine(appPayload, "cloudgime-host-control.exe"));
        CopyFileWithRetry(File.Exists(installedLauncherPath) ? installedLauncherPath : launcherExe, Path.Combine(appPayload, "open-host-control.exe"));
        CopyFileWithRetry(bootstrapExe, Path.Combine(appPayload, "cloudgime-host-bootstrap.exe"));
        CopyFileWithRetry(emergencyUninstallerPath, Path.Combine(appPayload, "uninstaller-cloudgime.exe"));
        EnsureFilePresent(appExecutablePath, Path.Combine(appPayload, "cloudgime-host-control.exe"), "Host Control executable");
        EnsureFilePresent(File.Exists(installedLauncherPath) ? installedLauncherPath : launcherExe, Path.Combine(appPayload, "open-host-control.exe"), "Host Control launcher");
        EnsureFilePresent(bootstrapExe, Path.Combine(appPayload, "cloudgime-host-bootstrap.exe"), "Host bootstrap");
        EnsureFilePresent(emergencyUninstallerPath, Path.Combine(appPayload, "uninstaller-cloudgime.exe"), "Host emergency uninstaller");

        CopyManagedBundleSeed(portableRoot, appPayload);
        CopyDirectoryContents(bundleRoot, bundlePayload);
        OverlayLatestRuntimeStatic(repoRoot, bundlePayload);
        OverlayDriverSeed(repoRoot, bundlePayload);
        CopyFileWithRetry(bootstrapExe, Path.Combine(bundlePayload, "cloudgime-host-bootstrap.exe"));
        CopyFileWithRetry(keeperTunnelAgentExe, Path.Combine(bundlePayload, "keeper-tunnel", "KeeperTunnelAgent.exe"));
        EnsureFilePresent(bootstrapExe, Path.Combine(bundlePayload, "cloudgime-host-bootstrap.exe"), "Bundle bootstrap");
        EnsureFilePresent(keeperTunnelAgentExe, Path.Combine(bundlePayload, "keeper-tunnel", "KeeperTunnelAgent.exe"), "Host keeper tunnel agent");
        RemoveLegacyOpenHostControlScripts(bundlePayload);
        StripLegacyFrpArtifacts(bundlePayload);
        RemoveStalePowerShellFiles(bundlePayload);
        WriteManagedBundleNotes(bundlePayload);

        Console.WriteLine($"NSIS payload prepared at {payloadRoot}");
        Console.WriteLine($"App executable: {Path.Combine(appPayload, "cloudgime-host-control.exe")}");
        if (!string.IsNullOrWhiteSpace(appInstallerPath) && File.Exists(appInstallerPath))
        {
            Console.WriteLine($"App installer : {Path.Combine(appPayload, "cloudgime-host-control.msi")}");
        }
        Console.WriteLine($"Bundle payload: {bundlePayload}");
    }

    private static void BuildNsis(ParsedArguments args)
    {
        var portableRoot = ResolvePortableRoot(args);
        var hostControlRoot = ResolveHostControlRoot(args, portableRoot);
        var installerRoot = Path.Combine(hostControlRoot, "installer");
        var payloadRoot = NormalizeFullPath(args.GetValue("payload-root") ?? Path.Combine(installerRoot, "payload"));
        var outputDir = NormalizeFullPath(args.GetValue("output-dir") ?? Path.Combine(installerRoot, "output"));
        var nsisScript = NormalizeFullPath(args.GetValue("nsis-script") ?? Path.Combine(installerRoot, "CloudgimeHostControl.nsi"));

        if (!File.Exists(nsisScript))
        {
            throw new InvalidOperationException($"NSIS script not found: {nsisScript}");
        }

        var payloadApp = Path.Combine(payloadRoot, "app");
        var payloadBundle = Path.Combine(payloadRoot, "bundle");
        if (!Directory.Exists(payloadApp) || !Directory.Exists(payloadBundle))
        {
            throw new InvalidOperationException($"NSIS payload not found in {payloadRoot}. Run prepare-nsis-payload first.");
        }

        Directory.CreateDirectory(outputDir);
        var makensisPath = ResolveMakensisPath();
        var outputDefine = ConvertToNsisDefinePath(installerRoot, outputDir);
        var appDefine = ConvertToNsisDefinePath(installerRoot, payloadApp);
        var bundleDefine = ConvertToNsisDefinePath(installerRoot, payloadBundle);

        RunChecked(
            makensisPath,
            [
                $"/DOUTPUT_DIR={outputDefine}",
                $"/DPAYLOAD_APP={appDefine}",
                $"/DPAYLOAD_BUNDLE={bundleDefine}",
                nsisScript
            ],
            installerRoot,
            "Build NSIS installer");

        Console.WriteLine($"NSIS installer built in {outputDir}");
        SyncHostArtifactsToAllInOne(Path.Combine(hostControlRoot, "release"), Path.Combine(outputDir, "CloudgimeHostSetup.exe"));
    }

    private static async Task ValidateEndToEndAsync(ParsedArguments args)
    {
        var portableRoot = ResolvePortableRoot(args);
        var repoRoot = ResolveRepoRoot(portableRoot);
        var bundleRoot = NormalizeFullPath(args.GetValue("bundle-root") ?? Path.Combine(repoRoot, "export", "mon1"));
        var apiBase = (args.GetValue("api-base") ?? "https://api.cloudgime.my.id/api/v1").TrimEnd('/');
        var backendRoot = NormalizeFullPath(args.GetValue("backend-root") ?? @"C:\cloudrental\cloudrental-backend");
        var hostInstaller = Path.Combine(bundleRoot, "host-installer.exe");
        var statePath = Path.Combine(bundleRoot, "moonlight", "server", "host_activation_state.json");

        if (!File.Exists(statePath))
        {
            throw new InvalidOperationException($"Activation state file not found: {statePath}");
        }

        if (!File.Exists(hostInstaller))
        {
            throw new InvalidOperationException($"host-installer.exe not found: {hostInstaller}");
        }

        var validationLogPath = Path.Combine(bundleRoot, "validate-end-to-end.log");
        void LogStep(string message)
        {
            var line = $"[{DateTimeOffset.UtcNow:O}] {message}";
            Console.WriteLine(line);
            File.AppendAllText(validationLogPath, line + Environment.NewLine, Utf8NoBom);
        }

        File.WriteAllText(validationLogPath, string.Empty, Utf8NoBom);
        LogStep("validate-end-to-end begin");

        var state = JsonNode.Parse(await File.ReadAllTextAsync(statePath, Utf8NoBom))
            ?.AsObject() ?? throw new InvalidOperationException("Could not parse activation state.");
        var hostId = GetRequiredString(state, "HostId");
        var displayName = GetString(state, "DisplayName", Environment.MachineName);
        var controlPlaneUrl = GetString(state, "ControlPlaneUrl", "https://cloudgime.my.id");
        var machineIdentity = GetString(state, "MachineIdentity");
        var installInstanceId = GetString(state, "InstallInstanceId");
        var sentinelPcId = GetString(state, "SentinelPcId");
        var sentinelDeviceId = GetString(state, "SentinelDeviceId");
        var keeperEntryId = GetString(state, "KeeperEntryId");

        var adminToken = args.GetValue("admin-token");
        if (string.IsNullOrWhiteSpace(adminToken))
        {
            adminToken = GetMasterAdminToken(backendRoot);
        }

        using var adminClient = new HttpClient();
        adminClient.DefaultRequestHeaders.Authorization = new AuthenticationHeaderValue("Bearer", adminToken);

        LogStep("delete existing activation record");
        await TryDeleteActivationRecordAsync(adminClient, apiBase, hostId);
        LogStep("stop bundle");
        TryInvokeInstaller(hostInstaller, bundleRoot, "stop-bundle");

        var resetState = new JsonObject
        {
            ["SchemaVersion"] = GetInt(state, "SchemaVersion", 1),
            ["HostId"] = hostId,
            ["MachineIdentity"] = machineIdentity,
            ["InstallInstanceId"] = installInstanceId,
            ["ActivationState"] = "locked_waiting_token",
            ["ControlPlaneUrl"] = controlPlaneUrl,
            ["DisplayName"] = displayName,
            ["SentinelPcId"] = sentinelPcId,
            ["SentinelDeviceId"] = sentinelDeviceId,
            ["KeeperEntryId"] = keeperEntryId,
            ["RuntimeToken"] = string.Empty,
            ["ActivatedAtUtc"] = string.Empty,
            ["RedeemedAtUtc"] = string.Empty,
            ["ActivationRecordId"] = string.Empty,
            ["LastHeartbeatAtUtc"] = string.Empty,
            ["LastReadyForStream"] = false,
            ["UpdatedAtUtc"] = DateTimeOffset.UtcNow.ToString("O")
        };
        LogStep("write reset activation state");
        await WriteJsonFileAsync(statePath, resetState);

        LogStep("prepare host");
        InvokeInstaller(hostInstaller, bundleRoot, "prepare-host");

        LogStep("issue activation token");
        var issue = await SendJsonAsync(
            adminClient,
            HttpMethod.Post,
            $"{apiBase}/admin/host-activation/issue",
            new JsonObject
            {
                ["hostId"] = hostId,
                ["displayName"] = displayName,
                ["sentinelPcId"] = sentinelPcId,
                ["sentinelDeviceId"] = sentinelDeviceId,
                ["keeperEntryId"] = keeperEntryId,
                ["replaceExisting"] = true
            });
        var activationToken = GetRequiredString(issue, "activationToken");

        using var publicClient = new HttpClient();
        LogStep("redeem activation token");
        var redeem = await SendJsonAsync(
            publicClient,
            HttpMethod.Post,
            $"{apiBase}/host-activation/redeem",
            new JsonObject
            {
                ["hostId"] = hostId,
                ["machineIdentity"] = machineIdentity,
                ["installInstanceId"] = installInstanceId,
                ["displayName"] = displayName,
                ["activationToken"] = activationToken
            });
        var redeemRuntimeToken = GetRequiredString(redeem, "runtimeToken");
        var redeemActivationRecordId = GetRequiredString(redeem, "activationRecordId");

        var redeemedState = new JsonObject
        {
            ["SchemaVersion"] = GetInt(state, "SchemaVersion", 1),
            ["HostId"] = GetRequiredString(redeem, "hostId"),
            ["MachineIdentity"] = machineIdentity,
            ["InstallInstanceId"] = installInstanceId,
            ["ActivationState"] = GetString(redeem, "activationState", "activated"),
            ["ControlPlaneUrl"] = GetString(redeem, "controlPlaneUrl", controlPlaneUrl),
            ["DisplayName"] = GetString(redeem, "displayName", displayName),
            ["SentinelPcId"] = sentinelPcId,
            ["SentinelDeviceId"] = sentinelDeviceId,
            ["KeeperEntryId"] = keeperEntryId,
            ["RuntimeToken"] = redeemRuntimeToken,
            ["ActivatedAtUtc"] = GetString(redeem, "activatedAtUtc"),
            ["RedeemedAtUtc"] = GetString(redeem, "redeemedAtUtc"),
            ["ActivationRecordId"] = redeemActivationRecordId,
            ["LastHeartbeatAtUtc"] = string.Empty,
            ["LastReadyForStream"] = false,
            ["UpdatedAtUtc"] = DateTimeOffset.UtcNow.ToString("O")
        };
        LogStep("write redeemed activation state");
        await WriteJsonFileAsync(statePath, redeemedState);

        LogStep("start bundle");
        InvokeInstaller(hostInstaller, bundleRoot, "start-bundle");

        JsonObject? readyStatus = null;
        for (var attempt = 0; attempt < 20; attempt++)
        {
            LogStep($"poll status attempt {attempt + 1}");
            await Task.Delay(TimeSpan.FromSeconds(2));
            var status = InvokeInstallerJson(hostInstaller, bundleRoot, "status");
            var publicUrlCandidate = GetBundlePublicUrl(bundleRoot);
            if (GetString(status, "lifecycle_phase") == "ready" &&
                GetBool(status, "local_http_ready") &&
                GetBool(status, "required_processes_ready") &&
                !string.IsNullOrWhiteSpace(publicUrlCandidate))
            {
                readyStatus = status;
                break;
            }
        }

        readyStatus ??= InvokeInstallerJson(hostInstaller, bundleRoot, "status");
        var publicUrl = GetBundlePublicUrl(bundleRoot);
        var serviceRaw = InvokeInstallerPlain(hostInstaller, bundleRoot, "service-status");
        var serviceState = serviceRaw.Contains("RUNNING", StringComparison.OrdinalIgnoreCase)
            ? "running"
            : serviceRaw.Contains("STOPPED", StringComparison.OrdinalIgnoreCase)
                ? "stopped"
                : serviceRaw.Contains("NOT INSTALLED", StringComparison.OrdinalIgnoreCase)
                    ? "not installed"
                    : "unknown";
        var readyForStream =
            GetBool(readyStatus, "local_http_ready") &&
            GetBool(readyStatus, "required_processes_ready") &&
            string.Equals(GetString(readyStatus, "lifecycle_phase"), "ready", StringComparison.OrdinalIgnoreCase) &&
            !string.IsNullOrWhiteSpace(publicUrl);

        LogStep("send heartbeat");
        var heartbeat = await SendJsonAsync(
            publicClient,
            HttpMethod.Post,
            $"{apiBase}/host-activation/heartbeat",
            new JsonObject
            {
                ["hostId"] = hostId,
                ["machineIdentity"] = machineIdentity,
                ["installInstanceId"] = installInstanceId,
                ["runtimeToken"] = redeemRuntimeToken,
                ["activationRecordId"] = redeemActivationRecordId,
                ["displayName"] = displayName,
                ["lifecyclePhase"] = GetString(readyStatus, "lifecycle_phase"),
                ["healthGrade"] = GetString(readyStatus, "health_grade"),
                ["runtimeDisplayName"] = GetString(readyStatus, "selected_runtime_display_name"),
                ["publicUrl"] = publicUrl,
                ["serviceState"] = serviceState,
                ["localHttpReady"] = GetBool(readyStatus, "local_http_ready"),
                ["requiredProcessesReady"] = GetBool(readyStatus, "required_processes_ready"),
                ["readyForStream"] = readyForStream,
                ["note"] = readyForStream ? "ready_for_stream" : null
            });

        redeemedState["LastHeartbeatAtUtc"] = GetString(heartbeat, "lastHeartbeatAtUtc");
        redeemedState["LastReadyForStream"] = GetBool(heartbeat, "readyForStream");
        redeemedState["UpdatedAtUtc"] = DateTimeOffset.UtcNow.ToString("O");
        LogStep("write final activation state");
        await WriteJsonFileAsync(statePath, redeemedState);

        LogStep("fetch admin status");
        var adminStatus = await GetJsonAsync(adminClient, $"{apiBase}/admin/host-activation/{hostId}");
        var result = new JsonObject
        {
            ["hostId"] = hostId,
            ["issued"] = GetBool(issue, "ok", true),
            ["redeemed"] = GetBool(redeem, "ok", true),
            ["activationState"] = GetString(adminStatus, "activationState"),
            ["lifecyclePhase"] = GetString(readyStatus, "lifecycle_phase"),
            ["localHttpReady"] = GetBool(readyStatus, "local_http_ready"),
            ["requiredProcessesReady"] = GetBool(readyStatus, "required_processes_ready"),
            ["publicUrl"] = publicUrl,
            ["heartbeatReady"] = GetBool(heartbeat, "readyForStream"),
            ["adminLastReadyForStream"] = GetBool(adminStatus, "lastReadyForStream"),
            ["adminLastHeartbeatAtUtc"] = GetString(adminStatus, "lastHeartbeatAtUtc"),
            ["runtimeDisplayName"] = GetString(readyStatus, "selected_runtime_display_name"),
        };

        LogStep("validate-end-to-end complete");
        Console.WriteLine(result.ToJsonString(JsonOptions()));
    }

    private static async Task TryDeleteActivationRecordAsync(HttpClient httpClient, string apiBase, string hostId)
    {
        try
        {
            using var response = await httpClient.DeleteAsync($"{apiBase}/admin/host-activation/{hostId}");
            _ = await response.Content.ReadAsStringAsync();
        }
        catch
        {
        }
    }

    private static void TryInvokeInstaller(string installerPath, string bundleRoot, string commandName)
    {
        try
        {
            InvokeInstaller(installerPath, bundleRoot, commandName);
        }
        catch
        {
        }
    }

    private static void InvokeInstaller(string installerPath, string bundleRoot, string commandName)
    {
        RunCheckedNoCapture(
            installerPath,
            ["--bundle-root", bundleRoot, commandName],
            bundleRoot,
            $"Run host-installer {commandName}");
    }

    private static string InvokeInstallerPlain(string installerPath, string bundleRoot, string commandName)
    {
        return RunCheckedCapture(
            installerPath,
            ["--bundle-root", bundleRoot, commandName],
            bundleRoot,
            $"Run host-installer {commandName}").StdOut.Trim();
    }

    private static JsonObject InvokeInstallerJson(string installerPath, string bundleRoot, string commandName)
    {
        var result = RunCheckedCapture(
            installerPath,
            ["--bundle-root", bundleRoot, commandName],
            bundleRoot,
            $"Run host-installer {commandName}");
        return ParseJsonObject(result.StdOut, $"host-installer {commandName} output");
    }

    private static async Task<JsonObject> SendJsonAsync(HttpClient client, HttpMethod method, string url, JsonObject body)
    {
        using var request = new HttpRequestMessage(method, url)
        {
            Content = new StringContent(body.ToJsonString(JsonOptions()), Utf8NoBom, "application/json")
        };
        using var response = await client.SendAsync(request);
        var raw = await response.Content.ReadAsStringAsync();
        if (!response.IsSuccessStatusCode)
        {
            throw new InvalidOperationException($"HTTP {(int)response.StatusCode} from {url}: {raw}");
        }

        return ParseJsonObject(raw, url);
    }

    private static async Task<JsonObject> GetJsonAsync(HttpClient client, string url)
    {
        using var response = await client.GetAsync(url);
        var raw = await response.Content.ReadAsStringAsync();
        if (!response.IsSuccessStatusCode)
        {
            throw new InvalidOperationException($"HTTP {(int)response.StatusCode} from {url}: {raw}");
        }

        return ParseJsonObject(raw, url);
    }

    private static JsonObject ParseJsonObject(string raw, string context)
    {
        var node = JsonNode.Parse(raw)?.AsObject();
        return node ?? throw new InvalidOperationException($"Could not parse JSON from {context}.");
    }

    private static async Task WriteJsonFileAsync(string path, JsonObject value)
    {
        Directory.CreateDirectory(Path.GetDirectoryName(path)!);
        await File.WriteAllTextAsync(path, value.ToJsonString(JsonOptions()) + Environment.NewLine, Utf8NoBom);
    }

    private static string GetBundlePublicUrl(string bundleRoot)
    {
        var publicUrlPath = Path.Combine(bundleRoot, "PUBLIC_URL.txt");
        var configPath = Path.Combine(bundleRoot, "moonlight", "server", "config.json");
        var rawPublicUrl = File.Exists(publicUrlPath)
            ? File.ReadAllText(publicUrlPath, Utf8NoBom).Trim()
            : string.Empty;

        if (!File.Exists(configPath))
        {
            return rawPublicUrl;
        }

        try
        {
            var config = JsonNode.Parse(File.ReadAllText(configPath, Utf8NoBom))?.AsObject();
            var pathPrefix = GetNormalizedPathPrefix(config?["web_server"]?["url_path_prefix"]?.GetValue<string>());
            if (string.IsNullOrWhiteSpace(pathPrefix) || string.IsNullOrWhiteSpace(rawPublicUrl))
            {
                return rawPublicUrl;
            }

            var uri = new Uri(rawPublicUrl);
            return uri.GetLeftPart(UriPartial.Authority) + pathPrefix + "/";
        }
        catch
        {
            return rawPublicUrl;
        }
    }

    private static string GetNormalizedPathPrefix(string? value)
    {
        var trimmed = value?.Trim() ?? string.Empty;
        if (string.IsNullOrWhiteSpace(trimmed))
        {
            return string.Empty;
        }

        var withoutTrailing = trimmed.TrimEnd('/');
        return withoutTrailing.StartsWith('/') ? withoutTrailing : $"/{withoutTrailing}";
    }

    private static string GetMasterAdminToken(string backendRoot)
    {
        var envPath = Path.Combine(backendRoot, ".env");
        var jwtSecret = GetEnvValue(envPath, "JWT_SECRET");
        if (string.IsNullOrWhiteSpace(jwtSecret))
        {
            throw new InvalidOperationException($"JWT_SECRET was not found in {envPath}");
        }

        var masterAdminId = GetEnvValue(envPath, "MASTER_ADMIN_ID");
        if (string.IsNullOrWhiteSpace(masterAdminId))
        {
            masterAdminId = "master_main";
        }

        return CreateJwtToken(masterAdminId, jwtSecret, TimeSpan.FromDays(30));
    }

    private static string CreateJwtToken(string adminId, string jwtSecret, TimeSpan validFor)
    {
        static string Base64UrlEncode(byte[] bytes) =>
            Convert.ToBase64String(bytes)
                .TrimEnd('=')
                .Replace('+', '-')
                .Replace('/', '_');

        var now = DateTimeOffset.UtcNow;
        var header = JsonSerializer.SerializeToUtf8Bytes(new JsonObject
        {
            ["alg"] = "HS256",
            ["typ"] = "JWT"
        });
        var payload = JsonSerializer.SerializeToUtf8Bytes(new JsonObject
        {
            ["sub"] = adminId,
            ["role"] = "MASTER",
            ["iat"] = now.ToUnixTimeSeconds(),
            ["exp"] = now.Add(validFor).ToUnixTimeSeconds()
        });

        var headerEncoded = Base64UrlEncode(header);
        var payloadEncoded = Base64UrlEncode(payload);
        var signingInput = $"{headerEncoded}.{payloadEncoded}";
        using var hmac = new HMACSHA256(Encoding.UTF8.GetBytes(jwtSecret));
        var signature = hmac.ComputeHash(Encoding.UTF8.GetBytes(signingInput));
        return $"{signingInput}.{Base64UrlEncode(signature)}";
    }

    private static string GetEnvValue(string path, string name)
    {
        if (!File.Exists(path))
        {
            return string.Empty;
        }

        foreach (var line in File.ReadLines(path))
        {
            if (line.StartsWith($"{name}=", StringComparison.Ordinal))
            {
                return line[(name.Length + 1)..].Trim().Trim('"');
            }
        }

        return string.Empty;
    }

    private static string ResolveMakensisPath()
    {
        var candidates = new List<string>();
        var pathValue = Environment.GetEnvironmentVariable("PATH") ?? string.Empty;
        foreach (var segment in pathValue.Split(Path.PathSeparator, StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries))
        {
            candidates.Add(Path.Combine(segment, "makensis.exe"));
        }

        candidates.Add(@"C:\Program Files (x86)\NSIS\makensis.exe");
        candidates.Add(@"C:\Program Files\NSIS\makensis.exe");
        var resolved = candidates.FirstOrDefault(File.Exists);
        return resolved ?? throw new InvalidOperationException("makensis.exe not found. Install NSIS and ensure it is on PATH.");
    }

    private static string ConvertToNsisDefinePath(string basePath, string targetPath)
    {
        var relative = Path.GetRelativePath(basePath, targetPath);
        return !relative.StartsWith("..", StringComparison.OrdinalIgnoreCase)
            ? relative.Replace('/', '\\')
            : targetPath;
    }

    private static string? ResolveInstalledHostRoot(string? explicitRoot)
    {
        if (!string.IsNullOrWhiteSpace(explicitRoot))
        {
            var normalized = NormalizeFullPath(explicitRoot);
            return Directory.Exists(normalized) ? normalized : null;
        }

        var defaultInstalledRoot = NormalizeFullPath(Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData),
            "Cloudgime",
            "Host"));
        return Directory.Exists(defaultInstalledRoot) ? defaultInstalledRoot : null;
    }

    private static void WriteReleaseWrappers(string releaseRoot, string bundleReleaseRoot)
    {
        DeleteIfExists(Path.Combine(releaseRoot, "open-host-control.cmd"));
        DeleteIfExists(Path.Combine(releaseRoot, "open-host-control.bat"));
        DeleteIfExists(Path.Combine(releaseRoot, "open-host-control-folder.cmd"));
        WriteAsciiFile(Path.Combine(releaseRoot, "install-program-files.cmd"), BuildInstallProgramFilesLines());
        RemoveLegacyOpenHostControlScripts(bundleReleaseRoot);
        WriteAsciiFile(Path.Combine(bundleReleaseRoot, "start-all.bat"), BuildStartAllLines());
        WriteAsciiFile(Path.Combine(bundleReleaseRoot, "stop-all.bat"), BuildStopAllLines());

        var fallbackAudioDirectory = Path.Combine(bundleReleaseRoot, "drivers", "fallback-audio");
        Directory.CreateDirectory(fallbackAudioDirectory);
        WriteAsciiFile(Path.Combine(fallbackAudioDirectory, "install-audio.bat"), BuildInstallAudioLines());

        var sunshineScriptsDirectory = Path.Combine(bundleReleaseRoot, "sunshine", "scripts");
        Directory.CreateDirectory(sunshineScriptsDirectory);
        WriteAsciiFile(Path.Combine(sunshineScriptsDirectory, "install-gamepad.bat"), BuildInstallGamepadLines());
        WriteAsciiFile(Path.Combine(sunshineScriptsDirectory, "uninstall-gamepad.bat"), BuildUninstallGamepadLines());
    }

    private static void DeleteIfExists(string path)
    {
        if (File.Exists(path))
        {
            File.Delete(path);
        }
    }

    private static void RemoveLegacyOpenHostControlScripts(string root)
    {
        DeleteIfExists(Path.Combine(root, "open-host-control.cmd"));
        DeleteIfExists(Path.Combine(root, "open-host-control.bat"));
        DeleteIfExists(Path.Combine(root, "open-host-control-folder.cmd"));
    }

    private static void StripLegacyFrpArtifacts(string bundleRoot)
    {
        EnsureDirectoryDeleted(Path.Combine(bundleRoot, "frp"));

        foreach (var file in new[]
                 {
                     Path.Combine(bundleRoot, "start-frp.bat")
                 })
        {
            if (File.Exists(file))
            {
                File.Delete(file);
            }
        }
    }

    private static void ResetManagedPublicRouteSeed(string bundleRoot)
    {
        WriteAsciiFile(Path.Combine(bundleRoot, "PUBLIC_URL.txt"), []);
    }

    private static void WriteManagedBundleNotes(string bundleRoot)
    {
        ResetManagedPublicRouteSeed(bundleRoot);
        WriteAsciiFile(Path.Combine(bundleRoot, "README.txt"), BuildBundleReadmeLines());
        WriteAsciiFile(Path.Combine(bundleRoot, "SETUP.txt"), BuildBundleSetupLines());
    }

    private static string[] BuildReleaseLauncherLines() =>
    [
        "@echo off",
        "setlocal",
        "set \"APP=%~dp0cloudgime-host-control.exe\"",
        "set \"BUNDLE=%~dp0bundle\"",
        "if not exist \"%APP%\" (",
        "  echo Host Control executable not found:",
        "  echo %APP%",
        "  pause",
        "  exit /b 1",
        ")",
        "if not exist \"%BUNDLE%\" (",
        "  echo Bundle root not found:",
        "  echo %BUNDLE%",
        "  pause",
        "  exit /b 1",
        ")",
        "start \"\" \"%APP%\" --bundle-root \"%BUNDLE%\"",
        "endlocal"
    ];

    private static string[] BuildBundleReadmeLines() =>
    [
        "Managed bundle for Cloudgime Host",
        "",
        "What is included:",
        "- Host runtime (portable user-mode launch)",
        "- Cloudgime Host web surface",
        "- Setup-token activation flow from CloudRental master admin",
        "- Self-contained runtime from this project folder",
        "",
        "How to use:",
        "1. Install CloudgimeHostSetup.exe.",
        "2. Open Cloudgime Host Control.",
        "3. Paste the setup token from CloudRental master admin.",
        "4. Host setup, activation, runtime start, and readiness heartbeat run automatically.",
        "",
        "Notes:",
        "- First run may trigger Windows Firewall prompts.",
        "- Host Control may request Administrator permission once to register firewall rules for the bundle's WebRTC UDP range.",
        "- The host runtime is launched in user mode for now, not as the final Windows service model.",
        "- Before the runtime starts, the bundle runs a host preflight probe to pick the best available encoder/capture path for that PC.",
        "- If the bundle contains multiple runtime variants such as sunshine and sunshine-legacy, preflight picks the healthiest one automatically.",
        "- The host web surface is preconfigured as 127.0.0.1:49000.",
        "- Local runtime admin UI: https://localhost:49001",
        "- Local runtime admin credentials are intentionally not written to disk.",
        "- Managed pairing state is already bundled for the local host runtime.",
        "- Public routing now comes from the control plane and keeper tunnel. FRP is not included in this package.",
        "- Transport preset: direct-safe (direct first, TURN fallback enabled).",
        "- Router port forward is optional fallback only.",
        "- Host capability profile is written to moonlight\\server\\host_capability_profile.json",
        "- If a target PC is missing Visual C++ runtime, install Microsoft Visual C++ Redistributable 2015-2022 x64."
    ];

    private static string[] BuildBundleSetupLines() =>
    [
        "START:",
        "- Open cloudgime-host-control.exe for local admin tasks",
        "- Paste the setup token from CloudRental master admin",
        "- Host Control prepares, activates, starts runtime, and sends readiness automatically",
        "",
        "RUN MODE:",
        "- Public route is published by the control plane and keeper tunnel",
        "- Manual FRP server setup is no longer part of the normal path",
        "",
        "LOCAL RUNTIME ADMIN:",
        "- URL: https://localhost:49001",
        "- Secret handling: local runtime admin credentials are intentionally not written to disk.",
        "",
        "NETWORK:",
        "- Host Control may ask for Administrator permission once so the bundle can add Windows Firewall rules automatically.",
        "- Host Control runs a host preflight probe first and writes moonlight\\server\\host_capability_profile.json",
        "- If sunshine-legacy is bundled too, Host Control will auto-select it when that host needs the older NVENC compatibility path.",
        "- Transport preset: direct-safe (TURN fallback on).",
        "- Router port forward is optional fallback only."
    ];

    private static string[] BuildInstallProgramFilesLines() =>
    [
        "@echo off",
        "setlocal",
        "set \"EXE=%~dp0cloudgime-host-bootstrap.exe\"",
        "if not exist \"%EXE%\" (",
        "  echo Host bootstrap executable not found:",
        "  echo %EXE%",
        "  pause",
        "  exit /b 1",
        ")",
        "\"%EXE%\" install --bundle-source-root \"%~dp0bundle\" %*",
        "endlocal"
    ];

    private static string[] BuildBundleLauncherLines() =>
    [
        "@echo off",
        "setlocal",
        "set \"APP=%~dp0cloudgime-host-control.exe\"",
        "if not exist \"%APP%\" (",
        "  echo Host Control executable not found:",
        "  echo %APP%",
        "  pause",
        "  exit /b 1",
        ")",
        "start \"\" \"%APP%\" --bundle-root \"%~dp0.\"",
        "endlocal"
    ];

    private static string[] BuildStartAllLines() =>
    [
        "@echo off",
        "setlocal",
        "if exist \"%~dp0host-installer.exe\" (",
        "  \"%~dp0host-installer.exe\" --bundle-root \"%~dp0.\" prepare-host",
        "  exit /b %ERRORLEVEL%",
        ")",
        "echo host-installer.exe not found.",
        "exit /b 1"
    ];

    private static string[] BuildStopAllLines() =>
    [
        "@echo off",
        "setlocal",
        "if exist \"%~dp0host-installer.exe\" (",
        "  \"%~dp0host-installer.exe\" --bundle-root \"%~dp0.\" stop-bundle",
        ") else if exist \"%~dp0moonlight\\system\\cloudgime-runtime-agent.exe\" (",
        "  \"%~dp0moonlight\\system\\cloudgime-runtime-agent.exe\" --bundle-root \"%~dp0.\" stop-bundle",
        ") else if exist \"%~dp0bundle-process-manager.exe\" (",
        "  \"%~dp0bundle-process-manager.exe\" stop --bundle-root \"%~dp0.\" --web-port 18080 --sunshine-port 49000",
        ") else (",
        "  echo Could not find a supported runtime stop helper.",
        "  exit /b 1",
        ")",
        "exit /b %ERRORLEVEL%"
    ];

    private static string[] BuildInstallAudioLines() =>
    [
        "@echo off",
        "setlocal",
        "set \"ROOT=%MOONLIGHT_BUNDLE_ROOT%\"",
        "if \"%ROOT%\"==\"\" set \"ROOT=%~dp0\\..\\..\"",
        "set \"BOOTSTRAP=%ROOT%\\cloudgime-host-bootstrap.exe\"",
        "if not exist \"%BOOTSTRAP%\" (",
        "  echo Cloudgime bootstrap executable not found:",
        "  echo %BOOTSTRAP%",
        "  exit /b 1",
        ")",
        "\"%BOOTSTRAP%\" install-fallback-audio --package-root \"%~dp0\"",
        "exit /b %ERRORLEVEL%"
    ];

    private static string[] BuildInstallGamepadLines() =>
    [
        "@echo off",
        "setlocal",
        "set \"ROOT=%MOONLIGHT_BUNDLE_ROOT%\"",
        "if \"%ROOT%\"==\"\" set \"ROOT=%~dp0\\..\\..\"",
        "set \"BOOTSTRAP=%ROOT%\\cloudgime-host-bootstrap.exe\"",
        "if not exist \"%BOOTSTRAP%\" (",
        "  echo Cloudgime bootstrap executable not found:",
        "  echo %BOOTSTRAP%",
        "  exit /b 1",
        ")",
        "\"%BOOTSTRAP%\" install-gamepad --package-root \"%~dp0\"",
        "exit /b %ERRORLEVEL%"
    ];

    private static string[] BuildUninstallGamepadLines() =>
    [
        "@echo off",
        "setlocal",
        "set \"ROOT=%MOONLIGHT_BUNDLE_ROOT%\"",
        "if \"%ROOT%\"==\"\" set \"ROOT=%~dp0\\..\\..\"",
        "set \"BOOTSTRAP=%ROOT%\\cloudgime-host-bootstrap.exe\"",
        "if not exist \"%BOOTSTRAP%\" (",
        "  echo Cloudgime bootstrap executable not found:",
        "  echo %BOOTSTRAP%",
        "  exit /b 1",
        ")",
        "\"%BOOTSTRAP%\" uninstall-gamepad",
        "exit /b %ERRORLEVEL%"
    ];

    private static void RemoveStalePowerShellFiles(string bundleRoot)
    {
        foreach (var stalePath in new[]
        {
            Path.Combine(bundleRoot, "open-host-control.ps1"),
            Path.Combine(bundleRoot, "ensure-firewall.ps1"),
            Path.Combine(bundleRoot, "verify-startup.ps1"),
            Path.Combine(bundleRoot, "stop-bundle.ps1"),
            Path.Combine(bundleRoot, "moonlight", "server", "dynamic_display_resolution.ps1"),
            Path.Combine(bundleRoot, "moonlight", "server", "window_primary_watch.ps1"),
            Path.Combine(bundleRoot, "drivers", "fallback-audio", "install-audio.ps1"),
            Path.Combine(bundleRoot, "sunshine", "scripts", "install-gamepad.ps1"),
            Path.Combine(bundleRoot, "sunshine", "scripts", "uninstall-gamepad.ps1"),
        })
        {
            if (File.Exists(stalePath))
            {
                File.Delete(stalePath);
            }
        }
    }

    private static void CopyManagedBundleSeed(string portableRoot, string targetRoot)
    {
        var pairSeedPath = Path.Combine(portableRoot, "managed-bundle-seed", "shared_pair_info.json");
        var sunshineSharedRoot = Path.Combine(portableRoot, "managed-bundle-seed", "sunshine-shared");

        if (File.Exists(pairSeedPath))
        {
            CopyFileWithRetry(pairSeedPath, Path.Combine(targetRoot, "managed-shared_pair_info.json"));
        }

        if (Directory.Exists(sunshineSharedRoot))
        {
            CopyDirectoryContents(sunshineSharedRoot, Path.Combine(targetRoot, "managed-sunshine-shared"));
        }
    }

    private static string PublishBootstrap(string portableRoot)
    {
        var bootstrapProjectPath = Path.Combine(portableRoot, "HostControlBootstrap", "HostControlBootstrap.csproj");
        if (!File.Exists(bootstrapProjectPath))
        {
            throw new InvalidOperationException($"HostControlBootstrap project not found: {bootstrapProjectPath}");
        }

        var publishRoot = Path.Combine(portableRoot, "_publish", "HostControlBootstrap");
        EnsureDirectoryDeleted(publishRoot);
        Directory.CreateDirectory(publishRoot);

        RunChecked(
            "dotnet",
            ["publish", bootstrapProjectPath, "-c", "Release", "-o", publishRoot],
            Path.GetDirectoryName(bootstrapProjectPath)!,
            "Publish HostControlBootstrap");

        var bootstrapExe = Path.Combine(publishRoot, "cloudgime-host-bootstrap.exe");
        if (!File.Exists(bootstrapExe))
        {
            throw new InvalidOperationException($"Published HostControlBootstrap executable not found: {bootstrapExe}");
        }

        return bootstrapExe;
    }

    private static string PublishEmergencyUninstaller(string portableRoot)
    {
        var projectPath = Path.Combine(portableRoot, "HostEmergencyUninstaller", "HostEmergencyUninstaller.csproj");
        if (!File.Exists(projectPath))
        {
            throw new InvalidOperationException($"HostEmergencyUninstaller project not found: {projectPath}");
        }

        var publishRoot = Path.Combine(portableRoot, "_publish", "HostEmergencyUninstaller");
        EnsureDirectoryDeleted(publishRoot);
        Directory.CreateDirectory(publishRoot);

        RunChecked(
            "dotnet",
            ["publish", projectPath, "-c", "Release", "-o", publishRoot],
            Path.GetDirectoryName(projectPath)!,
            "Publish HostEmergencyUninstaller");

        var executablePath = Path.Combine(publishRoot, "uninstaller-cloudgime.exe");
        if (!File.Exists(executablePath))
        {
            throw new InvalidOperationException($"Published HostEmergencyUninstaller executable not found: {executablePath}");
        }

        return executablePath;
    }

    private static string PublishLauncher(string portableRoot)
    {
        var projectPath = Path.Combine(portableRoot, "HostControlLauncher", "HostControlLauncher.csproj");
        if (!File.Exists(projectPath))
        {
            throw new InvalidOperationException($"HostControlLauncher project not found: {projectPath}");
        }

        var publishRoot = Path.Combine(portableRoot, "_publish", "HostControlLauncher");
        EnsureDirectoryDeleted(publishRoot);
        Directory.CreateDirectory(publishRoot);

        RunChecked(
            "dotnet",
            [
                "publish",
                projectPath,
                "-c",
                "Release",
                "-r",
                "win-x64",
                "--self-contained",
                "true",
                "/p:PublishAot=true",
                "/p:StripSymbols=true",
                "-o",
                publishRoot
            ],
            Path.GetDirectoryName(projectPath)!,
            "Publish HostControlLauncher");

        var executablePath = Path.Combine(publishRoot, "open-host-control.exe");
        if (!File.Exists(executablePath))
        {
            throw new InvalidOperationException($"Published HostControlLauncher executable not found: {executablePath}");
        }

        return executablePath;
    }

    private static string PublishKeeperTunnelAgent(ParsedArguments args, string repoRoot, string? installedRoot)
    {
        static string[] NormalizeCandidates(IEnumerable<string?> candidates) =>
            candidates
                .Where(candidate => !string.IsNullOrWhiteSpace(candidate))
                .Select(candidate => NormalizeFullPath(candidate!))
                .Distinct(StringComparer.OrdinalIgnoreCase)
                .ToArray();

        var projectCandidates = NormalizeCandidates(
        [
            args.GetValue("keeper-tunnel-project"),
            Path.Combine(repoRoot, "..", "power panel", "KeeperTunnelAgent", "KeeperTunnelAgent.csproj"),
            @"C:\projek-rdp-web-clean-final\KeeperTunnelAgent\KeeperTunnelAgent.csproj"
        ]);
        var projectPath = projectCandidates.FirstOrDefault(File.Exists);
        if (projectPath is not null)
        {
            var publishRoot = Path.Combine(Path.GetDirectoryName(projectPath)!, "bin", "Release", "net8.0", "win-x64", "selfcontained");
            RunChecked(
                "dotnet",
                [
                    "publish",
                    projectPath,
                    "-c",
                    "Release",
                    "-r",
                    "win-x64",
                    "/p:PublishSingleFile=true",
                    "/p:SelfContained=true",
                    "/p:IncludeNativeLibrariesForSelfExtract=true",
                    "-o",
                    publishRoot
                ],
                Path.GetDirectoryName(projectPath)!,
                "Publish KeeperTunnelAgent");

            var publishedExecutablePath = Path.Combine(publishRoot, "KeeperTunnelAgent.exe");
            if (!File.Exists(publishedExecutablePath))
            {
                throw new InvalidOperationException($"Published KeeperTunnelAgent executable not found: {publishedExecutablePath}");
            }

            return publishedExecutablePath;
        }

        var executableCandidates = NormalizeCandidates(
        [
            args.GetValue("keeper-tunnel-exe"),
            Path.Combine(repoRoot, "runtime", "tools", "keeper-tunnel", "KeeperTunnelAgent.exe"),
            installedRoot is not null ? Path.Combine(installedRoot, "keeper-tunnel", "KeeperTunnelAgent.exe") : null
        ]);
        var executablePath = executableCandidates.FirstOrDefault(File.Exists);
        if (executablePath is not null)
        {
            return executablePath;
        }

        throw new InvalidOperationException(
            $"KeeperTunnelAgent project or executable not found. Checked projects: {string.Join(", ", projectCandidates)}. Checked executables: {string.Join(", ", executableCandidates)}");
    }

    private static void BuildTauriApp(string hostControlRoot)
    {
        var packageJsonPath = Path.Combine(hostControlRoot, "package.json");
        if (!File.Exists(packageJsonPath))
        {
            throw new InvalidOperationException($"Host Control package.json not found: {packageJsonPath}");
        }

        RunCheckedNoCapture(ResolveNpmPath(), ["run", "tauri", "build"], hostControlRoot, "Build Tauri Host Control");
    }

    private static string ResolveNpmPath()
    {
        var programFiles = Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles);
        var npmCmdPath = Path.Combine(programFiles, "nodejs", "npm.cmd");
        return File.Exists(npmCmdPath) ? npmCmdPath : "npm.cmd";
    }

    private static string? GetLatestMsiPath(string searchRoot)
    {
        if (!Directory.Exists(searchRoot))
        {
            return null;
        }

        return Directory.GetFiles(searchRoot, "*.msi", SearchOption.TopDirectoryOnly)
            .OrderByDescending(path => File.GetLastWriteTimeUtc(path))
            .FirstOrDefault();
    }

    private static void RunChecked(string fileName, IReadOnlyList<string> args, string workingDirectory, string stepName)
    {
        _ = RunCheckedCapture(fileName, args, workingDirectory, stepName);
    }

    private static void RunCheckedNoCapture(string fileName, IReadOnlyList<string> args, string workingDirectory, string stepName)
    {
        var startInfo = new ProcessStartInfo(fileName)
        {
            WorkingDirectory = workingDirectory,
            UseShellExecute = false,
            CreateNoWindow = true,
        };

        foreach (var arg in args)
        {
            startInfo.ArgumentList.Add(arg);
        }

        using var process = Process.Start(startInfo)
            ?? throw new InvalidOperationException($"{stepName} failed to start.");
        process.WaitForExit();

        if (process.ExitCode != 0)
        {
            throw new InvalidOperationException($"{stepName} failed with exit code {process.ExitCode}.");
        }
    }

    private static ProcessCaptureResult RunCheckedCapture(string fileName, IReadOnlyList<string> args, string workingDirectory, string stepName)
    {
        var startInfo = new ProcessStartInfo(fileName)
        {
            WorkingDirectory = workingDirectory,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            UseShellExecute = false,
            CreateNoWindow = true,
        };

        foreach (var arg in args)
        {
            startInfo.ArgumentList.Add(arg);
        }

        using var process = Process.Start(startInfo)
            ?? throw new InvalidOperationException($"{stepName} failed to start.");
        var stdOut = process.StandardOutput.ReadToEnd();
        var stdErr = process.StandardError.ReadToEnd();
        process.WaitForExit();

        if (process.ExitCode != 0)
        {
            var detail = string.Join(Environment.NewLine, new[] { stdOut.Trim(), stdErr.Trim() }.Where(value => !string.IsNullOrWhiteSpace(value)));
            throw new InvalidOperationException(
                string.IsNullOrWhiteSpace(detail)
                    ? $"{stepName} failed with exit code {process.ExitCode}."
                    : $"{stepName} failed with exit code {process.ExitCode}:{Environment.NewLine}{detail}");
        }

        return new ProcessCaptureResult(stdOut, stdErr);
    }

    private static void CopyFileWithRetry(string sourcePath, string targetPath)
    {
        Directory.CreateDirectory(Path.GetDirectoryName(targetPath)!);
        Exception? lastError = null;
        for (var attempt = 0; attempt < 5; attempt++)
        {
            try
            {
                File.Copy(sourcePath, targetPath, overwrite: true);
                return;
            }
            catch (Exception ex)
            {
                lastError = ex;
                Thread.Sleep(300);
            }
        }

        throw lastError ?? new IOException($"Could not copy {sourcePath} to {targetPath}.");
    }

    private static void EnsureFilePresent(string sourcePath, string targetPath, string label)
    {
        if (!File.Exists(targetPath))
        {
            CopyFileWithRetry(sourcePath, targetPath);
        }

        if (!File.Exists(targetPath))
        {
            throw new InvalidOperationException($"{label} is missing from the payload: {targetPath}");
        }
    }

    private static void CopyOptionalFile(string? sourcePath, string targetPath)
    {
        if (string.IsNullOrWhiteSpace(sourcePath) || !File.Exists(sourcePath))
        {
            return;
        }

        CopyFileWithRetry(sourcePath, targetPath);
    }

    private static void SyncHostArtifactsToAllInOne(string releaseRoot, string? setupPath)
    {
        try
        {
            Directory.CreateDirectory(ReleaseAllInOneRoot);
            var legacyTargetRoot = Path.Combine(ReleaseAllInOneRoot, "Cloudgime Host");
            if (Directory.Exists(legacyTargetRoot))
            {
                Directory.Delete(legacyTargetRoot, recursive: true);
            }

            foreach (var (sourcePath, fileName) in new[]
                     {
                         (setupPath, "CloudgimeHostSetup.exe"),
                         (Path.Combine(releaseRoot, "uninstaller-cloudgime.exe"), "uninstaller-cloudgime.exe"),
                     })
            {
                if (string.IsNullOrWhiteSpace(sourcePath) || !File.Exists(sourcePath))
                {
                    continue;
                }

                CopyFileWithRetry(sourcePath, Path.Combine(ReleaseAllInOneRoot, fileName));
            }

            foreach (var staleFile in new[]
                     {
                         "cloudgime-host-control.exe",
                         "cloudgime-host-bootstrap.exe",
                         "cloudgime-host-control.msi",
                     })
            {
                var stalePath = Path.Combine(ReleaseAllInOneRoot, staleFile);
                if (File.Exists(stalePath))
                {
                    File.Delete(stalePath);
                }
            }
        }
        catch (UnauthorizedAccessException ex)
        {
            Console.Error.WriteLine($"Skipping optional all-in-one artifact sync: {ex.Message}");
        }
        catch (IOException ex)
        {
            Console.Error.WriteLine($"Skipping optional all-in-one artifact sync: {ex.Message}");
        }
    }

    private static void CopyDirectoryContents(string sourceRoot, string targetRoot)
    {
        Directory.CreateDirectory(targetRoot);
        foreach (var directory in Directory.GetDirectories(sourceRoot, "*", SearchOption.AllDirectories))
        {
            var relative = Path.GetRelativePath(sourceRoot, directory);
            Directory.CreateDirectory(Path.Combine(targetRoot, relative));
        }

        foreach (var file in Directory.GetFiles(sourceRoot, "*", SearchOption.AllDirectories))
        {
            var relative = Path.GetRelativePath(sourceRoot, file);
            CopyFileWithRetry(file, Path.Combine(targetRoot, relative));
        }
    }

    private static void OverlayLatestRuntimeStatic(string repoRoot, string bundleRoot)
    {
        var runtimeStaticRoot = Path.Combine(repoRoot, "runtime", "moonlight", "static");
        if (!Directory.Exists(runtimeStaticRoot))
        {
            throw new InvalidOperationException($"Runtime static root not found: {runtimeStaticRoot}");
        }

        var bundleStaticRoot = Path.Combine(bundleRoot, "moonlight", "static");
        CopyDirectoryContents(runtimeStaticRoot, bundleStaticRoot);
        var sourceWebRoot = Path.Combine(repoRoot, "web");
        if (Directory.Exists(sourceWebRoot))
        {
            CopyDirectoryContents(sourceWebRoot, bundleStaticRoot);
        }
        EnsureFilePresent(
            File.Exists(Path.Combine(sourceWebRoot, "native_bridge_panel.html"))
                ? Path.Combine(sourceWebRoot, "native_bridge_panel.html")
                : Path.Combine(runtimeStaticRoot, "native_bridge_panel.html"),
            Path.Combine(bundleStaticRoot, "native_bridge_panel.html"),
            "Native Android bridge panel");
    }

    private static void OverlayDriverSeed(string repoRoot, string bundleRoot)
    {
        var driverSeedRoot = Path.Combine(repoRoot, "payload-seed", "drivers");
        if (!Directory.Exists(driverSeedRoot))
        {
            return;
        }

        CopyDirectoryContents(driverSeedRoot, Path.Combine(bundleRoot, "drivers"));
    }

    private static void EnsureDirectoryDeleted(string path)
    {
        if (Directory.Exists(path))
        {
            Directory.Delete(path, recursive: true);
        }
    }

    private static void WriteAsciiFile(string path, IEnumerable<string> lines)
    {
        Directory.CreateDirectory(Path.GetDirectoryName(path)!);
        File.WriteAllLines(path, lines, Encoding.ASCII);
    }

    private static string ResolvePortableRoot(ParsedArguments args)
    {
        var explicitRoot = args.GetValue("portable-root");
        if (!string.IsNullOrWhiteSpace(explicitRoot))
        {
            return NormalizeFullPath(explicitRoot);
        }

        var current = new DirectoryInfo(AppContext.BaseDirectory);
        while (current is not null)
        {
            if (Directory.Exists(Path.Combine(current.FullName, "HostControlApp.Tauri")) &&
                Directory.Exists(Path.Combine(current.FullName, "HostControlBootstrap")))
            {
                return current.FullName;
            }

            current = current.Parent;
        }

        throw new InvalidOperationException("Could not resolve tools/portable root. Pass --portable-root explicitly.");
    }

    private static string ResolveHostControlRoot(ParsedArguments args, string portableRoot) =>
        NormalizeFullPath(args.GetValue("host-control-root") ?? Path.Combine(portableRoot, "HostControlApp.Tauri"));

    private static string ResolveTauriTargetRoot(string hostControlRoot)
    {
        var cargoTargetDir = Environment.GetEnvironmentVariable("CARGO_TARGET_DIR");
        if (!string.IsNullOrWhiteSpace(cargoTargetDir))
        {
            var normalizedCargoTargetDir = NormalizeFullPath(cargoTargetDir);
            if (Directory.Exists(normalizedCargoTargetDir))
            {
                return normalizedCargoTargetDir;
            }
        }

        return NormalizeFullPath(Path.Combine(hostControlRoot, "src-tauri", "target"));
    }

    private static string ResolveRepoRoot(string portableRoot) =>
        NormalizeFullPath(Path.Combine(portableRoot, "..", ".."));

    private static string NormalizeFullPath(string path) => Path.GetFullPath(path);

    private static string GetRequiredString(JsonObject obj, string propertyName)
    {
        var value = GetString(obj, propertyName);
        return !string.IsNullOrWhiteSpace(value)
            ? value
            : throw new InvalidOperationException($"Missing required property '{propertyName}'.");
    }

    private static string GetString(JsonObject obj, string propertyName, string fallback = "")
    {
        if (!obj.TryGetPropertyValue(propertyName, out var value) || value is null)
        {
            return fallback;
        }

        return value.GetValue<string?>() ?? fallback;
    }

    private static bool GetBool(JsonObject obj, string propertyName, bool fallback = false)
    {
        if (!obj.TryGetPropertyValue(propertyName, out var value) || value is null)
        {
            return fallback;
        }

        return value.GetValue<bool?>() ?? fallback;
    }

    private static int GetInt(JsonObject obj, string propertyName, int fallback = 0)
    {
        if (!obj.TryGetPropertyValue(propertyName, out var value) || value is null)
        {
            return fallback;
        }

        return value.GetValue<int?>() ?? fallback;
    }

    private static JsonSerializerOptions JsonOptions() => new()
    {
        WriteIndented = false
    };
}

internal sealed class ParsedArguments
{
    private readonly Dictionary<string, string> _values;
    private readonly HashSet<string> _flags;

    private ParsedArguments(string command, Dictionary<string, string> values, HashSet<string> flags)
    {
        Command = command;
        _values = values;
        _flags = flags;
    }

    public string Command { get; }

    public string? GetValue(string name) =>
        _values.TryGetValue(name, out var value) ? value : null;

    public bool HasFlag(string name) => _flags.Contains(name);

    public static ParsedArguments Parse(string[] args)
    {
        if (args.Length == 0)
        {
            throw new InvalidOperationException("missing command");
        }

        var values = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
        var flags = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        for (var i = 1; i < args.Length; i++)
        {
            var argument = args[i];
            if (!argument.StartsWith("--", StringComparison.Ordinal))
            {
                throw new InvalidOperationException($"unknown argument '{argument}'");
            }

            var name = argument[2..];
            if (i + 1 < args.Length && !args[i + 1].StartsWith("--", StringComparison.Ordinal))
            {
                values[name] = args[++i];
            }
            else
            {
                flags.Add(name);
            }
        }

        return new ParsedArguments(args[0], values, flags);
    }
}

internal readonly record struct ProcessCaptureResult(string StdOut, string StdErr);
