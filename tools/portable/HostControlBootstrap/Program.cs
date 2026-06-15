using System.Diagnostics;
using System.Management;
using System.Runtime.InteropServices;
using System.Security.AccessControl;
using System.Security.Cryptography.X509Certificates;
using System.Security.Cryptography;
using System.Security.Principal;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;
using Microsoft.Win32;
using Nefarius.ViGEm.Client;
using MessageBox = System.Windows.Forms.MessageBox;
using MessageBoxButtons = System.Windows.Forms.MessageBoxButtons;
using MessageBoxIcon = System.Windows.Forms.MessageBoxIcon;

namespace CloudgimeHostBootstrap;

internal static class Program
{
    private const string DefaultProductName = "Cloudgime Host";
    private const string DefaultAppProductName = "Cloudgime Host Control";
    private const string DefaultUninstallRegistryKey = @"Software\Microsoft\Windows\CurrentVersion\Uninstall\CloudgimeHostControl";
    private const string HostWindowsServiceName = "CloudgimeHost-Host";
    private const string RuntimeWindowsServiceName = "CloudgimeRuntime-Host";
    private const string HostUserDaemonTaskName = "CloudgimeHostUser-Host";
    private const string HostKeeperTunnelTaskName = "CloudgimeHostKeeperTunnelAgent";
    private const string HostKeepAwakeSystemTaskName = "CloudgimeHostKeepAwakeAgent";
    private const string HostKeepAwakeUserTaskName = "CloudgimeHostKeepAwakeAgentUser";
    private const string HostDisplayBootGuardTaskName = "CloudgimeHostDisplayBootGuard";
    private const string MttVddDeviceId = @"Root\MttVDD";
    private const string MttVddRuntimeRoot = @"C:\VirtualDisplayDriver";
    private const string MttVddRegistryPath = @"SOFTWARE\MikeTheTech\VirtualDisplayDriver";
    private static readonly TimeSpan PrepareInstallTimeout = TimeSpan.FromMinutes(4);
    private static readonly TimeSpan OptionalInstallerActionTimeout = TimeSpan.FromSeconds(25);
    private static readonly string LogPath = Path.Combine(Path.GetTempPath(), "cloudgime-host-bootstrap.log");
    private static readonly UTF8Encoding Utf8NoBom = new(false);
    private static readonly Regex HostIdRegex = new("^[a-z0-9][a-z0-9-]{5,63}$", RegexOptions.IgnoreCase | RegexOptions.Compiled);
    private static readonly Regex MachineIdentityRegex = new("^cgm-[a-f0-9]{32}$", RegexOptions.IgnoreCase | RegexOptions.Compiled);
    private static readonly Regex InstallInstanceRegex = new("^cgi-[a-f0-9]{16}$", RegexOptions.IgnoreCase | RegexOptions.Compiled);

    [STAThread]
    private static int Main(string[] args)
    {
        try
        {
            Log($"bootstrap start args={string.Join(" ", args.Select(QuoteArgument))}");
            return Run(args);
        }
        catch (Exception ex)
        {
            Log($"bootstrap error: {ex}");
            MessageBox.Show(ex.Message, DefaultProductName, MessageBoxButtons.OK, MessageBoxIcon.Error);
            return 1;
        }
    }

    private static int Run(string[] args)
    {
        var parsed = ParsedArguments.Parse(args);
        if ((parsed.Command == "install"
                || parsed.Command == "repair-state"
                || parsed.Command == "run-installed-uninstall"
                || parsed.Command == "install-vdd-display"
                || parsed.Command == "install-fallback-audio"
                || parsed.Command == "install-gamepad"
                || parsed.Command == "uninstall-gamepad")
            && !IsAdministrator())
        {
            return RelaunchElevated(args);
        }

        return parsed.Command switch
        {
            "install" => Install(parsed),
            "repair-state" => RepairState(parsed),
            "run-installed-uninstall" => RunInstalledUninstall(parsed),
            "install-vdd-display" => InstallVddDisplay(parsed),
            "install-fallback-audio" => InstallFallbackAudio(parsed),
            "install-gamepad" => InstallGamepad(parsed),
            "uninstall-gamepad" => UninstallGamepad(parsed),
            _ => throw new InvalidOperationException($"Unknown command '{parsed.Command}'.")
        };
    }

