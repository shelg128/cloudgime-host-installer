using System.IO.Pipes;
using System.Security.Principal;
using System.Text.Json;
using System.Text.RegularExpressions;

namespace DisplayPrepareHelper;

internal static partial class Program
{
    private static string? _stateBroker;
    private static int _stateBrokerPid;

    private static T? BrokerRequest<T>(string operation, object? value = null)
    {
        var reply = DisplayStateTransport.Exchange(_stateBroker!, _stateBrokerPid, new { operation, value });
        return reply.ValueKind == JsonValueKind.Null ? default : reply.Deserialize<T>(JsonOptions);
    }
    private static int RunBrokerChild(string[] args)
    {
        if (args.Length < 5 || args[2] != "--broker-pid" || !int.TryParse(args[3], out _stateBrokerPid)) return 1;
        _stateBroker = args[1];
        if (!Regex.IsMatch(_stateBroker, "^CloudgimeDisplay-[a-f0-9]{32}$")) return 1;
        var options = ParseArguments(args.Skip(4).ToArray());
        if (options.Command is not (CommandKind.Prepare or CommandKind.Resize or CommandKind.Restore or CommandKind.ProjectDisplay)) return 1;
        // Lifetime is enforced by the parent's kill-on-close job object, not by
        // opening the SYSTEM broker process from a limited renter token.
        // Authenticate the broker immediately, before touching the interactive display.
        BrokerRequest<PrepareStateFile>("read");
        HelperResult result;
        try { result = Run(args.Skip(4).ToArray()); }
        catch { result = Failure("display_operation_failed"); }
        try { BrokerRequest<JsonElement>("complete", result); }
        catch { return 1; }
        return result.Ok ? 0 : 1;
    }

