using System.Diagnostics;
using System.Management;
using System.Security.Principal;
using System.Text;
using System.Text.Json.Nodes;
using Microsoft.Win32;
using MessageBox = System.Windows.Forms.MessageBox;
using MessageBoxButtons = System.Windows.Forms.MessageBoxButtons;
using MessageBoxIcon = System.Windows.Forms.MessageBoxIcon;
using DialogResult = System.Windows.Forms.DialogResult;

namespace CloudgimeHostEmergencyUninstaller;

internal static class Program
{
    private const string ProductName = "Cloudgime Host Emergency Uninstaller";
    private const string DefaultHostProductName = "Cloudgime Host";
    private const string DefaultAppProductName = "Cloudgime Host Control";
    private const string DefaultUninstallRegistryKey = @"Software\Microsoft\Windows\CurrentVersion\Uninstall\CloudgimeHostControl";
    private static readonly UTF8Encoding Utf8NoBom = new(false);
    private static readonly string LogPath = Path.Combine(Path.GetTempPath(), "cloudgime-host-uninstaller.log");

    [STAThread]
    private static int Main(string[] args)
    {
        try
        {
            Log($"start args={string.Join(" ", args.Select(QuoteArgument))}");
            if (!IsAdministrator())
            {
                return RelaunchElevated(args);
            }

            var parsed = ParsedArguments.Parse(args);
            if (!string.Equals(parsed.Command, "uninstall", StringComparison.OrdinalIgnoreCase))
            {
                throw new InvalidOperationException($"Unknown command '{parsed.Command}'.");
            }

            return Uninstall(parsed);
        }
        catch (Exception ex)
        {
            Log($"fatal error: {ex}");
            MessageBox.Show(ex.Message, ProductName, MessageBoxButtons.OK, MessageBoxIcon.Error);
            return 1;
        }
    }

