using System.ComponentModel;
using System.Diagnostics;
using System.Management;
using System.Security.Principal;
using System.Text;

namespace BundleProcessManager;

internal sealed record StopOptions(
    string BundleRoot,
    int WebPort,
    int SunshinePort,
    bool Quiet,
    bool SelfElevated
);

internal sealed record ProcessInfo(
    int ProcessId,
    string Name,
    string? ExecutablePath,
    string? CommandLine
);

internal static class Program
{
    private static readonly string[] TargetNames = ["sunshine.exe", "frpc.exe", "web-server.exe", "streamer.exe", "mic_sidecar.exe", "gamepad_sidecar.exe"];

    private static int Main(string[] args)
    {
        if (args.Length == 0 || IsHelp(args[0]))
        {
            PrintUsage();
            return 1;
        }

        if (!string.Equals(args[0], "stop", StringComparison.OrdinalIgnoreCase))
        {
            PrintUsage();
            return 1;
        }

        try
        {
            var options = ParseStopOptions(args.Skip(1).ToArray());
            return StopBundleProcesses(options);
        }
        catch (ArgumentException ex)
        {
            Console.Error.WriteLine(ex.Message);
            PrintUsage();
            return 1;
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine($"[bundle-process-manager] {ex.Message}");
            return 1;
        }
    }

    private static bool IsHelp(string value) =>
        value is "-h" or "--help" or "/?" or "help";

    private static void PrintUsage()
    {
        Console.WriteLine("bundle-process-manager stop --bundle-root <PATH> --web-port <PORT> --sunshine-port <PORT> [--quiet] [--self-elevated]");
    }

    private static StopOptions ParseStopOptions(string[] args)
    {
        string? bundleRoot = null;
        int? webPort = null;
        int? sunshinePort = null;
        var quiet = false;
        var selfElevated = false;

        for (var i = 0; i < args.Length; i++)
        {
            var arg = args[i];
            switch (arg)
            {
                case "--bundle-root":
                    bundleRoot = RequireValue(args, ref i, arg);
                    break;
                case "--web-port":
                    webPort = ParsePort(RequireValue(args, ref i, arg), arg);
                    break;
                case "--sunshine-port":
                    sunshinePort = ParsePort(RequireValue(args, ref i, arg), arg);
                    break;
                case "--quiet":
                    quiet = true;
                    break;
                case "--self-elevated":
                    selfElevated = true;
                    break;
                default:
                    throw new ArgumentException($"Unknown argument: {arg}");
            }
        }

        if (string.IsNullOrWhiteSpace(bundleRoot))
        {
            throw new ArgumentException("Missing --bundle-root");
        }

        if (webPort is null)
        {
            throw new ArgumentException("Missing --web-port");
        }

        if (sunshinePort is null)
        {
            throw new ArgumentException("Missing --sunshine-port");
        }

        return new StopOptions(
            Path.GetFullPath(bundleRoot),
            webPort.Value,
            sunshinePort.Value,
            quiet,
            selfElevated
        );
    }

    private static string RequireValue(string[] args, ref int index, string argumentName)
    {
        if (index + 1 >= args.Length)
        {
            throw new ArgumentException($"Missing value for {argumentName}");
        }

        index += 1;
        return args[index];
    }

    private static int ParsePort(string raw, string argumentName)
    {
        if (!int.TryParse(raw, out var value) || value < 1 || value > 65535)
        {
            throw new ArgumentException($"Invalid port for {argumentName}: {raw}");
        }

        return value;
    }