    private static int Install(ParsedArguments args)
    {
        var installRoot = NormalizeFullPath(args.GetValue("install-root")
            ?? Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles), DefaultProductName));
        var dataRoot = NormalizeFullPath(args.GetValue("data-root")
            ?? Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData), "Cloudgime", "Host"));
        var releaseRoot = NormalizeFullPath(args.GetValue("release-root")
            ?? AppContext.BaseDirectory);
        var bundleSourceRoot = ResolveBundleSourceRoot(args.GetValue("bundle-source-root"), releaseRoot);
        var productName = args.GetValue("product-name") ?? DefaultProductName;
        var appProductName = args.GetValue("app-product-name") ?? DefaultAppProductName;
        var overwriteData = args.HasFlag("overwrite-data");
        var resetLocalIdentity = args.HasFlag("reset-local-identity") || overwriteData;
        var launchAfterInstall = args.HasFlag("launch-after-install");
        Log($"install installRoot={installRoot} dataRoot={dataRoot} releaseRoot={releaseRoot} bundleSourceRoot={bundleSourceRoot} overwriteData={overwriteData} resetLocalIdentity={resetLocalIdentity} launchAfterInstall={launchAfterInstall}");

        var appExeSource = Path.Combine(releaseRoot, "cloudgime-host-control.exe");
        if (!File.Exists(appExeSource))
        {
            throw new InvalidOperationException($"cloudgime-host-control.exe not found in release root: {releaseRoot}");
        }
        var emergencyUninstallerSource = Path.Combine(releaseRoot, "uninstaller-cloudgime.exe");
        var launcherSource = Path.Combine(releaseRoot, "open-host-control.exe");

        var bootstrapSource = Environment.ProcessPath
            ?? throw new InvalidOperationException("Current executable path could not be resolved.");

        if (string.IsNullOrWhiteSpace(bundleSourceRoot) || !Directory.Exists(bundleSourceRoot))
        {
            throw new InvalidOperationException($"Bundle source root not found: {bundleSourceRoot}");
        }

        if (!File.Exists(Path.Combine(bundleSourceRoot, "host-installer.exe")))
        {
            throw new InvalidOperationException($"host-installer.exe not found in bundle source: {bundleSourceRoot}");
        }

        if (Directory.Exists(installRoot))
        {
            Directory.Delete(installRoot, recursive: true);
        }

        Directory.CreateDirectory(installRoot);

        var installedAppExe = Path.Combine(installRoot, "cloudgime-host-control.exe");
        File.Copy(appExeSource, installedAppExe, overwrite: true);
        if (File.Exists(launcherSource))
        {
            File.Copy(launcherSource, Path.Combine(installRoot, "open-host-control.exe"), overwrite: true);
        }

        var installedBootstrapExe = Path.Combine(installRoot, "cloudgime-host-bootstrap.exe");
        File.Copy(bootstrapSource, installedBootstrapExe, overwrite: true);
        if (File.Exists(emergencyUninstallerSource))
        {
            var installedEmergencyUninstallerExe = Path.Combine(installRoot, "uninstaller-cloudgime.exe");
            File.Copy(emergencyUninstallerSource, installedEmergencyUninstallerExe, overwrite: true);
        }

        RemoveLegacyOpenHostControlShortcutScripts(installRoot);
        RemoveLegacyAppInstallerShortcuts(appProductName);

        var freshBundleData = false;
        var refreshedExistingData = false;
        if (Directory.Exists(dataRoot) && overwriteData)
        {
            PrepareExistingBundleForInstall(dataRoot);
            Directory.Delete(dataRoot, recursive: true);
            freshBundleData = true;
        }
        else if (Directory.Exists(dataRoot))
        {
            PrepareExistingBundleForInstall(dataRoot);
            RefreshExistingBundleData(bundleSourceRoot, dataRoot);
            refreshedExistingData = true;
        }

        if (!Directory.Exists(dataRoot))
        {
            Directory.CreateDirectory(dataRoot);
            CopyDirectory(bundleSourceRoot, dataRoot);
            freshBundleData = true;
        }

        RepairSunshineSharedIfMissing(dataRoot, releaseRoot);
        RepairPairSeedIfMissing(dataRoot, releaseRoot);

        AddWindowsDefenderExclusions(installRoot, dataRoot);

        WriteInstallLayout(installRoot, dataRoot, productName);
        WriteUninstallRegistration(installRoot, dataRoot, installedAppExe, productName);
        WriteStartMenuAndDesktopShortcuts(dataRoot, installedAppExe, productName);

        if (resetLocalIdentity || freshBundleData || refreshedExistingData || TestActivationStateRepairNeeded(dataRoot))
        {
            Log("install running finalize bundle");
            FinalizeBundle(dataRoot, releaseRoot, resetLocalIdentity, args.GetValue("auto-logon-password"));
        }
        else
        {
            Log("install running permission repair only");
            RepairCloudgimeRuntimePermissions(dataRoot);
        }

        if (launchAfterInstall)
        {
            Log("install launching cloudgime-host-control.exe");
            StartProcessHidden(installedAppExe, $"--bundle-root \"{dataRoot}\"", dataRoot, waitForExit: false);
        }

        Log("install completed successfully");
        return 0;
    }

    private static int RepairState(ParsedArguments args)
    {
        var bundleRoot = NormalizeFullPath(args.GetValue("bundle-root")
            ?? throw new InvalidOperationException("--bundle-root is required for repair-state."));
        var releaseRoot = NormalizeFullPath(args.GetValue("release-root") ?? AppContext.BaseDirectory);
        var installRoot = NormalizeFullPath(args.GetValue("install-root")
            ?? Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles), DefaultProductName));
        Log($"repair-state bundleRoot={bundleRoot} releaseRoot={releaseRoot}");
        AddWindowsDefenderExclusions(installRoot, bundleRoot);
        FinalizeBundle(bundleRoot, releaseRoot, args.HasFlag("reset-local-identity"), args.GetValue("auto-logon-password"));
        Log("repair-state completed successfully");
        return 0;
    }

    private static int InstallVddDisplay(ParsedArguments args)
    {
        var bundleRoot = NormalizeFullPath(args.GetValue("bundle-root")
            ?? throw new InvalidOperationException("--bundle-root is required for install-vdd-display."));
        Log($"install-vdd-display command bundleRoot={bundleRoot}");
        InstallVirtualDisplayDriver(bundleRoot);
        Log("install-vdd-display command completed successfully");
        return 0;
    }

    private static int RunInstalledUninstall(ParsedArguments args)
    {
        var bundleRoot = NormalizeFullPath(args.GetValue("bundle-root")
            ?? throw new InvalidOperationException("--bundle-root is required for run-installed-uninstall."));
        var installRoot = NormalizeFullPath(args.GetValue("install-root")
            ?? throw new InvalidOperationException("--install-root is required for run-installed-uninstall."));
        var targetPid = ParseIntArgument(args.GetValue("target-pid"), "target-pid");
        var uninstallRegistryKey = args.GetValue("uninstall-registry-key") ?? string.Empty;
        var appInstallerProductCode = args.GetValue("app-installer-product-code") ?? string.Empty;
        var appInstallerRegistryPath = args.GetValue("app-installer-registry-path") ?? string.Empty;
        var startMenuShortcut = args.GetValue("start-menu-shortcut") ?? string.Empty;
        var startMenuFolder = args.GetValue("start-menu-folder") ?? string.Empty;
        var publicDesktopShortcut = args.GetValue("public-desktop-shortcut") ?? string.Empty;

        Log($"run-installed-uninstall bundleRoot={bundleRoot} installRoot={installRoot} targetPid={targetPid}");

        WaitForProcessExit(targetPid, TimeSpan.FromSeconds(30));
        TryRunHostInstallerCommand(bundleRoot, "stop-bundle", OptionalInstallerActionTimeout);
        TryRunHostInstallerCommand(bundleRoot, "stop-service", OptionalInstallerActionTimeout);
        TryRunHostInstallerCommand(bundleRoot, "uninstall-service", OptionalInstallerActionTimeout);
        TryDeleteHostKeepAwakeTasks();
        StopProcessesWithinRoots(new[] { bundleRoot, installRoot }, TimeSpan.FromSeconds(20));

        if (!string.IsNullOrWhiteSpace(appInstallerProductCode))
        {
            StartProcessHidden("msiexec.exe", $"/x {QuoteArgument(appInstallerProductCode)} /qn /norestart", Environment.SystemDirectory, waitForExit: true);
        }

        TryDeleteFile(publicDesktopShortcut);
        TryDeleteFile(startMenuShortcut);
        TryDeleteDirectory(startMenuFolder);
        TryDeleteRegistryTree(appInstallerRegistryPath);
        TryDeleteRegistryTree(uninstallRegistryKey);
        var commonCloudgimeRoot = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData), "Cloudgime");
        TryDeleteFile(Path.Combine(commonCloudgimeRoot, "pc_identity.json"));
        TryDeleteFile(Path.Combine(commonCloudgimeRoot, "pending_uninstall.json"));
        TryDeleteHostKeeperTunnelTask();
        TryDeleteDirectory(bundleRoot);
        TryDeleteDirectory(installRoot);
        ScheduleFinalCleanup(Environment.ProcessPath, bundleRoot, installRoot);

        Log("run-installed-uninstall completed");
        return 0;
    }

    private static int InstallFallbackAudio(ParsedArguments args)
    {
        var packageRoot = NormalizeFullPath(args.GetValue("package-root")
            ?? throw new InvalidOperationException("--package-root is required for install-fallback-audio."));
        Log($"install-fallback-audio packageRoot={packageRoot}");

        var manifestPath = Path.Combine(packageRoot, "package.json");
        if (!File.Exists(manifestPath))
        {
            throw new InvalidOperationException($"Fallback audio manifest not found: {manifestPath}");
        }

        var manifest = JsonSerializer.Deserialize<FallbackAudioManifest>(File.ReadAllText(manifestPath))
            ?? throw new InvalidOperationException("Could not parse the fallback audio manifest.");
        if (string.IsNullOrWhiteSpace(manifest.Installer))
        {
            throw new InvalidOperationException("Fallback audio manifest must define 'installer'.");
        }

        var installerPath = NormalizeFullPath(Path.Combine(packageRoot, manifest.Installer));
        if (!File.Exists(installerPath))
        {
            throw new InvalidOperationException($"Fallback audio installer not found: {installerPath}");
        }

        EnsurePackageSignerTrusted(installerPath, packageRoot);
        var extension = Path.GetExtension(installerPath).ToLowerInvariant();
        switch (extension)
        {
            case ".inf":
                RunChecked("pnputil.exe", $"/add-driver \"{installerPath}\" /install", packageRoot, "Fallback audio driver install");
                break;
            case ".msi":
                RunChecked(
                    "msiexec.exe",
                    $"/i \"{installerPath}\" {JoinQuotedArguments(manifest.Arguments)} /qn /norestart".Trim(),
                    packageRoot,
                    "Fallback audio MSI install");
                break;
            default:
                RunChecked(installerPath, JoinQuotedArguments(manifest.Arguments), packageRoot, "Fallback audio package install");
                break;
        }

        if (manifest.PostInstallDelaySeconds > 0)
        {
            Thread.Sleep(TimeSpan.FromSeconds(manifest.PostInstallDelaySeconds));
        }

        VerifyExpectedAudioEndpoints(manifest);
        Log("install-fallback-audio completed");
        return 0;
    }

    private static int InstallGamepad(ParsedArguments args)
    {
        var packageRoot = NormalizeFullPath(args.GetValue("package-root")
            ?? throw new InvalidOperationException("--package-root is required for install-gamepad."));
        InstallOrRepairGamepadDriver(packageRoot);
        return 0;
    }

    private static int UninstallGamepad(ParsedArguments args)
    {
        _ = args;
        Log("uninstall-gamepad");

        var uninstallCommand = ResolveVigemUninstallCommand();
        if (uninstallCommand is null)
        {
            Log("uninstall-gamepad skipped because ViGEm Bus Driver is not installed");
            return 0;
        }

        RunChecked(uninstallCommand.FileName, uninstallCommand.Arguments, Environment.SystemDirectory, "ViGEmBus uninstall");
        Log("uninstall-gamepad completed");
        return 0;
    }

    private static void InstallBundledGamepadDriver(string bundleRoot)
    {
        var packageRoot = Path.Combine(bundleRoot, "sunshine", "scripts");
        if (!Directory.Exists(packageRoot))
        {
            Log($"install-gamepad skipped because package root is missing: {packageRoot}");
            return;
        }

        InstallOrRepairGamepadDriver(packageRoot);
    }

    private static void InstallOrRepairGamepadDriver(string packageRoot)
    {
        Log($"install-gamepad packageRoot={packageRoot}");
        var installerPath = Path.Combine(packageRoot, "vigembus_installer.exe");
        if (!File.Exists(installerPath))
        {
            throw new InvalidOperationException($"ViGEmBus installer not found: {installerPath}");
        }

        var driverVersion = ReadVigemBusDriverVersion() ?? "missing";
        var installRecordPresent = ResolveVigemUninstallCommand() is not null || HasVigemBusServiceKey();
        var clientHealthy = TryOpenVigemClient(out var initialProbeDetail);
        Log(
            $"install-gamepad probe driverVersion={driverVersion} installRecordPresent={installRecordPresent} clientHealthy={clientHealthy} detail={initialProbeDetail}");
        if (clientHealthy)
        {
            Log("install-gamepad skipped because ViGEm client probe succeeded");
            return;
        }

        var uninstallCommand = ResolveVigemUninstallCommand();
        if (installRecordPresent && uninstallCommand is not null)
        {
            Log($"install-gamepad repairing stale ViGEm install detail={initialProbeDetail}");
            RunChecked(
                uninstallCommand.FileName,
                uninstallCommand.Arguments,
                Environment.SystemDirectory,
                "ViGEmBus uninstall");
            WaitForVigemClientState(expectedHealthy: false, TimeSpan.FromSeconds(10), out _);
        }

        RunChecked(installerPath, "/passive /promptrestart", packageRoot, "ViGEmBus install");
        if (!WaitForVigemClientState(expectedHealthy: true, TimeSpan.FromSeconds(30), out var repairedProbeDetail))
        {
            throw new InvalidOperationException(
                $"ViGEmBus install completed but the client probe still failed ({repairedProbeDetail}). A reboot may be required.");
        }

        Log("install-gamepad completed");
    }

    private static string? ReadVigemBusDriverVersion()
    {
        var systemRoot = Environment.GetFolderPath(Environment.SpecialFolder.Windows);
        var vigemBusPath = Path.Combine(systemRoot, "System32", "drivers", "ViGEmBus.sys");
        if (!File.Exists(vigemBusPath))
        {
            return null;
        }

        return FileVersionInfo.GetVersionInfo(vigemBusPath).FileVersion;
    }

    private static bool HasVigemBusServiceKey()
    {
        using var serviceKey = Registry.LocalMachine.OpenSubKey(@"SYSTEM\CurrentControlSet\Services\ViGEmBus", writable: false);
        return serviceKey is not null;
    }

    private static bool TryOpenVigemClient(out string detail)
    {
        try
        {
            using var client = new ViGEmClient();
            detail = "ok";
            return true;
        }
        catch (Exception ex)
        {
            detail = $"{ex.GetType().Name}: {ex.Message}".Trim();
            return false;
        }
    }

    private static bool WaitForVigemClientState(bool expectedHealthy, TimeSpan timeout, out string detail)
    {
        var deadlineUtc = DateTime.UtcNow + timeout;
        do
        {
            var healthy = TryOpenVigemClient(out detail);
            if (healthy == expectedHealthy)
            {
                return true;
            }

            Thread.Sleep(TimeSpan.FromSeconds(1));
        } while (DateTime.UtcNow < deadlineUtc);

        _ = TryOpenVigemClient(out detail);
        return false;
    }

    private static void FinalizeBundle(string bundleRoot, string releaseRoot, bool resetLocalIdentity = false, string? autoLogonPassword = null)
    {
        Log($"finalize bundleRoot={bundleRoot} releaseRoot={releaseRoot} resetLocalIdentity={resetLocalIdentity}");
        var hostInstaller = Path.Combine(bundleRoot, "host-installer.exe");
        if (!File.Exists(hostInstaller))
        {
            throw new InvalidOperationException($"host-installer.exe not found in bundle root: {bundleRoot}");
        }

        SeedPreparedLocalActivationState(bundleRoot, resetLocalIdentity);
        if (resetLocalIdentity)
        {
            ResetManagedPublicRouteSeed(bundleRoot);
        }
        InstallVirtualDisplayDriver(bundleRoot);
        RunPrepareHostForInstall(bundleRoot);
        InstallDisplayBootGuardTask(bundleRoot);
        InstallBundledGamepadDriver(bundleRoot);
        SyncPublicRouteMetadata(bundleRoot);
        SeedPreparedLocalActivationState(bundleRoot, resetLocalIdentity);
        RepairSunshineSharedIfMissing(bundleRoot, releaseRoot);
        RepairPairSeedIfMissing(bundleRoot, releaseRoot);
        RepairCloudgimeRuntimePermissions(bundleRoot);
        InstallKeepAwakeTasks(bundleRoot);
        TryRunHostInstallerCommand(bundleRoot, "install-service", OptionalInstallerActionTimeout);
        RemoveLegacyRuntimeWindowsService();
        ApplyManagedServiceRecoveryPolicies();
        ApplyHostUserDaemonTaskPolicy(bundleRoot);
        TryRunHostInstallerCommand(bundleRoot, "start-service", OptionalInstallerActionTimeout);
        RemoveLegacyRuntimeWindowsService();

        ConfigureWindowsSystemTweaks();
        ConfigureWindowsAutoLogon(autoLogonPassword);

        Log("finalize completed");
    }

    private static void RunPrepareHostForInstall(string bundleRoot)
    {
        try
        {
            RunHostInstaller(bundleRoot, "prepare-host", PrepareInstallTimeout);
        }
        catch (InvalidOperationException ex) when (IsNonFatalPrepareStartupIssue(ex.Message))
        {
            Log($"prepare-host completed with non-fatal startup issue; continuing install. {ex.Message}");
        }
    }

    private static void InstallVirtualDisplayDriver(string bundleRoot)
    {
        var driverRoot = Path.Combine(bundleRoot, "drivers", "virtual-display-driver");
        var controlRoot = Path.Combine(bundleRoot, "drivers", "vdd-control", "Dependencies");
        var devconPath = Path.Combine(controlRoot, "devcon.exe");
        var driverInfPath = Path.Combine(driverRoot, "MttVDD.inf");
        var settingsSourcePath = Path.Combine(driverRoot, "vdd_settings.xml");

        Log($"install-vdd-display bundleRoot={bundleRoot} driverRoot={driverRoot}");
        if (!File.Exists(driverInfPath))
        {
            throw new InvalidOperationException($"MTT VDD driver INF not found: {driverInfPath}");
        }

        if (!File.Exists(devconPath))
        {
            throw new InvalidOperationException($"devcon.exe not found for MTT VDD install: {devconPath}");
        }

        PrepareMttVddRuntimeConfiguration(settingsSourcePath);
        var existingStatus = RunCapturedAndLog(
            devconPath,
            $"status {QuoteArgument(MttVddDeviceId)}",
            controlRoot,
            TimeSpan.FromSeconds(12),
            "MTT VDD fast status before install");
        if (IsMttVddRunning(existingStatus))
        {
            Log("install-vdd-display skipped because MTT VDD is already running");
            return;
        }

        TryRunCapturedAndLog(
            "pnputil.exe",
            $"/add-driver {QuoteArgument(driverInfPath)} /install",
            driverRoot,
            TimeSpan.FromSeconds(60),
            "MTT VDD driver store stage");

        var status = RunCapturedAndLog(
            devconPath,
            $"status {QuoteArgument(MttVddDeviceId)}",
            controlRoot,
            TimeSpan.FromSeconds(20),
            "MTT VDD status before install");

        if (IsMttVddMissing(status))
        {
            var install = RunCapturedAndLog(
                devconPath,
                $"install {QuoteArgument(driverInfPath)} {QuoteArgument(MttVddDeviceId)}",
                controlRoot,
                TimeSpan.FromSeconds(45),
                "MTT VDD devcon install");
            ThrowIfProcessFailed("MTT VDD devcon install", install);
            Thread.Sleep(1500);
        }

        var enable = RunCapturedAndLog(
            devconPath,
            $"enable {QuoteArgument(MttVddDeviceId)}",
            controlRoot,
            TimeSpan.FromSeconds(30),
            "MTT VDD devcon enable");
        if (enable.TimedOut || enable.ExitCode != 0)
        {
            Log($"MTT VDD enable returned non-zero; final status check will decide. exitCode={enable.ExitCode} timedOut={enable.TimedOut}");
        }

        var restart = RunCapturedAndLog(
            devconPath,
            $"restart {QuoteArgument(MttVddDeviceId)}",
            controlRoot,
            TimeSpan.FromSeconds(40),
            "MTT VDD devcon restart");
        if (restart.TimedOut || restart.ExitCode != 0)
        {
            Log($"MTT VDD restart returned non-zero; final status check will decide. exitCode={restart.ExitCode} timedOut={restart.TimedOut}");
        }

        Thread.Sleep(1500);
        var finalStatus = RunCapturedAndLog(
            devconPath,
            $"status {QuoteArgument(MttVddDeviceId)}",
            controlRoot,
            TimeSpan.FromSeconds(20),
            "MTT VDD status after install");

        if (IsMttVddMissing(finalStatus))
        {
            ThrowProcessFailure("MTT VDD final status", finalStatus, "MTT VDD device was not found after install.");
        }

        if (!IsMttVddRunning(finalStatus))
        {
            Log("MTT VDD device was found but not running; retrying one restart.");
            var retryRestart = RunCapturedAndLog(
                devconPath,
                $"restart {QuoteArgument(MttVddDeviceId)}",
                controlRoot,
                TimeSpan.FromSeconds(40),
                "MTT VDD devcon restart retry");
            if (retryRestart.TimedOut || retryRestart.ExitCode != 0)
            {
                Log($"MTT VDD restart retry returned non-zero. exitCode={retryRestart.ExitCode} timedOut={retryRestart.TimedOut}");
            }

            Thread.Sleep(1500);
            finalStatus = RunCapturedAndLog(
                devconPath,
                $"status {QuoteArgument(MttVddDeviceId)}",
                controlRoot,
                TimeSpan.FromSeconds(20),
                "MTT VDD status after restart retry");

            if (!IsMttVddRunning(finalStatus))
            {
                ThrowProcessFailure("MTT VDD final status", finalStatus, "MTT VDD device is installed but the driver is not running.");
            }
        }

        Log("install-vdd-display completed");
    }

    private static void PrepareMttVddRuntimeConfiguration(string settingsSourcePath)
    {
        Directory.CreateDirectory(MttVddRuntimeRoot);
        if (File.Exists(settingsSourcePath))
        {
            File.Copy(settingsSourcePath, Path.Combine(MttVddRuntimeRoot, "vdd_settings.xml"), overwrite: true);
            Log($"MTT VDD settings copied to {MttVddRuntimeRoot}");
        }
        else
        {
            Log($"MTT VDD settings source missing; keeping existing runtime settings. source={settingsSourcePath}");
        }

        using var baseKey = RegistryKey.OpenBaseKey(RegistryHive.LocalMachine, RegistryView.Registry64);
        using var key = baseKey.CreateSubKey(MttVddRegistryPath, writable: true)
            ?? throw new InvalidOperationException($"Could not create MTT VDD registry key: HKLM\\{MttVddRegistryPath}");
        key.SetValue("VDDPATH", MttVddRuntimeRoot, RegistryValueKind.String);
        Log($"MTT VDD registry VDDPATH={MttVddRuntimeRoot}");
    }

    private static bool IsMttVddMissing(ProcessCaptureResult status)
    {
        var output = status.Output ?? string.Empty;
        if (status.TimedOut)
        {
            return true;
        }

        if (output.Contains("No matching devices", StringComparison.OrdinalIgnoreCase)
            || output.Contains("No devices", StringComparison.OrdinalIgnoreCase))
        {
            return true;
        }

        return status.ExitCode != 0 && !output.Contains("MttVDD", StringComparison.OrdinalIgnoreCase);
    }

    private static bool IsMttVddRunning(ProcessCaptureResult status)
    {
        var output = status.Output ?? string.Empty;
        return !status.TimedOut
            && status.ExitCode == 0
            && !IsMttVddMissing(status)
            && output.Contains("Driver is running", StringComparison.OrdinalIgnoreCase);
    }

    private static void TryRunCapturedAndLog(string fileName, string arguments, string workingDirectory, TimeSpan timeout, string actionName)
    {
        try
        {
            _ = RunCapturedAndLog(fileName, arguments, workingDirectory, timeout, actionName);
        }
        catch (Exception ex)
        {
            Log($"{actionName} ignored error: {ex}");
        }
    }

    private static ProcessCaptureResult RunCapturedAndLog(string fileName, string arguments, string workingDirectory, TimeSpan timeout, string actionName)
    {
        var result = StartProcessCaptured(fileName, arguments, workingDirectory, timeout);
        Log($"{actionName} exitCode={result.ExitCode} timedOut={result.TimedOut}");
        if (!string.IsNullOrWhiteSpace(result.Output))
        {
            Log($"{actionName} output:{Environment.NewLine}{result.Output}");
        }

        return result;
    }

    private static void ThrowIfProcessFailed(string actionName, ProcessCaptureResult result)
    {
        if (result.TimedOut || result.ExitCode != 0)
        {
            ThrowProcessFailure(actionName, result, null);
        }
    }

    private static void ThrowProcessFailure(string actionName, ProcessCaptureResult result, string? prefix)
    {
        var summarizedOutput = SummarizeProcessOutput(result.Output);
        var failure = result.TimedOut
            ? $"{actionName} timed out."
            : $"{actionName} failed with exit code {result.ExitCode}.";
        var message = string.IsNullOrWhiteSpace(prefix) ? failure : $"{prefix} {failure}";
        if (!string.IsNullOrWhiteSpace(summarizedOutput))
        {
            message = $"{message}{Environment.NewLine}{summarizedOutput}";
        }

        throw new InvalidOperationException(message);
    }

    private static bool IsNonFatalPrepareStartupIssue(string message)
    {
        var text = message.ToLowerInvariant();
        return (text.Contains("start-bundle", StringComparison.Ordinal) || text.Contains("prepare-host", StringComparison.Ordinal))
            && (text.Contains("bundle startup incomplete", StringComparison.Ordinal)
                || text.Contains("timed out waiting", StringComparison.Ordinal)
                || text.Contains("49000", StringComparison.Ordinal)
                || text.Contains("connection timed out", StringComparison.Ordinal)
                || text.Contains("connection refused", StringComparison.Ordinal)
                || text.Contains("local_http_ready", StringComparison.Ordinal));
    }

    private static void PrepareExistingBundleForInstall(string bundleRoot)
    {
        if (!Directory.Exists(bundleRoot))
        {
            return;
        }

        Log($"prepare-existing-bundle bundleRoot={bundleRoot}");
        TryRunHostInstallerCommand(bundleRoot, "stop-bundle", OptionalInstallerActionTimeout);
        TryRunHostInstallerCommand(bundleRoot, "stop-service", OptionalInstallerActionTimeout);
        StopProcessesWithinRoots(new[] { bundleRoot }, TimeSpan.FromSeconds(20));
    }

    private static void RefreshExistingBundleData(string sourceRoot, string targetRoot)
    {
        Log($"refresh-existing-bundle sourceRoot={sourceRoot} targetRoot={targetRoot}");
        Directory.CreateDirectory(targetRoot);

        foreach (var directory in Directory.GetDirectories(sourceRoot, "*", SearchOption.AllDirectories))
        {
            var relative = Path.GetRelativePath(sourceRoot, directory);
            Directory.CreateDirectory(Path.Combine(targetRoot, relative));
        }

        foreach (var file in Directory.GetFiles(sourceRoot, "*", SearchOption.AllDirectories))
        {
            var relative = Path.GetRelativePath(sourceRoot, file);
            var targetPath = Path.Combine(targetRoot, relative);
            Directory.CreateDirectory(Path.GetDirectoryName(targetPath)!);

            if (ShouldPreserveExistingBundleFile(relative) && File.Exists(targetPath))
            {
                Log($"refresh-existing-bundle preserved {relative}");
                continue;
            }

            File.Copy(file, targetPath, overwrite: true);
        }

        StripLegacyBundleArtifacts(targetRoot);
        ResetManagedPublicRouteSeed(targetRoot);
    }

    private static bool ShouldPreserveExistingBundleFile(string relativePath)
    {
        var normalized = relativePath.Replace('/', '\\');
        if (normalized.StartsWith(@"moonlight\server\sunshine-shared\", StringComparison.OrdinalIgnoreCase))
        {
            return true;
        }

        if (normalized.StartsWith(@"sunshine\config\", StringComparison.OrdinalIgnoreCase)
            || normalized.StartsWith(@"sunshine-legacy\config\", StringComparison.OrdinalIgnoreCase))
        {
            return true;
        }

        return normalized.Equals(@"moonlight\server\audio_dependency_state.json", StringComparison.OrdinalIgnoreCase)
            || normalized.Equals(@"moonlight\server\config.json", StringComparison.OrdinalIgnoreCase)
            || normalized.Equals(@"moonlight\server\data.json", StringComparison.OrdinalIgnoreCase)
            || normalized.Equals(@"moonlight\server\force_legacy_nvenc.txt", StringComparison.OrdinalIgnoreCase)
            || normalized.Equals(@"moonlight\server\host_activation_state.json", StringComparison.OrdinalIgnoreCase)
            || normalized.Equals(@"moonlight\server\host_capability_profile.json", StringComparison.OrdinalIgnoreCase)
            || normalized.Equals(@"moonlight\server\host_control_admin.json", StringComparison.OrdinalIgnoreCase)
            || normalized.Equals(@"moonlight\server\host_self_heal_state.json", StringComparison.OrdinalIgnoreCase)
            || normalized.Equals(@"moonlight\server\host_supervisor.log", StringComparison.OrdinalIgnoreCase)
            || normalized.Equals(@"moonlight\server\host_supervisor_state.json", StringComparison.OrdinalIgnoreCase)
            || normalized.Equals(@"moonlight\server\host-stream-live.log", StringComparison.OrdinalIgnoreCase)
            || normalized.Equals(@"moonlight\server\selected_sunshine_runtime.txt", StringComparison.OrdinalIgnoreCase)
            || normalized.Equals(@"moonlight\server\shared_pair_info.json", StringComparison.OrdinalIgnoreCase);
    }

    private static void StripLegacyBundleArtifacts(string bundleRoot)
    {
        TryDeleteDirectory(Path.Combine(bundleRoot, "frp"));
        TryDeleteFile(Path.Combine(bundleRoot, "start-frp.bat"));
    }

    private static void ResetManagedPublicRouteSeed(string bundleRoot)
    {
        var publicUrlPath = Path.Combine(bundleRoot, "PUBLIC_URL.txt");
        WriteTextFile(publicUrlPath, string.Empty);
    }

    private static void RunHostInstaller(string bundleRoot, string action, TimeSpan timeout)
    {
        var hostInstaller = Path.Combine(bundleRoot, "host-installer.exe");
        var result = StartProcessCaptured(hostInstaller, $"--bundle-root \"{bundleRoot}\" {action}", bundleRoot, timeout);
        Log($"host-installer action={action} exitCode={result.ExitCode} timedOut={result.TimedOut}");
        if (!string.IsNullOrWhiteSpace(result.Output))
        {
            Log($"host-installer action={action} output:{Environment.NewLine}{result.Output}");
        }

        var exitCode = result.ExitCode;
        if (result.TimedOut)
        {
            var summarizedOutput = SummarizeProcessOutput(result.Output);
            if (!string.IsNullOrWhiteSpace(summarizedOutput))
            {
                throw new InvalidOperationException($"{action} timed out after {timeout.TotalSeconds:0} seconds.{Environment.NewLine}{summarizedOutput}");
            }

            throw new InvalidOperationException($"{action} timed out after {timeout.TotalSeconds:0} seconds.");
        }
        if (exitCode != 0)
        {
            var summarizedOutput = SummarizeProcessOutput(result.Output);
            if (!string.IsNullOrWhiteSpace(summarizedOutput))
            {
                throw new InvalidOperationException($"{action} failed with exit code {exitCode}.{Environment.NewLine}{summarizedOutput}");
            }

            throw new InvalidOperationException($"{action} failed with exit code {exitCode}.");
        }
    }

    private static void SyncPublicRouteMetadata(string bundleRoot)
    {
        var configPath = Path.Combine(bundleRoot, "moonlight", "server", "config.json");
        if (!File.Exists(configPath))
        {
            return;
        }

        var config = ReadJsonObject(configPath);
        if (config is null)
        {
            return;
        }

        var pathPrefix = NormalizePathPrefix(GetNestedString(config, "web_server", "url_path_prefix"));
        if (string.IsNullOrWhiteSpace(pathPrefix))
        {
            pathPrefix = "/stream";
        }

        var publicUrlPath = Path.Combine(bundleRoot, "PUBLIC_URL.txt");
        if (File.Exists(publicUrlPath))
        {
            var raw = File.ReadAllText(publicUrlPath).Trim();
            if (Uri.TryCreate(raw, UriKind.Absolute, out var publicUri))
            {
                var baseUrl = publicUri.GetLeftPart(UriPartial.Authority);
                WriteTextFile(publicUrlPath, $"{baseUrl}{pathPrefix}/" + Environment.NewLine);
            }
        }

        foreach (var targetPath in new[]
                 {
                     Path.Combine(bundleRoot, "README.txt"),
                     Path.Combine(bundleRoot, "SETUP.txt"),
                     Path.Combine(bundleRoot, "start-all.bat")
                 })
        {
            if (!File.Exists(targetPath))
            {
                continue;
            }

            var content = File.ReadAllText(targetPath);
            var updated = Regex.Replace(content, @"https://([A-Za-z0-9\.-]+)/moonlight/", $"https://$1{pathPrefix}/");
            updated = Regex.Replace(updated, @"http://127\.0\.0\.1:(\d+)/moonlight/", $"http://127.0.0.1:$1{pathPrefix}/");
            if (!string.Equals(content, updated, StringComparison.Ordinal))
            {
                WriteTextFile(targetPath, updated);
            }
        }
    }

    private static void SeedPreparedLocalActivationState(string bundleRoot, bool forceFreshIdentity = false)
    {
        var serverRoot = Path.Combine(bundleRoot, "moonlight", "server");
        Directory.CreateDirectory(serverRoot);

        var activationPath = Path.Combine(serverRoot, "host_activation_state.json");
        var sharedIdentityPath = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData), "Cloudgime", "pc_identity.json");
        var pendingUninstallPath = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData), "Cloudgime", "pending_uninstall.json");

        var state = ReadJsonObject(activationPath) ?? new JsonObject
        {
            ["SchemaVersion"] = 1,
            ["HostId"] = NewRandomHostId(),
            ["MachineIdentity"] = "",
            ["InstallInstanceId"] = "",
            ["ActivationState"] = "prepared_local",
            ["SetupTokenKind"] = "",
            ["InstanceType"] = "",
            ["ControlPlaneUrl"] = "https://cloudgime.my.id",
            ["DisplayName"] = Environment.MachineName,
            ["SentinelPcId"] = "",
            ["SentinelDeviceId"] = "",
            ["KeeperEntryId"] = "",
            ["RuntimeToken"] = "",
            ["ActivatedAtUtc"] = "",
            ["RedeemedAtUtc"] = "",
            ["ActivationRecordId"] = "",
            ["LastHeartbeatAtUtc"] = "",
            ["LastReadyForStream"] = false,
            ["UpdatedAtUtc"] = ""
        };

        var sharedIdentity = ReadJsonObject(sharedIdentityPath) ?? new JsonObject
        {
            ["schemaVersion"] = 1,
            ["hostId"] = "",
            ["machineIdentity"] = "",
            ["sentinelPcId"] = "",
            ["sentinelDeviceId"] = "",
            ["keeperEntryId"] = "",
            ["updatedAtUtc"] = ""
        };

        state["SchemaVersion"] = 1;
        sharedIdentity["schemaVersion"] = 1;

        if (forceFreshIdentity || ShouldResetPreparedLocalState(state))
        {
            Log(forceFreshIdentity
                ? "seed-prepared-local-activation-state forcing fresh local identity"
                : "seed-prepared-local-activation-state resetting stale activation identity");
            var freshMachineIdentity = NewStableMachineIdentity(string.Empty);
            var freshHostId = NewRandomHostId();
            sharedIdentity["machineIdentity"] = freshMachineIdentity;
            sharedIdentity["hostId"] = freshHostId;
            sharedIdentity["sentinelPcId"] = "";
            sharedIdentity["sentinelDeviceId"] = "";
            sharedIdentity["keeperEntryId"] = "";
            sharedIdentity["updatedAtUtc"] = DateTime.UtcNow.ToString("o");
            state["MachineIdentity"] = freshMachineIdentity;
            state["HostId"] = freshHostId;
            state["InstallInstanceId"] = NewInstallInstanceId();
            state["ActivationState"] = "prepared_local";
            state["SetupTokenKind"] = "";
            state["InstanceType"] = "";
            state["RuntimeToken"] = "";
            state["ActivationRecordId"] = "";
            state["ActivatedAtUtc"] = "";
            state["RedeemedAtUtc"] = "";
            state["LastHeartbeatAtUtc"] = "";
            state["LastReadyForStream"] = false;
            state["SentinelPcId"] = "";
            state["SentinelDeviceId"] = "";
            state["KeeperEntryId"] = "";
            TryDeleteFile(pendingUninstallPath);
        }

        var stableMachineIdentity = ResolveStableMachineIdentity(state, sharedIdentity);
        if (!string.Equals(GetString(sharedIdentity, "machineIdentity"), stableMachineIdentity, StringComparison.OrdinalIgnoreCase))
        {
            sharedIdentity["machineIdentity"] = stableMachineIdentity;
            sharedIdentity["updatedAtUtc"] = DateTime.UtcNow.ToString("o");
        }

        state["MachineIdentity"] = stableMachineIdentity;

        var stableHostId = ResolveStableHostId(state, sharedIdentity);
        if (!string.Equals(GetString(sharedIdentity, "hostId"), stableHostId, StringComparison.OrdinalIgnoreCase))
        {
            sharedIdentity["hostId"] = stableHostId;
            sharedIdentity["updatedAtUtc"] = DateTime.UtcNow.ToString("o");
        }

        state["HostId"] = stableHostId;

        if (string.IsNullOrWhiteSpace(GetString(state, "DisplayName")))
        {
            state["DisplayName"] = Environment.MachineName;
        }

        if (string.IsNullOrWhiteSpace(GetString(state, "ControlPlaneUrl")))
        {
            state["ControlPlaneUrl"] = "https://cloudgime.my.id";
        }

        RepairSetupTokenMetadata(state);

        CopyIfMissing(state, "SentinelPcId", sharedIdentity, "sentinelPcId");
        CopyIfMissing(state, "SentinelDeviceId", sharedIdentity, "sentinelDeviceId");
        CopyIfMissing(state, "KeeperEntryId", sharedIdentity, "keeperEntryId");

        var currentPhase = GetString(state, "ActivationState");
        if (string.IsNullOrWhiteSpace(currentPhase) || string.Equals(currentPhase, "installed_unprepared", StringComparison.OrdinalIgnoreCase))
        {
            state["ActivationState"] = "prepared_local";
        }

        if (!IsValidInstallInstanceId(GetString(state, "InstallInstanceId")))
        {
            state["InstallInstanceId"] = NewInstallInstanceId();
        }

        state["UpdatedAtUtc"] = DateTime.UtcNow.ToString("o");
        WriteJsonObject(sharedIdentityPath, sharedIdentity);
        WriteJsonObject(activationPath, state);
    }

    private static void RepairSetupTokenMetadata(JsonObject state)
    {
        if (!LooksLikeAlwaysOnHostState(state))
        {
            return;
        }

        if (string.IsNullOrWhiteSpace(GetString(state, "SetupTokenKind")))
        {
            state["SetupTokenKind"] = "always_on_host";
        }

        if (string.IsNullOrWhiteSpace(GetString(state, "InstanceType")))
        {
            state["InstanceType"] = "always-on";
        }
    }

    private static bool LooksLikeAlwaysOnHostState(JsonObject state)
    {
        return GetString(state, "HostId").StartsWith("cgslot-", StringComparison.OrdinalIgnoreCase)
            || GetString(state, "ActivationRecordId").StartsWith("cgslot-", StringComparison.OrdinalIgnoreCase)
            || GetString(state, "SetupTokenKind").Equals("always_on_host", StringComparison.OrdinalIgnoreCase)
            || GetString(state, "InstanceType").Equals("always-on", StringComparison.OrdinalIgnoreCase);
    }

    private static void RepairSunshineSharedIfMissing(string bundleRoot, string releaseRoot)
    {
        var managedRoot = Path.Combine(releaseRoot, "managed-sunshine-shared");
        if (!File.Exists(Path.Combine(managedRoot, "sunshine_state.json")))
        {
            return;
        }

        var sharedRoot = Path.Combine(bundleRoot, "moonlight", "server", "sunshine-shared");
        Directory.CreateDirectory(sharedRoot);
        Directory.CreateDirectory(Path.Combine(sharedRoot, "credentials"));

        foreach (var relativePath in new[]
                 {
                     "sunshine_state.json",
                     Path.Combine("credentials", "cacert.pem"),
                     Path.Combine("credentials", "cakey.pem")
                 })
        {
            var sourcePath = Path.Combine(managedRoot, relativePath);
            var targetPath = Path.Combine(sharedRoot, relativePath);
            if (File.Exists(sourcePath) && !File.Exists(targetPath))
            {
                Directory.CreateDirectory(Path.GetDirectoryName(targetPath)!);
                File.Copy(sourcePath, targetPath, overwrite: true);
            }
        }
    }

    private static void RepairPairSeedIfMissing(string bundleRoot, string releaseRoot)
    {
        var seedPath = Path.Combine(releaseRoot, "managed-shared_pair_info.json");
        if (!File.Exists(seedPath))
        {
            return;
        }

        var seed = ReadJsonObject(seedPath);
        var pairInfo = seed?["hosts"]?[0]?["pair_info"];
        if (seed is null || pairInfo is null)
        {
            return;
        }

        var serverRoot = Path.Combine(bundleRoot, "moonlight", "server");
        var pairInfoPath = Path.Combine(serverRoot, "shared_pair_info.json");
        var dataPath = Path.Combine(serverRoot, "data.json");

        var existingSeed = ReadJsonObject(pairInfoPath);
        if (existingSeed?["hosts"]?[0]?["pair_info"] is null)
        {
            WriteJsonObject(pairInfoPath, seed);
            File.AppendAllText(pairInfoPath, Environment.NewLine, Utf8NoBom);
        }

        var data = ReadJsonObject(dataPath);
        if (data?["hosts"] is not JsonObject hosts)
        {
            return;
        }

        var changed = false;
        foreach (var entry in hosts.ToList())
        {
            if (entry.Value is not JsonObject host || host["pair_info"] is not null)
            {
                continue;
            }

            host["pair_info"] = pairInfo.DeepClone();
            changed = true;
        }

        if (changed)
        {
            WriteJsonObject(dataPath, data);
            File.AppendAllText(dataPath, Environment.NewLine, Utf8NoBom);
        }
    }

    private static void RepairCloudgimeRuntimePermissions(string bundleRoot)
    {
        var cloudgimeRoot = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData), "Cloudgime");
        var serverRoot = Path.Combine(bundleRoot, "moonlight", "server");

        GrantUsersModifyOnDirectChildFiles(cloudgimeRoot);
        GrantUsersModifyOnDirectChildFiles(serverRoot);

        foreach (var filePath in new[]
                 {
                     Path.Combine(cloudgimeRoot, "pc_identity.json"),
                     Path.Combine(cloudgimeRoot, "pending_uninstall.json"),
                     Path.Combine(serverRoot, "host_activation_state.json"),
                     Path.Combine(serverRoot, "host_control_admin.json")
                 })
        {
            GrantUsersModifyOnFile(filePath);
        }
    }

    private static void GrantUsersModifyOnFile(string path)
    {
        if (!File.Exists(path))
        {
            return;
        }

        var fileInfo = new FileInfo(path);
        var security = fileInfo.GetAccessControl();
        var usersSid = new SecurityIdentifier(WellKnownSidType.BuiltinUsersSid, null);
        var rule = new FileSystemAccessRule(
            usersSid,
            FileSystemRights.Modify,
            InheritanceFlags.None,
            PropagationFlags.None,
            AccessControlType.Allow);
        security.SetAccessRule(rule);
        fileInfo.SetAccessControl(security);
    }

    private static void GrantUsersModifyOnDirectChildFiles(string directoryPath)
    {
        if (!Directory.Exists(directoryPath))
        {
            return;
        }

        var directoryInfo = new DirectoryInfo(directoryPath);
        var security = directoryInfo.GetAccessControl();
        var usersSid = new SecurityIdentifier(WellKnownSidType.BuiltinUsersSid, null);
        var rule = new FileSystemAccessRule(
            usersSid,
            FileSystemRights.Modify,
            InheritanceFlags.ObjectInherit,
            PropagationFlags.InheritOnly,
            AccessControlType.Allow);
        security.SetAccessRule(rule);
        directoryInfo.SetAccessControl(security);
    }

    private static void WriteInstallLayout(string installRoot, string dataRoot, string productName)
    {
        var layout = new JsonObject
        {
            ["schemaVersion"] = 1,
            ["installRoot"] = installRoot,
            ["bundleRoot"] = dataRoot,
            ["productName"] = productName,
            ["uninstallRegistryKey"] = $@"HKLM\{DefaultUninstallRegistryKey}",
            ["appInstallerProductCode"] = "",
            ["appInstallerRegistryPath"] = "",
            ["appExecutableName"] = "cloudgime-host-control.exe",
            ["updatedAtUtc"] = DateTime.UtcNow.ToString("o")
        };

        WriteJsonObject(Path.Combine(installRoot, "install-layout.json"), layout);
    }

    private static void WriteUninstallRegistration(string installRoot, string dataRoot, string appExePath, string productName)
    {
        using var baseKey = RegistryKey.OpenBaseKey(RegistryHive.LocalMachine, RegistryView.Registry64);
        using var uninstallKey = baseKey.CreateSubKey(DefaultUninstallRegistryKey, writable: true)
            ?? throw new InvalidOperationException("Could not create uninstall registry key.");

        var emergencyUninstallerPath = Path.Combine(installRoot, "uninstaller-cloudgime.exe");
        var uninstallString = File.Exists(emergencyUninstallerPath)
            ? $"\"{emergencyUninstallerPath}\" uninstall --install-root \"{installRoot}\" --bundle-root \"{dataRoot}\""
            : $"\"{appExePath}\" --bundle-root \"{dataRoot}\" --intent uninstall";
        var quietUninstallString = File.Exists(emergencyUninstallerPath)
            ? $"{uninstallString} --silent"
            : uninstallString;
        uninstallKey.SetValue("DisplayName", productName, RegistryValueKind.String);
        uninstallKey.SetValue("DisplayVersion", "0.1.0", RegistryValueKind.String);
        uninstallKey.SetValue("Publisher", "Cloudgime", RegistryValueKind.String);
        uninstallKey.SetValue("InstallLocation", installRoot, RegistryValueKind.String);
        uninstallKey.SetValue("DisplayIcon", appExePath, RegistryValueKind.String);
        uninstallKey.SetValue("UninstallString", uninstallString, RegistryValueKind.String);
        uninstallKey.SetValue("QuietUninstallString", quietUninstallString, RegistryValueKind.String);
        uninstallKey.SetValue("NoModify", 1, RegistryValueKind.DWord);
        uninstallKey.SetValue("NoRepair", 1, RegistryValueKind.DWord);
    }

    private static void RemoveLegacyAppInstallerShortcuts(string appProductName)
    {
        foreach (var path in new[]
                 {
                     Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.CommonStartMenu), "Programs", $"{appProductName}.lnk"),
                     Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.StartMenu), "Programs", $"{appProductName}.lnk"),
                     Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.CommonDesktopDirectory), $"{appProductName}.lnk"),
                     Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.DesktopDirectory), $"{appProductName}.lnk")
                 })
        {
            if (File.Exists(path))
            {
                File.Delete(path);
            }
        }
    }

    private static void WriteStartMenuAndDesktopShortcuts(string dataRoot, string appExePath, string productName)
    {
        var commonPrograms = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.CommonStartMenu), "Programs");
        Directory.CreateDirectory(commonPrograms);

        var startMenuShortcutPath = Path.Combine(commonPrograms, $"{productName}.lnk");
        var desktopShortcutPath = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.CommonDesktopDirectory), $"{productName}.lnk");
        var arguments = $"--bundle-root \"{dataRoot}\"";

        CreateShortcut(startMenuShortcutPath, appExePath, arguments, dataRoot, appExePath, productName);
        CreateShortcut(desktopShortcutPath, appExePath, arguments, dataRoot, appExePath, productName);
    }

    private static void RemoveLegacyOpenHostControlShortcutScripts(string installRoot)
    {
        var openAppCmd = Path.Combine(installRoot, "open-host-control.cmd");
        var openFolderCmd = Path.Combine(installRoot, "open-host-control-folder.cmd");

        if (File.Exists(openAppCmd))
        {
            File.Delete(openAppCmd);
        }

        if (File.Exists(openFolderCmd))
        {
            File.Delete(openFolderCmd);
        }
    }

    private static void EnsurePackageSignerTrusted(string installerPath, string packageRoot)
    {
        var candidates = new[]
        {
            Path.GetExtension(installerPath).Equals(".cat", StringComparison.OrdinalIgnoreCase) ? installerPath : null,
            Directory.Exists(packageRoot)
                ? Directory.GetFiles(packageRoot, "*.cat", SearchOption.AllDirectories).FirstOrDefault()
                : null
        }
        .Where(static value => !string.IsNullOrWhiteSpace(value))
        .Select(static value => value!)
        .Distinct(StringComparer.OrdinalIgnoreCase);

        foreach (var catalogPath in candidates)
        {
            try
            {
                using var signer = new X509Certificate2(X509Certificate.CreateFromSignedFile(catalogPath));
                AddCertificateIfMissing(StoreName.Root, signer);
                AddCertificateIfMissing(StoreName.TrustedPublisher, signer);
                return;
            }
            catch
            {
            }
        }
    }

    private static void AddCertificateIfMissing(StoreName storeName, X509Certificate2 certificate)
    {
        using var store = new X509Store(storeName, StoreLocation.LocalMachine);
        store.Open(OpenFlags.ReadWrite);
        if (store.Certificates.Find(X509FindType.FindByThumbprint, certificate.Thumbprint, validOnly: false).Count == 0)
        {
            store.Add(certificate);
        }
    }

    private static void VerifyExpectedAudioEndpoints(FallbackAudioManifest manifest)
    {
        if (manifest.ExpectedAudioEndpoints.Count == 0)
        {
            return;
        }

        var timeout = Math.Max(5, manifest.VerifyTimeoutSeconds);
        var pollInterval = Math.Max(1, manifest.VerifyPollIntervalSeconds);
        var deadline = DateTime.UtcNow.AddSeconds(timeout);
        List<string> missing = [];

        do
        {
            var detected = GetAudioEndpoints();
            missing = manifest.ExpectedAudioEndpoints
                .Where(expected => !detected.Any(actual =>
                    string.Equals(actual.Direction, expected.Direction ?? string.Empty, StringComparison.OrdinalIgnoreCase)
                    && string.Equals(actual.Name, expected.Name ?? string.Empty, StringComparison.Ordinal)))
                .Select(expected => $"{expected.Direction}:{expected.Name}")
                .ToList();

            if (missing.Count == 0)
            {
                return;
            }

            Thread.Sleep(TimeSpan.FromSeconds(pollInterval));
        } while (DateTime.UtcNow < deadline);

        throw new InvalidOperationException($"Fallback audio install completed but expected endpoints are missing: {string.Join(", ", missing)}");
    }

    private static List<AudioEndpointInfo> GetAudioEndpoints()
    {
        using var searcher = new ManagementObjectSearcher("SELECT Name FROM Win32_SoundDevice");
        var endpoints = new List<AudioEndpointInfo>();
        foreach (var item in searcher.Get().Cast<ManagementObject>())
        {
            var driverName = item["Name"]?.ToString();
            if (string.IsNullOrWhiteSpace(driverName))
            {
                continue;
            }

            endpoints.AddRange(ExpandDriverNameToEndpoints(driverName));
        }

        return endpoints
            .GroupBy(static endpoint => $"{endpoint.Direction}|{endpoint.Name}", StringComparer.OrdinalIgnoreCase)
            .Select(static group => group.First())
            .ToList();
    }

    private static IEnumerable<AudioEndpointInfo> ExpandDriverNameToEndpoints(string driverName)
    {
        if (driverName.Contains("VB-Audio Cable A", StringComparison.OrdinalIgnoreCase))
        {
            yield return new AudioEndpointInfo("output", "CABLE-A Input (VB-Audio Cable A)");
            yield return new AudioEndpointInfo("input", "CABLE-A Output (VB-Audio Cable A)");
            yield break;
        }

        if (driverName.Contains("VB-Audio Cable B", StringComparison.OrdinalIgnoreCase))
        {
            yield return new AudioEndpointInfo("output", "CABLE-B Input (VB-Audio Cable B)");
            yield return new AudioEndpointInfo("input", "CABLE-B Output (VB-Audio Cable B)");
            yield break;
        }

        if (driverName.Contains("VB-Audio Virtual Cable", StringComparison.OrdinalIgnoreCase))
        {
            yield return new AudioEndpointInfo("output", "CABLE Input (VB-Audio Virtual Cable)");
            yield return new AudioEndpointInfo("input", "CABLE Output (VB-Audio Virtual Cable)");
            yield break;
        }

        if (driverName.Contains("Steam Streaming Speakers", StringComparison.OrdinalIgnoreCase))
        {
            yield return new AudioEndpointInfo("output", "Speakers (Steam Streaming Speakers)");
            yield break;
        }

        if (driverName.Contains("Virtual Speakers for AudioRelay", StringComparison.OrdinalIgnoreCase))
        {
            yield return new AudioEndpointInfo("output", "Virtual Speakers for AudioRelay");
            yield break;
        }

        if (driverName.Contains("Virtual Mic for AudioRelay", StringComparison.OrdinalIgnoreCase))
        {
            yield return new AudioEndpointInfo("input", "Virtual Mic for AudioRelay");
            yield break;
        }

        if (driverName.Contains("SYMO Virtual Audio Output", StringComparison.OrdinalIgnoreCase))
        {
            yield return new AudioEndpointInfo("output", "SYMO Virtual Audio Output");
            yield break;
        }

        if (driverName.Contains("SYMO Virtual Audio Input", StringComparison.OrdinalIgnoreCase))
        {
            yield return new AudioEndpointInfo("input", "SYMO Virtual Audio Input");
            yield break;
        }

        if (driverName.Contains("Virtual Audio Driver by MTT", StringComparison.OrdinalIgnoreCase))
        {
            yield return new AudioEndpointInfo("output", "Virtual Audio Driver by MTT");
            yield break;
        }

        if (driverName.Contains("Virtual Mic Driver by MTT", StringComparison.OrdinalIgnoreCase))
        {
            yield return new AudioEndpointInfo("input", "Virtual Mic Driver by MTT");
            yield break;
        }

        if (driverName.Contains("Virtual Audio Driver Output", StringComparison.OrdinalIgnoreCase))
        {
            yield return new AudioEndpointInfo("output", "Virtual Audio Driver Output");
            yield break;
        }

        if (driverName.Contains("Virtual Audio Driver Input", StringComparison.OrdinalIgnoreCase))
        {
            yield return new AudioEndpointInfo("input", "Virtual Audio Driver Input");
            yield break;
        }

        if (driverName.Contains("Virtual Audio Driver", StringComparison.OrdinalIgnoreCase))
        {
            yield return new AudioEndpointInfo("output", "Virtual Audio Driver Output");
            yield return new AudioEndpointInfo("input", "Virtual Audio Driver Input");
        }
    }

    private static ProcessCommand? ResolveVigemUninstallCommand()
    {
        foreach (var subKeyPath in new[]
                 {
                     @"Software\Microsoft\Windows\CurrentVersion\Uninstall",
                     @"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"
                 })
        {
            using var hive = RegistryKey.OpenBaseKey(RegistryHive.LocalMachine, RegistryView.Registry64);
            using var root = hive.OpenSubKey(subKeyPath, writable: false);
            if (root is null)
            {
                continue;
            }

            foreach (var childName in root.GetSubKeyNames())
            {
                using var child = root.OpenSubKey(childName, writable: false);
                if (child is null)
                {
                    continue;
                }

                var displayName = child.GetValue("DisplayName")?.ToString() ?? string.Empty;
                if (!displayName.Contains("ViGEm Bus Driver", StringComparison.OrdinalIgnoreCase))
                {
                    continue;
                }

                if (childName.StartsWith('{') && childName.EndsWith('}'))
                {
                    return new ProcessCommand("msiexec.exe", $"/x {QuoteArgument(childName)} /qn /norestart");
                }

                var quietUninstall = child.GetValue("QuietUninstallString")?.ToString();
                if (!string.IsNullOrWhiteSpace(quietUninstall))
                {
                    return new ProcessCommand("cmd.exe", $"/c {quietUninstall}");
                }

                var uninstallString = child.GetValue("UninstallString")?.ToString();
                if (!string.IsNullOrWhiteSpace(uninstallString))
                {
                    return new ProcessCommand("cmd.exe", $"/c {uninstallString}");
                }
            }
        }

        return null;
    }

    private static string JoinQuotedArguments(IEnumerable<string>? arguments) =>
        arguments is null
            ? string.Empty
            : string.Join(" ", arguments.Where(static value => !string.IsNullOrWhiteSpace(value)).Select(QuoteArgument));

    private static void RunChecked(string fileName, string arguments, string workingDirectory, string actionName)
    {
        var exitCode = StartProcessHidden(fileName, arguments, workingDirectory, waitForExit: true);
        if (exitCode != 0)
        {
            throw new InvalidOperationException($"{actionName} failed with exit code {exitCode}.");
        }
    }

    private static int ParseIntArgument(string? value, string name)
    {
        if (int.TryParse(value, out var parsed))
        {
            return parsed;
        }

        throw new InvalidOperationException($"--{name} must be a valid integer.");
    }

    private static void WaitForProcessExit(int processId, TimeSpan timeout)
    {
        if (processId <= 0)
        {
            return;
        }

        try
        {
            using var process = Process.GetProcessById(processId);
            if (process.HasExited)
            {
                return;
            }

            process.WaitForExit((int)timeout.TotalMilliseconds);
        }
        catch
        {
        }
    }

    private static void TryRunHostInstallerCommand(string bundleRoot, string action, TimeSpan timeout)
    {
        try
        {
            var hostInstaller = Path.Combine(bundleRoot, "host-installer.exe");
            if (!File.Exists(hostInstaller))
            {
                Log($"host-installer action={action} skipped because binary is missing");
                return;
            }

            var result = StartProcessHiddenWithTimeout(hostInstaller, $"--bundle-root \"{bundleRoot}\" {action}", bundleRoot, timeout);
            Log($"host-installer optional action={action} exitCode={result.ExitCode} timedOut={result.TimedOut}");
            if (!string.IsNullOrWhiteSpace(result.Output))
            {
                Log($"host-installer optional action={action} output:{Environment.NewLine}{result.Output}");
            }
        }
        catch (Exception ex)
        {
            Log($"host-installer optional action={action} ignored error: {ex}");
        }
    }

    private static void ApplyManagedServiceRecoveryPolicies()
    {
        TryApplyServiceRecoveryPolicy(
            HostWindowsServiceName,
            delayedAutoStart: false,
            firstRestartDelay: TimeSpan.FromMinutes(1),
            secondRestartDelay: TimeSpan.FromMinutes(1),
            thirdRestartDelay: TimeSpan.FromMinutes(2));
    }

    private static void RemoveLegacyRuntimeWindowsService()
    {
        try
        {
            if (!WindowsServiceExists(RuntimeWindowsServiceName))
            {
                return;
            }

            var stopResult = StartProcessCaptured(
                "sc.exe",
                $"stop \"{RuntimeWindowsServiceName}\"",
                Environment.SystemDirectory,
                TimeSpan.FromSeconds(15));
            Log($"legacy runtime service stop exitCode={stopResult.ExitCode} timedOut={stopResult.TimedOut}");
            if (!string.IsNullOrWhiteSpace(stopResult.Output))
            {
                Log($"legacy runtime service stop output:{Environment.NewLine}{stopResult.Output}");
            }

            RunScCommand(
                $"delete \"{RuntimeWindowsServiceName}\"",
                $"Delete legacy runtime service {RuntimeWindowsServiceName}");
            Log($"legacy runtime service removed: {RuntimeWindowsServiceName}");
        }
        catch (Exception ex)
        {
            Log($"legacy runtime service cleanup ignored for {RuntimeWindowsServiceName}: {ex}");
        }
    }

    private static void TryApplyServiceRecoveryPolicy(
        string serviceName,
        bool delayedAutoStart,
        TimeSpan firstRestartDelay,
        TimeSpan secondRestartDelay,
        TimeSpan thirdRestartDelay)
    {
        try
        {
            if (!WindowsServiceExists(serviceName))
            {
                Log($"service recovery skipped because service is missing: {serviceName}");
                return;
            }

            RunScCommand(
                $"config \"{serviceName}\" start= {(delayedAutoStart ? "delayed-auto" : "auto")}",
                $"Configure {(delayedAutoStart ? "delayed" : "immediate")} auto-start for {serviceName}");

            RunScCommand(
                $"failure \"{serviceName}\" reset= 86400 actions= restart/{(int)firstRestartDelay.TotalMilliseconds}/restart/{(int)secondRestartDelay.TotalMilliseconds}/restart/{(int)thirdRestartDelay.TotalMilliseconds}",
                $"Configure recovery actions for {serviceName}");
            RunScCommand(
                $"failureflag \"{serviceName}\" 1",
                $"Enable failure flag for {serviceName}");
            Log($"service recovery policy applied for {serviceName}");
        }
        catch (Exception ex)
        {
            Log($"service recovery policy ignored for {serviceName}: {ex}");
        }
    }

    private static bool WindowsServiceExists(string serviceName)
    {
        var result = StartProcessCaptured(
            "sc.exe",
            $"query \"{serviceName}\"",
            Environment.SystemDirectory,
            TimeSpan.FromSeconds(15));
        Log($"service existence query name={serviceName} exitCode={result.ExitCode} timedOut={result.TimedOut}");
        if (!string.IsNullOrWhiteSpace(result.Output))
        {
            Log($"service existence query output {serviceName}:{Environment.NewLine}{result.Output}");
        }

        return !result.TimedOut && result.ExitCode == 0;
    }

    private static void RunScCommand(string arguments, string operationName)
    {
        var result = StartProcessCaptured("sc.exe", arguments, Environment.SystemDirectory, TimeSpan.FromSeconds(20));
        Log($"sc {operationName} exitCode={result.ExitCode} timedOut={result.TimedOut}");
        if (!string.IsNullOrWhiteSpace(result.Output))
        {
            Log($"sc {operationName} output:{Environment.NewLine}{result.Output}");
        }

        if (result.TimedOut)
        {
            throw new InvalidOperationException($"{operationName} timed out.");
        }

        if (result.ExitCode != 0)
        {
            throw new InvalidOperationException($"{operationName} failed with exit code {result.ExitCode}.");
        }
    }

    private static void ApplyHostUserDaemonTaskPolicy(string bundleRoot)
    {
        try
        {
            var daemonPath = Path.Combine(bundleRoot, "moonlight", "system", "cloudgime-runtime-agent.exe");
            var normalizedBundleRoot = bundleRoot.Replace("'", "''");
            var normalizedDaemonPath = daemonPath.Replace("'", "''");
            var taskName = HostUserDaemonTaskName.Replace("'", "''");
            var script = $@"
$ErrorActionPreference = 'Stop'
$taskName = '{taskName}'
$bundleRoot = '{normalizedBundleRoot}'
$daemonPath = '{normalizedDaemonPath}'
$healthPath = Join-Path $bundleRoot 'moonlight\server\host_user_daemon_task_health.json'
$action = New-ScheduledTaskAction -Execute $daemonPath -Argument ('--bundle-root ""' + $bundleRoot + '"" run-daemon')
$startupTrigger = New-ScheduledTaskTrigger -AtStartup
$logonTrigger = New-ScheduledTaskTrigger -AtLogOn
$principal = New-ScheduledTaskPrincipal -UserId 'SYSTEM' -RunLevel Highest
$settingsSeed = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable -ExecutionTimeLimit (New-TimeSpan -Seconds 0) -MultipleInstances IgnoreNew
Register-ScheduledTask -TaskName $taskName -Action $action -Trigger @($startupTrigger, $logonTrigger) -Principal $principal -Settings $settingsSeed -Force | Out-Null
Write-Output 'task-policy-system-startup'
$xmlRaw = & schtasks.exe /Query /TN $taskName /XML 2>$null | Out-String
if ([string]::IsNullOrWhiteSpace($xmlRaw)) {{
    throw 'Scheduled task was created but could not be queried back from Task Scheduler.'
}}
[xml]$taskXml = $xmlRaw
$ns = New-Object System.Xml.XmlNamespaceManager($taskXml.NameTable)
$ns.AddNamespace('t', 'http://schemas.microsoft.com/windows/2004/02/mit/task')
$settings = $taskXml.SelectSingleNode('/t:Task/t:Settings', $ns)
if ($null -eq $settings) {{
    throw 'Settings node missing in scheduled task XML.'
}}
function New-TaskNode([string]$name, [string]$value) {{
    $node = $taskXml.CreateElement($name, $taskXml.DocumentElement.NamespaceURI)
    $node.InnerText = $value
    return $node
}}
function New-RestartOnFailureNode([string]$count, [string]$interval) {{
    $node = $taskXml.CreateElement('RestartOnFailure', $taskXml.DocumentElement.NamespaceURI)
    $countNode = $taskXml.CreateElement('Count', $taskXml.DocumentElement.NamespaceURI)
    $countNode.InnerText = $count
    [void]$node.AppendChild($countNode)
    $intervalNode = $taskXml.CreateElement('Interval', $taskXml.DocumentElement.NamespaceURI)
    $intervalNode.InnerText = $interval
    [void]$node.AppendChild($intervalNode)
    return $node
}}
$taskXml.DocumentElement.SetAttribute('version', '1.3')
while ($settings.HasChildNodes) {{
    [void]$settings.RemoveChild($settings.FirstChild)
}}
[void]$settings.AppendChild((New-TaskNode 'DisallowStartIfOnBatteries' 'false'))
[void]$settings.AppendChild((New-TaskNode 'StopIfGoingOnBatteries' 'false'))
[void]$settings.AppendChild((New-TaskNode 'ExecutionTimeLimit' 'PT0S'))
[void]$settings.AppendChild((New-TaskNode 'Hidden' 'true'))
[void]$settings.AppendChild((New-TaskNode 'MultipleInstancesPolicy' 'StopExisting'))
[void]$settings.AppendChild((New-RestartOnFailureNode '999' 'PT1M'))
[void]$settings.AppendChild((New-TaskNode 'StartWhenAvailable' 'true'))
[void]$settings.AppendChild((New-TaskNode 'UseUnifiedSchedulingEngine' 'true'))
[void]$settings.AppendChild((New-TaskNode 'Priority' '4'))
$tempXml = Join-Path $env:TEMP ('cloudgime-host-user-task-' + [guid]::NewGuid().ToString('N') + '.xml')
try {{
    $taskXml.Save($tempXml)
    & schtasks.exe /Create /TN $taskName /XML $tempXml /F | Out-Null
}} finally {{
    Remove-Item $tempXml -Force -ErrorAction SilentlyContinue
}}
$task = Get-ScheduledTask -TaskName $taskName -ErrorAction Stop
$daemon = Get-CimInstance Win32_Process | Where-Object {{
    $_.ExecutablePath -eq $daemonPath -and
    $_.CommandLine -like '* run-daemon*'
}} | Select-Object -First 1
$taskInfo = Get-ScheduledTaskInfo -TaskName $taskName
if ($taskInfo.LastTaskResult -eq 267009) {{
    Write-Output 'task-last-result-running'
}}
if ($null -eq $daemon) {{
    if ($task.State -eq 'Running') {{
        Stop-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue
        Start-Sleep -Seconds 2
    }}
    Start-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue
    Write-Output 'task-daemon-start-requested'
}}
function Get-TaskTextValue($parent, [string]$xpath, [System.Xml.XmlNamespaceManager]$xmlNs) {{
    $node = $parent.SelectSingleNode($xpath, $xmlNs)
    if ($null -eq $node) {{
        return ''
    }}
    return [string]$node.InnerText
}}
$exportRaw = Export-ScheduledTask -TaskName $taskName
[xml]$exportXml = $exportRaw
$exportNs = New-Object System.Xml.XmlNamespaceManager($exportXml.NameTable)
$exportNs.AddNamespace('t', 'http://schemas.microsoft.com/windows/2004/02/mit/task')
$exportSettings = $exportXml.SelectSingleNode('/t:Task/t:Settings', $exportNs)
$task = Get-ScheduledTask -TaskName $taskName -ErrorAction Stop
$taskInfo = Get-ScheduledTaskInfo -TaskName $taskName -ErrorAction SilentlyContinue
$daemon = Get-CimInstance Win32_Process | Where-Object {{
    $_.ExecutablePath -eq $daemonPath -and
    $_.CommandLine -like '* run-daemon*'
}} | Select-Object -First 1
$issues = New-Object System.Collections.Generic.List[string]
$multipleInstances = Get-TaskTextValue $exportSettings 't:MultipleInstancesPolicy' $exportNs
$restartCount = Get-TaskTextValue $exportSettings 't:RestartOnFailure/t:Count' $exportNs
$restartInterval = Get-TaskTextValue $exportSettings 't:RestartOnFailure/t:Interval' $exportNs
$executionTimeLimit = Get-TaskTextValue $exportSettings 't:ExecutionTimeLimit' $exportNs
$startWhenAvailable = Get-TaskTextValue $exportSettings 't:StartWhenAvailable' $exportNs
$hidden = Get-TaskTextValue $exportSettings 't:Hidden' $exportNs
$disallowBattery = Get-TaskTextValue $exportSettings 't:DisallowStartIfOnBatteries' $exportNs
$stopBattery = Get-TaskTextValue $exportSettings 't:StopIfGoingOnBatteries' $exportNs
$useUnifiedSchedulingEngine = Get-TaskTextValue $exportSettings 't:UseUnifiedSchedulingEngine' $exportNs
$idleStopOnIdleEnd = Get-TaskTextValue $exportSettings 't:IdleSettings/t:StopOnIdleEnd' $exportNs
$idleRestartOnIdle = Get-TaskTextValue $exportSettings 't:IdleSettings/t:RestartOnIdle' $exportNs
if ($multipleInstances -ne 'StopExisting') {{ [void]$issues.Add(""multiple_instances:$multipleInstances"") }}
if ($restartCount -ne '999') {{ [void]$issues.Add(""restart_count:$restartCount"") }}
if ($restartInterval -ne 'PT1M') {{ [void]$issues.Add(""restart_interval:$restartInterval"") }}
if ($executionTimeLimit -ne 'PT0S') {{ [void]$issues.Add(""execution_time_limit:$executionTimeLimit"") }}
if ($startWhenAvailable -ne 'true') {{ [void]$issues.Add(""start_when_available:$startWhenAvailable"") }}
if ($hidden -ne 'true') {{ [void]$issues.Add(""hidden:$hidden"") }}
if ($disallowBattery -ne 'false') {{ [void]$issues.Add(""disallow_start_if_on_batteries:$disallowBattery"") }}
if ($stopBattery -ne 'false') {{ [void]$issues.Add(""stop_if_going_on_batteries:$stopBattery"") }}
if ($useUnifiedSchedulingEngine -ne 'true') {{ [void]$issues.Add(""use_unified_scheduling_engine:$useUnifiedSchedulingEngine"") }}
$health = [ordered]@{{
    schemaVersion = 1
    taskName = $taskName
    bundleRoot = $bundleRoot
    daemonPath = $daemonPath
    checkedAtUtc = [DateTime]::UtcNow.ToString('o')
    policyValid = ($issues.Count -eq 0)
    taskState = if ($null -ne $task) {{ [string]$task.State }} else {{ '' }}
    lastTaskResult = if ($null -ne $taskInfo) {{ [int]$taskInfo.LastTaskResult }} else {{ 0 }}
    lastRunTimeUtc = if ($null -ne $taskInfo -and $taskInfo.LastRunTime -is [DateTime]) {{ $taskInfo.LastRunTime.ToUniversalTime().ToString('o') }} else {{ '' }}
    daemonRunning = ($null -ne $daemon)
    daemonPid = if ($null -ne $daemon) {{ [int]$daemon.ProcessId }} else {{ 0 }}
    taskSettings = [ordered]@{{
        multipleInstancesPolicy = $multipleInstances
        restartCount = $restartCount
        restartInterval = $restartInterval
        executionTimeLimit = $executionTimeLimit
        startWhenAvailable = $startWhenAvailable
        hidden = $hidden
        disallowStartIfOnBatteries = $disallowBattery
        stopIfGoingOnBatteries = $stopBattery
        useUnifiedSchedulingEngine = $useUnifiedSchedulingEngine
        idleStopOnIdleEnd = $idleStopOnIdleEnd
        idleRestartOnIdle = $idleRestartOnIdle
    }}
    issues = @($issues)
}}
$healthDir = Split-Path -Parent $healthPath
if (-not (Test-Path $healthDir)) {{
    New-Item -ItemType Directory -Path $healthDir -Force | Out-Null
}}
$health | ConvertTo-Json -Depth 8 | Set-Content -Path $healthPath -Encoding UTF8
if ($issues.Count -gt 0) {{
    throw ('Host user-daemon task policy validation failed: ' + ($issues -join ', '))
}}
Write-Output 'task-policy-applied'
";

            var encodedScript = Convert.ToBase64String(Encoding.Unicode.GetBytes(script));
            var result = StartProcessCaptured(
                "powershell.exe",
                $"-NoProfile -ExecutionPolicy Bypass -EncodedCommand {encodedScript}",
                Environment.SystemDirectory,
                TimeSpan.FromSeconds(45));

            Log($"host-user-daemon task policy exitCode={result.ExitCode} timedOut={result.TimedOut}");
            if (!string.IsNullOrWhiteSpace(result.Output))
            {
                Log($"host-user-daemon task policy output:{Environment.NewLine}{result.Output}");
            }

            if (result.TimedOut)
            {
                throw new InvalidOperationException("Host user-daemon task policy timed out.");
            }

            if (result.ExitCode != 0)
            {
                throw new InvalidOperationException($"Host user-daemon task policy failed with exit code {result.ExitCode}.");
            }
        }
        catch (Exception ex)
        {
            Log($"host user-daemon task policy failed: {ex}");
            throw new InvalidOperationException("Failed to harden and validate the host user-daemon scheduled task.", ex);
        }
    }

    private static void InstallKeepAwakeTasks(string bundleRoot)
    {
        try
        {
            var agentPath = ResolveKeepAwakeAgentPath(bundleRoot);
            if (!File.Exists(agentPath))
            {
                Log($"keep-awake task skipped because agent is missing: {agentPath}");
                return;
            }

            var systemArguments = $"--bundle-root {QuoteArgument(bundleRoot)} --mode system --no-nudge";
            var userArguments = $"--bundle-root {QuoteArgument(bundleRoot)} --mode user --allow-nudge";
            var script = $@"
$ErrorActionPreference = 'Stop'
$systemTask = '{EscapePowerShellSingleQuoted(HostKeepAwakeSystemTaskName)}'
$userTask = '{EscapePowerShellSingleQuoted(HostKeepAwakeUserTaskName)}'
$exe = '{EscapePowerShellSingleQuoted(agentPath)}'
$systemArguments = '{EscapePowerShellSingleQuoted(systemArguments)}'
$userArguments = '{EscapePowerShellSingleQuoted(userArguments)}'

function New-KeepAwakeSettings {{
    $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable -MultipleInstances IgnoreNew -ExecutionTimeLimit (New-TimeSpan -Seconds 0)
    try {{ $settings.Hidden = $true }} catch {{ }}
    return $settings
}}

$systemAction = New-ScheduledTaskAction -Execute $exe -Argument $systemArguments
$systemTrigger = New-ScheduledTaskTrigger -AtStartup
$systemPrincipal = New-ScheduledTaskPrincipal -UserId 'SYSTEM' -RunLevel Highest
Register-ScheduledTask -TaskName $systemTask -Action $systemAction -Trigger $systemTrigger -Principal $systemPrincipal -Settings (New-KeepAwakeSettings) -Description 'Keeps Cloudgime Host awake and sends a light heartbeat.' -Force | Out-Null
try {{ Start-ScheduledTask -TaskName $systemTask }} catch {{ Write-Output ('system-task-start-skipped: ' + $_.Exception.Message) }}

try {{
    $usersGroup = ([System.Security.Principal.SecurityIdentifier]'S-1-5-32-545').Translate([System.Security.Principal.NTAccount]).Value
    $userAction = New-ScheduledTaskAction -Execute $exe -Argument $userArguments
    $userTrigger = New-ScheduledTaskTrigger -AtLogOn
    $userPrincipal = New-ScheduledTaskPrincipal -GroupId $usersGroup -RunLevel Limited
    Register-ScheduledTask -TaskName $userTask -Action $userAction -Trigger $userTrigger -Principal $userPrincipal -Settings (New-KeepAwakeSettings) -Description 'Runs Cloudgime Host idle-safe cursor nudge for interactive sessions.' -Force | Out-Null
}} catch {{
    Write-Output ('user-task-skipped: ' + $_.Exception.Message)
}}
";
            var encodedScript = Convert.ToBase64String(Encoding.Unicode.GetBytes(script));
            var result = StartProcessCaptured(
                "powershell.exe",
                $"-NoProfile -ExecutionPolicy Bypass -EncodedCommand {encodedScript}",
                Environment.SystemDirectory,
                TimeSpan.FromSeconds(45));

            Log($"keep-awake task install exitCode={result.ExitCode} timedOut={result.TimedOut}");
            if (!string.IsNullOrWhiteSpace(result.Output))
            {
                Log($"keep-awake task install output:{Environment.NewLine}{result.Output}");
            }

            if (result.ExitCode == 0)
            {
                StartProcessHidden(agentPath, userArguments, Path.GetDirectoryName(agentPath) ?? bundleRoot, waitForExit: false);
            }
        }
        catch (Exception ex)
        {
            Log($"keep-awake task install ignored error: {ex}");
        }
    }

    private static void InstallDisplayBootGuardTask(string bundleRoot)
    {
        try
        {
            var helperPath = ResolveDisplayPrepareHelperPath(bundleRoot);
            if (!File.Exists(helperPath))
            {
                Log($"display boot guard skipped because helper is missing: {helperPath}");
                return;
            }

            var taskArguments = $"persistent-vdd-only --bundle-root {QuoteArgument(bundleRoot)} --poll-ms 5000";
            var script = $@"
$ErrorActionPreference = 'Stop'
$taskName = '{EscapePowerShellSingleQuoted(HostDisplayBootGuardTaskName)}'
$exe = '{EscapePowerShellSingleQuoted(helperPath)}'
$arguments = '{EscapePowerShellSingleQuoted(taskArguments)}'

$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable -MultipleInstances IgnoreNew -ExecutionTimeLimit (New-TimeSpan -Seconds 0)
try {{ $settings.Hidden = $true }} catch {{ }}
$action = New-ScheduledTaskAction -Execute $exe -Argument $arguments
$trigger = New-ScheduledTaskTrigger -AtStartup
$principal = New-ScheduledTaskPrincipal -UserId 'SYSTEM' -RunLevel Highest
Register-ScheduledTask -TaskName $taskName -Action $action -Trigger $trigger -Principal $principal -Settings $settings -Description 'Keeps the MTT VDD ready before Cloudgime Host accepts streams.' -Force | Out-Null
try {{ Start-ScheduledTask -TaskName $taskName }} catch {{ Write-Output ('display-boot-guard-start-skipped: ' + $_.Exception.Message) }}
";
            var encodedScript = Convert.ToBase64String(Encoding.Unicode.GetBytes(script));
            var result = StartProcessCaptured(
                "powershell.exe",
                $"-NoProfile -ExecutionPolicy Bypass -EncodedCommand {encodedScript}",
                Environment.SystemDirectory,
                TimeSpan.FromSeconds(35));

            Log($"display boot guard task install exitCode={result.ExitCode} timedOut={result.TimedOut}");
            if (!string.IsNullOrWhiteSpace(result.Output))
            {
                Log($"display boot guard task install output:{Environment.NewLine}{result.Output}");
            }
        }
        catch (Exception ex)
        {
            Log($"display boot guard task install ignored error: {ex}");
        }
    }

    private static string ResolveDisplayPrepareHelperPath(string bundleRoot)
    {
        var serverPath = Path.Combine(bundleRoot, "moonlight", "server", "display-prepare-helper.exe");
        if (File.Exists(serverPath))
        {
            return serverPath;
        }

        return Path.Combine(bundleRoot, "tools", "display-prepare-helper.exe");
    }

    private static string ResolveKeepAwakeAgentPath(string bundleRoot)
    {
        var systemPath = Path.Combine(bundleRoot, "moonlight", "system", "cloudgime-keep-awake-agent.exe");
        if (File.Exists(systemPath))
        {
            return systemPath;
        }

        return Path.Combine(bundleRoot, "tools", "cloudgime-keep-awake-agent.exe");
    }

    private static void TryDeleteHostKeeperTunnelTask()
    {
        try
        {
            _ = StartProcessHiddenWithTimeout(
                "schtasks.exe",
                $"/Delete /TN \"{HostKeeperTunnelTaskName}\" /F",
                Environment.SystemDirectory,
                TimeSpan.FromSeconds(15));
        }
        catch (Exception ex)
        {
            Log($"delete-host-keeper-task ignored error: {ex}");
        }
    }

    private static void TryDeleteHostKeepAwakeTasks()
    {
        foreach (var taskName in new[] { HostKeepAwakeSystemTaskName, HostKeepAwakeUserTaskName, HostDisplayBootGuardTaskName })
        {
            try
            {
                _ = StartProcessHiddenWithTimeout(
                    "schtasks.exe",
                    $"/Delete /TN \"{taskName}\" /F",
                    Environment.SystemDirectory,
                    TimeSpan.FromSeconds(15));
            }
            catch (Exception ex)
            {
                Log($"delete-host-keep-awake-task name={taskName} ignored error: {ex}");
            }
        }
    }

    private static void StopProcessesWithinRoots(IEnumerable<string> roots, TimeSpan timeout)
    {
        var normalizedRoots = roots
            .Where(static value => !string.IsNullOrWhiteSpace(value))
            .Select(NormalizeDirectoryPrefix)
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .ToArray();

        if (normalizedRoots.Length == 0)
        {
            return;
        }

        var deadline = DateTime.UtcNow.Add(timeout);
        while (DateTime.UtcNow < deadline)
        {
            var matches = Process.GetProcesses()
                .Where(process => IsProcessWithinRoots(process, normalizedRoots))
                .ToArray();

            if (matches.Length == 0)
            {
                return;
            }

            foreach (var process in matches)
            {
                try
                {
                    process.Kill(entireProcessTree: true);
                }
                catch
                {
                }
                finally
                {
                    process.Dispose();
                }
            }

            Thread.Sleep(700);
        }
    }

    private static bool IsProcessWithinRoots(Process process, IReadOnlyList<string> roots)
    {
        try
        {
            var executablePath = process.MainModule?.FileName;
            if (string.IsNullOrWhiteSpace(executablePath))
            {
                return false;
            }

            var normalizedPath = Path.GetFullPath(executablePath);
            return roots.Any(root => normalizedPath.StartsWith(root, StringComparison.OrdinalIgnoreCase));
        }
        catch
        {
            return false;
        }
    }

    private static string NormalizeDirectoryPrefix(string path)
    {
        var fullPath = NormalizeFullPath(path);
        if (fullPath.EndsWith(Path.DirectorySeparatorChar) || fullPath.EndsWith(Path.AltDirectorySeparatorChar))
        {
            return fullPath;
        }

        return fullPath + Path.DirectorySeparatorChar;
    }

    private static void TryDeleteFile(string? path)
    {
        if (string.IsNullOrWhiteSpace(path) || !File.Exists(path))
        {
            return;
        }

        try
        {
            File.Delete(path);
        }
        catch
        {
        }
    }

    private static void TryDeleteDirectory(string? path)
    {
        if (string.IsNullOrWhiteSpace(path) || !Directory.Exists(path))
        {
            return;
        }

        for (var attempt = 0; attempt < 12; attempt++)
        {
            try
            {
                Directory.Delete(path, recursive: true);
                return;
            }
            catch
            {
                Thread.Sleep(1000);
            }
        }
    }

    private static void TryDeleteRegistryTree(string? keyPath)
    {
        if (string.IsNullOrWhiteSpace(keyPath))
        {
            return;
        }

        try
        {
            if (TryResolveRegistryPath(keyPath, out var hive, out var subKey))
            {
                using var baseKey = RegistryKey.OpenBaseKey(hive, RegistryView.Registry64);
                baseKey.DeleteSubKeyTree(subKey, throwOnMissingSubKey: false);
            }
        }
        catch
        {
        }
    }

    private static bool TryResolveRegistryPath(string keyPath, out RegistryHive hive, out string subKey)
    {
        hive = RegistryHive.LocalMachine;
        subKey = string.Empty;

        if (keyPath.StartsWith(@"Registry::HKEY_LOCAL_MACHINE\", StringComparison.OrdinalIgnoreCase))
        {
            hive = RegistryHive.LocalMachine;
            subKey = keyPath[@"Registry::HKEY_LOCAL_MACHINE\".Length..];
            return true;
        }

        if (keyPath.StartsWith(@"Registry::HKEY_CURRENT_USER\", StringComparison.OrdinalIgnoreCase))
        {
            hive = RegistryHive.CurrentUser;
            subKey = keyPath[@"Registry::HKEY_CURRENT_USER\".Length..];
            return true;
        }

        if (keyPath.StartsWith(@"HKLM\", StringComparison.OrdinalIgnoreCase))
        {
            hive = RegistryHive.LocalMachine;
            subKey = keyPath[5..];
            return true;
        }

        if (keyPath.StartsWith(@"HKCU\", StringComparison.OrdinalIgnoreCase))
        {
            hive = RegistryHive.CurrentUser;
            subKey = keyPath[5..];
            return true;
        }

        return false;
    }

    private static void ScheduleFinalCleanup(string? helperPath, string bundleRoot, string installRoot)
    {
        var directoryPaths = new[] { bundleRoot, installRoot }
            .Where(static value => !string.IsNullOrWhiteSpace(value))
            .Select(static value => value!)
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .ToArray();

        if (string.IsNullOrWhiteSpace(helperPath) && directoryPaths.Length == 0)
        {
            return;
        }

        var cleanupSegments = directoryPaths
            .Select(path => $"if exist {QuoteForCmd(path)} rd /s /q {QuoteForCmd(path)}")
            .ToList();

        if (!string.IsNullOrWhiteSpace(helperPath))
        {
            cleanupSegments.Insert(0, $"if exist {QuoteForCmd(helperPath)} del /f /q {QuoteForCmd(helperPath)}");
        }

        var command = $"ping 127.0.0.1 -n 4 >nul & {string.Join(" & ", cleanupSegments)}";
        try
        {
            Process.Start(new ProcessStartInfo
            {
                FileName = "cmd.exe",
                Arguments = $"/c {command}",
                WorkingDirectory = Environment.SystemDirectory,
                UseShellExecute = false,
                CreateNoWindow = true,
                WindowStyle = ProcessWindowStyle.Hidden
            });
        }
        catch
        {
        }
    }

    private static string QuoteForCmd(string value) => $"\"{value.Replace("\"", "\"\"")}\"";

    private static void CreateShortcut(string shortcutPath, string targetPath, string arguments, string workingDirectory, string iconLocation, string description)
    {
        var shellType = Type.GetTypeFromProgID("WScript.Shell")
            ?? throw new InvalidOperationException("WScript.Shell is unavailable.");
        dynamic shell = Activator.CreateInstance(shellType)
            ?? throw new InvalidOperationException("Could not create WScript.Shell.");
        dynamic shortcut = shell.CreateShortcut(shortcutPath);
        shortcut.TargetPath = targetPath;
        shortcut.Arguments = arguments;
        shortcut.WorkingDirectory = workingDirectory;
        shortcut.IconLocation = iconLocation;
        shortcut.Description = description;
        shortcut.Save();
    }

    private static bool TestActivationStateRepairNeeded(string dataRoot)
    {
        var activationPath = Path.Combine(dataRoot, "moonlight", "server", "host_activation_state.json");
        var sharedIdentityPath = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData), "Cloudgime", "pc_identity.json");

        var state = ReadJsonObject(activationPath);
        if (state is null)
        {
            return true;
        }

        foreach (var field in new[] { "HostId", "MachineIdentity", "InstallInstanceId", "ActivationState", "ControlPlaneUrl", "DisplayName" })
        {
            if (string.IsNullOrWhiteSpace(GetString(state, field)))
            {
                return true;
            }
        }

        if (GetString(state, "ActivationState").Equals("installed_unprepared", StringComparison.OrdinalIgnoreCase))
        {
            return true;
        }

        if (ShouldResetPreparedLocalState(state))
        {
            return true;
        }

        var sharedIdentity = ReadJsonObject(sharedIdentityPath);
        if (sharedIdentity is null)
        {
            return true;
        }

        return string.IsNullOrWhiteSpace(GetString(sharedIdentity, "hostId"))
            || string.IsNullOrWhiteSpace(GetString(sharedIdentity, "machineIdentity"));
    }

    private static bool ShouldResetPreparedLocalState(JsonObject state)
    {
        var phase = GetString(state, "ActivationState");
        if (phase.Equals("revoked", StringComparison.OrdinalIgnoreCase)
            || phase.Equals("suspended", StringComparison.OrdinalIgnoreCase))
        {
            return true;
        }

        if (phase.Equals("locked_waiting_token", StringComparison.OrdinalIgnoreCase)
            && !string.IsNullOrWhiteSpace(GetString(state, "ActivationRecordId")))
        {
            return true;
        }

        return false;
    }

    private static string ResolveStableMachineIdentity(JsonObject state, JsonObject sharedIdentity)
    {
        var stateMachineIdentity = GetString(state, "MachineIdentity");
        if (IsValidMachineIdentity(stateMachineIdentity))
        {
            return stateMachineIdentity.ToLowerInvariant();
        }

        var sharedMachineIdentity = GetString(sharedIdentity, "machineIdentity");
        if (IsValidMachineIdentity(sharedMachineIdentity))
        {
            return sharedMachineIdentity.ToLowerInvariant();
        }

        var seedParts = new[]
        {
            GetString(sharedIdentity, "sentinelDeviceId"),
            GetString(sharedIdentity, "sentinelPcId"),
            GetString(sharedIdentity, "keeperEntryId"),
            GetString(sharedIdentity, "hostId")
        }.Where(static value => !string.IsNullOrWhiteSpace(value)).ToArray();

        return NewStableMachineIdentity(seedParts.Length > 0 ? string.Join("|", seedParts) : string.Empty);
    }

    private static string ResolveStableHostId(JsonObject state, JsonObject sharedIdentity)
    {
        var stateHostId = GetString(state, "HostId");
        if (IsValidHostId(stateHostId))
        {
            return stateHostId.ToLowerInvariant();
        }

        var sharedHostId = GetString(sharedIdentity, "hostId");
        if (IsValidHostId(sharedHostId))
        {
            return sharedHostId.ToLowerInvariant();
        }

        var seedParts = new[]
        {
            GetString(sharedIdentity, "sentinelDeviceId"),
            GetString(sharedIdentity, "sentinelPcId"),
            GetString(sharedIdentity, "keeperEntryId"),
            GetString(sharedIdentity, "machineIdentity")
        }.Where(static value => !string.IsNullOrWhiteSpace(value)).ToArray();

        return NewStableHostId(seedParts.Length > 0 ? string.Join("|", seedParts) : string.Empty);
    }

    private static string NewStableHostId(string seed)
    {
        if (string.IsNullOrWhiteSpace(seed))
        {
            return NewRandomHostId();
        }

        var hex = Sha256Hex(seed);
        return $"cg-{hex[..16]}";
    }

    private static string NewStableMachineIdentity(string seed)
    {
        if (string.IsNullOrWhiteSpace(seed))
        {
            return $"cgm-{Guid.NewGuid():N}".ToLowerInvariant();
        }

        var hex = Sha256Hex(seed);
        return $"cgm-{hex[..32]}";
    }

    private static string NewInstallInstanceId()
    {
        var raw = Guid.NewGuid().ToString("N").ToLowerInvariant();
        return $"cgi-{raw[..16]}";
    }

    private static string NewRandomHostId()
    {
        var raw = Guid.NewGuid().ToString("N").ToLowerInvariant();
        return $"cg-{raw[..16]}";
    }

    private static bool IsValidHostId(string value) => HostIdRegex.IsMatch(value ?? string.Empty);
    private static bool IsValidMachineIdentity(string value) => MachineIdentityRegex.IsMatch(value ?? string.Empty);
    private static bool IsValidInstallInstanceId(string value) => InstallInstanceRegex.IsMatch(value ?? string.Empty);

    private static string Sha256Hex(string value)
    {
        var hash = SHA256.HashData(Encoding.UTF8.GetBytes(value));
        return Convert.ToHexString(hash).ToLowerInvariant();
    }

    private static string NormalizePathPrefix(string? value)
    {
        var trimmed = value?.Trim() ?? string.Empty;
        if (string.IsNullOrWhiteSpace(trimmed))
        {
            return string.Empty;
        }

        var withoutTrailing = trimmed.TrimEnd('/');
        return withoutTrailing.StartsWith('/') ? withoutTrailing : $"/{withoutTrailing}";
    }

    private static string NormalizeFullPath(string path) => Path.GetFullPath(path);

    private static string ResolveBundleSourceRoot(string? configuredPath, string releaseRoot)
    {
        if (!string.IsNullOrWhiteSpace(configuredPath))
        {
            return NormalizeFullPath(configuredPath);
        }

        var candidates = new[]
        {
            Path.Combine(releaseRoot, "bundle"),
            Path.Combine(AppContext.BaseDirectory, "bundle"),
            releaseRoot,
            AppContext.BaseDirectory
        };

        foreach (var candidate in candidates)
        {
            if (Directory.Exists(candidate) && File.Exists(Path.Combine(candidate, "host-installer.exe")))
            {
                return NormalizeFullPath(candidate);
            }
        }

        return NormalizeFullPath(Path.Combine(releaseRoot, "bundle"));
    }

    private static JsonObject? ReadJsonObject(string path)
    {
        if (!File.Exists(path))
        {
            return null;
        }

        try
        {
            return JsonNode.Parse(File.ReadAllText(path)) as JsonObject;
        }
        catch
        {
            return null;
        }
    }

    private static string GetString(JsonObject obj, string propertyName)
    {
        return obj[propertyName]?.GetValue<string>()?.Trim() ?? string.Empty;
    }

    private static string GetNestedString(JsonObject obj, params string[] propertyPath)
    {
        JsonNode? current = obj;
        foreach (var property in propertyPath)
        {
            current = current?[property];
            if (current is null)
            {
                return string.Empty;
            }
        }

        return current.GetValue<string>()?.Trim() ?? string.Empty;
    }

    private static void CopyIfMissing(JsonObject target, string targetKey, JsonObject source, string sourceKey)
    {
        if (!string.IsNullOrWhiteSpace(GetString(target, targetKey)))
        {
            return;
        }

        var sourceValue = GetString(source, sourceKey);
        if (!string.IsNullOrWhiteSpace(sourceValue))
        {
            target[targetKey] = sourceValue;
        }
    }

    private static void WriteJsonObject(string path, JsonObject obj)
    {
        Directory.CreateDirectory(Path.GetDirectoryName(path)!);
        WriteTextFile(path, obj.ToJsonString(new JsonSerializerOptions { WriteIndented = true }));
    }

    private static void WriteTextFile(string path, string content)
    {
        Directory.CreateDirectory(Path.GetDirectoryName(path)!);
        File.WriteAllText(path, content, Utf8NoBom);
    }

    private static void CopyDirectory(string sourceRoot, string targetRoot)
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
            var targetPath = Path.Combine(targetRoot, relative);
            Directory.CreateDirectory(Path.GetDirectoryName(targetPath)!);
            File.Copy(file, targetPath, overwrite: true);
        }
    }

    private static int StartProcessHidden(string fileName, string arguments, string workingDirectory, bool waitForExit)
    {
        using var process = new Process
        {
            StartInfo = new ProcessStartInfo
            {
                FileName = fileName,
                Arguments = arguments,
                WorkingDirectory = workingDirectory,
                UseShellExecute = false,
                CreateNoWindow = true,
                WindowStyle = ProcessWindowStyle.Hidden
            }
        };

        process.Start();
        if (!waitForExit)
        {
            return 0;
        }

        process.WaitForExit();
        return process.ExitCode;
    }

    private static ProcessCaptureResult StartProcessCaptured(string fileName, string arguments, string workingDirectory, TimeSpan timeout)
    {
        using var process = new Process
        {
            StartInfo = new ProcessStartInfo
            {
                FileName = fileName,
                Arguments = arguments,
                WorkingDirectory = workingDirectory,
                UseShellExecute = false,
                CreateNoWindow = true,
                WindowStyle = ProcessWindowStyle.Hidden,
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                StandardOutputEncoding = Utf8NoBom,
                StandardErrorEncoding = Utf8NoBom
            }
        };

        process.Start();
        var stdoutTask = process.StandardOutput.ReadToEndAsync();
        var stderrTask = process.StandardError.ReadToEndAsync();
        var exited = process.WaitForExit((int)timeout.TotalMilliseconds);
        var timedOut = !exited;
        if (!exited)
        {
            try
            {
                process.Kill(entireProcessTree: true);
            }
            catch
            {
            }

            process.WaitForExit(5000);
        }
        Task.WhenAny(Task.WhenAll(stdoutTask, stderrTask), Task.Delay(5000)).GetAwaiter().GetResult();

        var combined = string.Join(
            Environment.NewLine,
            new[]
            {
                stdoutTask.IsCompletedSuccessfully ? stdoutTask.Result : string.Empty,
                stderrTask.IsCompletedSuccessfully ? stderrTask.Result : string.Empty
            }
                .Where(static value => !string.IsNullOrWhiteSpace(value))
                .Select(static value => value.Trim()));

        return new ProcessCaptureResult(process.ExitCode, combined.Trim(), timedOut);
    }

    private static ProcessCaptureResult StartProcessHiddenWithTimeout(string fileName, string arguments, string workingDirectory, TimeSpan timeout)
    {
        using var process = new Process
        {
            StartInfo = new ProcessStartInfo
            {
                FileName = fileName,
                Arguments = arguments,
                WorkingDirectory = workingDirectory,
                UseShellExecute = false,
                CreateNoWindow = true,
                WindowStyle = ProcessWindowStyle.Hidden
            }
        };

        process.Start();
        var exited = process.WaitForExit((int)timeout.TotalMilliseconds);
        if (!exited)
        {
            try
            {
                process.Kill(entireProcessTree: true);
            }
            catch
            {
            }

            process.WaitForExit(5000);
        }

        return new ProcessCaptureResult(process.ExitCode, string.Empty, !exited);
    }

    private static string SummarizeProcessOutput(string output)
    {
        if (string.IsNullOrWhiteSpace(output))
        {
            return string.Empty;
        }

        var lines = output
            .Replace('\0', ' ')
            .Split(new[] { "\r\n", "\n" }, StringSplitOptions.RemoveEmptyEntries)
            .Select(static value => value.Trim())
            .Where(static value => !string.IsNullOrWhiteSpace(value))
            .ToArray();
        if (lines.Length == 0)
        {
            return string.Empty;
        }

        var selected = lines.Length > 8 ? lines[^8..] : lines;
        var summary = string.Join(Environment.NewLine, selected);
        return summary.Length > 1800 ? summary[^1800..] : summary;
    }

    private static bool IsAdministrator()
    {
        using var identity = WindowsIdentity.GetCurrent();
        var principal = new WindowsPrincipal(identity);
        return principal.IsInRole(WindowsBuiltInRole.Administrator);
    }

    private static int RelaunchElevated(string[] args)
    {
        var executablePath = Environment.ProcessPath
            ?? throw new InvalidOperationException("Current executable path could not be resolved.");
        var quotedArgs = string.Join(" ", args.Select(QuoteArgument));
        using var process = Process.Start(new ProcessStartInfo
        {
            FileName = executablePath,
            Arguments = quotedArgs,
            UseShellExecute = true,
            Verb = "runas",
            WorkingDirectory = AppContext.BaseDirectory
        }) ?? throw new InvalidOperationException("Could not relaunch elevated process.");
        process.WaitForExit();
        return process.ExitCode;
    }

    private static string QuoteArgument(string arg)
    {
        if (string.IsNullOrEmpty(arg))
        {
            return "\"\"";
        }

        return arg.IndexOfAny(new[] { ' ', '\t', '"' }) >= 0
            ? $"\"{arg.Replace("\"", "\\\"")}\""
            : arg;
    }

    private static string EscapePowerShellSingleQuoted(string value) => value.Replace("'", "''", StringComparison.Ordinal);

    private static void Log(string message)
    {
        try
        {
            File.AppendAllText(LogPath, $"[{DateTime.Now:yyyy-MM-dd HH:mm:ss}] {message}{Environment.NewLine}", Utf8NoBom);
        }
        catch
        {
        }
    }

    private static void AddWindowsDefenderExclusions(string installRoot, string dataRoot)
    {
        try
        {
            Log($"Adding Windows Defender exclusions for '{installRoot}' and '{dataRoot}'...");
            
            // Exclude the install root path
            var installRootArgs = $"-NoProfile -ExecutionPolicy Bypass -Command \"Add-MpPreference -ExclusionPath '{installRoot}' -Force\"";
            _ = StartProcessHidden("powershell.exe", installRootArgs, Environment.SystemDirectory, waitForExit: true);
            
            // Exclude the data root path
            var dataRootArgs = $"-NoProfile -ExecutionPolicy Bypass -Command \"Add-MpPreference -ExclusionPath '{dataRoot}' -Force\"";
            _ = StartProcessHidden("powershell.exe", dataRootArgs, Environment.SystemDirectory, waitForExit: true);

            Log("Windows Defender exclusions added successfully.");
        }
        catch (Exception ex)
        {
            Log($"Failed to add Windows Defender exclusions: {ex.Message}");
        }
    }

    [DllImport("advapi32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    private static extern bool LogonUser(
        string lpszUsername,
        string lpszDomain,
        string lpszPassword,
        int dwLogonType,
        int dwLogonProvider,
        out IntPtr phToken);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CloseHandle(IntPtr hObject);

    private static void ConfigureWindowsSystemTweaks()
    {
        try
        {
            Log("configuring windows system tweaks");

            // Disable Fast Startup (Hiberboot)
            using (var baseKey = RegistryKey.OpenBaseKey(RegistryHive.LocalMachine, RegistryView.Registry64))
            using (var key = baseKey.OpenSubKey(@"SYSTEM\CurrentControlSet\Control\Power", writable: true))
            {
                if (key != null)
                {
                    key.SetValue("HiberbootEnabled", 0, RegistryValueKind.DWord);
                    Log("registry HiberbootEnabled=0");
                }
            }

            // Disable Blank Password Logon Restriction over Network/Console
            using (var baseKey = RegistryKey.OpenBaseKey(RegistryHive.LocalMachine, RegistryView.Registry64))
            using (var key = baseKey.OpenSubKey(@"SYSTEM\CurrentControlSet\Control\Lsa", writable: true))
            {
                if (key != null)
                {
                    key.SetValue("LimitBlankPasswordUse", 0, RegistryValueKind.DWord);
                    Log("registry LimitBlankPasswordUse=0");
                }
            }

            // Fast Shutdown Tweaks: WaitToKillServiceTimeout (String, value in ms)
            using (var baseKey = RegistryKey.OpenBaseKey(RegistryHive.LocalMachine, RegistryView.Registry64))
            using (var key = baseKey.OpenSubKey(@"SYSTEM\CurrentControlSet\Control", writable: true))
            {
                if (key != null)
                {
                    key.SetValue("WaitToKillServiceTimeout", "3000", RegistryValueKind.String);
                    Log("registry WaitToKillServiceTimeout=3000");
                }
            }

            // Fast Shutdown Tweaks for Current User: AutoEndTasks and WaitToKillAppTimeout
            using (var baseKey = RegistryKey.OpenBaseKey(RegistryHive.Users, RegistryView.Registry64))
            using (var key = baseKey.OpenSubKey(@".DEFAULT\Control Panel\Desktop", writable: true))
            {
                if (key != null)
                {
                    key.SetValue("AutoEndTasks", "1", RegistryValueKind.String);
                    key.SetValue("WaitToKillAppTimeout", "3000", RegistryValueKind.String);
                    Log("registry HKEY_USERS\\.DEFAULT AutoEndTasks=1, WaitToKillAppTimeout=3000");
                }
            }
        }
        catch (Exception ex)
        {
            Log($"failed to configure windows system tweaks: {ex}");
        }
    }

    private static void ConfigureWindowsAutoLogon(string? passwordOverride = null)
    {
        try
        {
            Log("configuring windows auto-logon");
            string domain;
            string userName = GetInteractiveUserName(out domain);
            if (string.IsNullOrWhiteSpace(userName) || userName.Equals("SYSTEM", StringComparison.OrdinalIgnoreCase))
            {
                Log("auto-logon skipped: no valid interactive user found");
                return;
            }

            string password = passwordOverride ?? string.Empty;
            if (passwordOverride == null)
            {
                // Probe if blank password works
                IntPtr token = IntPtr.Zero;
                bool isBlank = false;
                try
                {
                    isBlank = LogonUser(userName, domain, "", 2, 0, out token);
                }
                catch (Exception ex)
                {
                    Log($"logon user probe failed: {ex}");
                }

                if (isBlank)
                {
                    Log($"detected blank password for user {userName}");
                    password = "";
                    if (token != IntPtr.Zero)
                    {
                        CloseHandle(token);
                    }
                }
                else
                {
                    Log($"user {userName} has password or logon probe failed; using provided/empty password");
                }
            }

            using (var baseKey = RegistryKey.OpenBaseKey(RegistryHive.LocalMachine, RegistryView.Registry64))
            using (var key = baseKey.OpenSubKey(@"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon", writable: true))
            {
                if (key != null)
                {
                    key.SetValue("AutoAdminLogon", "1", RegistryValueKind.String);
                    key.SetValue("DefaultUserName", userName, RegistryValueKind.String);
                    key.SetValue("DefaultDomainName", domain, RegistryValueKind.String);
                    key.SetValue("DefaultPassword", password, RegistryValueKind.String);
                    Log($"registry AutoAdminLogon=1 DefaultUserName={userName} DefaultDomainName={domain}");
                }
            }
        }
        catch (Exception ex)
        {
            Log($"failed to configure windows auto-logon: {ex}");
        }
    }

    private static string GetInteractiveUserName(out string domain)
    {
        domain = Environment.MachineName;
        try
        {
            using (var searcher = new ManagementObjectSearcher("SELECT UserName FROM Win32_ComputerSystem"))
            using (var collection = searcher.Get())
            {
                foreach (ManagementObject obj in collection)
                {
                    var userNameVal = obj["UserName"] as string;
                    if (!string.IsNullOrWhiteSpace(userNameVal))
                    {
                        var parts = userNameVal.Split('\\');
                        if (parts.Length == 2)
                        {
                            domain = parts[0];
                            return parts[1];
                        }
                        return userNameVal;
                    }
                }
            }
        }
        catch (Exception ex)
        {
            Log($"failed to query interactive username via Win32_ComputerSystem: {ex}");
        }

        // Fallback: Check explorer.exe process owner
        try
        {
            var processes = Process.GetProcessesByName("explorer");
            foreach (var process in processes)
            {
                try
                {
                    var sq = new ObjectQuery($"SELECT * FROM Win32_Process WHERE ProcessId = {process.Id}");
                    using (var searcher = new ManagementObjectSearcher(sq))
                    using (var collection = searcher.Get())
                    {
                        foreach (ManagementObject obj in collection)
                        {
                            var argList = new object[2];
                            var returnVal = Convert.ToInt32(obj.InvokeMethod("GetOwner", argList));
                            if (returnVal == 0)
                            {
                                var user = argList[0] as string;
                                var dom = argList[1] as string;
                                if (!string.IsNullOrWhiteSpace(user))
                                {
                                    if (!string.IsNullOrWhiteSpace(dom))
                                    {
                                        domain = dom;
                                    }
                                    return user;
                                }
                            }
                        }
                    }
                }
                catch
                {
                    // Ignore
                }
            }
        }
        catch (Exception ex)
        {
            Log($"failed to query explorer owner: {ex}");
        }

        return string.Empty;
    }
}

internal sealed record ProcessCaptureResult(int ExitCode, string Output, bool TimedOut);

internal sealed class ParsedArguments
{
    private readonly Dictionary<string, string?> _values = new(StringComparer.OrdinalIgnoreCase);
    private readonly HashSet<string> _flags = new(StringComparer.OrdinalIgnoreCase);

    public string Command { get; private set; } = "install";

    public static ParsedArguments Parse(IReadOnlyList<string> args)
    {
        var parsed = new ParsedArguments();
        var index = 0;
        if (args.Count > 0 && !args[0].StartsWith('-'))
        {
            parsed.Command = args[0].Trim().ToLowerInvariant();
            index = 1;
        }

        while (index < args.Count)
        {
            var current = args[index];
            if (!current.StartsWith("--", StringComparison.Ordinal))
            {
                throw new InvalidOperationException($"Unexpected argument '{current}'.");
            }

            var key = current[2..];
            if (index + 1 < args.Count && !args[index + 1].StartsWith("--", StringComparison.Ordinal))
            {
                parsed._values[key] = args[index + 1];
                index += 2;
                continue;
            }

            parsed._flags.Add(key);
            index += 1;
        }

        return parsed;
    }

    public string? GetValue(string key) => _values.TryGetValue(key, out var value) ? value : null;
    public bool HasFlag(string key) => _flags.Contains(key);
}

internal sealed class FallbackAudioManifest
{
    [System.Text.Json.Serialization.JsonPropertyName("installer")]
    public string Installer { get; set; } = string.Empty;

    [System.Text.Json.Serialization.JsonPropertyName("arguments")]
    public List<string> Arguments { get; set; } = [];

    [System.Text.Json.Serialization.JsonPropertyName("post_install_delay_seconds")]
    public int PostInstallDelaySeconds { get; set; } = 10;

    [System.Text.Json.Serialization.JsonPropertyName("verify_timeout_seconds")]
    public int VerifyTimeoutSeconds { get; set; } = 30;

    [System.Text.Json.Serialization.JsonPropertyName("verify_poll_interval_seconds")]
    public int VerifyPollIntervalSeconds { get; set; } = 2;

    [System.Text.Json.Serialization.JsonPropertyName("expected_audio_endpoints")]
    public List<AudioEndpointInfo> ExpectedAudioEndpoints { get; set; } = [];
}

internal sealed record AudioEndpointInfo(
    [property: System.Text.Json.Serialization.JsonPropertyName("direction")] string Direction,
    [property: System.Text.Json.Serialization.JsonPropertyName("name")] string Name
);

internal sealed record ProcessCommand(string FileName, string Arguments);