    private static HelperResult RunStateBroker(string[] args)
    {
        if (!WindowsIdentity.GetCurrent().IsSystem) return Failure("display_broker_requires_system");
        if (args.Length < 4 || !uint.TryParse(args[1], out var session) || session == 0) return Failure("invalid_interactive_session");
        var childArgs = args.Skip(2).ToArray();
        var options = ParseArguments(childArgs);
        if (options.Command is not (CommandKind.Prepare or CommandKind.Resize or CommandKind.Restore or CommandKind.ProjectDisplay))
            return Failure("broker_action_not_allowed");
        if (string.IsNullOrWhiteSpace(options.SessionToken) || options.SessionToken.Length > 512) return Failure("broker_session_required");
        // Lock is in the administrator-owned helper directory, never in renter TEMP.
        using var stateLock = new FileStream(StatePath + ".lock", FileMode.OpenOrCreate, FileAccess.ReadWrite, FileShare.None);
        var before = ReadState();
        if (options.Command != CommandKind.Prepare && StateBelongsToDifferentSession(before, options.SessionToken))
            return options.Command == CommandKind.Restore
                ? new HelperResult { Ok = true, Skipped = true, Reason = "superseded_by_newer_session" }
                : Failure("superseded_by_newer_session");
        if (options.Command is CommandKind.Resize or CommandKind.ProjectDisplay && before == null) return Failure("display_session_not_prepared");

        if (options.Command is CommandKind.Prepare or CommandKind.Resize && options.Width > 0 && options.Height > 0 &&
            PreferenceMayUseMttVdd(ReadStreamDisplayPreference(options.BundleRoot))) {
            // Only the already-authorized command dimensions are accepted, never
            // a child-supplied path, driver identifier or executable.
            EnsureBrokerExactVddModes(options.Width, options.Height, options.Fps);
        }

        var sameSessionBefore = before != null && before.SessionToken == options.SessionToken ? before : null;
        var pipeName = "CloudgimeDisplay-" + Guid.NewGuid().ToString("N");
        var sid = DisplayStateTransport.SessionSid(session);
        using var pipe = DisplayStateTransport.CreateServer(pipeName, sid);
        var launch = DisplayStateTransport.StartChild(session, childArgs, pipeName);
        using var child = launch.child;
        if (launch.sid != sid) return Failure("interactive_user_changed");
        using var deadline = new CancellationTokenSource(TimeSpan.FromSeconds(70));
        bool committed = false;
        string stage = "connect";
        for (int messages = 0; messages < 32; messages++) {
            try {
                stage = "wait_request";
                pipe.WaitForConnectionAsync(deadline.Token).GetAwaiter().GetResult();
                if (!DisplayStateTransport.IsChild(pipe, child.Pid, session)) { pipe.Disconnect(); continue; }
                var request = DisplayStateTransport.Read(pipe, deadline.Token).GetAwaiter().GetResult();
                var operation = request.GetProperty("operation").GetString();
                stage = operation is "read" or "write" or "delete" or "capture" or "complete" ? operation : "unknown";
                var value = request.GetProperty("value");
                object? reply;
                bool complete = false;
                switch (operation) {
                    case "read": reply = ReadState(); break;
                    case "write": {
                        if (options.Command == CommandKind.Restore) throw new UnauthorizedAccessException();
                        var next = value.Deserialize<PrepareStateFile>(JsonOptions) ?? throw new InvalidDataException();
                        ValidateBrokerState(next, options.SessionToken);
                        // Only interactive display state is accepted. Process/device changes cannot be
                        // supplied by an unprivileged child and later replayed by an elevated restore.
                        next.SuspendedRemoteProcesses = sameSessionBefore?.SuspendedRemoteProcesses ?? [];
                        next.BoostedProcesses = sameSessionBefore?.BoostedProcesses ?? [];
                        next.TemperedRemoteProcesses = sameSessionBefore?.TemperedRemoteProcesses ?? [];
                        next.DisabledDisplayClassDevices = sameSessionBefore?.DisabledDisplayClassDevices ?? [];
                        if (sameSessionBefore != null) {
                            next.PreviousPrimary = sameSessionBefore.PreviousPrimary;
                            next.PreviousVdd = sameSessionBefore.PreviousVdd;
                            next.PreviousOtherDisplays = sameSessionBefore.PreviousOtherDisplays;
                            next.PreviousCursor = sameSessionBefore.PreviousCursor;
                        }
                        WriteState(next); committed = true; reply = null; break;
                    }
                    case "delete":
                        if (options.Command != CommandKind.Restore) throw new UnauthorizedAccessException();
                        var current = ReadState();
                        if (StateBelongsToDifferentSession(current, options.SessionToken)) throw new UnauthorizedAccessException();
                        File.Delete(StatePath); committed = true; reply = null; break;
                    case "capture": {
                        if (options.Command == CommandKind.Restore) throw new UnauthorizedAccessException();
                        var display = value.Deserialize<DisplaySnapshot>(JsonOptions) ?? throw new InvalidDataException();
                        ValidateCaptureDisplay(display);
                        // Only this launched interactive child can observe its desktop. Session 0
                        // must not substitute its different display topology. No client filesystem path
                        // or config text crosses the pipe: parent resolves its configured bundle itself.
                        reply = ApplyBrokerCaptureConfig(options.BundleRoot, display); break;
                    }
                    case "complete": {
                        var result = value.Deserialize<HelperResult>(JsonOptions) ?? throw new InvalidDataException();
                        if (result.Ok && !result.Skipped && options.Command != CommandKind.Restore && !committed)
                            result = Failure("display_state_not_committed");
                        reply = result; complete = true; break;
                    }
                    default: throw new UnauthorizedAccessException("Broker operation not allowed.");
                }
                stage = "send_" + stage;
                DisplayStateTransport.Send(pipe, new { ok = true, value = reply }, deadline.Token).GetAwaiter().GetResult();
                stage = "receipt_" + stage;
                var receipt = DisplayStateTransport.Read(pipe, deadline.Token).GetAwaiter().GetResult();
                if (!receipt.TryGetProperty("received", out var received) || !received.GetBoolean()) throw new InvalidDataException("Missing response receipt.");
                pipe.Disconnect();
                if (complete) {
                    var limit = DateTime.UtcNow.AddSeconds(3);
                    while (!child.Exited && DateTime.UtcNow < limit) Thread.Sleep(10);
                    if (!child.Exited) return Failure("display_worker_exit_timeout");
                    var result = (HelperResult)reply!;
                    if (child.ExitCode != (result.Ok ? 0 : 1)) return Failure("display_worker_status_mismatch");
                    return result;
                }
            } catch (Exception error) {
                Console.Error.WriteLine("Display broker rejected/failed operation: " + error.GetType().Name + " stage=" + stage);
                return Failure("display_broker_operation_failed_" + stage + "_" + error.GetType().Name.ToLowerInvariant());
            }
        }
        return Failure("display_broker_message_limit");
    }
    private static bool AddBrokerExactModes(System.Xml.Linq.XDocument doc, int width, int height, int fps)
    {
        var root = doc.Root ?? throw new InvalidDataException("Missing VDD configuration.");
        var resolutions = root.Element("resolutions") ?? throw new InvalidDataException("Missing VDD resolutions.");
        return DisplayModeCatalog.AddDeviceOrientations(resolutions,width,height,fps);
    }
    private static void EnsureBrokerExactVddModes(int width, int height, int fps)
    {
        width = Math.Clamp(width,320,7680) & ~1; height = Math.Clamp(height,320,4320) & ~1;
        fps = fps >= 30 ? Math.Min(fps,120) : 60;
        if (!File.Exists(VddSettingsPath)) throw new IOException("VDD configuration missing.");
        var directory = Path.GetDirectoryName(VddSettingsPath)!;
        if ((File.GetAttributes(VddSettingsPath) & FileAttributes.ReparsePoint) != 0 ||
            (File.GetAttributes(directory) & FileAttributes.ReparsePoint) != 0) throw new IOException("VDD configuration reparse point rejected.");
        var settings = new System.Xml.XmlReaderSettings { DtdProcessing = System.Xml.DtdProcessing.Prohibit, XmlResolver = null, MaxCharactersInDocument = 262144 };
        System.Xml.Linq.XDocument doc;
        using (var reader = System.Xml.XmlReader.Create(VddSettingsPath, settings)) doc = System.Xml.Linq.XDocument.Load(reader);
        if (!AddBrokerExactModes(doc,width,height,fps)) {
            Console.Error.WriteLine($"DISPLAY_PROFILE_REUSED width={width} height={height} fps={fps}");
            return;
        }
        // Persist only an additive mode-list update. Other settings and all existing
        // modes survive; rotating back does not require a second driver reload.
        var backup = Path.Combine(AppContext.BaseDirectory,"display-vdd-before-"+Guid.NewGuid().ToString("N")+".xml");
        File.Copy(VddSettingsPath,backup,false);
        var temp = Path.Combine(directory,"cg-mode-"+Guid.NewGuid().ToString("N")+".tmp");
        try { doc.Save(temp); File.Replace(temp,VddSettingsPath,null); }
        finally { if(File.Exists(temp)) File.Delete(temp); }
        EnsureVddRegistryPath();
        TryRestartMttVddDevice();
        Console.Error.WriteLine($"DISPLAY_PROFILE_REGISTERED width={width} height={height} fps={fps} orientations=2");
    }