    private static int StopBundleProcesses(StopOptions options)
    {
        var bundleRoot = NormalizePath(options.BundleRoot);
        var targetPaths = new HashSet<string>(StringComparer.OrdinalIgnoreCase)
        {
            NormalizePath(Path.Combine(bundleRoot, "sunshine", "sunshine.exe")),
            NormalizePath(Path.Combine(bundleRoot, "frp", "frpc.exe")),
            NormalizePath(Path.Combine(bundleRoot, "moonlight", "web-server.exe")),
            NormalizePath(Path.Combine(bundleRoot, "moonlight", "streamer.exe")),
            NormalizePath(Path.Combine(bundleRoot, "moonlight", "mic_sidecar.exe")),
            NormalizePath(Path.Combine(bundleRoot, "moonlight", "gamepad_sidecar.exe"))
        };

        var targetPorts = new HashSet<int>
        {
            options.WebPort,
            options.SunshinePort,
            options.SunshinePort + 1
        };

        for (var attempt = 0; attempt < 6; attempt++)
        {
            var candidates = GetBundleProcesses(bundleRoot, targetPaths, targetPorts);
            if (candidates.Count == 0)
            {
                return 0;
            }

            foreach (var process in candidates.OrderByDescending(item => item.ProcessId))
            {
                Log(options, $"Stopping PID {process.ProcessId} ({process.Name})");
                TryKillProcess(process.ProcessId, options);
            }

            Thread.Sleep(500);
        }

        var remaining = GetBundleProcesses(bundleRoot, targetPaths, targetPorts);
        if (remaining.Count == 0)
        {
            return 0;
        }

        if (!options.SelfElevated && !IsAdministrator())
        {
            Log(options, "Cleanup still blocked. Requesting Administrator elevation.");
            return RelaunchElevated(options);
        }

        Log(options, $"Cleanup incomplete. Remaining processes: {string.Join(", ", remaining.Select(item => $"{item.ProcessId}:{item.Name}"))}");
        return 1;
    }

    private static List<ProcessInfo> GetBundleProcesses(string bundleRoot, HashSet<string> targetPaths, HashSet<int> targetPorts)
    {
        var portOwners = GetPortOwners(targetPorts);
        var results = new List<ProcessInfo>();

        using var searcher = new ManagementObjectSearcher("SELECT ProcessId, Name, ExecutablePath, CommandLine FROM Win32_Process");
        using var objects = searcher.Get();
        foreach (ManagementObject managementObject in objects)
        {
            var processId = Convert.ToInt32(managementObject["ProcessId"] ?? 0);
            if (processId <= 0 || processId == Environment.ProcessId)
            {
                continue;
            }

            var name = (managementObject["Name"]?.ToString() ?? string.Empty).Trim();
            if (string.IsNullOrWhiteSpace(name))
            {
                continue;
            }

            var executablePath = NormalizePathOrNull(managementObject["ExecutablePath"]?.ToString());
            var commandLine = managementObject["CommandLine"]?.ToString();

            var isTargetName = TargetNames.Contains(name, StringComparer.OrdinalIgnoreCase);
            var pathMatch = executablePath is not null && targetPaths.Contains(executablePath);
            var commandMatch = isTargetName
                && !string.IsNullOrWhiteSpace(commandLine)
                && commandLine.Contains(bundleRoot, StringComparison.OrdinalIgnoreCase);
            var portMatch = isTargetName && portOwners.Contains(processId);

            if (!pathMatch && !commandMatch && !portMatch)
            {
                continue;
            }

            results.Add(new ProcessInfo(processId, name, executablePath, commandLine));
        }

        return results
            .GroupBy(item => item.ProcessId)
            .Select(group => group.First())
            .ToList();
    }

    private static HashSet<int> GetPortOwners(HashSet<int> targetPorts)
    {
        var owners = new HashSet<int>();
        ParseNetstatOwners("tcp", targetPorts, owners);
        ParseNetstatOwners("udp", targetPorts, owners);
        return owners;
    }

    private static void ParseNetstatOwners(string protocol, HashSet<int> targetPorts, HashSet<int> owners)
    {
        var process = new Process
        {
            StartInfo = new ProcessStartInfo
            {
                FileName = "netstat.exe",
                Arguments = $"-ano -p {protocol}",
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                UseShellExecute = false,
                CreateNoWindow = true,
                StandardOutputEncoding = Encoding.UTF8
            }
        };

        process.Start();
        while (!process.StandardOutput.EndOfStream)
        {
            var line = process.StandardOutput.ReadLine();
            if (string.IsNullOrWhiteSpace(line))
            {
                continue;
            }

            var parts = line.Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries);
            if (parts.Length < 4 || !parts[0].Equals(protocol, StringComparison.OrdinalIgnoreCase))
            {
                continue;
            }

            var localEndpoint = parts[1];
            if (!TryParsePortFromEndpoint(localEndpoint, out var localPort))
            {
                continue;
            }

            if (!targetPorts.Contains(localPort))
            {
                continue;
            }

            if (int.TryParse(parts[^1], out var processId) && processId > 0)
            {
                owners.Add(processId);
            }
        }