    private static int Uninstall(ParsedArguments args)
    {
        var installRoot = ResolveInstallRoot(args.GetValue("install-root"));
        var bundleRoot = ResolveBundleRoot(args.GetValue("bundle-root"), installRoot);
        var purgeSharedState = !args.HasFlag("keep-shared-state");
        var noConfirm = args.HasFlag("no-confirm") || args.HasFlag("silent");
        var commonPrograms = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.CommonStartMenu), "Programs");
        var userPrograms = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.StartMenu), "Programs");
        var commonDesktop = Environment.GetFolderPath(Environment.SpecialFolder.CommonDesktopDirectory);
        var userDesktop = Environment.GetFolderPath(Environment.SpecialFolder.DesktopDirectory);

        Log($"resolved installRoot={installRoot}");
        Log($"resolved bundleRoot={bundleRoot}");
        Log($"purgeSharedState={purgeSharedState}");

        if (!noConfirm)
        {
            var prompt = $"Hapus paksa {DefaultHostProductName} dari PC ini?{Environment.NewLine}{Environment.NewLine}" +
                         $"Install root:{Environment.NewLine}{installRoot}{Environment.NewLine}{Environment.NewLine}" +
                         $"Data root:{Environment.NewLine}{bundleRoot}{Environment.NewLine}{Environment.NewLine}" +
                         "Aksi ini akan menghentikan service/proses dan menghapus shortcut, registry, serta folder host.";
            if (MessageBox.Show(prompt, ProductName, MessageBoxButtons.YesNo, MessageBoxIcon.Warning) != DialogResult.Yes)
            {
                Log("cancelled by user");
                return 1;
            }
        }

        TryRunHostInstallerCommand(bundleRoot, "stop-bundle");
        TryRunHostInstallerCommand(bundleRoot, "stop-service");
        TryRunHostInstallerCommand(bundleRoot, "uninstall-service");

        RemoveKnownScheduledTasks(bundleRoot);
        RemoveKnownServices(bundleRoot, installRoot);
        RemoveMatchingFirewallRules();
        StopProcessesWithinRoots(new[] { installRoot, bundleRoot }, TimeSpan.FromSeconds(20));

        foreach (var path in new[]
                 {
                     Path.Combine(commonPrograms, $"{DefaultHostProductName}.lnk"),
                     Path.Combine(commonPrograms, $"{DefaultAppProductName}.lnk"),
                     Path.Combine(userPrograms, $"{DefaultHostProductName}.lnk"),
                     Path.Combine(userPrograms, $"{DefaultAppProductName}.lnk"),
                     Path.Combine(commonDesktop, $"{DefaultHostProductName}.lnk"),
                     Path.Combine(commonDesktop, $"{DefaultAppProductName}.lnk"),
                     Path.Combine(userDesktop, $"{DefaultHostProductName}.lnk"),
                     Path.Combine(userDesktop, $"{DefaultAppProductName}.lnk"),
                     Path.Combine(installRoot, "open-host-control.cmd"),
                     Path.Combine(installRoot, "open-host-control-folder.cmd")
                 })
        {
            TryDeleteFile(path);
        }

        TryDeleteRegistryTree($@"HKLM\{DefaultUninstallRegistryKey}");
        TryDeleteDirectory(bundleRoot);
        TryDeleteDirectory(installRoot);

        if (purgeSharedState)
        {
            RemoveSharedCloudgimeState();
        }

        ScheduleFinalCleanup(new[] { installRoot, bundleRoot });
        Log("uninstall completed");

        if (!args.HasFlag("silent"))
        {
            MessageBox.Show($"{DefaultHostProductName} berhasil dibersihkan dari PC ini.", ProductName, MessageBoxButtons.OK, MessageBoxIcon.Information);
        }

        return 0;
    }

    private static string ResolveInstallRoot(string? configuredInstallRoot)
    {
        if (!string.IsNullOrWhiteSpace(configuredInstallRoot))
        {
            return NormalizeFullPath(configuredInstallRoot);
        }

        var registryInstallLocation = TryReadRegistryString(DefaultUninstallRegistryKey, "InstallLocation");
        if (!string.IsNullOrWhiteSpace(registryInstallLocation))
        {
            return NormalizeFullPath(registryInstallLocation);
        }

        var defaultInstallRoot = Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles),
            DefaultHostProductName);
        return NormalizeFullPath(defaultInstallRoot);
    }

    private static string ResolveBundleRoot(string? configuredBundleRoot, string installRoot)
    {
        if (!string.IsNullOrWhiteSpace(configuredBundleRoot))
        {
            return NormalizeFullPath(configuredBundleRoot);
        }

        var layoutPath = Path.Combine(installRoot, "install-layout.json");
        var layout = ReadJsonObject(layoutPath);
        var bundleRoot = layout is null ? string.Empty : GetString(layout, "bundleRoot");
        if (!string.IsNullOrWhiteSpace(bundleRoot))
        {
            return NormalizeFullPath(bundleRoot);
        }

        foreach (var valueName in new[] { "QuietUninstallString", "UninstallString" })
        {
            var uninstallString = TryReadRegistryString(DefaultUninstallRegistryKey, valueName);
            var parsedBundleRoot = TryExtractBundleRootFromCommand(uninstallString);
            if (!string.IsNullOrWhiteSpace(parsedBundleRoot))
            {
                return NormalizeFullPath(parsedBundleRoot);
            }
        }

        var defaultBundleRoot = Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData),
            "Cloudgime",
            "Host");
        return NormalizeFullPath(defaultBundleRoot);
    }

    private static string? TryReadRegistryString(string subKeyPath, string valueName)
    {
        try
        {
            using var baseKey = RegistryKey.OpenBaseKey(RegistryHive.LocalMachine, RegistryView.Registry64);
            using var key = baseKey.OpenSubKey(subKeyPath, writable: false);
            return key?.GetValue(valueName)?.ToString();
        }
        catch
        {
            return null;
        }
    }

    private static string? TryExtractBundleRootFromCommand(string? commandText)
    {
        if (string.IsNullOrWhiteSpace(commandText))
        {
            return null;
        }

        var marker = "--bundle-root";
        var index = commandText.IndexOf(marker, StringComparison.OrdinalIgnoreCase);
        if (index < 0)
        {
            return null;
        }

        var tail = commandText[(index + marker.Length)..].TrimStart();
        if (tail.StartsWith('"'))
        {
            var endQuote = tail.IndexOf('"', 1);
            if (endQuote > 1)
            {
                return tail[1..endQuote];
            }
        }

        var nextSpace = tail.IndexOf(' ');
        return nextSpace > 0 ? tail[..nextSpace] : tail;
    }

    private static void TryRunHostInstallerCommand(string bundleRoot, string action)
    {
        try
        {
            var hostInstaller = Path.Combine(bundleRoot, "host-installer.exe");
            if (!File.Exists(hostInstaller))
            {
                Log($"host-installer action={action} skipped because binary is missing");
                return;
            }

            var result = StartProcessCaptured(hostInstaller, $"--bundle-root \"{bundleRoot}\" {action}", bundleRoot);
            Log($"host-installer action={action} exitCode={result.ExitCode}");
            if (!string.IsNullOrWhiteSpace(result.Output))
            {
                Log($"host-installer action={action} output:{Environment.NewLine}{result.Output}");
            }
        }
        catch (Exception ex)
        {
            Log($"host-installer action={action} ignored error: {ex}");
        }
    }

    private static void RemoveKnownScheduledTasks(string bundleRoot)
    {
        var bundleName = new DirectoryInfo(bundleRoot).Name;
        foreach (var taskName in new[]
                 {
                     $"CloudgimeHostUser-{bundleName}",
                     "CloudgimeHostUser-Host",
                     "CloudgimeHostKeeperTunnelAgent",
                     "CloudgimeHostKeepAwakeAgent",
                     "CloudgimeHostKeepAwakeAgentUser"
                  }.Distinct(StringComparer.OrdinalIgnoreCase))
        {
            TryDeleteTask(taskName);
        }
    }

    private static void RemoveKnownServices(string bundleRoot, string installRoot)
    {
        var bundleName = new DirectoryInfo(bundleRoot).Name;
        foreach (var serviceName in new[]
                 {
                     $"CloudgimeHost-{bundleName}",
                     $"CloudgimeRuntime-{bundleName}",
                     "CloudgimeHost-Host",
                     "CloudgimeRuntime-Host"
                 }.Distinct(StringComparer.OrdinalIgnoreCase))
        {
            TryDeleteService(serviceName);
        }

        try
        {
            using var searcher = new ManagementObjectSearcher("SELECT Name, DisplayName, PathName FROM Win32_Service");
            foreach (var item in searcher.Get().Cast<ManagementObject>())
            {
                var serviceName = item["Name"]?.ToString() ?? string.Empty;
                var displayName = item["DisplayName"]?.ToString() ?? string.Empty;
                var pathName = item["PathName"]?.ToString() ?? string.Empty;
                if (string.IsNullOrWhiteSpace(serviceName))
                {
                    continue;
                }

                if (!ShouldRemoveService(serviceName, displayName, pathName, installRoot, bundleRoot))
                {
                    continue;
                }

                TryDeleteService(serviceName);
            }
        }
        catch (Exception ex)
        {
            Log($"service scan ignored error: {ex}");
        }
    }

    private static bool ShouldRemoveService(string serviceName, string displayName, string pathName, string installRoot, string bundleRoot)
    {
        if (serviceName.StartsWith("CloudgimeHost-", StringComparison.OrdinalIgnoreCase)
            || serviceName.StartsWith("CloudgimeRuntime-", StringComparison.OrdinalIgnoreCase))
        {
            return true;
        }

        if (displayName.Contains("Cloudgime Host", StringComparison.OrdinalIgnoreCase)
            || displayName.Contains("Cloudgime Runtime", StringComparison.OrdinalIgnoreCase))
        {
            return true;
        }

        var normalizedInstallRoot = NormalizeDirectoryPrefix(installRoot);
        var normalizedBundleRoot = NormalizeDirectoryPrefix(bundleRoot);
        return pathName.Contains(normalizedInstallRoot, StringComparison.OrdinalIgnoreCase)
               || pathName.Contains(normalizedBundleRoot, StringComparison.OrdinalIgnoreCase)
               || pathName.Contains("cloudgime-runtime-agent.exe", StringComparison.OrdinalIgnoreCase);
    }

    private static void TryDeleteService(string serviceName)
    {
        try
        {
            _ = StartProcessHidden("sc.exe", $"stop {serviceName}", Environment.SystemDirectory, waitForExit: true);
            var result = StartProcessCaptured("sc.exe", $"delete {serviceName}", Environment.SystemDirectory);
            Log($"service delete name={serviceName} exitCode={result.ExitCode}");
            if (!string.IsNullOrWhiteSpace(result.Output))
            {
                Log($"service delete name={serviceName} output:{Environment.NewLine}{result.Output}");
            }
        }
        catch (Exception ex)
        {
            Log($"service delete ignored name={serviceName} error={ex}");
        }
    }

    private static void TryDeleteTask(string taskName)
    {
        try
        {
            var result = StartProcessCaptured("schtasks.exe", $"/Delete /TN {QuoteArgument(taskName)} /F", Environment.SystemDirectory);
            Log($"task delete name={taskName} exitCode={result.ExitCode}");
            if (!string.IsNullOrWhiteSpace(result.Output))
            {
                Log($"task delete name={taskName} output:{Environment.NewLine}{result.Output}");
            }
        }
        catch (Exception ex)
        {
            Log($"task delete ignored name={taskName} error={ex}");
        }
    }

    private static void RemoveMatchingFirewallRules()
    {
        const string script = "Get-NetFirewallRule -ErrorAction SilentlyContinue | Where-Object { $_.DisplayName -like 'Cloudgime Host *' } | Remove-NetFirewallRule -ErrorAction SilentlyContinue";
        try
        {
            var result = StartProcessCaptured(
                "powershell.exe",
                $"-NoProfile -ExecutionPolicy Bypass -Command {QuoteArgument(script)}",
                Environment.SystemDirectory);
            Log($"firewall cleanup exitCode={result.ExitCode}");
            if (!string.IsNullOrWhiteSpace(result.Output))
            {
                Log($"firewall cleanup output:{Environment.NewLine}{result.Output}");
            }
        }
        catch (Exception ex)
        {
            Log($"firewall cleanup ignored error: {ex}");
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

            var normalizedPath = NormalizeFullPath(executablePath);
            return roots.Any(root => normalizedPath.StartsWith(root, StringComparison.OrdinalIgnoreCase));
        }
        catch
        {
            return false;
        }
    }

    private static void RemoveSharedCloudgimeState()
    {
        var commonRoot = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData), "Cloudgime");
        foreach (var path in new[]
                 {
                     Path.Combine(commonRoot, "pending_uninstall.json"),
                     Path.Combine(commonRoot, "pc_identity.json")
                 })
        {
            TryDeleteFile(path);
        }

        if (Directory.Exists(commonRoot) && !Directory.EnumerateFileSystemEntries(commonRoot).Any())
        {
            TryDeleteDirectory(commonRoot);
        }
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
        catch (Exception ex)
        {
            Log($"file delete ignored path={path} error={ex.Message}");
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
            catch (Exception ex)
            {
                Log($"directory delete retry path={path} attempt={attempt + 1} error={ex.Message}");
                Thread.Sleep(1000);
            }
        }
    }

    private static void TryDeleteRegistryTree(string keyPath)
    {
        try
        {
            using var baseKey = RegistryKey.OpenBaseKey(RegistryHive.LocalMachine, RegistryView.Registry64);
            baseKey.DeleteSubKeyTree(keyPath, throwOnMissingSubKey: false);
        }
        catch (Exception ex)
        {
            Log($"registry delete ignored path={keyPath} error={ex.Message}");
        }
    }

    private static void ScheduleFinalCleanup(IEnumerable<string> directoryPaths)
    {
        var paths = directoryPaths
            .Where(static value => !string.IsNullOrWhiteSpace(value))
            .Select(static value => value!)
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .ToArray();
        if (paths.Length == 0)
        {
            return;
        }

        var cleanupSegments = paths.Select(path => $"if exist {QuoteForCmd(path)} rd /s /q {QuoteForCmd(path)}").ToArray();
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
        catch (Exception ex)
        {
            Log($"final cleanup schedule ignored error: {ex.Message}");
        }
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

    private static string GetString(JsonObject obj, string propertyName) =>
        obj[propertyName]?.GetValue<string>()?.Trim() ?? string.Empty;

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

    private static ProcessCaptureResult StartProcessCaptured(string fileName, string arguments, string workingDirectory)
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
        process.WaitForExit();
        Task.WhenAll(stdoutTask, stderrTask).GetAwaiter().GetResult();

        var combined = string.Join(
            Environment.NewLine,
            new[] { stdoutTask.Result, stderrTask.Result }
                .Where(static value => !string.IsNullOrWhiteSpace(value))
                .Select(static value => value.Trim()));

        return new ProcessCaptureResult(process.ExitCode, combined.Trim());
    }

    private static string NormalizeFullPath(string path) => Path.GetFullPath(path);

    private static string NormalizeDirectoryPrefix(string path)
    {
        var fullPath = NormalizeFullPath(path);
        if (fullPath.EndsWith(Path.DirectorySeparatorChar) || fullPath.EndsWith(Path.AltDirectorySeparatorChar))
        {
            return fullPath;
        }

        return fullPath + Path.DirectorySeparatorChar;
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
        using var process = Process.Start(new ProcessStartInfo
        {
            FileName = executablePath,
            Arguments = string.Join(" ", args.Select(QuoteArgument)),
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

    private static string QuoteForCmd(string value) => $"\"{value.Replace("\"", "\"\"")}\"";

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
}

internal sealed record ProcessCaptureResult(int ExitCode, string Output);

internal sealed class ParsedArguments
{
    private readonly Dictionary<string, string?> _values = new(StringComparer.OrdinalIgnoreCase);
    private readonly HashSet<string> _flags = new(StringComparer.OrdinalIgnoreCase);

    public string Command { get; private set; } = "uninstall";

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