    private static void ValidateCaptureDisplay(DisplaySnapshot display)
    {
        if (!display.Active || !Regex.IsMatch(display.DeviceName ?? "", @"^\\\\\.\\DISPLAY[0-9]{1,3}$") ||
            display.Width is < 320 or > 16384 || display.Height is < 240 or > 16384 || display.Frequency is < 1 or > 1000 ||
            display.DeviceId.Length > 4096 || display.DeviceString.Length > 4096)
            throw new InvalidDataException("Invalid capture display.");
    }
    private static SunshineCaptureConfigApplyResult ApplyBrokerCaptureConfig(string bundleRoot, DisplaySnapshot display)
    {
        var configPath = ResolveActiveSunshineConfigPath(bundleRoot);
        var result = TryApplySunshineStreamCaptureConfig(configPath, display);
        var expected = DisplayLooksLikeMttVdd(display) || IsParsecDisplay(display)
            ? display.DeviceName : null;
        var actual = NormalizeConfigValue(TryReadConfigValue(configPath, "output_name"));
        if (!string.Equals(expected, actual, StringComparison.OrdinalIgnoreCase))
            throw new IOException("Capture output configuration not persisted.");
        return result;
    }

    private static void ValidateBrokerState(PrepareStateFile state, string session)
    {
        if (state.SessionToken != session) throw new UnauthorizedAccessException("State scope mismatch.");
        var modes = new[] { state.Requested, state.AppliedDisplay, state.AppliedVdd, state.PreviousPrimary, state.PreviousVdd }
            .Concat(state.PreviousOtherDisplays);
        if (state.PreviousOtherDisplays.Count > 32) throw new InvalidDataException();
        foreach (var mode in modes) {
            if (mode == null) continue;
            if (mode.Width < 0 || mode.Height < 0 || mode.Width > 16384 || mode.Height > 16384 || mode.Frequency < 0 || mode.Frequency > 1000)
                throw new InvalidDataException("Invalid display dimensions.");
            if (!string.IsNullOrEmpty(mode.DeviceName) && !Regex.IsMatch(mode.DeviceName, @"^\\\\\.\\DISPLAY[0-9]{1,3}$")) throw new InvalidDataException();
        }
    }
}