        process.WaitForExit();
    }

    private static bool TryParsePortFromEndpoint(string endpoint, out int port)
    {
        port = 0;
        if (string.IsNullOrWhiteSpace(endpoint))
        {
            return false;
        }

        var lastColon = endpoint.LastIndexOf(':');
        if (lastColon < 0 || lastColon == endpoint.Length - 1)
        {
            return false;
        }

        return int.TryParse(endpoint[(lastColon + 1)..], out port);
    }

    private static void TryKillProcess(int processId, StopOptions options)
    {
        try
        {
            using var process = Process.GetProcessById(processId);
            process.Kill(entireProcessTree: true);
            if (!process.WaitForExit(2000))
            {
                process.WaitForExit(2000);
            }
            return;
        }
        catch
        {
            // Fall through to taskkill.
        }

        try
        {
            var taskkill = Path.Combine(Environment.SystemDirectory, "taskkill.exe");
            using var taskkillProcess = new Process
            {
                StartInfo = new ProcessStartInfo
                {
                    FileName = taskkill,
                    Arguments = $"/PID {processId} /T /F",
                    RedirectStandardOutput = true,
                    RedirectStandardError = true,
                    UseShellExecute = false,
                    CreateNoWindow = true
                }
            };
            taskkillProcess.Start();
            taskkillProcess.WaitForExit(4000);
        }
        catch (Exception ex)
        {
            Log(options, $"taskkill failed for PID {processId}: {ex.Message}");
        }
    }

    private static int RelaunchElevated(StopOptions options)
    {
        try
        {
            var executablePath = Environment.ProcessPath;
            if (string.IsNullOrWhiteSpace(executablePath))
            {
                Log(options, "Cannot request elevation because ProcessPath is empty.");
                return 1;
            }

            var argumentList = new[]
            {
                "stop",
                "--bundle-root", QuoteArgument(options.BundleRoot),
                "--web-port", options.WebPort.ToString(),
                "--sunshine-port", options.SunshinePort.ToString(),
                "--self-elevated"
            }.ToList();

            if (options.Quiet)
            {
                argumentList.Add("--quiet");
            }

            using var process = Process.Start(new ProcessStartInfo
            {
                FileName = executablePath,
                Arguments = string.Join(" ", argumentList),
                UseShellExecute = true,
                Verb = "runas"
            });

            if (process is null)
            {
                return 1;
            }

            process.WaitForExit();
            return process.ExitCode;
        }
        catch (Win32Exception ex) when (ex.NativeErrorCode == 1223)
        {
            Log(options, "Administrator elevation was cancelled.");
            return 1;
        }
    }

    private static string QuoteArgument(string value)
    {
        if (string.IsNullOrEmpty(value))
        {
            return "\"\"";
        }

        if (!value.Any(char.IsWhiteSpace) && !value.Contains('"'))
        {
            return value;
        }

        return $"\"{value.Replace("\\", "\\\\").Replace("\"", "\\\"")}\"";
    }

    private static bool IsAdministrator()
    {
        using var identity = WindowsIdentity.GetCurrent();
        var principal = new WindowsPrincipal(identity);
        return principal.IsInRole(WindowsBuiltInRole.Administrator);
    }

    private static string NormalizePath(string value) =>
        Path.GetFullPath(value.Trim().Trim('"'));

    private static string? NormalizePathOrNull(string? value)
    {
        if (string.IsNullOrWhiteSpace(value))
        {
            return null;
        }

        try
        {
            return NormalizePath(value);
        }
        catch
        {
            return null;
        }
    }

    private static void Log(StopOptions options, string message)
    {
        if (!options.Quiet)
        {
            Console.WriteLine($"[bundle-process-manager] {message}");
        }
    }
}
