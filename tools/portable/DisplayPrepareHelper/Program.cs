using System.ComponentModel;
using System.Diagnostics;
using System.Drawing;
using System.Globalization;
using Microsoft.Win32;
using System.Runtime.InteropServices;
using System.Xml.Linq;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Text.Json.Nodes;
using System.Windows.Forms;

namespace DisplayPrepareHelper;

internal enum CommandKind
{
    Prepare,
    Resize,
    Restore,
    Preflight,
    WatchWindowPrimary,
    ProjectDisplay,
    PersistentVddOnly,
    ListDisplays,
    SetStreamDisplay
}

internal sealed record Options(
    CommandKind Command,
    string SessionToken,
    int Width,
    int Height,
    int Fps,
    string BundleRoot,
    bool ApplyPreflightConfig,
    bool BypassPreflightCache,
    int PollMs,
    string ProjectMode,
    string DisplayMode,
    string DisplayDeviceName,
    string DisplayDeviceId,
    string DisplayLabel
);

internal sealed class HelperResult
{
    [JsonPropertyName("ok")]
    public bool Ok { get; set; }

    [JsonPropertyName("changed")]
    public bool Changed { get; set; }

    [JsonPropertyName("restored")]
    public bool Restored { get; set; }

    [JsonPropertyName("skipped")]
    public bool Skipped { get; set; }

    [JsonPropertyName("reason")]
    public string Reason { get; set; } = string.Empty;

    [JsonPropertyName("applied")]
    public SavedDisplayState? Applied { get; set; }

    [JsonPropertyName("profile_path")]
    public string? ProfilePath { get; set; }

    [JsonPropertyName("config_path")]
    public string? ConfigPath { get; set; }

    [JsonPropertyName("selected_encoder")]
    public string? SelectedEncoder { get; set; }

    [JsonPropertyName("selected_capture")]
    public string? SelectedCapture { get; set; }

    [JsonPropertyName("selected_runtime")]
    public string? SelectedRuntime { get; set; }

    [JsonPropertyName("sunshine_capture_changed")]
    public bool SunshineCaptureChanged { get; set; }

    [JsonPropertyName("sunshine_capture_target_changed")]
    public bool SunshineCaptureTargetChanged { get; set; }

    [JsonPropertyName("sunshine_capture_display")]
    public string? SunshineCaptureDisplay { get; set; }

    [JsonPropertyName("sunshine_capture_config_path")]
    public string? SunshineCaptureConfigPath { get; set; }

    [JsonPropertyName("displays")]
    public List<DisplayControlDisplayInfo> Displays { get; set; } = [];

    [JsonPropertyName("stream_display_preference")]
    public DisplayControlPreferenceInfo? StreamDisplayPreference { get; set; }

    [JsonPropertyName("selected_display_label")]
    public string? SelectedDisplayLabel { get; set; }

    [JsonPropertyName("active_display_label")]
    public string? ActiveDisplayLabel { get; set; }
}

internal sealed record SunshineCaptureConfigApplyResult(
    bool Changed,
    bool TargetChanged,
    string? ConfigPath,
    string? DisplayName
);

internal sealed class GpuControllerInfo
{
    [JsonPropertyName("name")]
    public string Name { get; set; } = string.Empty;

    [JsonPropertyName("driver_version")]
    public string DriverVersion { get; set; } = string.Empty;
}

internal sealed class EncoderProbeResult
{
    [JsonPropertyName("runtime_key")]
    public string RuntimeKey { get; set; } = string.Empty;

    [JsonPropertyName("runtime_directory")]
    public string RuntimeDirectory { get; set; } = string.Empty;

    [JsonPropertyName("encoder_key")]
    public string EncoderKey { get; set; } = string.Empty;

    [JsonPropertyName("ffmpeg_codec")]
    public string FfmpegCodec { get; set; } = string.Empty;

    [JsonPropertyName("available")]
    public bool Available { get; set; }

    [JsonPropertyName("ok")]
    public bool Ok { get; set; }

    [JsonPropertyName("detail")]
    public string Detail { get; set; } = string.Empty;
}

internal sealed class SunshineRuntimeCandidate
{
    [JsonPropertyName("key")]
    public string Key { get; set; } = string.Empty;

    [JsonPropertyName("relative_directory")]
    public string RelativeDirectory { get; set; } = string.Empty;

    [JsonPropertyName("root_path")]
    public string RootPath { get; set; } = string.Empty;

    [JsonPropertyName("config_path")]
    public string ConfigPath { get; set; } = string.Empty;

    [JsonPropertyName("ffmpeg_path")]
    public string? FfmpegPath { get; set; }

    [JsonPropertyName("ffmpeg_source")]
    public string? FfmpegSource { get; set; }

    [JsonPropertyName("requires_bundled_ffmpeg")]
    public bool RequiresBundledFfmpeg { get; set; }

    [JsonPropertyName("healthy_encoders")]
    public List<string> HealthyEncoders { get; set; } = [];

    [JsonPropertyName("runtime_status")]
    public string RuntimeStatus { get; set; } = "unknown";

    [JsonPropertyName("runtime_status_reason")]
    public string? RuntimeStatusReason { get; set; }

    [JsonPropertyName("legacy")]
    public bool Legacy { get; set; }

    [JsonPropertyName("display_name")]
    public string? DisplayName { get; set; }

    [JsonPropertyName("runtime_version")]
    public string? RuntimeVersion { get; set; }

    [JsonPropertyName("runtime_fingerprint")]
    public string? RuntimeFingerprint { get; set; }

    [JsonPropertyName("priority")]
    public int Priority { get; set; }

    [JsonPropertyName("auto_select")]
    public bool AutoSelect { get; set; } = true;

    [JsonPropertyName("startup_validation_status")]
    public string? StartupValidationStatus { get; set; }

    [JsonPropertyName("startup_validation_reason")]
    public string? StartupValidationReason { get; set; }

    [JsonPropertyName("startup_validation_checked_at")]
    public string? StartupValidationCheckedAt { get; set; }
}

internal sealed class SunshineRuntimeManifestEntry
{
    [JsonPropertyName("key")]
    public string Key { get; set; } = string.Empty;

    [JsonPropertyName("relative_directory")]
    public string RelativeDirectory { get; set; } = string.Empty;

    [JsonPropertyName("ffmpeg_relative_path")]
    public string? FfmpegRelativePath { get; set; }

    [JsonPropertyName("requires_bundled_ffmpeg")]
    public bool RequiresBundledFfmpeg { get; set; }

    [JsonPropertyName("legacy")]
    public bool Legacy { get; set; }

    [JsonPropertyName("display_name")]
    public string? DisplayName { get; set; }

    [JsonPropertyName("runtime_version")]
    public string? RuntimeVersion { get; set; }

    [JsonPropertyName("priority")]
    public int Priority { get; set; }

    [JsonPropertyName("auto_select")]
    public bool AutoSelect { get; set; } = true;

    [JsonPropertyName("startup_validation_status")]
    public string? StartupValidationStatus { get; set; }

    [JsonPropertyName("startup_validation_reason")]
    public string? StartupValidationReason { get; set; }

    [JsonPropertyName("startup_validation_checked_at")]
    public string? StartupValidationCheckedAt { get; set; }
}

internal sealed class SunshineRuntimeManifest
{
    [JsonPropertyName("version")]
    public int Version { get; set; } = 1;

    [JsonPropertyName("runtimes")]
    public List<SunshineRuntimeManifestEntry> Runtimes { get; set; } = [];
}

internal sealed class HostCapabilityProfile
{
    [JsonPropertyName("updated_at")]
    public string UpdatedAt { get; set; } = DateTimeOffset.UtcNow.ToString("O");

    [JsonPropertyName("probe_mode")]
    public string ProbeMode { get; set; } = "fresh";

    [JsonPropertyName("bundle_root")]
    public string BundleRoot { get; set; } = string.Empty;

    [JsonPropertyName("config_path")]
    public string ConfigPath { get; set; } = string.Empty;

    [JsonPropertyName("ffmpeg_path")]
    public string? FfmpegPath { get; set; }

    [JsonPropertyName("selected_ffmpeg_source")]
    public string? SelectedFfmpegSource { get; set; }

    [JsonPropertyName("force_nvenc_enabled")]
    public bool ForceNvencEnabled { get; set; }

    [JsonPropertyName("gpu_controllers")]
    public List<GpuControllerInfo> GpuControllers { get; set; } = [];

    [JsonPropertyName("audio_endpoints")]
    public List<HostAudioEndpointInfo> AudioEndpoints { get; set; } = [];

    [JsonPropertyName("runtime_candidates")]
    public List<SunshineRuntimeCandidate> RuntimeCandidates { get; set; } = [];

    [JsonPropertyName("selected_runtime_key")]
    public string SelectedRuntimeKey { get; set; } = "default";

    [JsonPropertyName("selected_runtime_directory")]
    public string SelectedRuntimeDirectory { get; set; } = "sunshine";

    [JsonPropertyName("selected_runtime_display_name")]
    public string? SelectedRuntimeDisplayName { get; set; }

    [JsonPropertyName("selected_runtime_version")]
    public string? SelectedRuntimeVersion { get; set; }

    [JsonPropertyName("selected_runtime_fingerprint")]
    public string? SelectedRuntimeFingerprint { get; set; }

    [JsonPropertyName("selected_encoder")]
    public string SelectedEncoder { get; set; } = "auto";

    [JsonPropertyName("selected_capture")]
    public string SelectedCapture { get; set; } = "ddx";

    [JsonPropertyName("selected_capture_reason")]
    public string? SelectedCaptureReason { get; set; }

    [JsonPropertyName("selected_audio_sink_name")]
    public string? SelectedAudioSinkName { get; set; }

    [JsonPropertyName("selected_virtual_sink_name")]
    public string? SelectedVirtualSinkName { get; set; }

    [JsonPropertyName("selected_microphone_name")]
    public string? SelectedMicrophoneName { get; set; }

    [JsonPropertyName("audio_selection_mode")]
    public string AudioSelectionMode { get; set; } = "auto";

    [JsonPropertyName("audio_selection_reason")]
    public string? AudioSelectionReason { get; set; }

    [JsonPropertyName("selection_reason")]
    public string SelectionReason { get; set; } = string.Empty;

    [JsonPropertyName("software_min_threads")]
    public int SoftwareMinThreads { get; set; }

    [JsonPropertyName("encoder_probes")]
    public List<EncoderProbeResult> EncoderProbes { get; set; } = [];

    [JsonPropertyName("warnings")]
    public List<string> Warnings { get; set; } = [];

    [JsonPropertyName("config_applied")]
    public bool ConfigApplied { get; set; }
}

internal sealed class HostAudioEndpointInfo
{
    [JsonPropertyName("direction")]
    public string Direction { get; set; } = string.Empty;

    [JsonPropertyName("device_id")]
    public string DeviceId { get; set; } = string.Empty;

    [JsonPropertyName("name")]
    public string Name { get; set; } = string.Empty;
}

internal sealed class HostAudioPreferences
{
    [JsonPropertyName("schema_version")]
    public int SchemaVersion { get; set; } = 1;

    [JsonPropertyName("mode")]
    public string Mode { get; set; } = "auto";

    [JsonPropertyName("selected_audio_sink_name")]
    public string? SelectedAudioSinkName { get; set; }

    [JsonPropertyName("selected_virtual_sink_name")]
    public string? SelectedVirtualSinkName { get; set; }

    [JsonPropertyName("selected_microphone_name")]
    public string? SelectedMicrophoneName { get; set; }

    [JsonPropertyName("updated_at")]
    public string UpdatedAt { get; set; } = DateTimeOffset.UtcNow.ToString("O");
}

internal sealed class RuntimeMetadata
{
    public string? DisplayName { get; set; }
    public string? RuntimeVersion { get; set; }
    public string? RuntimeFingerprint { get; set; }
    public bool RequiresBundledFfmpeg { get; set; }
    public bool AutoSelect { get; set; } = true;
    public string? StartupValidationStatus { get; set; }
    public string? StartupValidationReason { get; set; }
    public string? StartupValidationCheckedAt { get; set; }
}

internal sealed class SharedSunshineIdentityPaths
{
    public string RootPath { get; set; } = string.Empty;
    public string StatePath { get; set; } = string.Empty;
    public string KeyPath { get; set; } = string.Empty;
    public string CertPath { get; set; } = string.Empty;
    public string StatePathForConfig { get; set; } = string.Empty;
    public string KeyPathForConfig { get; set; } = string.Empty;
    public string CertPathForConfig { get; set; } = string.Empty;
}

internal sealed class SunshineIdentitySource
{
    public string Label { get; set; } = string.Empty;
    public string? StatePath { get; set; }
    public string? KeyPath { get; set; }
    public string? CertPath { get; set; }
}

internal sealed class ResolvedFfmpegPath
{
    public string? Path { get; set; }
    public string? Source { get; set; }
}

internal sealed class CursorState
{
    [JsonPropertyName("x")]
    public int X { get; set; }

    [JsonPropertyName("y")]
    public int Y { get; set; }
}

internal sealed class SuspendedProcessState
{
    [JsonPropertyName("pid")]
    public int Pid { get; set; }

    [JsonPropertyName("name")]
    public string Name { get; set; } = string.Empty;
}

internal sealed class RemoteProcessMitigationResult
{
    public List<SuspendedProcessState> SuspendedProcesses { get; } = [];

    public bool Changed => SuspendedProcesses.Count > 0;
}

internal sealed class PriorityBoostState
{
    [JsonPropertyName("pid")]
    public int Pid { get; set; }

    [JsonPropertyName("name")]
    public string Name { get; set; } = string.Empty;

    [JsonPropertyName("previous_priority")]
    public string PreviousPriority { get; set; } = string.Empty;
}

internal sealed class SavedDisplayState
{
    [JsonPropertyName("display_id")]
    public int DisplayId { get; set; }

    [JsonPropertyName("device_name")]
    public string DeviceName { get; set; } = string.Empty;

    [JsonPropertyName("device_id")]
    public string DeviceId { get; set; } = string.Empty;

    [JsonPropertyName("device_string")]
    public string DeviceString { get; set; } = string.Empty;

    [JsonPropertyName("width")]
    public int Width { get; set; }

    [JsonPropertyName("height")]
    public int Height { get; set; }

    [JsonPropertyName("frequency")]
    public double Frequency { get; set; }

    [JsonPropertyName("position_x")]
    public int PositionX { get; set; }

    [JsonPropertyName("position_y")]
    public int PositionY { get; set; }

    [JsonPropertyName("primary")]
    public bool Primary { get; set; }

    [JsonPropertyName("active")]
    public bool Active { get; set; }
}

internal sealed class PrepareStateFile
{
    [JsonPropertyName("helper")]
    public Dictionary<string, string> Helper { get; set; } = new()
    {
        ["updated_at"] = DateTimeOffset.UtcNow.ToString("O"),
        ["kind"] = "display_prepare_helper",
    };

    [JsonPropertyName("session_token")]
    public string SessionToken { get; set; } = string.Empty;

    [JsonPropertyName("requested")]
    public SavedDisplayState? Requested { get; set; }

    [JsonPropertyName("previous_primary")]
    public SavedDisplayState? PreviousPrimary { get; set; }

    [JsonPropertyName("previous_vdd")]
    public SavedDisplayState? PreviousVdd { get; set; }

    [JsonPropertyName("previous_other_displays")]
    public List<SavedDisplayState> PreviousOtherDisplays { get; set; } = [];

    [JsonPropertyName("previous_cursor")]
    public CursorState? PreviousCursor { get; set; }

    [JsonPropertyName("cursor_hidden")]
    public bool CursorHidden { get; set; }

    [JsonPropertyName("applied_vdd")]
    public SavedDisplayState? AppliedVdd { get; set; }

    [JsonPropertyName("applied_display")]
    public SavedDisplayState? AppliedDisplay { get; set; }

    [JsonPropertyName("stream_display_mode")]
    public string StreamDisplayMode { get; set; } = "mtt_vdd";

    [JsonPropertyName("suspended_remote_processes")]
    public List<SuspendedProcessState> SuspendedRemoteProcesses { get; set; } = [];

    [JsonPropertyName("boosted_processes")]
    public List<PriorityBoostState> BoostedProcesses { get; set; } = [];

    [JsonPropertyName("tempered_remote_processes")]
    public List<PriorityBoostState> TemperedRemoteProcesses { get; set; } = [];

    [JsonPropertyName("disabled_display_class_devices")]
    public List<string> DisabledDisplayClassDevices { get; set; } = [];
}

internal sealed class StreamDisplayPreference
{
    [JsonPropertyName("schema_version")]
    public int SchemaVersion { get; set; } = 1;

    [JsonPropertyName("manual_override")]
    public bool ManualOverride { get; set; }

    [JsonPropertyName("mode")]
    public string Mode { get; set; } = "auto";

    [JsonPropertyName("custom_device_name")]
    public string CustomDeviceName { get; set; } = string.Empty;

    [JsonPropertyName("custom_device_id")]
    public string CustomDeviceId { get; set; } = string.Empty;

    [JsonPropertyName("custom_label")]
    public string CustomLabel { get; set; } = string.Empty;
}

internal sealed class DisplayControlPreferenceInfo
{
    [JsonPropertyName("mode")]
    public string Mode { get; set; } = "mtt_vdd";

    [JsonPropertyName("manual_override")]
    public bool ManualOverride { get; set; }

    [JsonPropertyName("custom_device_name")]
    public string CustomDeviceName { get; set; } = string.Empty;

    [JsonPropertyName("custom_device_id")]
    public string CustomDeviceId { get; set; } = string.Empty;

    [JsonPropertyName("custom_label")]
    public string CustomLabel { get; set; } = string.Empty;
}

internal sealed class DisplayControlDisplayInfo
{
    [JsonPropertyName("display_id")]
    public int DisplayId { get; set; }

    [JsonPropertyName("device_name")]
    public string DeviceName { get; set; } = string.Empty;

    [JsonPropertyName("device_id")]
    public string DeviceId { get; set; } = string.Empty;

    [JsonPropertyName("device_string")]
    public string DeviceString { get; set; } = string.Empty;

    [JsonPropertyName("label")]
    public string Label { get; set; } = string.Empty;

    [JsonPropertyName("width")]
    public int Width { get; set; }

    [JsonPropertyName("height")]
    public int Height { get; set; }

    [JsonPropertyName("frequency")]
    public int Frequency { get; set; }

    [JsonPropertyName("active")]
    public bool Active { get; set; }

    [JsonPropertyName("primary")]
    public bool Primary { get; set; }

    [JsonPropertyName("is_virtual_display")]
    public bool IsVirtualDisplay { get; set; }

    [JsonPropertyName("is_mtt_vdd")]
    public bool IsMttVdd { get; set; }

    [JsonPropertyName("selected_preference")]
    public bool SelectedPreference { get; set; }

    [JsonPropertyName("current_stream_target")]
    public bool CurrentStreamTarget { get; set; }
}

internal sealed record DisplaySnapshot(
    int DisplayId,
    string DeviceName,
    string DeviceId,
    string DeviceString,
    int Width,
    int Height,
    int Frequency,
    int PositionX,
    int PositionY,
    bool Primary,
    bool Active,
    bool IsVdd,
    int Orientation,
    int StreamDisplayPriority
)
{
    private bool QuarterTurn => Orientation is 1 or 3;
    private bool NeedsQuarterTurnSizeSwap => QuarterTurn && Width > Height;

    public SavedDisplayState ToSavedState() => new()
    {
        DisplayId = DisplayId,
        DeviceName = DeviceName,
        DeviceId = DeviceId,
        DeviceString = DeviceString,
        Width = NeedsQuarterTurnSizeSwap ? Height : Width,
        Height = NeedsQuarterTurnSizeSwap ? Width : Height,
        Frequency = Frequency,
        PositionX = PositionX,
        PositionY = PositionY,
        Primary = Primary,
        Active = Active,
    };
}

internal static class Program
{
    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
        WriteIndented = false,
    };
    private static readonly SavedDisplayState DefaultIdleVddMode = new()
    {
        Width = 1920,
        Height = 1080,
        Frequency = 60,
    };
    private static readonly bool DisableOtherDisplaysDuringStream = true;
    private static readonly bool SuspendCompetingRemoteAppsDuringStream = false;
    private static readonly bool LowerCompetingRemoteAppsDuringStream = false;
    private static readonly bool ArrangeWindowsOnStreamDisplay = true;
    private static readonly bool CenterCursorOnStreamDisplay = false;
    private static readonly bool DisableOtherDisplaysWhenStreamAuthorityFails = true;
    private static readonly bool RepositionOtherDisplaysWhenMakingPrimary = false;
    private static readonly bool DuplicateMttVddWithPrimary = false;
    private static readonly HashSet<string> CompetingRemoteProcessNames = new(StringComparer.OrdinalIgnoreCase)
    {
        "remoting_host",
        "anydesk",
        "teamviewer",
        "teamviewer_service",
        "tv_x64",
        "parsecd",
        "parsec",
        "rustdesk",
        "todesk",
        "srmanager",
        "srservice",
        "splashtop_streamer",
        "awrcc",
        "awremoteservice",
    };
    private static readonly HashSet<string> PriorityBoostProcessNames = new(StringComparer.OrdinalIgnoreCase)
    {
        "web-server",
        "web-server-live",
        "streamer",
        "mic_sidecar",
        "sunshine",
        "sunshinesvc",
    };

    private static readonly string StatePath = Path.Combine(AppContext.BaseDirectory, "display_prepare_state.json");
    private static bool _driverActionAttempted = false;
    private static readonly string VddSettingsPath = Path.Combine(@"C:\VirtualDisplayDriver", "vdd_settings.xml");
    private const string StreamDisplayPreferencesFileName = "stream_display_preferences.json";
    private static readonly (int Width, int Height, int Fps)[] CloudgimeVddBaseModes = new (int Width, int Height, int Fps)[]
    {
        (800, 600, 30),
        (1280, 720, 45),
        (1280, 720, 60),
        (1280, 800, 30),
        (1280, 800, 60),
        (1920, 1080, 30),
        (1920, 1080, 60),
        (2560, 1440, 30),
        (3840, 2160, 30),
        (540, 960, 30),
        (720, 1280, 30),
        (720, 1280, 60),
        (720, 1360, 30),
        (720, 1408, 30),
        (720, 1440, 30),
        (720, 1520, 30),
        (720, 1560, 30),
        (720, 1600, 30),
        (720, 1640, 30),
        (720, 1680, 30),
        (1080, 1920, 30),
        (1080, 1920, 60),
        (1080, 2160, 30),
        (1080, 2280, 30),
        (1080, 2340, 30),
        (1080, 2400, 30),
        (1080, 2460, 30),
        (1200, 1920, 30),
        (1200, 1920, 60),
        (1344, 2160, 30),
        (1344, 2160, 60),
        (1440, 2560, 30),
        (2160, 3840, 30),
        (1360, 720, 30),
        (1408, 720, 30),
        (1440, 720, 30),
        (1520, 720, 30),
        (1560, 720, 30),
        (1600, 720, 30),
        (1640, 720, 30),
        (1680, 720, 30),
        (2160, 1080, 30),
        (2280, 1080, 30),
        (2340, 1080, 30),
        (2400, 1080, 30),
        (2460, 1080, 30),
        (2160, 1344, 30),
        (2160, 1344, 60),
    };

    private const int EnumCurrentSettings = -1;
    private const int EnumRegistrySettings = -2;
    private const int DmPosition = 0x00000020;
    private const int DmDisplayOrientation = 0x00000080;
    private const int DmBitsPerPel = 0x00040000;
    private const int DmPelsWidth = 0x00080000;
    private const int DmPelsHeight = 0x00100000;
    private const int DmDisplayFrequency = 0x00400000;
    private const int DmdoDefault = 0;
    private const int Dmdo90 = 1;
    private const int Dmdo180 = 2;
    private const int Dmdo270 = 3;
    private const int DispChangeSuccessful = 0;
    private const int DispChangeBadMode = -2;
    private const int WindowWatchToolWindow = 0x00000080;
    private const int WindowWatchExStyleIndex = -20;
    private const uint WindowWatchNoZOrder = 0x0004;
    private const uint WindowWatchNoActivate = 0x0010;
    private const uint WindowWatchAsyncWindowPos = 0x4000;
    private const uint WindowWatchMoveFlags = WindowWatchNoZOrder | WindowWatchNoActivate | WindowWatchAsyncWindowPos;

    private static int Main(string[] args)
    {
        EnableProcessDpiAwareness();
        Console.OutputEncoding = System.Text.Encoding.UTF8;
        var result = Run(args);
        Console.WriteLine(JsonSerializer.Serialize(result, JsonOptions));
        return result.Ok ? 0 : 1;
    }

    private static void EnableProcessDpiAwareness()
    {
        try
        {
            if (NativeMethods.SetProcessDpiAwarenessContext(new IntPtr(-4)))
            {
                return;
            }
        }
        catch
        {
            // Older Windows builds may not expose per-monitor-v2 awareness.
        }

        try
        {
            NativeMethods.SetProcessDPIAware();
        }
        catch
        {
            // Best effort. Display prep still works without DPI awareness.
        }
    }

    private static HelperResult Run(string[] args)
    {
        try
        {
            var options = ParseArguments(args);
            return options.Command switch
            {
                CommandKind.Prepare => PrepareDisplay(options),
                CommandKind.Resize => ResizeDisplay(options),
                CommandKind.Restore => RestoreDisplay(options),
                CommandKind.Preflight => PreflightHost(options),
                CommandKind.WatchWindowPrimary => WatchWindowPrimary(options),
                CommandKind.PersistentVddOnly => EnforcePersistentVddOnly(options),
                CommandKind.ProjectDisplay => ProjectDisplay(options),
                CommandKind.ListDisplays => ListDisplays(options),
                CommandKind.SetStreamDisplay => SetStreamDisplay(options),
                _ => Failure("unsupported_command"),
            };
        }
        catch (Exception ex)
        {
            return Failure(ex.Message);
        }
    }

    private static Options ParseArguments(string[] args)
    {
        if (args.Length == 0)
        {
            throw new ArgumentException("missing command");
        }

        var command = args[0].ToLowerInvariant() switch
        {
            "prepare" => CommandKind.Prepare,
            "resize" => CommandKind.Resize,
            "restore" => CommandKind.Restore,
            "preflight" => CommandKind.Preflight,
            "watch-window-primary" => CommandKind.WatchWindowPrimary,
            "persistent-vdd-only" => CommandKind.PersistentVddOnly,
            "project-display" => CommandKind.ProjectDisplay,
            "project" => CommandKind.ProjectDisplay,
            "list-displays" => CommandKind.ListDisplays,
            "set-stream-display" => CommandKind.SetStreamDisplay,
            _ => throw new ArgumentException($"unknown command: {args[0]}"),
        };

        string sessionToken = string.Empty;
        var width = 0;
        var height = 0;
        var fps = 0;
        var bundleRoot = string.Empty;
        var applyPreflightConfig = true;
        var bypassPreflightCache = false;
        var pollMs = 350;
        var projectMode = string.Empty;
        var displayMode = string.Empty;
        var displayDeviceName = string.Empty;
        var displayDeviceId = string.Empty;
        var displayLabel = string.Empty;
        for (var i = 1; i < args.Length; i++)
        {
            switch (args[i])
            {
                case "--session-token":
                    if (i + 1 >= args.Length)
                    {
                        throw new ArgumentException("missing value for --session-token");
                    }

                    sessionToken = args[++i];
                    break;
                case "--width":
                    if (i + 1 >= args.Length || !int.TryParse(args[++i], out width))
                    {
                        throw new ArgumentException("invalid value for --width");
                    }
                    break;
                case "--height":
                    if (i + 1 >= args.Length || !int.TryParse(args[++i], out height))
                    {
                        throw new ArgumentException("invalid value for --height");
                    }
                    break;
                case "--fps":
                    if (i + 1 >= args.Length || !int.TryParse(args[++i], out fps))
                    {
                        throw new ArgumentException("invalid value for --fps");
                    }
                    break;
                case "--bundle-root":
                    if (i + 1 >= args.Length)
                    {
                        throw new ArgumentException("missing value for --bundle-root");
                    }

                    bundleRoot = args[++i];
                    break;
                case "--profile-only":
                    applyPreflightConfig = false;
                    break;
                case "--refresh":
                case "--no-cache":
                    bypassPreflightCache = true;
                    break;
                case "--poll-ms":
                    if (i + 1 >= args.Length || !int.TryParse(args[++i], out pollMs))
                    {
                        throw new ArgumentException("invalid value for --poll-ms");
                    }
                    break;
                case "--mode":
                    if (i + 1 >= args.Length)
                    {
                        throw new ArgumentException("missing value for --mode");
                    }

                    projectMode = args[++i];
                    break;
                case "--display-mode":
                    if (i + 1 >= args.Length)
                    {
                        throw new ArgumentException("missing value for --display-mode");
                    }

                    displayMode = args[++i];
                    break;
                case "--device-name":
                    if (i + 1 >= args.Length)
                    {
                        throw new ArgumentException("missing value for --device-name");
                    }

                    displayDeviceName = args[++i];
                    break;
                case "--device-id":
                    if (i + 1 >= args.Length)
                    {
                        throw new ArgumentException("missing value for --device-id");
                    }

                    displayDeviceId = args[++i];
                    break;
                case "--label":
                    if (i + 1 >= args.Length)
                    {
                        throw new ArgumentException("missing value for --label");
                    }

                    displayLabel = args[++i];
                    break;
                default:
                    throw new ArgumentException($"unknown argument: {args[i]}");
            }
        }

        return new Options(
            command,
            sessionToken,
            width,
            height,
            fps,
            bundleRoot,
            applyPreflightConfig,
            bypassPreflightCache,
            pollMs,
            projectMode,
            displayMode,
            displayDeviceName,
            displayDeviceId,
            displayLabel);
    }

    private static readonly (string EncoderKey, string FfmpegCodec)[] EncoderProbeCandidates =
    [
        ("nvenc", "h264_nvenc"),
        ("quicksync", "h264_qsv"),
        ("amdvce", "h264_amf"),
        ("software", "libx264"),
    ];

    private static bool IsTruthyFlagValue(string? value)
    {
        if (string.IsNullOrWhiteSpace(value))
        {
            return true;
        }

        return !value.Equals("0", StringComparison.OrdinalIgnoreCase)
            && !value.Equals("false", StringComparison.OrdinalIgnoreCase)
            && !value.Equals("off", StringComparison.OrdinalIgnoreCase)
            && !value.Equals("no", StringComparison.OrdinalIgnoreCase);
    }

    private static bool ForceLegacyNvencEnabled(string bundleRoot)
    {
        try
        {
            var flagPath = Path.Combine(bundleRoot, "moonlight", "server", "force_legacy_nvenc.txt");
            if (!File.Exists(flagPath))
            {
                return false;
            }

            return IsTruthyFlagValue(File.ReadAllText(flagPath).Trim());
        }
        catch
        {
            return false;
        }
    }

    private static bool ForceNvencEnabled(string bundleRoot)
    {
        try
        {
            var flagPath = Path.Combine(bundleRoot, "moonlight", "server", "force_nvenc.txt");
            if (!File.Exists(flagPath))
            {
                return false;
            }

            return IsTruthyFlagValue(File.ReadAllText(flagPath).Trim());
        }
        catch
        {
            return false;
        }
    }

    private static RuntimeMetadata ResolveRuntimeMetadata(
        string bundleRoot,
        string runtimeRoot,
        string runtimeDirectory,
        string runtimeKey,
        bool legacy,
        string? ffmpegPath,
        bool forceNvenc)
    {
        var metadataPath = Path.Combine(runtimeRoot, "sunshine_runtime_info.json");
        try
        {
            if (File.Exists(metadataPath))
            {
                using var document = JsonDocument.Parse(File.ReadAllText(metadataPath));
                var root = document.RootElement;
                var metadataDisplayName = root.TryGetProperty("display_name", out var displayNameElement)
                    ? displayNameElement.GetString()
                    : null;
                var runtimeVersion = root.TryGetProperty("runtime_version", out var runtimeVersionElement)
                    ? runtimeVersionElement.GetString()
                    : null;
                var runtimeFingerprint = root.TryGetProperty("runtime_fingerprint", out var runtimeFingerprintElement)
                    ? runtimeFingerprintElement.GetString()
                    : null;
                var requiresBundledFfmpeg = root.TryGetProperty("requires_bundled_ffmpeg", out var requiresBundledFfmpegElement)
                    && requiresBundledFfmpegElement.ValueKind is JsonValueKind.True or JsonValueKind.False
                    && requiresBundledFfmpegElement.GetBoolean();
                var configuredAutoSelect = !root.TryGetProperty("auto_select", out var autoSelectElement)
                    || autoSelectElement.ValueKind is not (JsonValueKind.True or JsonValueKind.False)
                    || autoSelectElement.GetBoolean();
                var startupValidationStatus = root.TryGetProperty("startup_validation_status", out var startupValidationStatusElement)
                    ? startupValidationStatusElement.GetString()
                    : null;
                var startupValidationReason = root.TryGetProperty("startup_validation_reason", out var startupValidationReasonElement)
                    ? startupValidationReasonElement.GetString()
                    : null;
                var startupValidationCheckedAt = root.TryGetProperty("startup_validation_checked_at", out var startupValidationCheckedAtElement)
                    ? startupValidationCheckedAtElement.GetString()
                    : null;
                var startupValidationRuntimeFingerprint = root.TryGetProperty("startup_validation_runtime_fingerprint", out var startupValidationRuntimeFingerprintElement)
                    ? startupValidationRuntimeFingerprintElement.GetString()
                    : null;
                var computedRuntimeFingerprint = runtimeFingerprint ?? ComputeRuntimeFingerprint(runtimeRoot, ffmpegPath);
                var effectiveAutoSelect = configuredAutoSelect;

                var forceLegacyNvenc = legacy && ForceLegacyNvencEnabled(bundleRoot);

                if (legacy && configuredAutoSelect && !forceLegacyNvenc)
                {
                    if (!string.Equals(startupValidationStatus, "passed", StringComparison.OrdinalIgnoreCase))
                    {
                        effectiveAutoSelect = false;
                        startupValidationStatus ??= "pending";
                        startupValidationReason ??= "runtime_start_validation_required";
                    }
                    else if (LegacyStartupValidationExpired(startupValidationCheckedAt))
                    {
                        effectiveAutoSelect = false;
                        startupValidationStatus = "stale";
                        startupValidationReason = "runtime_start_validation_expired";
                    }
                    else if (!string.Equals(
                        startupValidationRuntimeFingerprint ?? string.Empty,
                        computedRuntimeFingerprint ?? string.Empty,
                        StringComparison.OrdinalIgnoreCase))
                    {
                        effectiveAutoSelect = false;
                        startupValidationStatus = "stale";
                        startupValidationReason = "runtime_fingerprint_changed";
                    }
                    else
                    {
                        var runtimeFailure = DetectRuntimeOperationalFailure(runtimeRoot, startupValidationCheckedAt, forceNvenc: false);
                        if (runtimeFailure is not null)
                        {
                            effectiveAutoSelect = false;
                            startupValidationStatus = runtimeFailure.Value.Status;
                            startupValidationReason = runtimeFailure.Value.Reason;
                        }
                    }
                }
                else if (forceLegacyNvenc)
                {
                    effectiveAutoSelect = true;
                    startupValidationStatus ??= "forced";
                    startupValidationReason ??= "force_legacy_nvenc";
                }

                if (forceNvenc && legacy)
                {
                    var runtimeFailure = DetectRuntimeOperationalFailure(runtimeRoot, startupValidationCheckedAt, forceNvenc: true);
                    if (runtimeFailure is not null)
                    {
                        effectiveAutoSelect = false;
                        startupValidationStatus = runtimeFailure.Value.Status;
                        startupValidationReason = runtimeFailure.Value.Reason;
                    }
                }

                if (!string.IsNullOrWhiteSpace(metadataDisplayName) ||
                    !string.IsNullOrWhiteSpace(runtimeVersion) ||
                    !string.IsNullOrWhiteSpace(runtimeFingerprint) ||
                    requiresBundledFfmpeg ||
                    !configuredAutoSelect ||
                    !string.IsNullOrWhiteSpace(startupValidationStatus) ||
                    !string.IsNullOrWhiteSpace(startupValidationReason) ||
                    !string.IsNullOrWhiteSpace(startupValidationCheckedAt))
                {
                    return new RuntimeMetadata
                    {
                        DisplayName = metadataDisplayName,
                        RuntimeVersion = runtimeVersion,
                        RuntimeFingerprint = computedRuntimeFingerprint,
                        RequiresBundledFfmpeg = requiresBundledFfmpeg,
                        AutoSelect = effectiveAutoSelect,
                        StartupValidationStatus = startupValidationStatus,
                        StartupValidationReason = startupValidationReason,
                        StartupValidationCheckedAt = startupValidationCheckedAt,
                    };
                }
            }
        }
        catch
        {
            // Best effort. Fall back to inferred metadata.
        }

        string? inferredVersion = null;
        try
        {
            var sunshineExe = Path.Combine(runtimeRoot, "sunshine.exe");
            if (File.Exists(sunshineExe))
            {
                var versionInfo = FileVersionInfo.GetVersionInfo(sunshineExe);
                inferredVersion = string.IsNullOrWhiteSpace(versionInfo.ProductVersion)
                    ? versionInfo.FileVersion
                    : versionInfo.ProductVersion;
            }
        }
        catch
        {
            // Best effort.
        }

        string displayName = runtimeKey.Equals("default", StringComparison.OrdinalIgnoreCase)
            ? "Cloudgime Modern Runtime"
            : legacy
                ? $"Cloudgime Compatibility Runtime ({runtimeKey})"
                : $"Cloudgime Runtime ({runtimeKey})";

        if (runtimeDirectory.Equals("sunshine", StringComparison.OrdinalIgnoreCase))
        {
            displayName = "Cloudgime Modern Runtime";
        }

        var fallbackRuntimeFailure = forceNvenc && legacy
            ? DetectRuntimeOperationalFailure(runtimeRoot, validationCheckedAt: null, forceNvenc: true)
            : null;

        return new RuntimeMetadata
        {
            DisplayName = displayName,
            RuntimeVersion = inferredVersion,
            RuntimeFingerprint = ComputeRuntimeFingerprint(runtimeRoot, ffmpegPath),
            RequiresBundledFfmpeg = legacy,
            AutoSelect = fallbackRuntimeFailure is null,
            StartupValidationStatus = fallbackRuntimeFailure is not null
                ? fallbackRuntimeFailure.Value.Status
                : legacy ? "pending" : null,
            StartupValidationReason = fallbackRuntimeFailure is not null
                ? fallbackRuntimeFailure.Value.Reason
                : legacy ? "runtime_start_validation_required" : null,
        };
    }

    private static bool LegacyStartupValidationExpired(string? validationCheckedAt)
    {
        if (string.IsNullOrWhiteSpace(validationCheckedAt))
        {
            return true;
        }

        if (!DateTimeOffset.TryParse(validationCheckedAt, CultureInfo.InvariantCulture, DateTimeStyles.RoundtripKind, out var checkedAt))
        {
            return true;
        }

        var now = DateTimeOffset.UtcNow;
        var checkedAtUtc = checkedAt.ToUniversalTime();
        if (checkedAtUtc > now.AddMinutes(5))
        {
            return true;
        }

        return now - checkedAtUtc > TimeSpan.FromDays(7);
    }

    private static (string Status, string Reason)? DetectRuntimeOperationalFailure(
        string runtimeRoot,
        string? validationCheckedAt,
        bool forceNvenc)
    {
        var sunshineLogPath = Path.Combine(runtimeRoot, "config", "sunshine.log");
        if (!File.Exists(sunshineLogPath))
        {
            return null;
        }

        var relevantLines = new List<string>();
        DateTimeOffset? validationCutoff = null;
        if (!string.IsNullOrWhiteSpace(validationCheckedAt) &&
            DateTimeOffset.TryParse(validationCheckedAt, CultureInfo.InvariantCulture, DateTimeStyles.RoundtripKind, out var parsedValidationCutoff))
        {
            validationCutoff = parsedValidationCutoff;
        }

        foreach (var line in ReadTextFileShared(sunshineLogPath)
                     .Split(["\r\n", "\n"], StringSplitOptions.None))
        {
            if (validationCutoff is not null &&
                TryParseSunshineLogTimestamp(line, out var loggedAt) &&
                loggedAt <= validationCutoff.Value)
            {
                continue;
            }

            relevantLines.Add(line);
        }

        if (validationCutoff is null && relevantLines.Count > 600)
        {
            relevantLines = relevantLines[^600..];
        }

        var probeRecovered = relevantLines.Any(line =>
            line.Contains("Found encoder nvenc", StringComparison.OrdinalIgnoreCase) ||
            line.Contains("Configuration UI available at", StringComparison.OrdinalIgnoreCase));

        foreach (var line in relevantLines)
        {
            var failureReason = ClassifyRuntimeOperationalFailure(line, probeRecovered, forceNvenc);
            if (!string.IsNullOrWhiteSpace(failureReason))
            {
                return ("failed", failureReason);
            }
        }

        return null;
    }

    private static bool TryParseSunshineLogTimestamp(string line, out DateTimeOffset loggedAt)
    {
        loggedAt = default;
        if (string.IsNullOrWhiteSpace(line) || line[0] != '[')
        {
            return false;
        }

        var endIndex = line.IndexOf(']');
        if (endIndex <= 1)
        {
            return false;
        }

        var timestampText = line[1..endIndex];
        if (!DateTime.TryParseExact(
                timestampText,
                "yyyy:MM:dd:HH:mm:ss",
                CultureInfo.InvariantCulture,
                DateTimeStyles.AssumeLocal,
                out var parsed))
        {
            return false;
        }

        loggedAt = new DateTimeOffset(parsed);
        return true;
    }

    private static string? ClassifyRuntimeOperationalFailure(
        string line,
        bool probeRecovered,
        bool forceNvenc)
    {
        if (string.IsNullOrWhiteSpace(line))
        {
            return null;
        }

        if (forceNvenc &&
            (line.Contains("Trying encoder [quicksync]", StringComparison.OrdinalIgnoreCase) ||
             line.Contains("Trying encoder [amdvce]", StringComparison.OrdinalIgnoreCase) ||
             line.Contains("Trying encoder [software]", StringComparison.OrdinalIgnoreCase) ||
             line.Contains("Found H.264 encoder: libx264 [software]", StringComparison.OrdinalIgnoreCase) ||
             line.Contains("Creating encoder [libx264]", StringComparison.OrdinalIgnoreCase)))
        {
            return "runtime_non_nvenc_fallback_blocked";
        }

        if (line.Contains("No such node (root.devices)", StringComparison.OrdinalIgnoreCase))
        {
            return "runtime_state_schema_incompatible";
        }

        if (line.Contains("terminate called after throwing", StringComparison.OrdinalIgnoreCase))
        {
            return "runtime_process_terminated_after_exception";
        }

        if (line.Contains("Couldn't find any working encoder", StringComparison.OrdinalIgnoreCase))
        {
            return "runtime_no_working_encoder";
        }

        if (line.Contains("Could not open codec [h264_nvenc]", StringComparison.OrdinalIgnoreCase) ||
            line.Contains("Could not open codec [hevc_nvenc]", StringComparison.OrdinalIgnoreCase))
        {
            if (probeRecovered)
            {
                return null;
            }

            return line.Contains("Function not implemented", StringComparison.OrdinalIgnoreCase)
                ? "runtime_encoder_open_failed_nvenc_function_not_implemented"
                : "runtime_encoder_open_failed_nvenc";
        }

        if (line.Contains("InitializeEncoder failed", StringComparison.OrdinalIgnoreCase) &&
            line.Contains("nvenc", StringComparison.OrdinalIgnoreCase))
        {
            if (probeRecovered)
            {
                return null;
            }

            return "runtime_encoder_init_failed_nvenc";
        }

        return null;
    }

    private static string? ComputeRuntimeFingerprint(string runtimeRoot, string? effectiveFfmpegPath)
    {
        var components = new List<string>();
        var sunshineExe = Path.Combine(runtimeRoot, "sunshine.exe");

        var sunshineComponent = DescribeFileFingerprintComponent("sunshine", sunshineExe);
        if (!string.IsNullOrWhiteSpace(sunshineComponent))
        {
            components.Add(sunshineComponent);
        }

        var ffmpegComponent = DescribeFileFingerprintComponent("ffmpeg", effectiveFfmpegPath ?? string.Empty);
        if (!string.IsNullOrWhiteSpace(ffmpegComponent))
        {
            components.Add(ffmpegComponent);
        }

        if (components.Count == 0)
        {
            return null;
        }

        return string.Join(";", components);
    }

    private static string? DescribeFileFingerprintComponent(string label, string path)
    {
        if (!File.Exists(path))
        {
            return null;
        }

        try
        {
            var fileInfo = new FileInfo(path);
            var versionInfo = FileVersionInfo.GetVersionInfo(path);
            var version = !string.IsNullOrWhiteSpace(versionInfo.ProductVersion)
                ? versionInfo.ProductVersion
                : !string.IsNullOrWhiteSpace(versionInfo.FileVersion)
                    ? versionInfo.FileVersion
                    : "unknown";

            return $"{label}:{version}:{fileInfo.Length}:{fileInfo.LastWriteTimeUtc.Ticks}";
        }
        catch
        {
            return null;
        }
    }

    private static HelperResult PreflightHost(Options options)
    {
        var bundleRoot = ResolveBundleRoot(options.BundleRoot);
        var profilePath = Path.Combine(bundleRoot, "moonlight", "server", "host_capability_profile.json");
        var runtimeSelectionPath = Path.Combine(bundleRoot, "moonlight", "server", "selected_sunshine_runtime.txt");
        var forceNvenc = ForceNvencEnabled(bundleRoot);

        var gpuControllers = DetectGpuControllers();
        var audioEndpoints = DetectAudioEndpoints(bundleRoot);
        var runtimes = DiscoverSunshineRuntimes(bundleRoot, forceNvenc);
        if (!options.BypassPreflightCache &&
            TryUseCachedPreflightProfile(bundleRoot, profilePath, runtimes, gpuControllers, audioEndpoints, forceNvenc, options.ApplyPreflightConfig, runtimeSelectionPath, out var cachedResult))
        {
            return cachedResult;
        }

        var probes = ProbeEncoders(runtimes, forceNvenc);
        AnnotateRuntimeCandidatesWithProbeResults(runtimes, probes, forceNvenc);
        var softwareMinThreads = Math.Clamp(Math.Max(2, Environment.ProcessorCount / 2), 2, 4);
        var selectedRuntime = SelectRuntimeAndEncoder(gpuControllers, runtimes, probes, forceNvenc, out var selectedEncoder, out var selectionReason);
        var (selectedCapture, selectedCaptureReason) = SelectCaptureBackend(gpuControllers, selectedRuntime);
        var configPath = selectedRuntime?.ConfigPath ?? Path.Combine(bundleRoot, "sunshine", "config", "sunshine.conf");
        var ffmpegPath = selectedRuntime?.FfmpegPath;
        var selectedRuntimeKey = selectedRuntime?.Key ?? "default";
        var selectedRuntimeDirectory = selectedRuntime?.RelativeDirectory ?? "sunshine";
        var selectedRuntimeDisplayName = selectedRuntime?.DisplayName;
        var selectedRuntimeVersion = selectedRuntime?.RuntimeVersion;
        var selectedRuntimeFingerprint = selectedRuntime?.RuntimeFingerprint;
        var (selectedAudioSinkName, selectedVirtualSinkName, selectedMicrophoneName, audioSelectionReason, audioSelectionMode) =
            SelectPreferredVirtualAudioEndpoints(bundleRoot, audioEndpoints);
        var warnings = BuildPreflightWarnings(gpuControllers, runtimes, probes, selectedRuntime, selectedEncoder, selectionReason, forceNvenc);

        var profile = new HostCapabilityProfile
        {
            UpdatedAt = DateTimeOffset.UtcNow.ToString("O"),
            ProbeMode = "fresh",
            BundleRoot = bundleRoot,
            ConfigPath = configPath,
            FfmpegPath = ffmpegPath,
            SelectedFfmpegSource = selectedRuntime?.FfmpegSource,
            ForceNvencEnabled = forceNvenc,
            GpuControllers = gpuControllers,
            AudioEndpoints = audioEndpoints,
            RuntimeCandidates = runtimes,
            SelectedRuntimeKey = selectedRuntimeKey,
            SelectedRuntimeDirectory = selectedRuntimeDirectory,
            SelectedRuntimeDisplayName = selectedRuntimeDisplayName,
            SelectedRuntimeVersion = selectedRuntimeVersion,
            SelectedRuntimeFingerprint = selectedRuntimeFingerprint,
            SelectedEncoder = selectedEncoder,
            SelectedCapture = selectedCapture,
            SelectedCaptureReason = selectedCaptureReason,
            SelectedAudioSinkName = selectedAudioSinkName,
            SelectedVirtualSinkName = selectedVirtualSinkName,
            SelectedMicrophoneName = selectedMicrophoneName,
            AudioSelectionMode = audioSelectionMode,
            AudioSelectionReason = audioSelectionReason,
            SelectionReason = selectionReason,
            SoftwareMinThreads = softwareMinThreads,
            EncoderProbes = probes,
            Warnings = warnings,
        };

        Directory.CreateDirectory(Path.GetDirectoryName(profilePath)!);
        File.WriteAllText(runtimeSelectionPath, $"{selectedRuntimeDirectory}{Environment.NewLine}");

        if (options.ApplyPreflightConfig)
        {
            EnsureConfigDirectory(configPath);
            ApplyRecommendedSunshineConfig(configPath, profile);
            TryApplyPreferredSunshineCaptureConfig(configPath, bundleRoot);
            profile.ConfigApplied = true;
        }

        File.WriteAllText(profilePath, JsonSerializer.Serialize(profile, JsonOptions));

        return new HelperResult
        {
            Ok = true,
            Changed = profile.ConfigApplied,
            Restored = false,
            Skipped = !profile.ConfigApplied,
            Reason = selectionReason,
            ProfilePath = profilePath,
            ConfigPath = configPath,
            SelectedEncoder = selectedEncoder,
            SelectedCapture = selectedCapture,
            SelectedRuntime = selectedRuntimeDirectory,
        };
    }

    private static bool TryUseCachedPreflightProfile(
        string bundleRoot,
        string profilePath,
        IReadOnlyCollection<SunshineRuntimeCandidate> runtimes,
        IReadOnlyCollection<GpuControllerInfo> gpuControllers,
        IReadOnlyCollection<HostAudioEndpointInfo> audioEndpoints,
        bool forceNvenc,
        bool applyPreflightConfig,
        string runtimeSelectionPath,
        out HelperResult result)
    {
        result = default!;
        if (!File.Exists(profilePath))
        {
            return false;
        }

        HostCapabilityProfile? cachedProfile;
        try
        {
            cachedProfile = JsonSerializer.Deserialize<HostCapabilityProfile>(File.ReadAllText(profilePath), JsonOptions);
        }
        catch
        {
            return false;
        }

        if (cachedProfile is null || !CanReuseCachedProfile(cachedProfile, runtimes, gpuControllers, audioEndpoints, forceNvenc))
        {
            return false;
        }

        var selectedRuntime = runtimes.FirstOrDefault(runtime =>
            runtime.RelativeDirectory.Equals(cachedProfile.SelectedRuntimeDirectory, StringComparison.OrdinalIgnoreCase));
        cachedProfile.RuntimeCandidates = runtimes.Select(CloneRuntimeCandidate).ToList();
        AnnotateRuntimeCandidatesWithProbeResults(cachedProfile.RuntimeCandidates, cachedProfile.EncoderProbes, forceNvenc);
        selectedRuntime = cachedProfile.RuntimeCandidates.FirstOrDefault(runtime =>
            runtime.RelativeDirectory.Equals(cachedProfile.SelectedRuntimeDirectory, StringComparison.OrdinalIgnoreCase));
        cachedProfile.ConfigPath = selectedRuntime?.ConfigPath ?? cachedProfile.ConfigPath;
        cachedProfile.FfmpegPath = selectedRuntime?.FfmpegPath;
        cachedProfile.SelectedFfmpegSource = selectedRuntime?.FfmpegSource;
        cachedProfile.ForceNvencEnabled = forceNvenc;
        cachedProfile.SelectedRuntimeDisplayName = selectedRuntime?.DisplayName;
        cachedProfile.SelectedRuntimeVersion = selectedRuntime?.RuntimeVersion;
        cachedProfile.SelectedRuntimeFingerprint = selectedRuntime?.RuntimeFingerprint;
        cachedProfile.SelectedCaptureReason = SelectCaptureBackend(gpuControllers, selectedRuntime).Reason;
        cachedProfile.AudioEndpoints = audioEndpoints.ToList();
        var (selectedAudioSinkName, selectedVirtualSinkName, selectedMicrophoneName, audioSelectionReason, audioSelectionMode) =
            SelectPreferredVirtualAudioEndpoints(bundleRoot, audioEndpoints);
        cachedProfile.SelectedAudioSinkName = selectedAudioSinkName;
        cachedProfile.SelectedVirtualSinkName = selectedVirtualSinkName;
        cachedProfile.SelectedMicrophoneName = selectedMicrophoneName;
        cachedProfile.AudioSelectionMode = audioSelectionMode;
        cachedProfile.AudioSelectionReason = audioSelectionReason;
        cachedProfile.Warnings = BuildPreflightWarnings(
            gpuControllers,
            cachedProfile.RuntimeCandidates,
            cachedProfile.EncoderProbes,
            selectedRuntime,
            cachedProfile.SelectedEncoder,
            cachedProfile.SelectionReason,
            forceNvenc);

        if (applyPreflightConfig)
        {
            EnsureConfigDirectory(cachedProfile.ConfigPath);
            ApplyRecommendedSunshineConfig(cachedProfile.ConfigPath, cachedProfile);
        }

        cachedProfile.UpdatedAt = DateTimeOffset.UtcNow.ToString("O");
        cachedProfile.ProbeMode = "cache";
        cachedProfile.ConfigApplied = applyPreflightConfig;
        File.WriteAllText(profilePath, JsonSerializer.Serialize(cachedProfile, JsonOptions));
        Directory.CreateDirectory(Path.GetDirectoryName(runtimeSelectionPath)!);
        File.WriteAllText(runtimeSelectionPath, $"{cachedProfile.SelectedRuntimeDirectory}{Environment.NewLine}");
        result = new HelperResult
        {
            Ok = true,
            Changed = applyPreflightConfig,
            Restored = false,
            Skipped = !applyPreflightConfig,
            Reason = $"cache:{cachedProfile.SelectionReason}",
            ProfilePath = profilePath,
            ConfigPath = cachedProfile.ConfigPath,
            SelectedEncoder = cachedProfile.SelectedEncoder,
            SelectedCapture = cachedProfile.SelectedCapture,
            SelectedRuntime = cachedProfile.SelectedRuntimeDirectory,
        };
        return true;
    }

    private static bool CanReuseCachedProfile(
        HostCapabilityProfile cachedProfile,
        IReadOnlyCollection<SunshineRuntimeCandidate> runtimes,
        IReadOnlyCollection<GpuControllerInfo> gpuControllers,
        IReadOnlyCollection<HostAudioEndpointInfo> audioEndpoints,
        bool forceNvenc)
    {
        if (string.IsNullOrWhiteSpace(cachedProfile.SelectedRuntimeDirectory) ||
            string.IsNullOrWhiteSpace(cachedProfile.ConfigPath) ||
            string.IsNullOrWhiteSpace(cachedProfile.SelectedEncoder) ||
            string.IsNullOrWhiteSpace(cachedProfile.SelectedCapture))
        {
            return false;
        }

        var selectedRuntime = runtimes.FirstOrDefault(runtime =>
            runtime.RelativeDirectory.Equals(cachedProfile.SelectedRuntimeDirectory, StringComparison.OrdinalIgnoreCase));
        if (selectedRuntime is null || !File.Exists(selectedRuntime.ConfigPath))
        {
            return false;
        }

        if (!selectedRuntime.AutoSelect)
        {
            return false;
        }

        if (!SameGpuControllers(cachedProfile.GpuControllers, gpuControllers))
        {
            return false;
        }

        if (!SameAudioEndpoints(cachedProfile.AudioEndpoints, audioEndpoints))
        {
            return false;
        }

        if (cachedProfile.ForceNvencEnabled != forceNvenc)
        {
            return false;
        }

        if (forceNvenc && !cachedProfile.SelectedEncoder.Equals("nvenc", StringComparison.OrdinalIgnoreCase))
        {
            return false;
        }

        if (forceNvenc &&
            cachedProfile.EncoderProbes.Any(probe =>
                probe.Ok &&
                probe.EncoderKey.Equals("software", StringComparison.OrdinalIgnoreCase)))
        {
            return false;
        }

        var cachedRuntimeMap = cachedProfile.RuntimeCandidates
            .Where(item => !string.IsNullOrWhiteSpace(item.RelativeDirectory))
            .ToDictionary(item => item.RelativeDirectory, StringComparer.OrdinalIgnoreCase);
        var currentRuntimeMap = runtimes
            .Where(item => !string.IsNullOrWhiteSpace(item.RelativeDirectory))
            .ToDictionary(item => item.RelativeDirectory, StringComparer.OrdinalIgnoreCase);

        if (cachedRuntimeMap.Count != currentRuntimeMap.Count)
        {
            return false;
        }

        foreach (var (runtimeDirectory, currentRuntime) in currentRuntimeMap)
        {
            if (!cachedRuntimeMap.TryGetValue(runtimeDirectory, out var cachedRuntime))
            {
                return false;
            }

            if (!string.Equals(
                cachedRuntime.RuntimeFingerprint ?? string.Empty,
                currentRuntime.RuntimeFingerprint ?? string.Empty,
                StringComparison.OrdinalIgnoreCase))
            {
                return false;
            }
        }

        if (!string.Equals(
            cachedProfile.SelectedRuntimeFingerprint ?? string.Empty,
            selectedRuntime.RuntimeFingerprint ?? string.Empty,
            StringComparison.OrdinalIgnoreCase))
        {
            return false;
        }

        if (!string.Equals(
            cachedProfile.SelectedFfmpegSource ?? string.Empty,
            selectedRuntime.FfmpegSource ?? string.Empty,
            StringComparison.OrdinalIgnoreCase))
        {
            return false;
        }

        return true;
    }

    private static bool SameGpuControllers(
        IReadOnlyCollection<GpuControllerInfo> left,
        IReadOnlyCollection<GpuControllerInfo> right)
    {
        if (left.Count != right.Count)
        {
            return false;
        }

        var leftKeys = left
            .Select(controller => $"{controller.Name}|{controller.DriverVersion}")
            .OrderBy(value => value, StringComparer.OrdinalIgnoreCase)
            .ToArray();
        var rightKeys = right
            .Select(controller => $"{controller.Name}|{controller.DriverVersion}")
            .OrderBy(value => value, StringComparer.OrdinalIgnoreCase)
            .ToArray();

        return leftKeys.SequenceEqual(rightKeys, StringComparer.OrdinalIgnoreCase);
    }

    private static bool SameAudioEndpoints(
        IReadOnlyCollection<HostAudioEndpointInfo> left,
        IReadOnlyCollection<HostAudioEndpointInfo> right)
    {
        if (left.Count != right.Count)
        {
            return false;
        }

        var leftKeys = left
            .Select(endpoint => $"{endpoint.Direction}|{endpoint.Name}|{endpoint.DeviceId}")
            .OrderBy(value => value, StringComparer.OrdinalIgnoreCase)
            .ToArray();
        var rightKeys = right
            .Select(endpoint => $"{endpoint.Direction}|{endpoint.Name}|{endpoint.DeviceId}")
            .OrderBy(value => value, StringComparer.OrdinalIgnoreCase)
            .ToArray();

        return leftKeys.SequenceEqual(rightKeys, StringComparer.OrdinalIgnoreCase);
    }

    private static List<HostAudioEndpointInfo> DetectAudioEndpoints(string bundleRoot)
    {
        var endpoints = DetectAudioEndpointsFromAudioInfo(bundleRoot)
            .Concat(DetectAudioEndpointsFromDriverInventory())
            .Where(endpoint => !string.IsNullOrWhiteSpace(endpoint.Name))
            .GroupBy(endpoint => $"{endpoint.Direction}|{endpoint.Name}", StringComparer.OrdinalIgnoreCase)
            .Select(group =>
            {
                var withDeviceId = group.FirstOrDefault(endpoint => !string.IsNullOrWhiteSpace(endpoint.DeviceId));
                return withDeviceId ?? group.First();
            })
            .ToList();

        return endpoints;
    }

    private static List<HostAudioEndpointInfo> DetectAudioEndpointsFromAudioInfo(string bundleRoot)
    {
        var audioInfoPath = ResolveAudioInfoPath(bundleRoot);
        if (string.IsNullOrWhiteSpace(audioInfoPath) || !File.Exists(audioInfoPath))
        {
            return [];
        }

        var result = RunProcess(audioInfoPath, string.Empty, null, 8000);
        if (!result.Ok || string.IsNullOrWhiteSpace(result.StdOut))
        {
            return [];
        }

        var endpoints = new List<HostAudioEndpointInfo>();
        string? currentDeviceId = null;
        string? currentName = null;
        string? currentState = null;

        void FlushCurrent()
        {
            if (string.IsNullOrWhiteSpace(currentName))
            {
                currentDeviceId = null;
                currentName = null;
                currentState = null;
                return;
            }

            if (!string.IsNullOrWhiteSpace(currentState) &&
                !currentState.Equals("Active", StringComparison.OrdinalIgnoreCase))
            {
                currentDeviceId = null;
                currentName = null;
                currentState = null;
                return;
            }

            endpoints.Add(new HostAudioEndpointInfo
            {
                Direction = InferAudioEndpointDirection(currentName),
                DeviceId = currentDeviceId ?? string.Empty,
                Name = currentName,
            });

            currentDeviceId = null;
            currentName = null;
            currentState = null;
        }

        foreach (var rawLine in result.StdOut.Split(['\r', '\n'], StringSplitOptions.RemoveEmptyEntries))
        {
            var line = rawLine.Trim();
            if (line.StartsWith("===== Device", StringComparison.OrdinalIgnoreCase))
            {
                FlushCurrent();
                continue;
            }

            if (line.StartsWith("Device ID", StringComparison.OrdinalIgnoreCase))
            {
                currentDeviceId = ReadAudioInfoValue(line);
                continue;
            }

            if (line.StartsWith("Device name", StringComparison.OrdinalIgnoreCase))
            {
                currentName = ReadAudioInfoValue(line);
                continue;
            }

            if (line.StartsWith("Device state", StringComparison.OrdinalIgnoreCase))
            {
                currentState = ReadAudioInfoValue(line);
            }
        }

        FlushCurrent();
        return endpoints
            .Where(endpoint => !string.IsNullOrWhiteSpace(endpoint.Name))
            .GroupBy(endpoint => $"{endpoint.Direction}|{endpoint.Name}|{endpoint.DeviceId}", StringComparer.OrdinalIgnoreCase)
            .Select(group => group.First())
            .ToList();
    }

    private static List<HostAudioEndpointInfo> DetectAudioEndpointsFromDriverInventory()
    {
        var result = RunProcess(
            "powershell.exe",
            "-NoProfile -ExecutionPolicy Bypass -Command \"Get-CimInstance Win32_SoundDevice | Select-Object Name,DeviceID,Status | ConvertTo-Json -Compress\"",
            null,
            8000
        );

        if (!result.Ok || string.IsNullOrWhiteSpace(result.StdOut))
        {
            return [];
        }

        try
        {
            using var document = JsonDocument.Parse(result.StdOut);
            var rawDevices = document.RootElement.ValueKind switch
            {
                JsonValueKind.Array => document.RootElement.EnumerateArray().ToArray(),
                JsonValueKind.Object => [document.RootElement],
                _ => [],
            };

            var endpoints = new List<HostAudioEndpointInfo>();
            foreach (var item in rawDevices)
            {
                var name = item.TryGetProperty("Name", out var nameElement)
                    ? nameElement.GetString() ?? string.Empty
                    : string.Empty;
                var deviceId = item.TryGetProperty("DeviceID", out var deviceIdElement)
                    ? deviceIdElement.GetString() ?? string.Empty
                    : string.Empty;
                if (string.IsNullOrWhiteSpace(name))
                {
                    continue;
                }

                foreach (var endpoint in ExpandAudioDriverToEndpoints(name, deviceId))
                {
                    endpoints.Add(endpoint);
                }
            }

            return endpoints
                .GroupBy(endpoint => $"{endpoint.Direction}|{endpoint.Name}|{endpoint.DeviceId}", StringComparer.OrdinalIgnoreCase)
                .Select(group => group.First())
                .ToList();
        }
        catch
        {
            return [];
        }
    }

    private static string? ResolveAudioInfoPath(string bundleRoot)
    {
        var candidates = new[]
        {
            Path.Combine(bundleRoot, "sunshine", "tools", "audio-info.exe"),
            Path.Combine(bundleRoot, "sunshine-legacy", "tools", "audio-info.exe"),
            Path.Combine(AppContext.BaseDirectory, "sunshine", "tools", "audio-info.exe"),
            Path.Combine(AppContext.BaseDirectory, "tools", "audio-info.exe"),
        };

        return candidates.FirstOrDefault(File.Exists);
    }

    private static string ReadAudioInfoValue(string line)
    {
        var separatorIndex = line.IndexOf(':');
        if (separatorIndex < 0 || separatorIndex >= line.Length - 1)
        {
            return string.Empty;
        }

        return line[(separatorIndex + 1)..].Trim();
    }

    private static string InferAudioEndpointDirection(string name)
    {
        if (name.Contains("output", StringComparison.OrdinalIgnoreCase) ||
            name.Contains("microphone", StringComparison.OrdinalIgnoreCase) ||
            name.Contains("mic ", StringComparison.OrdinalIgnoreCase) ||
            name.StartsWith("mic", StringComparison.OrdinalIgnoreCase) ||
            name.Contains("cable output", StringComparison.OrdinalIgnoreCase))
        {
            return "input";
        }

        return "output";
    }

    private static IEnumerable<HostAudioEndpointInfo> ExpandAudioDriverToEndpoints(string driverName, string deviceId)
    {
        if (driverName.Contains("VB-Audio Cable A", StringComparison.OrdinalIgnoreCase))
        {
            yield return new HostAudioEndpointInfo { Direction = "output", DeviceId = deviceId, Name = "CABLE-A Input (VB-Audio Cable A)" };
            yield return new HostAudioEndpointInfo { Direction = "input", DeviceId = deviceId, Name = "CABLE-A Output (VB-Audio Cable A)" };
            yield break;
        }

        if (driverName.Contains("VB-Audio Cable B", StringComparison.OrdinalIgnoreCase))
        {
            yield return new HostAudioEndpointInfo { Direction = "output", DeviceId = deviceId, Name = "CABLE-B Input (VB-Audio Cable B)" };
            yield return new HostAudioEndpointInfo { Direction = "input", DeviceId = deviceId, Name = "CABLE-B Output (VB-Audio Cable B)" };
            yield break;
        }

        if (driverName.Contains("VB-Audio Virtual Cable", StringComparison.OrdinalIgnoreCase))
        {
            yield return new HostAudioEndpointInfo { Direction = "output", DeviceId = deviceId, Name = "CABLE Input (VB-Audio Virtual Cable)" };
            yield return new HostAudioEndpointInfo { Direction = "input", DeviceId = deviceId, Name = "CABLE Output (VB-Audio Virtual Cable)" };
            yield break;
        }

        if (driverName.Contains("Steam Streaming Speakers", StringComparison.OrdinalIgnoreCase))
        {
            yield return new HostAudioEndpointInfo { Direction = "output", DeviceId = deviceId, Name = "Speakers (Steam Streaming Speakers)" };
            yield break;
        }

        if (driverName.Contains("Virtual Speakers for AudioRelay", StringComparison.OrdinalIgnoreCase))
        {
            yield return new HostAudioEndpointInfo { Direction = "output", DeviceId = deviceId, Name = "Virtual Speakers for AudioRelay" };
            yield break;
        }

        if (driverName.Contains("Virtual Mic for AudioRelay", StringComparison.OrdinalIgnoreCase))
        {
            yield return new HostAudioEndpointInfo { Direction = "input", DeviceId = deviceId, Name = "Virtual Mic for AudioRelay" };
            yield break;
        }

        if (driverName.Contains("SYMO Virtual Audio Output", StringComparison.OrdinalIgnoreCase))
        {
            yield return new HostAudioEndpointInfo { Direction = "output", DeviceId = deviceId, Name = "SYMO Virtual Audio Output" };
            yield break;
        }

        if (driverName.Contains("SYMO Virtual Audio Input", StringComparison.OrdinalIgnoreCase))
        {
            yield return new HostAudioEndpointInfo { Direction = "input", DeviceId = deviceId, Name = "SYMO Virtual Audio Input" };
            yield break;
        }

        if (driverName.Contains("Virtual Audio Driver by MTT", StringComparison.OrdinalIgnoreCase))
        {
            yield return new HostAudioEndpointInfo { Direction = "output", DeviceId = deviceId, Name = "Virtual Audio Driver by MTT" };
            yield break;
        }

        if (driverName.Contains("Virtual Mic Driver by MTT", StringComparison.OrdinalIgnoreCase))
        {
            yield return new HostAudioEndpointInfo { Direction = "input", DeviceId = deviceId, Name = "Virtual Mic Driver by MTT" };
            yield break;
        }

        if (driverName.Contains("Virtual Audio Driver Output", StringComparison.OrdinalIgnoreCase))
        {
            yield return new HostAudioEndpointInfo { Direction = "output", DeviceId = deviceId, Name = "Virtual Audio Driver Output" };
            yield break;
        }

        if (driverName.Contains("Virtual Audio Driver Input", StringComparison.OrdinalIgnoreCase))
        {
            yield return new HostAudioEndpointInfo { Direction = "input", DeviceId = deviceId, Name = "Virtual Audio Driver Input" };
            yield break;
        }

        if (driverName.Contains("Virtual Audio Driver", StringComparison.OrdinalIgnoreCase))
        {
            yield return new HostAudioEndpointInfo { Direction = "output", DeviceId = deviceId, Name = "Virtual Audio Driver Output" };
            yield return new HostAudioEndpointInfo { Direction = "input", DeviceId = deviceId, Name = "Virtual Audio Driver Input" };
        }
    }

    private static string ResolveAudioPreferencesPath(string bundleRoot) =>
        Path.Combine(bundleRoot, "moonlight", "server", "host_audio_preferences.json");

    private static HostAudioPreferences? ReadAudioPreferences(string bundleRoot)
    {
        var path = ResolveAudioPreferencesPath(bundleRoot);
        if (!File.Exists(path))
        {
            return null;
        }

        try
        {
            var parsed = JsonSerializer.Deserialize<HostAudioPreferences>(File.ReadAllText(path), JsonOptions);
            if (parsed is null || parsed.SchemaVersion != 1)
            {
                return null;
            }

            parsed.Mode = string.Equals(parsed.Mode?.Trim(), "manual", StringComparison.OrdinalIgnoreCase)
                ? "manual"
                : "auto";
            parsed.SelectedAudioSinkName = string.IsNullOrWhiteSpace(parsed.SelectedAudioSinkName)
                ? null
                : parsed.SelectedAudioSinkName.Trim();
            parsed.SelectedVirtualSinkName = string.IsNullOrWhiteSpace(parsed.SelectedVirtualSinkName)
                ? null
                : parsed.SelectedVirtualSinkName.Trim();
            parsed.SelectedMicrophoneName = string.IsNullOrWhiteSpace(parsed.SelectedMicrophoneName)
                ? null
                : parsed.SelectedMicrophoneName.Trim();
            return parsed;
        }
        catch
        {
            return null;
        }
    }

    private static string? FindAudioEndpointName(
        IReadOnlyCollection<string> candidates,
        string? preferredName)
    {
        if (string.IsNullOrWhiteSpace(preferredName))
        {
            return null;
        }

        return candidates.FirstOrDefault(name =>
            name.Equals(preferredName.Trim(), StringComparison.OrdinalIgnoreCase));
    }

    private static string? InferMicrophoneNameFromAudioSink(string? selectedAudioSinkName)
    {
        if (string.IsNullOrWhiteSpace(selectedAudioSinkName))
        {
            return null;
        }

        if (selectedAudioSinkName.Contains("SYMO Virtual Audio Output", StringComparison.OrdinalIgnoreCase))
        {
            return "SYMO Virtual Audio Input";
        }
        if (selectedAudioSinkName.Contains("Virtual Audio Driver by MTT", StringComparison.OrdinalIgnoreCase))
        {
            return "Virtual Mic Driver by MTT";
        }
        if (selectedAudioSinkName.Contains("Virtual Audio Driver Output", StringComparison.OrdinalIgnoreCase))
        {
            return "Virtual Audio Driver Input";
        }
        if (selectedAudioSinkName.Contains("CABLE-A Input", StringComparison.OrdinalIgnoreCase))
        {
            return "CABLE-A Output (VB-Audio Cable A)";
        }
        if (selectedAudioSinkName.Contains("CABLE-B Input", StringComparison.OrdinalIgnoreCase))
        {
            return "CABLE-B Output (VB-Audio Cable B)";
        }
        if (selectedAudioSinkName.Contains("CABLE Input", StringComparison.OrdinalIgnoreCase))
        {
            return "CABLE Output (VB-Audio Virtual Cable)";
        }
        if (selectedAudioSinkName.Contains("Virtual Speakers for AudioRelay", StringComparison.OrdinalIgnoreCase))
        {
            return "Virtual Mic for AudioRelay";
        }

        return null;
    }

    private static (string? SelectedAudioSinkName, string? SelectedVirtualSinkName, string? SelectedMicrophoneName, string? AudioSelectionReason)
        AutoSelectPreferredVirtualAudioEndpoints(IReadOnlyCollection<HostAudioEndpointInfo> audioEndpoints)
    {
        var outputs = audioEndpoints
            .Where(endpoint => endpoint.Direction.Equals("output", StringComparison.OrdinalIgnoreCase))
            .Select(endpoint => endpoint.Name)
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .ToList();
        var inputs = audioEndpoints
            .Where(endpoint => endpoint.Direction.Equals("input", StringComparison.OrdinalIgnoreCase))
            .Select(endpoint => endpoint.Name)
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .ToList();

        string? Pick(IReadOnlyCollection<string> candidates, params string[] patterns)
        {
            foreach (var pattern in patterns)
            {
                var match = candidates.FirstOrDefault(name => name.Contains(pattern, StringComparison.OrdinalIgnoreCase));
                if (!string.IsNullOrWhiteSpace(match))
                {
                    return match;
                }
            }

            return null;
        }

        var selectedAudioSinkName = Pick(
            outputs,
            "Steam Streaming Speakers",
            "SYMO Virtual Audio Output",
            "Virtual Audio Driver by MTT",
            "Virtual Audio Driver Output",
            "Virtual Speakers for AudioRelay",
            "CABLE Input (VB-Audio Virtual Cable)",
            "CABLE-A Input",
            "CABLE-B Input",
            "CABLE Input");

        var selectedVirtualSinkName = Pick(
            outputs,
            "Steam Streaming Speakers",
            "SYMO Virtual Audio Output",
            "Virtual Audio Driver by MTT",
            "Virtual Audio Driver Output",
            "Virtual Speakers for AudioRelay",
            "CABLE Input (VB-Audio Virtual Cable)",
            "CABLE-A Input",
            "CABLE-B Input",
            "CABLE Input");

        if (string.IsNullOrWhiteSpace(selectedVirtualSinkName))
        {
            selectedVirtualSinkName = selectedAudioSinkName;
        }

        var selectedMicrophoneName = Pick(
            inputs,
            "SYMO Virtual Audio Input",
            "Virtual Mic Driver by MTT",
            "Virtual Audio Driver Input",
            "CABLE Output (VB-Audio Virtual Cable)",
            "CABLE-A Output",
            "CABLE-B Output",
            "CABLE Output",
            "Virtual Mic for AudioRelay");

        var audioSelectionReason = string.IsNullOrWhiteSpace(selectedAudioSinkName)
            ? "no_known_virtual_audio_output_detected"
            : $"audio_sink={selectedAudioSinkName};virtual_sink={selectedVirtualSinkName ?? "none"};mic_input={selectedMicrophoneName ?? "none"}";

        return (selectedAudioSinkName, selectedVirtualSinkName, selectedMicrophoneName, audioSelectionReason);
    }

    private static (string? SelectedAudioSinkName, string? SelectedVirtualSinkName, string? SelectedMicrophoneName, string? AudioSelectionReason, string AudioSelectionMode)
        SelectPreferredVirtualAudioEndpoints(string bundleRoot, IReadOnlyCollection<HostAudioEndpointInfo> audioEndpoints)
    {
        var automatic = AutoSelectPreferredVirtualAudioEndpoints(audioEndpoints);
        var preferences = ReadAudioPreferences(bundleRoot);
        if (preferences is null || !preferences.Mode.Equals("manual", StringComparison.OrdinalIgnoreCase))
        {
            return (
                automatic.SelectedAudioSinkName,
                automatic.SelectedVirtualSinkName,
                automatic.SelectedMicrophoneName,
                automatic.AudioSelectionReason,
                "auto");
        }

        var outputs = audioEndpoints
            .Where(endpoint => endpoint.Direction.Equals("output", StringComparison.OrdinalIgnoreCase))
            .Select(endpoint => endpoint.Name)
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .ToList();
        var inputs = audioEndpoints
            .Where(endpoint => endpoint.Direction.Equals("input", StringComparison.OrdinalIgnoreCase))
            .Select(endpoint => endpoint.Name)
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .ToList();

        var selectedAudioSinkName =
            FindAudioEndpointName(outputs, preferences.SelectedAudioSinkName) ??
            preferences.SelectedAudioSinkName;
        var selectedVirtualSinkName =
            FindAudioEndpointName(outputs, preferences.SelectedVirtualSinkName) ??
            preferences.SelectedVirtualSinkName;
        if (string.IsNullOrWhiteSpace(selectedVirtualSinkName))
        {
            selectedVirtualSinkName = selectedAudioSinkName;
        }

        var selectedMicrophoneName =
            FindAudioEndpointName(inputs, preferences.SelectedMicrophoneName) ??
            preferences.SelectedMicrophoneName ??
            InferMicrophoneNameFromAudioSink(selectedAudioSinkName) ??
            automatic.SelectedMicrophoneName;

        if (string.IsNullOrWhiteSpace(selectedAudioSinkName) || string.IsNullOrWhiteSpace(selectedVirtualSinkName))
        {
            return (
                automatic.SelectedAudioSinkName,
                automatic.SelectedVirtualSinkName,
                automatic.SelectedMicrophoneName,
                automatic.AudioSelectionReason,
                "auto");
        }

        var audioSelectionReason =
            $"manual_override:audio_sink={selectedAudioSinkName};virtual_sink={selectedVirtualSinkName};mic_input={selectedMicrophoneName ?? "none"}";

        return (
            selectedAudioSinkName,
            selectedVirtualSinkName,
            selectedMicrophoneName,
            audioSelectionReason,
            "manual");
    }

    private static string ResolveBundleRoot(string explicitBundleRoot)
    {
        var baseDir = AppContext.BaseDirectory.TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar);
        var rawCandidates = new List<string>();
        if (!string.IsNullOrWhiteSpace(explicitBundleRoot))
        {
            rawCandidates.Add(explicitBundleRoot);
        }

        rawCandidates.AddRange([
            baseDir,
            Path.GetFullPath(Path.Combine(baseDir, "..")),
            Path.GetFullPath(Path.Combine(baseDir, "..", "..")),
            Path.GetFullPath(Path.Combine(baseDir, "..", "..", "..")),
        ]);

        var candidates = rawCandidates
            .Where(path => !string.IsNullOrWhiteSpace(path))
            .Select(Path.GetFullPath)
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .ToList();

        foreach (var candidate in candidates)
        {
            var normalized = TryNormalizeBundleRootFromMoonlightServerLayout(candidate);
            if (!string.IsNullOrWhiteSpace(normalized))
            {
                return normalized;
            }
        }

        foreach (var candidate in candidates)
        {
            var bundleRoot = TryResolveBundleRootCandidate(candidate);
            if (!string.IsNullOrWhiteSpace(bundleRoot))
            {
                return bundleRoot;
            }
        }

        foreach (var candidate in candidates)
        {
            if (Directory.Exists(Path.Combine(candidate, "sunshine", "config")))
            {
                return candidate;
            }
        }

        return Path.GetFullPath(Path.Combine(baseDir, "..", ".."));
    }

    private static string? TryNormalizeBundleRootFromMoonlightServerLayout(string path)
    {
        try
        {
            var current = new DirectoryInfo(Path.GetFullPath(path));
            string? normalized = null;
            while (current is not null)
            {
                if (current.Name.Equals("server", StringComparison.OrdinalIgnoreCase)
                    && current.Parent?.Name.Equals("moonlight", StringComparison.OrdinalIgnoreCase) == true
                    && current.Parent.Parent is not null)
                {
                    normalized = current.Parent.Parent.FullName;
                }

                current = current.Parent;
            }

            return normalized;
        }
        catch
        {
            return null;
        }
    }

    private static string? TryResolveBundleRootCandidate(string path)
    {
        try
        {
            var current = new DirectoryInfo(Path.GetFullPath(path));
            while (current is not null)
            {
                var candidate = current.FullName;
                var moonlightServerPath = Path.Combine(candidate, "moonlight", "server");
                var hasBundleMarker = Directory.Exists(moonlightServerPath);
                var hasRuntimeConfig =
                    Directory.Exists(Path.Combine(candidate, "sunshine", "config")) ||
                    Directory.EnumerateDirectories(candidate, "sunshine*")
                        .Any(directory => Directory.Exists(Path.Combine(directory, "config")));
                if (hasBundleMarker && hasRuntimeConfig)
                {
                    return candidate;
                }

                current = current.Parent;
            }
        }
        catch
        {
            // Fall back to the generic candidate list.
        }

        return null;
    }

    private static List<GpuControllerInfo> DetectGpuControllers()
    {
        var result = RunProcess(
            "powershell.exe",
            "-NoProfile -ExecutionPolicy Bypass -Command \"Get-CimInstance Win32_VideoController | Select-Object Name,DriverVersion | ConvertTo-Json -Compress\"",
            null,
            8000
        );

        if (!result.Ok || string.IsNullOrWhiteSpace(result.StdOut))
        {
            return [];
        }

        try
        {
            using var document = JsonDocument.Parse(result.StdOut);
            return document.RootElement.ValueKind switch
            {
                JsonValueKind.Array => document.RootElement.EnumerateArray().Select(ParseGpuController).ToList(),
                JsonValueKind.Object => [ParseGpuController(document.RootElement)],
                _ => [],
            };
        }
        catch
        {
            return [];
        }
    }

    private static GpuControllerInfo ParseGpuController(JsonElement element) => new()
    {
        Name = element.TryGetProperty("Name", out var name) ? name.GetString() ?? string.Empty : string.Empty,
        DriverVersion = element.TryGetProperty("DriverVersion", out var driverVersion) ? driverVersion.GetString() ?? string.Empty : string.Empty,
    };

    private static List<SunshineRuntimeCandidate> DiscoverSunshineRuntimes(string bundleRoot, bool forceNvenc)
    {
        if (!Directory.Exists(bundleRoot))
        {
            return [];
        }

        var manifestRuntimes = LoadRuntimeManifest(bundleRoot, forceNvenc);
        if (manifestRuntimes.Count > 0)
        {
            return manifestRuntimes;
        }

        var runtimeRoots = Directory
            .GetDirectories(bundleRoot, "sunshine*")
            .Where(path => File.Exists(Path.Combine(path, "sunshine.exe")))
            .OrderBy(path => Path.GetFileName(path).Equals("sunshine", StringComparison.OrdinalIgnoreCase) ? 0 : 1)
            .ThenBy(path => Path.GetFileName(path), StringComparer.OrdinalIgnoreCase)
            .ToArray();

        var runtimes = new List<SunshineRuntimeCandidate>();
        foreach (var runtimeRoot in runtimeRoots)
        {
            var runtimeDirectory = Path.GetRelativePath(bundleRoot, runtimeRoot);
            var key = ResolveRuntimeKey(runtimeDirectory);
            var legacy = key.Contains("legacy", StringComparison.OrdinalIgnoreCase);
            var resolvedFfmpeg = FindFfmpegPath(bundleRoot, runtimeRoot, runtimeDirectory, key, null);
            var ffmpegPath = resolvedFfmpeg.Path;
            var metadata = ResolveRuntimeMetadata(bundleRoot, runtimeRoot, runtimeDirectory, key, legacy, ffmpegPath, forceNvenc);
            runtimes.Add(new SunshineRuntimeCandidate
            {
                Key = key,
                RelativeDirectory = runtimeDirectory,
                RootPath = runtimeRoot,
                ConfigPath = Path.Combine(runtimeRoot, "config", "sunshine.conf"),
                FfmpegPath = ffmpegPath,
                FfmpegSource = resolvedFfmpeg.Source,
                RequiresBundledFfmpeg = metadata.RequiresBundledFfmpeg,
                Legacy = legacy,
                DisplayName = metadata.DisplayName,
                RuntimeVersion = metadata.RuntimeVersion,
                RuntimeFingerprint = metadata.RuntimeFingerprint,
                Priority = key.Equals("default", StringComparison.OrdinalIgnoreCase) ? 0 : 100,
                AutoSelect = metadata.AutoSelect,
                StartupValidationStatus = metadata.StartupValidationStatus,
                StartupValidationReason = metadata.StartupValidationReason,
                StartupValidationCheckedAt = metadata.StartupValidationCheckedAt,
            });
        }

        return runtimes;
    }

    private static List<SunshineRuntimeCandidate> LoadRuntimeManifest(string bundleRoot, bool forceNvenc)
    {
        var manifestPath = Path.Combine(bundleRoot, "moonlight", "server", "sunshine_runtime_manifest.json");
        if (!File.Exists(manifestPath))
        {
            return [];
        }

        try
        {
            var manifest = JsonSerializer.Deserialize<SunshineRuntimeManifest>(File.ReadAllText(manifestPath), JsonOptions);
            if (manifest?.Runtimes is null || manifest.Runtimes.Count == 0)
            {
                return [];
            }

            return manifest.Runtimes
                .OrderBy(item => item.Priority)
                .ThenBy(item => item.Key, StringComparer.OrdinalIgnoreCase)
                .Select(item =>
                {
                    var runtimeDirectory = item.RelativeDirectory.Trim();
                    var runtimeRoot = Path.GetFullPath(Path.Combine(bundleRoot, runtimeDirectory));
                    if (!File.Exists(Path.Combine(runtimeRoot, "sunshine.exe")))
                    {
                        return null;
                    }

                    var runtimeKey = string.IsNullOrWhiteSpace(item.Key) ? ResolveRuntimeKey(runtimeDirectory) : item.Key;
                    var resolvedFfmpeg = FindFfmpegPath(bundleRoot, runtimeRoot, runtimeDirectory, item.Key, item.FfmpegRelativePath);
                    var ffmpegPath = resolvedFfmpeg.Path;
                    var metadata = ResolveRuntimeMetadata(
                        bundleRoot,
                        runtimeRoot,
                        runtimeDirectory,
                        runtimeKey,
                        item.Legacy,
                        ffmpegPath,
                        forceNvenc
                    );

                    return new SunshineRuntimeCandidate
                    {
                        Key = runtimeKey,
                        RelativeDirectory = runtimeDirectory,
                        RootPath = runtimeRoot,
                        ConfigPath = Path.Combine(runtimeRoot, "config", "sunshine.conf"),
                        FfmpegPath = ffmpegPath,
                        FfmpegSource = resolvedFfmpeg.Source,
                        RequiresBundledFfmpeg = item.RequiresBundledFfmpeg || metadata.RequiresBundledFfmpeg,
                        Legacy = item.Legacy,
                        DisplayName = string.IsNullOrWhiteSpace(item.DisplayName) ? metadata.DisplayName : item.DisplayName,
                        RuntimeVersion = string.IsNullOrWhiteSpace(item.RuntimeVersion) ? metadata.RuntimeVersion : item.RuntimeVersion,
                        RuntimeFingerprint = metadata.RuntimeFingerprint,
                        Priority = item.Priority,
                        AutoSelect = item.AutoSelect && metadata.AutoSelect,
                        StartupValidationStatus = metadata.StartupValidationStatus,
                        StartupValidationReason = metadata.StartupValidationReason,
                        StartupValidationCheckedAt = metadata.StartupValidationCheckedAt,
                    };
                })
                .Where(item => item is not null)
                .Cast<SunshineRuntimeCandidate>()
                .ToList();
        }
        catch
        {
            return [];
        }
    }

    private static string ResolveRuntimeKey(string runtimeDirectory)
    {
        var name = Path.GetFileName(runtimeDirectory.TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar));
        if (name.Equals("sunshine", StringComparison.OrdinalIgnoreCase))
        {
            return "default";
        }

        if (name.StartsWith("sunshine-", StringComparison.OrdinalIgnoreCase))
        {
            return name["sunshine-".Length..];
        }

        return name;
    }

    private static ResolvedFfmpegPath FindFfmpegPath(string bundleRoot, string runtimeRoot, string runtimeDirectory, string runtimeKey, string? manifestRelativePath)
    {
        var runtimeName = Path.GetFileName(runtimeRoot.TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar));
        var candidates = new (string? Path, string Source)[]
        {
            (string.IsNullOrWhiteSpace(manifestRelativePath) ? null : Path.GetFullPath(Path.Combine(bundleRoot, manifestRelativePath)), "runtime-manifest"),
            (Path.Combine(runtimeRoot, "ffmpeg.exe"), "runtime-local"),
            (Path.Combine(runtimeRoot, "tools", "ffmpeg.exe"), "runtime-local"),
            (Path.Combine(bundleRoot, "tools", runtimeName, "ffmpeg.exe"), "bundle-local"),
            (Path.Combine(bundleRoot, "tools", runtimeKey, "ffmpeg.exe"), "bundle-local"),
            (Path.Combine(bundleRoot, "tools", runtimeDirectory, "ffmpeg.exe"), "bundle-local"),
            (Path.Combine(bundleRoot, "tools", "ffmpeg.exe"), "bundle-local"),
            (Path.Combine(bundleRoot, "ffmpeg.exe"), "bundle-local"),
        };

        var seenCandidates = new HashSet<string>(StringComparer.OrdinalIgnoreCase);

        foreach (var candidate in candidates)
        {
            if (string.IsNullOrWhiteSpace(candidate.Path) || !seenCandidates.Add(candidate.Path))
            {
                continue;
            }

            if (File.Exists(candidate.Path))
            {
                return new ResolvedFfmpegPath
                {
                    Path = candidate.Path,
                    Source = candidate.Source,
                };
            }
        }

        var externalPath = FindFirstCommandPath("ffmpeg.exe", "ffmpeg");
        if (!string.IsNullOrWhiteSpace(externalPath))
        {
            return new ResolvedFfmpegPath
            {
                Path = externalPath,
                Source = "external-path",
            };
        }

        return new ResolvedFfmpegPath
        {
            Path = null,
            Source = null,
        };
    }

    private static string? FindFirstCommandPath(params string[] names)
    {
        foreach (var name in names)
        {
            var result = RunProcess("where.exe", name, null, 4000);
            if (!result.Ok || string.IsNullOrWhiteSpace(result.StdOut))
            {
                continue;
            }

            var match = result.StdOut
                .Split(['\r', '\n'], StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
                .FirstOrDefault(File.Exists);
            if (!string.IsNullOrWhiteSpace(match))
            {
                return match;
            }
        }

        return null;
    }

    private static List<EncoderProbeResult> ProbeEncoders(
        IReadOnlyCollection<SunshineRuntimeCandidate> runtimes,
        bool forceNvenc)
    {
        var probes = new List<EncoderProbeResult>();
        foreach (var runtime in runtimes)
        {
            if (runtime.RequiresBundledFfmpeg && !IsBundledFfmpegSource(runtime.FfmpegSource))
            {
                probes.AddRange(EncoderProbeCandidates.Select(candidate => new EncoderProbeResult
                {
                    RuntimeKey = runtime.Key,
                    RuntimeDirectory = runtime.RelativeDirectory,
                    EncoderKey = candidate.EncoderKey,
                    FfmpegCodec = candidate.FfmpegCodec,
                    Available = false,
                    Ok = false,
                    Detail = "bundled_ffmpeg_required",
                }));
                continue;
            }

            var ffmpegPath = runtime.FfmpegPath;
            if (string.IsNullOrWhiteSpace(ffmpegPath))
            {
                probes.AddRange(EncoderProbeCandidates.Select(candidate => new EncoderProbeResult
                {
                    RuntimeKey = runtime.Key,
                    RuntimeDirectory = runtime.RelativeDirectory,
                    EncoderKey = candidate.EncoderKey,
                    FfmpegCodec = candidate.FfmpegCodec,
                    Available = false,
                    Ok = false,
                    Detail = "ffmpeg_not_found",
                }));
                continue;
            }

            var encodersOutput = RunProcess(ffmpegPath, "-hide_banner -encoders", null, 10000);
            var encodersText = $"{encodersOutput.StdOut}\n{encodersOutput.StdErr}";
            foreach (var candidate in EncoderProbeCandidates)
            {
                if (forceNvenc &&
                    runtime.Legacy &&
                    candidate.EncoderKey.Equals("software", StringComparison.OrdinalIgnoreCase))
                {
                    probes.Add(new EncoderProbeResult
                    {
                        RuntimeKey = runtime.Key,
                        RuntimeDirectory = runtime.RelativeDirectory,
                        EncoderKey = candidate.EncoderKey,
                        FfmpegCodec = candidate.FfmpegCodec,
                        Available = false,
                        Ok = false,
                        Detail = "disabled_by_force_nvenc_policy",
                    });
                    continue;
                }

                var available = encodersText.IndexOf(candidate.FfmpegCodec, StringComparison.OrdinalIgnoreCase) >= 0;
                if (!available)
                {
                    probes.Add(new EncoderProbeResult
                    {
                        RuntimeKey = runtime.Key,
                        RuntimeDirectory = runtime.RelativeDirectory,
                        EncoderKey = candidate.EncoderKey,
                        FfmpegCodec = candidate.FfmpegCodec,
                        Available = false,
                        Ok = false,
                        Detail = "codec_not_listed",
                    });
                    continue;
                }

                var probe = RunProcess(
                    ffmpegPath,
                    $"-hide_banner -loglevel error -f lavfi -i testsrc2=size=1280x720:rate=30 -frames:v 1 -c:v {candidate.FfmpegCodec} -f null -",
                    null,
                    30000
                );

                var detail = probe.Ok
                    ? "probe_ok"
                    : SummarizeEncoderProbeFailure(candidate.EncoderKey, $"{probe.StdOut}\n{probe.StdErr}");
                var ok = probe.Ok;

                if (ok && candidate.EncoderKey.Equals("nvenc", StringComparison.OrdinalIgnoreCase))
                {
                    var sustainedProbe = RunNvencSustainedProbe(ffmpegPath, runtime.Legacy);
                    ok = sustainedProbe.Ok;
                    detail = sustainedProbe.Ok
                        ? (runtime.Legacy ? "probe_ok:nvenc_stream_sanity_legacy" : "probe_ok:nvenc_stream_sanity")
                        : $"nvenc_stream_sanity_failed {SummarizeEncoderProbeFailure(candidate.EncoderKey, $"{sustainedProbe.StdOut}\n{sustainedProbe.StdErr}")}";
                }

                probes.Add(new EncoderProbeResult
                {
                    RuntimeKey = runtime.Key,
                    RuntimeDirectory = runtime.RelativeDirectory,
                    EncoderKey = candidate.EncoderKey,
                    FfmpegCodec = candidate.FfmpegCodec,
                    Available = true,
                    Ok = ok,
                    Detail = detail,
                });
            }
        }

        return probes;
    }

    private static ProcessResult RunNvencSustainedProbe(string ffmpegPath, bool legacyRuntime)
    {
        var size = legacyRuntime ? "900x1440" : "1280x720";
        var frames = legacyRuntime ? 90 : 120;
        var bitrate = legacyRuntime ? "4500k" : "8000k";
        var buffer = legacyRuntime ? "9000k" : "16000k";
        var gop = legacyRuntime ? 30 : 60;

        return RunProcess(
            ffmpegPath,
            $"-hide_banner -loglevel error -f lavfi -i testsrc2=size={size}:rate=30 -frames:v {frames} -pix_fmt nv12 -c:v h264_nvenc -preset p4 -tune ull -rc cbr -g {gop} -b:v {bitrate} -maxrate {bitrate} -bufsize {buffer} -f null -",
            null,
            45000
        );
    }

    private static bool IsBundledFfmpegSource(string? source) =>
        source is "runtime-manifest" or "runtime-local" or "bundle-local";

    private static SunshineRuntimeCandidate CloneRuntimeCandidate(SunshineRuntimeCandidate runtime) => new()
    {
        Key = runtime.Key,
        RelativeDirectory = runtime.RelativeDirectory,
        RootPath = runtime.RootPath,
        ConfigPath = runtime.ConfigPath,
        FfmpegPath = runtime.FfmpegPath,
        FfmpegSource = runtime.FfmpegSource,
        RequiresBundledFfmpeg = runtime.RequiresBundledFfmpeg,
        HealthyEncoders = [.. runtime.HealthyEncoders],
        RuntimeStatus = runtime.RuntimeStatus,
        RuntimeStatusReason = runtime.RuntimeStatusReason,
        Legacy = runtime.Legacy,
        DisplayName = runtime.DisplayName,
        RuntimeVersion = runtime.RuntimeVersion,
        RuntimeFingerprint = runtime.RuntimeFingerprint,
        Priority = runtime.Priority,
        AutoSelect = runtime.AutoSelect,
        StartupValidationStatus = runtime.StartupValidationStatus,
        StartupValidationReason = runtime.StartupValidationReason,
        StartupValidationCheckedAt = runtime.StartupValidationCheckedAt,
    };

    private static bool ProbeCountsAsHealthyForPolicy(EncoderProbeResult probe, bool forceNvenc) =>
        probe.Ok &&
        (!forceNvenc || probe.EncoderKey.Equals("nvenc", StringComparison.OrdinalIgnoreCase));

    private static void AnnotateRuntimeCandidatesWithProbeResults(
        IReadOnlyCollection<SunshineRuntimeCandidate> runtimes,
        IReadOnlyCollection<EncoderProbeResult> probes,
        bool forceNvenc)
    {
        foreach (var runtime in runtimes)
        {
            if (!runtime.AutoSelect)
            {
                runtime.HealthyEncoders = probes
                    .Where(probe =>
                        string.Equals(probe.RuntimeKey, runtime.Key, StringComparison.OrdinalIgnoreCase) &&
                        ProbeCountsAsHealthyForPolicy(probe, forceNvenc))
                    .Select(probe => probe.EncoderKey)
                    .Distinct(StringComparer.OrdinalIgnoreCase)
                    .OrderBy(encoder => encoder, StringComparer.OrdinalIgnoreCase)
                    .ToList();

                if (string.Equals(runtime.StartupValidationStatus, "failed", StringComparison.OrdinalIgnoreCase))
                {
                    runtime.RuntimeStatus = "validation_failed";
                    runtime.RuntimeStatusReason = runtime.StartupValidationReason ?? "runtime_start_validation_failed";
                    continue;
                }

                if (string.Equals(runtime.StartupValidationStatus, "stale", StringComparison.OrdinalIgnoreCase))
                {
                    runtime.RuntimeStatus = "validation_stale";
                    runtime.RuntimeStatusReason = runtime.StartupValidationReason ?? "runtime_fingerprint_changed";
                    continue;
                }

                if (string.Equals(runtime.StartupValidationStatus, "passed", StringComparison.OrdinalIgnoreCase))
                {
                    runtime.RuntimeStatus = "manual_only";
                    runtime.RuntimeStatusReason = runtime.StartupValidationReason ?? "validated_manual_enable_required";
                    continue;
                }

                runtime.RuntimeStatus = "validation_required";
                runtime.RuntimeStatusReason = runtime.StartupValidationReason ?? "runtime_start_validation_required";
                continue;
            }

            runtime.HealthyEncoders = probes
                .Where(probe =>
                    string.Equals(probe.RuntimeKey, runtime.Key, StringComparison.OrdinalIgnoreCase) &&
                    ProbeCountsAsHealthyForPolicy(probe, forceNvenc))
                .Select(probe => probe.EncoderKey)
                .Distinct(StringComparer.OrdinalIgnoreCase)
                .OrderBy(encoder => encoder, StringComparer.OrdinalIgnoreCase)
                .ToList();

            if (runtime.RequiresBundledFfmpeg && !IsBundledFfmpegSource(runtime.FfmpegSource))
            {
                runtime.RuntimeStatus = "bundled_ffmpeg_required";
                runtime.RuntimeStatusReason = "runtime requires bundled ffmpeg";
                continue;
            }

            if (runtime.HealthyEncoders.Count > 0)
            {
                runtime.RuntimeStatus = runtime.HealthyEncoders.All(encoder => string.Equals(encoder, "software", StringComparison.OrdinalIgnoreCase))
                    ? "software_only"
                    : "ready";
                runtime.RuntimeStatusReason = $"healthy={string.Join(",", runtime.HealthyEncoders)}";
                continue;
            }

            var runtimeProbeFailures = probes
                .Where(probe => string.Equals(probe.RuntimeKey, runtime.Key, StringComparison.OrdinalIgnoreCase) && !probe.Ok)
                .Select(probe => probe.Detail)
                .Where(detail => !string.IsNullOrWhiteSpace(detail))
                .Distinct(StringComparer.OrdinalIgnoreCase)
                .ToList();

            if (runtimeProbeFailures.Count > 0)
            {
                runtime.RuntimeStatus = "probe_failed";
                runtime.RuntimeStatusReason = runtimeProbeFailures[0];
                continue;
            }

            runtime.RuntimeStatus = "not_probed";
            runtime.RuntimeStatusReason = null;
        }
    }

    private static List<string> BuildPreflightWarnings(
        IReadOnlyCollection<GpuControllerInfo> gpuControllers,
        IReadOnlyCollection<SunshineRuntimeCandidate> runtimes,
        IReadOnlyCollection<EncoderProbeResult> probes,
        SunshineRuntimeCandidate? selectedRuntime,
        string selectedEncoder,
        string selectionReason,
        bool forceNvenc)
    {
        var warnings = new List<string>();

        if (selectedRuntime is not null && string.Equals(selectedRuntime.FfmpegSource, "external-path", StringComparison.OrdinalIgnoreCase))
        {
            warnings.Add("external_ffmpeg_path");
        }

        foreach (var runtime in runtimes.Where(runtime => runtime.RequiresBundledFfmpeg && !IsBundledFfmpegSource(runtime.FfmpegSource)))
        {
            warnings.Add($"runtime_requires_bundled_ffmpeg:{runtime.Key}");
        }

        foreach (var runtime in runtimes.Where(runtime =>
            string.Equals(runtime.RuntimeStatus, "validation_failed", StringComparison.OrdinalIgnoreCase) ||
            string.Equals(runtime.RuntimeStatus, "validation_stale", StringComparison.OrdinalIgnoreCase)))
        {
            warnings.Add($"runtime_start_validation_failed:{runtime.Key}");
        }

        if (string.Equals(selectedEncoder, "software", StringComparison.OrdinalIgnoreCase))
        {
            warnings.Add("software_encoder_selected");
        }

        if (string.Equals(selectedEncoder, "nvenc", StringComparison.OrdinalIgnoreCase) &&
            !probes.Any(probe =>
                probe.Ok &&
                string.Equals(probe.EncoderKey, "nvenc", StringComparison.OrdinalIgnoreCase) &&
                (selectedRuntime is null || string.Equals(probe.RuntimeKey, selectedRuntime.Key, StringComparison.OrdinalIgnoreCase))))
        {
            warnings.Add("nvenc_forced_without_probe");
        }

        var hasNvidia = gpuControllers.Any(gpu => gpu.Name.Contains("nvidia", StringComparison.OrdinalIgnoreCase));
        if (hasNvidia && probes.Any(probe =>
                string.Equals(probe.EncoderKey, "nvenc", StringComparison.OrdinalIgnoreCase) &&
                probe.Detail.Contains("nvenc_api_mismatch", StringComparison.OrdinalIgnoreCase)))
        {
            warnings.Add("nvenc_api_mismatch");
        }

        if (hasNvidia && probes.Any(probe =>
                string.Equals(probe.EncoderKey, "nvenc", StringComparison.OrdinalIgnoreCase) &&
                probe.Detail.Contains("nvenc_stream_sanity_failed", StringComparison.OrdinalIgnoreCase)))
        {
            warnings.Add("nvenc_stream_sanity_failed");
        }

        if (selectionReason.StartsWith("ffmpeg_probe:software:", StringComparison.OrdinalIgnoreCase))
        {
            warnings.Add("hardware_encoder_probe_failed");
        }

        if (forceNvenc)
        {
            warnings.Add("software_encoder_disabled");
        }

        return warnings
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .ToList();
    }

    private static (string Capture, string Reason) SelectCaptureBackend(
        IReadOnlyCollection<GpuControllerInfo> gpuControllers,
        SunshineRuntimeCandidate? selectedRuntime)
    {
        if (File.Exists(Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "force-wgc.txt")))
        {
            return ("wgc", "forced_by_user");
        }

        if (gpuControllers.Any(gpu => gpu.Name.Contains("virtual display driver", StringComparison.OrdinalIgnoreCase)))
        {
            return ("ddx", "virtual_display_driver_present");
        }

        if (HasRemoteDisplayAdapter(gpuControllers))
        {
            return ("wgc", "rdp_remote_display_active");
        }

        if (selectedRuntime?.Legacy == true)
        {
            return ("ddx", "legacy_runtime_safe_default");
        }

        var hasIntel = gpuControllers.Any(gpu => gpu.Name.Contains("intel", StringComparison.OrdinalIgnoreCase));
        var hasNvidia = gpuControllers.Any(gpu => gpu.Name.Contains("nvidia", StringComparison.OrdinalIgnoreCase));
        var hasAmd = gpuControllers.Any(gpu =>
            gpu.Name.Contains("amd", StringComparison.OrdinalIgnoreCase) ||
            gpu.Name.Contains("radeon", StringComparison.OrdinalIgnoreCase));

        if (hasIntel && !hasNvidia && !hasAmd)
        {
            return ("wgc", "intel_only_default");
        }

        return ("ddx", "safe_default");
    }

    private static bool HasRemoteDisplayAdapter(IReadOnlyCollection<GpuControllerInfo> gpuControllers) =>
        gpuControllers.Any(gpu =>
            gpu.Name.Contains("remote display adapter", StringComparison.OrdinalIgnoreCase) ||
            gpu.Name.Contains("rdp", StringComparison.OrdinalIgnoreCase));

    private static SunshineRuntimeCandidate? SelectRuntimeAndEncoder(
        IReadOnlyCollection<GpuControllerInfo> gpuControllers,
        IReadOnlyCollection<SunshineRuntimeCandidate> runtimes,
        IReadOnlyCollection<EncoderProbeResult> probes,
        bool forceNvenc,
        out string selectedEncoder,
        out string reason
    )
    {
        var runtimeByKey = runtimes.ToDictionary(item => item.Key, StringComparer.OrdinalIgnoreCase);
        var defaultRuntime = runtimes.FirstOrDefault(item => !item.Legacy)
            ?? runtimes.FirstOrDefault();

        selectedEncoder = "auto";
        reason = "probe_failed:auto";

        (bool Found, SunshineRuntimeCandidate? Runtime, string Encoder, string Reason) Pick(
            string encoderKey,
            Func<SunshineRuntimeCandidate, bool> runtimePredicate,
            string reasonPrefix,
            bool allowDisabledAutoSelect = false)
        {
            var match = probes.FirstOrDefault(probe =>
                probe.Ok
                && probe.EncoderKey.Equals(encoderKey, StringComparison.OrdinalIgnoreCase)
                && runtimeByKey.TryGetValue(probe.RuntimeKey, out var runtime)
                && (runtime.AutoSelect || allowDisabledAutoSelect)
                && runtimePredicate(runtime));
            if (match is null || !runtimeByKey.TryGetValue(match.RuntimeKey, out var selectedRuntime))
            {
                return (false, null, string.Empty, string.Empty);
            }

            return (true, selectedRuntime, match.EncoderKey, $"{reasonPrefix}:{selectedRuntime.Key}");
        }

        var hasNvidia = gpuControllers.Any(controller => ContainsAny(controller.Name, "nvidia", "geforce", "quadro", "tesla", "grid", "rtx"));
        var hasIntel = gpuControllers.Any(controller => ContainsAny(controller.Name, "intel", "arc", "iris", "uhd"));
        var hasAmd = gpuControllers.Any(controller => ContainsAny(controller.Name, "amd", "radeon", "firepro"));

        if (hasNvidia)
        {
            var modernNvenc = Pick("nvenc", runtime => !runtime.Legacy, "ffmpeg_probe:nvenc");
            if (modernNvenc.Found)
            {
                selectedEncoder = modernNvenc.Encoder;
                reason = modernNvenc.Reason;
                return modernNvenc.Runtime;
            }

            var legacyNvenc = Pick("nvenc", runtime => runtime.Legacy, "ffmpeg_probe:nvenc_legacy");
            if (legacyNvenc.Found)
            {
                selectedEncoder = legacyNvenc.Encoder;
                reason = legacyNvenc.Reason;
                return legacyNvenc.Runtime;
            }

            var modernSoftware = Pick("software", runtime => !runtime.Legacy, "ffmpeg_probe:software");
            if (modernSoftware.Found)
            {
                selectedEncoder = modernSoftware.Encoder;
                reason = modernSoftware.Reason;
                return modernSoftware.Runtime;
            }

            var legacyNvencRecovery = Pick(
                "nvenc",
                runtime => runtime.Legacy,
                "ffmpeg_probe:nvenc_legacy_recovery",
                allowDisabledAutoSelect: true);
            if (legacyNvencRecovery.Found)
            {
                selectedEncoder = legacyNvencRecovery.Encoder;
                reason = legacyNvencRecovery.Reason;
                return legacyNvencRecovery.Runtime;
            }

            if (forceNvenc)
            {
                var forcedRuntime = runtimes
                    .Where(runtime => runtime.AutoSelect || !runtime.Legacy)
                    .OrderBy(runtime => runtime.Legacy ? 1 : 0)
                    .ThenBy(runtime => runtime.Priority)
                    .FirstOrDefault();
                if (forcedRuntime is not null)
                {
                    selectedEncoder = "nvenc";
                    reason = forcedRuntime.Legacy
                        ? "force_nvenc:nvidia_present:legacy_runtime"
                        : "force_nvenc:nvidia_present";
                    return forcedRuntime;
                }
            }
        }

        if (forceNvenc)
        {
            var forcedRuntime = runtimes
                .OrderBy(runtime => runtime.Legacy ? 1 : 0)
                .ThenBy(runtime => runtime.Priority)
                .FirstOrDefault();
            if (forcedRuntime is not null)
            {
                selectedEncoder = "nvenc";
                reason = "force_nvenc:nvenc_required";
                return forcedRuntime;
            }
        }

        if (hasIntel)
        {
            var quicksync = Pick("quicksync", runtime => !runtime.Legacy, "ffmpeg_probe:quicksync");
            if (quicksync.Found)
            {
                selectedEncoder = quicksync.Encoder;
                reason = quicksync.Reason;
                return quicksync.Runtime;
            }
        }

        if (hasAmd)
        {
            var amdvce = Pick("amdvce", runtime => !runtime.Legacy, "ffmpeg_probe:amdvce");
            if (amdvce.Found)
            {
                selectedEncoder = amdvce.Encoder;
                reason = amdvce.Reason;
                return amdvce.Runtime;
            }
        }

        var software = Pick("software", runtime => !runtime.Legacy, "ffmpeg_probe:software");
        if (software.Found)
        {
            selectedEncoder = software.Encoder;
            reason = software.Reason;
            return software.Runtime;
        }

        var anyHealthy = probes.FirstOrDefault(probe =>
            probe.Ok
            && runtimeByKey.TryGetValue(probe.RuntimeKey, out var runtime)
            && runtime.AutoSelect);
        if (anyHealthy is not null && runtimeByKey.TryGetValue(anyHealthy.RuntimeKey, out var fallbackRuntime))
        {
            selectedEncoder = anyHealthy.EncoderKey;
            reason = $"ffmpeg_probe:fallback:{fallbackRuntime.Key}";
            return fallbackRuntime;
        }

        if (runtimes.Count == 0)
        {
            reason = "sunshine_runtime_not_found";
            return null;
        }

        if (hasNvidia)
        {
            reason = "probe_failed:auto_nvidia";
            return defaultRuntime;
        }

        if (hasIntel)
        {
            reason = "probe_failed:auto_intel";
            return defaultRuntime;
        }

        if (hasAmd)
        {
            reason = "probe_failed:auto_amd";
            return defaultRuntime;
        }

        reason = "probe_failed:auto";
        return defaultRuntime;
    }

    private static bool ContainsAny(string value, params string[] needles) =>
        needles.Any(needle => value.Contains(needle, StringComparison.OrdinalIgnoreCase));

    private static void ApplyRecommendedSunshineConfig(string configPath, HostCapabilityProfile profile)
    {
        var lines = File.Exists(configPath)
            ? File.ReadAllLines(configPath).ToList()
            : [];

        var isLegacyRuntime =
            profile.SelectedRuntimeKey.Equals("legacy", StringComparison.OrdinalIgnoreCase) ||
            profile.SelectedRuntimeDirectory.Contains("legacy", StringComparison.OrdinalIgnoreCase);
        var sharedIdentity = EnsureSharedSunshineIdentity(lines, configPath, profile);

        SetConfigValue(lines, "credentials_file", sharedIdentity.StatePathForConfig);
        SetConfigValue(lines, "file_state", sharedIdentity.StatePathForConfig);
        SetConfigValue(lines, "pkey", sharedIdentity.KeyPathForConfig);
        SetConfigValue(lines, "cert", sharedIdentity.CertPathForConfig);

        SetConfigValue(lines, "capture", profile.SelectedCapture);
        if (!string.IsNullOrWhiteSpace(profile.SelectedAudioSinkName))
        {
            SetConfigValue(lines, "audio_sink", profile.SelectedAudioSinkName);
        }
        else
        {
            RemoveConfigKey(lines, "audio_sink");
        }

        if (!string.IsNullOrWhiteSpace(profile.SelectedVirtualSinkName))
        {
            SetConfigValue(lines, "virtual_sink", profile.SelectedVirtualSinkName);
        }
        else
        {
            RemoveConfigKey(lines, "virtual_sink");
        }

        if (profile.SelectedEncoder.Equals("auto", StringComparison.OrdinalIgnoreCase))
        {
            RemoveConfigKey(lines, "encoder");
        }
        else
        {
            SetConfigValue(lines, "encoder", profile.SelectedEncoder);
        }

        if (profile.SelectedEncoder.Equals("software", StringComparison.OrdinalIgnoreCase))
        {
            SetConfigValue(lines, "sw_preset", "ultrafast");
            SetConfigValue(lines, "sw_tune", "zerolatency");
            SetConfigValue(lines, "min_threads", profile.SoftwareMinThreads.ToString());
            RemoveConfigKey(lines, "nvenc_preset");
            RemoveConfigKey(lines, "nvenc_twopass");
            RemoveConfigKey(lines, "nvenc_spatial_aq");
            RemoveConfigKey(lines, "nvenc_vbv_increase");
            RemoveConfigKey(lines, "nvenc_realtime_hags");
            RemoveConfigKey(lines, "nvenc_latency_over_power");
        }
        else if (profile.SelectedEncoder.Equals("nvenc", StringComparison.OrdinalIgnoreCase))
        {
            SetConfigValue(lines, "nvenc_preset", "1");
            SetConfigValue(lines, "nvenc_twopass", "disabled");
            SetConfigValue(lines, "nvenc_spatial_aq", "enabled");
            SetConfigValue(lines, "nvenc_vbv_increase", "0");
            SetConfigValue(lines, "nvenc_realtime_hags", "disabled");
            SetConfigValue(lines, "nvenc_latency_over_power", "enabled");
            RemoveConfigKey(lines, "sw_preset");
            RemoveConfigKey(lines, "sw_tune");
            RemoveConfigKey(lines, "min_threads");
        }
        else
        {
            RemoveConfigKey(lines, "sw_preset");
            RemoveConfigKey(lines, "sw_tune");
            RemoveConfigKey(lines, "min_threads");
            RemoveConfigKey(lines, "nvenc_preset");
            RemoveConfigKey(lines, "nvenc_twopass");
            RemoveConfigKey(lines, "nvenc_spatial_aq");
            RemoveConfigKey(lines, "nvenc_vbv_increase");
            RemoveConfigKey(lines, "nvenc_realtime_hags");
            RemoveConfigKey(lines, "nvenc_latency_over_power");
        }

        if (isLegacyRuntime)
        {
            // Legacy Sunshine expects the older nv_* keys. Keep this path strictly H.264
            // and avoid mixing newer nvenc_* keys that may not map cleanly on older builds.
            SetConfigValue(lines, "hevc_mode", "0");
            RemoveConfigKey(lines, "nvenc_preset");
            RemoveConfigKey(lines, "nvenc_twopass");
            RemoveConfigKey(lines, "nvenc_spatial_aq");
            RemoveConfigKey(lines, "nvenc_vbv_increase");
            RemoveConfigKey(lines, "nvenc_h264_cavlc");
            RemoveConfigKey(lines, "nvenc_realtime_hags");
            RemoveConfigKey(lines, "nvenc_latency_over_power");
            if (profile.SelectedEncoder.Equals("nvenc", StringComparison.OrdinalIgnoreCase))
            {
                // Keep the legacy runtime on the safest NVENC profile we can
                // use broadly across older NVIDIA/vGPU stacks.
                SetConfigValue(lines, "nv_preset", "p4");
                SetConfigValue(lines, "nv_tune", "ull");
                SetConfigValue(lines, "nv_rc", "cbr");
                SetConfigValue(lines, "nv_coder", "auto");
            }
        }

        File.WriteAllText(configPath, string.Join(Environment.NewLine, lines) + Environment.NewLine);
        SyncSharedIdentityToRuntimeConfigs(profile, sharedIdentity, configPath);

        if (isLegacyRuntime)
        {
            EnsureLegacyStateCompatibility(sharedIdentity.StatePath);
        }
    }

    private static SunshineCaptureConfigApplyResult TryApplyPreferredSunshineCaptureConfig(string configPath, string bundleRoot)
    {
        try
        {
            var displayPreference = ReadStreamDisplayPreference(bundleRoot);
            var displays = EnumerateDisplays();
            var (_, preferredDisplay) = ResolveStreamDisplayForPrepare(
                displays,
                requestedMode: null,
                displayPreference);
            return TryApplySunshineStreamCaptureConfig(configPath, ResolveSunshineCaptureDisplay(preferredDisplay));
        }
        catch
        {
            return new SunshineCaptureConfigApplyResult(false, false, configPath, null);
        }
    }

    private static SunshineCaptureConfigApplyResult TryApplySunshineStreamCaptureConfigForBundle(string bundleRoot, DisplaySnapshot streamDisplay)
    {
        try
        {
            var captureDisplay = ResolveSunshineCaptureDisplay(streamDisplay);
            return TryApplySunshineStreamCaptureConfig(ResolveActiveSunshineConfigPath(bundleRoot), captureDisplay);
        }
        catch
        {
            return new SunshineCaptureConfigApplyResult(false, false, null, streamDisplay.DeviceName);
        }
    }

    private static DisplaySnapshot ResolveSunshineCaptureDisplay(DisplaySnapshot streamDisplay)
    {
        if (!DisplayLooksLikeMttVdd(streamDisplay))
        {
            return streamDisplay;
        }

        try
        {
            var displays = EnumerateDisplays();
            var visibleCaptureDisplay = FindPreferredNonMttCaptureDisplay(displays, streamDisplay);
            if (visibleCaptureDisplay is not null)
            {
                return visibleCaptureDisplay;
            }

            return FindDuplicateCompanionDisplay(displays, streamDisplay)
                ?? FindPrimaryDisplay(displays)
                ?? streamDisplay;
        }
        catch
        {
            return streamDisplay;
        }
    }

    private static SunshineCaptureConfigApplyResult TryApplySunshineStreamCaptureConfig(string configPath, DisplaySnapshot streamDisplay)
    {
        if (string.IsNullOrWhiteSpace(configPath) || string.IsNullOrWhiteSpace(streamDisplay.DeviceName))
        {
            return new SunshineCaptureConfigApplyResult(false, false, configPath, streamDisplay.DeviceName);
        }

        try
        {
            EnsureConfigDirectory(configPath);
            var originalText = File.Exists(configPath) ? File.ReadAllText(configPath) : string.Empty;
            var originalOutputName = File.Exists(configPath)
                ? TryReadConfigValue(configPath, "output_name")
                : null;
            var lines = string.IsNullOrEmpty(originalText)
                ? []
                : originalText.Split(["\r\n", "\n"], StringSplitOptions.None).ToList();

            while (lines.Count > 0 && string.IsNullOrWhiteSpace(lines[^1]))
            {
                lines.RemoveAt(lines.Count - 1);
            }

            var outputName = ShouldUseSunshinePrimaryCapture(streamDisplay)
                ? null
                : ResolveSunshineCaptureOutputName(configPath, streamDisplay) ?? streamDisplay.DeviceName;
            if (string.IsNullOrWhiteSpace(outputName))
            {
                RemoveConfigKey(lines, "output_name");
            }
            else
            {
                SetConfigValue(lines, "output_name", outputName);
            }
            // Keep Sunshine on automatic GPU selection. A stale adapter_name can
            // block capture when the selected display changes between QEMU, MTT, and Parsec.
            RemoveConfigKey(lines, "adapter_name");

            var captureTargetChanged = !string.Equals(
                NormalizeConfigValue(originalOutputName),
                NormalizeConfigValue(outputName),
                StringComparison.OrdinalIgnoreCase);

            var renderedText = string.Join(Environment.NewLine, lines) + Environment.NewLine;
            if (string.Equals(originalText, renderedText, StringComparison.Ordinal))
            {
                return new SunshineCaptureConfigApplyResult(false, false, configPath, outputName ?? "primary-display");
            }

            File.WriteAllText(configPath, renderedText);
            return new SunshineCaptureConfigApplyResult(true, captureTargetChanged, configPath, outputName ?? "primary-display");
        }
        catch
        {
            return new SunshineCaptureConfigApplyResult(false, false, configPath, streamDisplay.DeviceName);
        }
    }

    private static bool ShouldUseSunshinePrimaryCapture(DisplaySnapshot streamDisplay) =>
        DisplayLooksLikeMttVdd(streamDisplay) && StreamDisplayHasPrimaryAuthority(streamDisplay);

    private static string? NormalizeConfigValue(string? value)
    {
        var trimmed = value?.Trim();
        return string.IsNullOrWhiteSpace(trimmed) ? null : trimmed;
    }

    private static string? ResolveSunshineCaptureOutputName(string configPath, DisplaySnapshot streamDisplay)
    {
        if (DisplayLooksLikeMttVdd(streamDisplay) || IsParsecDisplay(streamDisplay))
        {
            // Sunshine device_id can rotate when virtual displays are
            // re-enumerated. Pin capture to the current Windows display name
            // so the refreshed Sunshine process does not keep targeting a
            // stale/removed output.
            return streamDisplay.DeviceName;
        }

        var logPath = ResolveSunshineLogPath(configPath);
        if (string.IsNullOrWhiteSpace(logPath) || !File.Exists(logPath))
        {
            return null;
        }

        var text = string.Empty;
        var markerIndex = -1;
        var streamDisplayLooksLikeVdd = DisplayLooksLikeMttVdd(streamDisplay);
        try
        {
            text = ReadTextFileShared(logPath);
            var marker = "Currently available display devices:";
            markerIndex = text.LastIndexOf(marker, StringComparison.OrdinalIgnoreCase);
            if (markerIndex < 0)
            {
                return ResolveKnownSunshineOutputName(streamDisplay);
            }

            if (streamDisplayLooksLikeVdd)
            {
                var forcedVddOutputName = TryResolveAnySunshineVddOutputNameFromLogText(text, markerIndex);
                if (!string.IsNullOrWhiteSpace(forcedVddOutputName))
                {
                    return forcedVddOutputName;
                }
            }

            var arrayStart = text.IndexOf('[', markerIndex + marker.Length);
            if (arrayStart < 0 || !TryExtractJsonArray(text, arrayStart, out var json))
            {
                return TryResolveSunshineOutputNameFromDisplayLogText(text, markerIndex, streamDisplay)
                    ?? ResolveKnownSunshineOutputName(streamDisplay);
            }

            using var document = JsonDocument.Parse(json);
            if (document.RootElement.ValueKind is not JsonValueKind.Array)
            {
                return TryResolveSunshineOutputNameFromDisplayLogText(text, markerIndex, streamDisplay)
                    ?? ResolveKnownSunshineOutputName(streamDisplay);
            }

            foreach (var display in document.RootElement.EnumerateArray())
            {
                var displayName = display.TryGetProperty("display_name", out var displayNameElement)
                    ? displayNameElement.GetString()
                    : null;
                var friendlyName = display.TryGetProperty("friendly_name", out var friendlyNameElement)
                    ? friendlyNameElement.GetString()
                    : null;
                var edidLooksLikeVdd =
                    display.TryGetProperty("edid", out var edidElement)
                    && edidElement.ValueKind is JsonValueKind.Object
                    && (
                        string.Equals(
                            edidElement.TryGetProperty("manufacturer_id", out var manufacturerIdElement)
                                ? manufacturerIdElement.GetString()
                                : null,
                            "MTT",
                            StringComparison.OrdinalIgnoreCase)
                        || string.Equals(
                            edidElement.TryGetProperty("product_code", out var productCodeElement)
                                ? productCodeElement.GetString()
                                : null,
                            "1337",
                            StringComparison.OrdinalIgnoreCase));
                var sunshineDisplayLooksLikeVdd =
                    TextLooksLikeCloudgimeVirtualDisplay(friendlyName)
                    || edidLooksLikeVdd;
                var isTargetDisplay = string.Equals(displayName, streamDisplay.DeviceName, StringComparison.OrdinalIgnoreCase)
                    || streamDisplayLooksLikeVdd && sunshineDisplayLooksLikeVdd;
                if (!isTargetDisplay)
                {
                    continue;
                }

                var deviceId = display.TryGetProperty("device_id", out var deviceIdElement)
                    ? deviceIdElement.GetString()
                    : null;
                return string.IsNullOrWhiteSpace(deviceId) ? displayName : deviceId;
            }
        }
        catch
        {
            return TryResolveSunshineOutputNameFromDisplayLogText(text, markerIndex, streamDisplay)
                ?? ResolveKnownSunshineOutputName(streamDisplay);
        }

        return TryResolveSunshineOutputNameFromDisplayLogText(text, markerIndex, streamDisplay)
            ?? ResolveKnownSunshineOutputName(streamDisplay);
    }

    private static string? ResolveKnownSunshineOutputName(DisplaySnapshot streamDisplay)
    {
        // Do not fall back to a baked device_id here. Sunshine can rotate the
        // VDD device GUID after reinstall/re-enumeration, and a stale value can
        // pin capture to Microsoft Basic Render Driver. If live log parsing does
        // not yield a device_id, let the caller fall back to the current
        // display_name instead.
        _ = streamDisplay;
        return null;
    }

    private static bool DisplayLooksLikeMttVdd(DisplaySnapshot display)
    {
        var text = $"{display.DeviceString} {display.DeviceId}";
        if (TextLooksLikeNonMttVirtualDisplay(text))
        {
            return false;
        }

        if (TextLooksLikeMttVdd(text))
        {
            return true;
        }

        return display.IsVdd
            && text.Contains("Virtual Display Driver", StringComparison.OrdinalIgnoreCase)
            && !TextLooksLikeNonMttVirtualDisplay(text);
    }

    private static bool TextLooksLikeMttVdd(string? text)
    {
        if (string.IsNullOrWhiteSpace(text))
        {
            return false;
        }

        return text.Contains("VDD by MTT", StringComparison.OrdinalIgnoreCase)
            || text.Contains("Virtual Display Driver by MTT", StringComparison.OrdinalIgnoreCase)
            || text.Contains("MttVDD", StringComparison.OrdinalIgnoreCase)
            || text.Contains("MTT1337", StringComparison.OrdinalIgnoreCase)
            || text.Contains("MikeTheTech", StringComparison.OrdinalIgnoreCase)
            || text.Contains("Cloudgime VDD", StringComparison.OrdinalIgnoreCase);
    }

    private static bool TextLooksLikeNonMttVirtualDisplay(string? text)
    {
        if (string.IsNullOrWhiteSpace(text))
        {
            return false;
        }

        return text.Contains("Parsec", StringComparison.OrdinalIgnoreCase)
            || text.Contains("PSCCDD", StringComparison.OrdinalIgnoreCase)
            || text.Contains("Qdesk", StringComparison.OrdinalIgnoreCase)
            || text.Contains("Easy Virtual Display", StringComparison.OrdinalIgnoreCase)
            || text.Contains("KtzeAbyss", StringComparison.OrdinalIgnoreCase)
            || text.Contains("spacedesk", StringComparison.OrdinalIgnoreCase)
            || text.Contains("RustDesk", StringComparison.OrdinalIgnoreCase)
            || text.Contains("Splashtop", StringComparison.OrdinalIgnoreCase)
            || text.Contains("Duet", StringComparison.OrdinalIgnoreCase)
            || text.Contains("QEMU", StringComparison.OrdinalIgnoreCase)
            || text.Contains("VirtIO", StringComparison.OrdinalIgnoreCase)
            || text.Contains("Red Hat", StringComparison.OrdinalIgnoreCase);
    }

    private static bool TextLooksLikeCloudgimeVirtualDisplay(string? text)
    {
        if (string.IsNullOrWhiteSpace(text))
        {
            return false;
        }

        return TextLooksLikeMttVdd(text)
            || text.Contains("Virtual Display Driver", StringComparison.OrdinalIgnoreCase)
            || text.Contains("Qdesk Virtual Display", StringComparison.OrdinalIgnoreCase)
            || text.Contains("Qdesk", StringComparison.OrdinalIgnoreCase)
            || text.Contains("Easy Virtual Display", StringComparison.OrdinalIgnoreCase)
            || text.Contains("KtzeAbyss", StringComparison.OrdinalIgnoreCase);
    }

    private static string? TryResolveAnySunshineVddOutputNameFromLogText(string text, int markerIndex)
    {
        try
        {
            var anchorIndex = new[]
                {
                    "\"friendly_name\": \"VDD by MTT\"",
                    "\"friendly_name\": \"Qdesk Virtual Display Adapter\"",
                    "Qdesk Virtual Display",
                    "Easy Virtual Display",
                    "Cloudgime VDD",
                    "\"manufacturer_id\": \"MTT\"",
                }
                .Select(anchor => text.LastIndexOf(anchor, StringComparison.OrdinalIgnoreCase))
                .Where(index => index >= markerIndex)
                .DefaultIfEmpty(-1)
                .Max();
            if (anchorIndex < markerIndex)
            {
                return null;
            }

            var deviceIdKeyIndex = text.LastIndexOf("\"device_id\"", anchorIndex, StringComparison.OrdinalIgnoreCase);
            if (deviceIdKeyIndex < markerIndex)
            {
                return null;
            }

            var colonIndex = text.IndexOf(':', deviceIdKeyIndex);
            if (colonIndex < 0 || colonIndex > anchorIndex)
            {
                return null;
            }

            var firstQuote = text.IndexOf('"', colonIndex + 1);
            var secondQuote = firstQuote >= 0 ? text.IndexOf('"', firstQuote + 1) : -1;
            if (firstQuote < 0 || secondQuote <= firstQuote)
            {
                return null;
            }

            var value = text[(firstQuote + 1)..secondQuote].Trim();
            return string.IsNullOrWhiteSpace(value) ? null : value;
        }
        catch
        {
            return null;
        }
    }

    private static string? TryResolveSunshineOutputNameFromDisplayLogText(
        string text,
        int markerIndex,
        DisplaySnapshot streamDisplay)
    {
        try
        {
            var streamDisplayLooksLikeVdd = DisplayLooksLikeMttVdd(streamDisplay);
            var displayNameNeedle = $"\"display_name\": \"{streamDisplay.DeviceName.Replace("\\", "\\\\")}\"";
            var displayNameIndex = text.IndexOf(displayNameNeedle, markerIndex, StringComparison.OrdinalIgnoreCase);
            if (displayNameIndex < 0 && streamDisplayLooksLikeVdd)
            {
                displayNameIndex = text.IndexOf("\"friendly_name\": \"VDD by MTT\"", markerIndex, StringComparison.OrdinalIgnoreCase);
            }
            if (displayNameIndex < 0 && streamDisplayLooksLikeVdd)
            {
                displayNameIndex = text.IndexOf("\"manufacturer_id\": \"MTT\"", markerIndex, StringComparison.OrdinalIgnoreCase);
            }

            if (displayNameIndex < 0)
            {
                return null;
            }

            var deviceIdKeyIndex = text.LastIndexOf("\"device_id\"", displayNameIndex, StringComparison.OrdinalIgnoreCase);
            if (deviceIdKeyIndex < markerIndex)
            {
                return null;
            }

            var colonIndex = text.IndexOf(':', deviceIdKeyIndex);
            if (colonIndex < 0 || colonIndex > displayNameIndex)
            {
                return null;
            }

            var firstQuote = text.IndexOf('"', colonIndex + 1);
            var secondQuote = firstQuote >= 0 ? text.IndexOf('"', firstQuote + 1) : -1;
            if (firstQuote < 0 || secondQuote <= firstQuote)
            {
                return null;
            }

            var value = text[(firstQuote + 1)..secondQuote].Trim();
            return string.IsNullOrWhiteSpace(value) ? null : value;
        }
        catch
        {
            return null;
        }
    }

    private static string? ResolveSunshineLogPath(string configPath)
    {
        var configDirectory = Path.GetDirectoryName(configPath);
        if (string.IsNullOrWhiteSpace(configDirectory))
        {
            return null;
        }

        try
        {
            if (File.Exists(configPath))
            {
                foreach (var line in File.ReadLines(configPath))
                {
                    var trimmed = line.Trim();
                    if (!trimmed.StartsWith("log_path =", StringComparison.OrdinalIgnoreCase))
                    {
                        continue;
                    }

                    var configuredLogPath = trimmed["log_path =".Length..].Trim();
                    if (!string.IsNullOrWhiteSpace(configuredLogPath))
                    {
                        return Path.IsPathRooted(configuredLogPath)
                            ? configuredLogPath
                            : Path.GetFullPath(Path.Combine(configDirectory, configuredLogPath));
                    }
                }
            }
        }
        catch
        {
            // Fall back to Sunshine's default log name in the config directory.
        }

        return Path.Combine(configDirectory, "sunshine.log");
    }

    private static string ReadTextFileShared(string path)
    {
        using var stream = new FileStream(
            path,
            FileMode.Open,
            FileAccess.Read,
            FileShare.ReadWrite | FileShare.Delete);
        using var reader = new StreamReader(stream);
        return reader.ReadToEnd();
    }

    private static bool TryExtractJsonArray(string text, int arrayStart, out string json)
    {
        json = string.Empty;
        var depth = 0;
        var inString = false;
        var escaping = false;

        for (var index = arrayStart; index < text.Length; index++)
        {
            var ch = text[index];
            if (inString)
            {
                if (escaping)
                {
                    escaping = false;
                }
                else if (ch == '\\')
                {
                    escaping = true;
                }
                else if (ch == '"')
                {
                    inString = false;
                }
                continue;
            }

            if (ch == '"')
            {
                inString = true;
                continue;
            }

            if (ch == '[')
            {
                depth++;
            }
            else if (ch == ']')
            {
                depth--;
                if (depth == 0)
                {
                    json = text[arrayStart..(index + 1)];
                    return true;
                }
            }
        }

        return false;
    }

    private static string ResolveActiveSunshineConfigPath(string explicitBundleRoot)
    {
        var bundleRoot = ResolveBundleRoot(explicitBundleRoot);
        var profilePath = Path.Combine(bundleRoot, "moonlight", "server", "host_capability_profile.json");
        try
        {
            if (File.Exists(profilePath))
            {
                var profile = JsonSerializer.Deserialize<HostCapabilityProfile>(File.ReadAllText(profilePath), JsonOptions);
                if (!string.IsNullOrWhiteSpace(profile?.ConfigPath))
                {
                    return Path.GetFullPath(profile.ConfigPath);
                }
            }
        }
        catch
        {
            // Fall back to the selected runtime file below.
        }

        var selectedRuntimeDirectory = "sunshine";
        var runtimeSelectionPath = Path.Combine(bundleRoot, "moonlight", "server", "selected_sunshine_runtime.txt");
        try
        {
            if (File.Exists(runtimeSelectionPath))
            {
                var selected = File.ReadLines(runtimeSelectionPath)
                    .Select(line => line.Trim())
                    .FirstOrDefault(line => !string.IsNullOrWhiteSpace(line));
                if (!string.IsNullOrWhiteSpace(selected))
                {
                    selectedRuntimeDirectory = selected;
                }
            }
        }
        catch
        {
            // Keep default runtime.
        }

        return Path.Combine(bundleRoot, selectedRuntimeDirectory, "config", "sunshine.conf");
    }

    private static void EnsureConfigDirectory(string configPath)
    {
        var directory = Path.GetDirectoryName(configPath);
        if (!string.IsNullOrWhiteSpace(directory))
        {
            Directory.CreateDirectory(directory);
        }
    }

    private static void SetConfigValue(List<string> lines, string key, string value)
    {
        var index = lines.FindIndex(line => line.TrimStart().StartsWith($"{key} =", StringComparison.OrdinalIgnoreCase));
        var rendered = $"{key} = {value}";
        if (index >= 0)
        {
            lines[index] = rendered;
            return;
        }

        lines.Add(rendered);
    }

    private static void RemoveConfigKey(List<string> lines, string key)
    {
        lines.RemoveAll(line => line.TrimStart().StartsWith($"{key} =", StringComparison.OrdinalIgnoreCase));
    }

    private static string? TryReadConfigValue(string configPath, string key)
    {
        if (string.IsNullOrWhiteSpace(configPath) || string.IsNullOrWhiteSpace(key) || !File.Exists(configPath))
        {
            return null;
        }

        try
        {
            foreach (var line in File.ReadLines(configPath))
            {
                var trimmed = line.Trim();
                if (trimmed.StartsWith("#", StringComparison.Ordinal) || trimmed.StartsWith(";", StringComparison.Ordinal))
                {
                    continue;
                }

                if (!trimmed.StartsWith($"{key} =", StringComparison.OrdinalIgnoreCase))
                {
                    continue;
                }

                var separatorIndex = trimmed.IndexOf('=');
                if (separatorIndex < 0 || separatorIndex >= trimmed.Length - 1)
                {
                    continue;
                }

                var value = trimmed[(separatorIndex + 1)..].Trim();
                return string.IsNullOrWhiteSpace(value) ? null : value;
            }
        }
        catch
        {
            // Best effort only.
        }

        return null;
    }

    private static string? TryReadConfiguredSunshineOutputNameFromHelperBundle()
    {
        try
        {
            var bundleRoot = ResolveBundleRootFromHelper();
            if (string.IsNullOrWhiteSpace(bundleRoot))
            {
                return null;
            }

            var configPath = ResolveActiveSunshineConfigPath(bundleRoot);
            return TryReadConfigValue(configPath, "output_name");
        }
        catch
        {
            return null;
        }
    }

    private static SharedSunshineIdentityPaths EnsureSharedSunshineIdentity(
        List<string> lines,
        string configPath,
        HostCapabilityProfile profile)
    {
        var configDirectory = Path.GetDirectoryName(configPath) ?? Path.Combine(profile.BundleRoot, profile.SelectedRuntimeDirectory, "config");
        var sharedRoot = Path.Combine(profile.BundleRoot, "moonlight", "server", "sunshine-shared");
        var sharedCredentialsDirectory = Path.Combine(sharedRoot, "credentials");
        Directory.CreateDirectory(sharedCredentialsDirectory);

        var sharedStatePath = Path.Combine(sharedRoot, "sunshine_state.json");
        var sharedKeyPath = Path.Combine(sharedCredentialsDirectory, "cakey.pem");
        var sharedCertPath = Path.Combine(sharedCredentialsDirectory, "cacert.pem");

        var candidates = EnumerateSunshineIdentitySources(lines, configPath, profile);
        foreach (var candidate in candidates)
        {
            PromoteSharedIdentityFile(candidate.StatePath, sharedStatePath);
            PromoteSharedIdentityFile(candidate.KeyPath, sharedKeyPath);
            PromoteSharedIdentityFile(candidate.CertPath, sharedCertPath);

            if (File.Exists(sharedStatePath) && File.Exists(sharedKeyPath) && File.Exists(sharedCertPath))
            {
                break;
            }
        }

        Directory.CreateDirectory(Path.GetDirectoryName(sharedStatePath) ?? sharedRoot);
        Directory.CreateDirectory(Path.GetDirectoryName(sharedKeyPath) ?? sharedCredentialsDirectory);
        Directory.CreateDirectory(Path.GetDirectoryName(sharedCertPath) ?? sharedCredentialsDirectory);

        return new SharedSunshineIdentityPaths
        {
            RootPath = sharedRoot,
            StatePath = sharedStatePath,
            KeyPath = sharedKeyPath,
            CertPath = sharedCertPath,
            StatePathForConfig = RenderRelativeConfigPath(configDirectory, sharedStatePath),
            KeyPathForConfig = RenderRelativeConfigPath(configDirectory, sharedKeyPath),
            CertPathForConfig = RenderRelativeConfigPath(configDirectory, sharedCertPath),
        };
    }

    private static IEnumerable<SunshineIdentitySource> EnumerateSunshineIdentitySources(
        List<string> activeConfigLines,
        string activeConfigPath,
        HostCapabilityProfile profile)
    {
        yield return new SunshineIdentitySource
        {
            Label = "active",
            StatePath = ResolveConfiguredStatePath(activeConfigPath, activeConfigLines),
            KeyPath = ResolveConfiguredPath(activeConfigPath, activeConfigLines, "pkey", Path.Combine("credentials", "cakey.pem")),
            CertPath = ResolveConfiguredPath(activeConfigPath, activeConfigLines, "cert", Path.Combine("credentials", "cacert.pem")),
        };

        foreach (var runtime in profile.RuntimeCandidates)
        {
            if (string.IsNullOrWhiteSpace(runtime.ConfigPath) || !File.Exists(runtime.ConfigPath))
            {
                continue;
            }

            var runtimeLines = File.ReadAllLines(runtime.ConfigPath).ToList();
            yield return new SunshineIdentitySource
            {
                Label = runtime.Key,
                StatePath = ResolveConfiguredStatePath(runtime.ConfigPath, runtimeLines),
                KeyPath = ResolveConfiguredPath(runtime.ConfigPath, runtimeLines, "pkey", Path.Combine("credentials", "cakey.pem")),
                CertPath = ResolveConfiguredPath(runtime.ConfigPath, runtimeLines, "cert", Path.Combine("credentials", "cacert.pem")),
            };
        }
    }

    private static void PromoteSharedIdentityFile(string? sourcePath, string targetPath)
    {
        if (string.IsNullOrWhiteSpace(sourcePath) ||
            !File.Exists(sourcePath) ||
            File.Exists(targetPath) ||
            string.Equals(Path.GetFullPath(sourcePath), Path.GetFullPath(targetPath), StringComparison.OrdinalIgnoreCase))
        {
            return;
        }

        var targetDirectory = Path.GetDirectoryName(targetPath);
        if (!string.IsNullOrWhiteSpace(targetDirectory))
        {
            Directory.CreateDirectory(targetDirectory);
        }

        File.Copy(sourcePath, targetPath, overwrite: false);
    }

    private static string? ResolveConfigValue(IEnumerable<string> lines, string key)
    {
        foreach (var line in lines)
        {
            var trimmed = line.Trim();
            if (trimmed.StartsWith("#", StringComparison.Ordinal) || !trimmed.Contains('='))
            {
                continue;
            }

            var separator = trimmed.IndexOf('=');
            if (separator <= 0)
            {
                continue;
            }

            var parsedKey = trimmed[..separator].Trim();
            if (!parsedKey.Equals(key, StringComparison.OrdinalIgnoreCase))
            {
                continue;
            }

            var value = trimmed[(separator + 1)..].Trim();
            if (!string.IsNullOrWhiteSpace(value))
            {
                return value;
            }
        }

        return null;
    }

    private static string ResolveConfiguredPath(
        string configPath,
        IEnumerable<string> lines,
        string key,
        string fallbackRelativePath)
    {
        var configDirectory = Path.GetDirectoryName(configPath);
        if (string.IsNullOrWhiteSpace(configDirectory))
        {
            return string.Empty;
        }

        var configured = ResolveConfigValue(lines, key) ?? fallbackRelativePath;
        return Path.IsPathRooted(configured)
            ? configured
            : Path.GetFullPath(Path.Combine(configDirectory, configured));
    }

    private static string ResolveConfiguredStatePath(string configPath, IEnumerable<string> lines)
    {
        var statePath = ResolveConfigValue(lines, "file_state");
        if (string.IsNullOrWhiteSpace(statePath))
        {
            statePath = ResolveConfigValue(lines, "credentials_file");
        }

        return ResolveConfiguredPath(
            configPath,
            lines,
            "file_state",
            statePath ?? "sunshine_state.json");
    }

    private static string RenderRelativeConfigPath(string configDirectory, string targetPath) =>
        Path.GetRelativePath(configDirectory, targetPath).Replace('\\', '/');

    private static void SyncSharedIdentityToRuntimeConfigs(
        HostCapabilityProfile profile,
        SharedSunshineIdentityPaths sharedIdentity,
        string activeConfigPath)
    {
        var configPaths = profile.RuntimeCandidates
            .Select(runtime => runtime.ConfigPath)
            .Append(activeConfigPath)
            .Where(path => !string.IsNullOrWhiteSpace(path) && File.Exists(path))
            .Distinct(StringComparer.OrdinalIgnoreCase);

        foreach (var configPath in configPaths)
        {
            var runtimeLines = File.ReadAllLines(configPath).ToList();
            var runtimeConfigDirectory = Path.GetDirectoryName(configPath);
            if (string.IsNullOrWhiteSpace(runtimeConfigDirectory))
            {
                continue;
            }

            SetConfigValue(runtimeLines, "credentials_file", RenderRelativeConfigPath(runtimeConfigDirectory, sharedIdentity.StatePath));
            SetConfigValue(runtimeLines, "file_state", RenderRelativeConfigPath(runtimeConfigDirectory, sharedIdentity.StatePath));
            SetConfigValue(runtimeLines, "pkey", RenderRelativeConfigPath(runtimeConfigDirectory, sharedIdentity.KeyPath));
            SetConfigValue(runtimeLines, "cert", RenderRelativeConfigPath(runtimeConfigDirectory, sharedIdentity.CertPath));
            File.WriteAllText(configPath, string.Join(Environment.NewLine, runtimeLines) + Environment.NewLine);
        }
    }

    private static void EnsureLegacyStateCompatibility(string statePath)
    {
        if (string.IsNullOrWhiteSpace(statePath) || !File.Exists(statePath))
        {
            return;
        }

        JsonNode? rootNode;
        try
        {
            rootNode = JsonNode.Parse(File.ReadAllText(statePath));
        }
        catch
        {
            return;
        }

        if (rootNode is not JsonObject documentRoot)
        {
            return;
        }

        if (documentRoot["root"] is not JsonObject runtimeRoot)
        {
            return;
        }

        if (runtimeRoot["devices"] is JsonArray)
        {
            return;
        }

        var devices = new JsonArray();
        if (runtimeRoot["named_devices"] is JsonArray namedDevices)
        {
            foreach (var entryNode in namedDevices)
            {
                if (entryNode is not JsonObject entry)
                {
                    continue;
                }

                var uniqueId = entry["uuid"]?.GetValue<string>();
                if (string.IsNullOrWhiteSpace(uniqueId))
                {
                    continue;
                }

                var device = new JsonObject
                {
                    ["uniqueid"] = uniqueId
                };

                var certs = new JsonArray();
                var cert = entry["cert"]?.GetValue<string>();
                if (!string.IsNullOrWhiteSpace(cert))
                {
                    certs.Add(cert);
                }

                device["certs"] = certs;
                devices.Add(device);
            }
        }

        runtimeRoot["devices"] = devices;
        File.WriteAllText(
            statePath,
            documentRoot.ToJsonString() + Environment.NewLine);
    }

    private static string ResolveConfiguredStatePath(string configPath)
    {
        if (!File.Exists(configPath))
        {
            return string.Empty;
        }

        return ResolveConfiguredStatePath(configPath, File.ReadAllLines(configPath));
    }

    private static string CompactProcessOutput(string value)
    {
        var text = string.Join(
            " | ",
            value.Split(['\r', '\n'], StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
                .Where(line => !string.IsNullOrWhiteSpace(line))
                .Take(6)
        );

        return text.Length <= 320 ? text : text[..320];
    }

    private static string SummarizeEncoderProbeFailure(string encoderKey, string rawOutput)
    {
        var text = rawOutput ?? string.Empty;
        if (string.IsNullOrWhiteSpace(text))
        {
            return "probe_failed";
        }

        if (encoderKey.Equals("nvenc", StringComparison.OrdinalIgnoreCase))
        {
            if (text.Contains("required nvenc api version", StringComparison.OrdinalIgnoreCase))
            {
                var required = ExtractAfter(text, "Required:");
                var found = ExtractAfter(text, "Found:");
                return $"nvenc_api_mismatch required={required} found={found}";
            }

            if (text.Contains("minimum required Nvidia driver", StringComparison.OrdinalIgnoreCase))
            {
                return CompactProcessOutput(ExtractMatchingLines(text, "minimum required Nvidia driver", "required nvenc api version"));
            }
        }

        if (encoderKey.Equals("quicksync", StringComparison.OrdinalIgnoreCase) &&
            text.Contains("unsupported (-3)", StringComparison.OrdinalIgnoreCase))
        {
            return "qsv_unsupported";
        }

        if (encoderKey.Equals("amdvce", StringComparison.OrdinalIgnoreCase) &&
            text.Contains("amf", StringComparison.OrdinalIgnoreCase))
        {
            return CompactProcessOutput(ExtractMatchingLines(text, "amf", "CreateComponent", "DLL"));
        }

        return CompactProcessOutput(text);
    }

    private static string ExtractMatchingLines(string text, params string[] patterns)
    {
        var lines = text.Split(['\r', '\n'], StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
        var matches = lines.Where(line => patterns.Any(pattern => line.Contains(pattern, StringComparison.OrdinalIgnoreCase))).ToArray();
        return matches.Length > 0 ? string.Join(" | ", matches) : text;
    }

    private static string ExtractAfter(string text, string marker)
    {
        var index = text.IndexOf(marker, StringComparison.OrdinalIgnoreCase);
        if (index < 0)
        {
            return "unknown";
        }

        var start = index + marker.Length;
        var tail = text[start..];
        var token = tail
            .Split([' ', '\r', '\n', '\t', ','], StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
            .FirstOrDefault();
        return string.IsNullOrWhiteSpace(token) ? "unknown" : token;
    }

    private static ProcessResult RunProcess(string fileName, string arguments, string? workingDirectory, int timeoutMs)
    {
        try
        {
            using var process = new Process
            {
                StartInfo = new ProcessStartInfo
                {
                    FileName = fileName,
                    Arguments = arguments,
                    WorkingDirectory = string.IsNullOrWhiteSpace(workingDirectory) ? Environment.CurrentDirectory : workingDirectory,
                    UseShellExecute = false,
                    RedirectStandardOutput = true,
                    RedirectStandardError = true,
                    CreateNoWindow = true,
                },
            };

            process.Start();
            var stdoutTask = process.StandardOutput.ReadToEndAsync();
            var stderrTask = process.StandardError.ReadToEndAsync();

            if (!process.WaitForExit(timeoutMs))
            {
                try
                {
                    process.Kill(entireProcessTree: true);
                }
                catch
                {
                    // ignored
                }

                Task.WaitAll(stdoutTask, stderrTask);
                return new ProcessResult(false, -1, stdoutTask.Result, $"timeout | {stderrTask.Result}");
            }

            Task.WaitAll(stdoutTask, stderrTask);
            return new ProcessResult(process.ExitCode == 0, process.ExitCode, stdoutTask.Result, stderrTask.Result);
        }
        catch (Exception ex)
        {
            return new ProcessResult(false, -1, string.Empty, ex.Message);
        }
    }

private sealed record ProcessResult(bool Ok, int ExitCode, string StdOut, string StdErr);

private sealed record DisplayClassDeviceEntry(
    string InstanceId,
    string DeviceDescription,
    string ManufacturerName,
    string Status
);

private sealed record DisplayModeApplyResult(bool Applied, bool Fallback, bool RequiresApply);

    private sealed record DisplayModeAttempt(int Result, bool RequiresApply);

    private static string ResolveBundleRootFromHelper()
    {
        var serverDirectory = new DirectoryInfo(AppContext.BaseDirectory);
        var moonlightDirectory = serverDirectory.Parent;
        var bundleRoot = moonlightDirectory?.Parent;
        return bundleRoot?.FullName ?? string.Empty;
    }

    private static bool LooksLikeProcessOutputFailure(ProcessResult result)
    {
        var statusText = $"{result.StdOut}\n{result.StdErr}";
        return !result.Ok ||
               statusText.Contains("No matching devices", StringComparison.OrdinalIgnoreCase) ||
               statusText.Contains("No devices", StringComparison.OrdinalIgnoreCase);
    }

    private static ProcessResult RunPnpUtil(string arguments, int timeoutMs = 15000) =>
        RunProcess("pnputil.exe", arguments, null, timeoutMs);

    private static bool TryParsePnpUtilField(string line, string fieldName, out string value)
    {
        value = string.Empty;
        if (!line.StartsWith(fieldName, StringComparison.OrdinalIgnoreCase))
        {
            return false;
        }

        var separatorIndex = line.IndexOf(':');
        if (separatorIndex < 0)
        {
            return false;
        }

        value = line[(separatorIndex + 1)..].Trim();
        return true;
    }

    private static List<DisplayClassDeviceEntry> EnumeratePnpClassDevices(string className)
    {
        try
        {
            if (string.IsNullOrWhiteSpace(className) || className.Any(ch => !char.IsLetterOrDigit(ch) && ch != '-' && ch != '_'))
            {
                return [];
            }

            var result = RunPnpUtil($"/enum-devices /class {className}", 15000);
            var combined = $"{result.StdOut}\n{result.StdErr}";
            if (string.IsNullOrWhiteSpace(combined))
            {
                return [];
            }

            var entries = new List<DisplayClassDeviceEntry>();
            string instanceId = string.Empty;
            string deviceDescription = string.Empty;
            string manufacturerName = string.Empty;
            string status = string.Empty;

            void Flush()
            {
                if (string.IsNullOrWhiteSpace(instanceId) &&
                    string.IsNullOrWhiteSpace(deviceDescription) &&
                    string.IsNullOrWhiteSpace(manufacturerName) &&
                    string.IsNullOrWhiteSpace(status))
                {
                    return;
                }

                entries.Add(new DisplayClassDeviceEntry(
                    instanceId.Trim(),
                    deviceDescription.Trim(),
                    manufacturerName.Trim(),
                    status.Trim()));

                instanceId = string.Empty;
                deviceDescription = string.Empty;
                manufacturerName = string.Empty;
                status = string.Empty;
            }

            foreach (var rawLine in combined.Replace("\r", string.Empty).Split('\n'))
            {
                var line = rawLine.Trim();
                if (string.IsNullOrWhiteSpace(line))
                {
                    Flush();
                    continue;
                }

                if (TryParsePnpUtilField(line, "Instance ID", out var parsed))
                {
                    instanceId = parsed;
                }
                else if (TryParsePnpUtilField(line, "Device Description", out parsed))
                {
                    deviceDescription = parsed;
                }
                else if (TryParsePnpUtilField(line, "Manufacturer Name", out parsed))
                {
                    manufacturerName = parsed;
                }
                else if (TryParsePnpUtilField(line, "Status", out parsed))
                {
                    status = parsed;
                }
            }

            Flush();
            return entries;
        }
        catch
        {
            return [];
        }
    }

    private static List<DisplayClassDeviceEntry> EnumerateDisplayClassDevices() =>
        EnumeratePnpClassDevices("Display");

    private static bool IsRootDisplayClassDevice(DisplayClassDeviceEntry entry) =>
        entry.InstanceId.StartsWith(@"ROOT\DISPLAY\", StringComparison.OrdinalIgnoreCase);

    private static bool IsDisplayClassDeviceStarted(DisplayClassDeviceEntry entry) =>
        entry.Status.Contains("Started", StringComparison.OrdinalIgnoreCase) ||
        entry.Status.Equals("OK", StringComparison.OrdinalIgnoreCase) ||
        entry.Status.Contains("Running", StringComparison.OrdinalIgnoreCase);

    private static bool IsParsecDisplayClassDevice(DisplayClassDeviceEntry entry)
    {
        var text = $"{entry.InstanceId} {entry.DeviceDescription} {entry.ManufacturerName}";
        return text.Contains("Parsec", StringComparison.OrdinalIgnoreCase) ||
               text.Contains("PSCCDD", StringComparison.OrdinalIgnoreCase);
    }

    private static bool IsQemuDisplayClassDevice(DisplayClassDeviceEntry entry)
    {
        var text = $"{entry.InstanceId} {entry.DeviceDescription} {entry.ManufacturerName}";
        return text.Contains("QEMU", StringComparison.OrdinalIgnoreCase) ||
               text.Contains("VirtIO", StringComparison.OrdinalIgnoreCase) ||
               text.Contains("Red Hat", StringComparison.OrdinalIgnoreCase);
    }

    private static bool IsKnownNonMttVirtualDisplayClassDevice(DisplayClassDeviceEntry entry)
    {
        var text = $"{entry.InstanceId} {entry.DeviceDescription} {entry.ManufacturerName}";
        return IsParsecDisplayClassDevice(entry) ||
               IsQemuDisplayClassDevice(entry) ||
               TextLooksLikeNonMttVirtualDisplay(text);
    }

    private static bool IsLikelyMttDisplayClassDevice(DisplayClassDeviceEntry entry)
    {
        var text = $"{entry.InstanceId} {entry.DeviceDescription} {entry.ManufacturerName}";
        return TextLooksLikeMttVdd(text);
    }

    private static bool IsCompetingMttExclusiveDisplayClassDevice(DisplayClassDeviceEntry entry)
    {
        if (string.IsNullOrWhiteSpace(entry.InstanceId) || IsLikelyMttDisplayClassDevice(entry))
        {
            return false;
        }

        var text = $"{entry.InstanceId} {entry.DeviceDescription} {entry.ManufacturerName}";
        if (IsParsecDisplayClassDevice(entry))
        {
            return true;
        }

        var providerOwnedVirtual =
            entry.InstanceId.StartsWith(@"ROOT\", StringComparison.OrdinalIgnoreCase) ||
            entry.InstanceId.StartsWith(@"SWD\", StringComparison.OrdinalIgnoreCase);
        if (!providerOwnedVirtual)
        {
            return false;
        }

        return TextLooksLikeNonMttVirtualDisplay(text)
            || text.Contains("IddSampleDriver", StringComparison.OrdinalIgnoreCase)
            || text.Contains("Virtual Desktop Monitor", StringComparison.OrdinalIgnoreCase);
    }

    private static List<DisplayClassDeviceEntry> ResolveMttDisplayClassDevices(List<DisplayClassDeviceEntry> entries)
    {
        var rootDisplayDevices = entries
            .Where(IsRootDisplayClassDevice)
            .ToList();
        if (rootDisplayDevices.Count == 0)
        {
            return [];
        }

        var explicitMtt = rootDisplayDevices
            .Where(IsLikelyMttDisplayClassDevice)
            .OrderByDescending(IsDisplayClassDeviceStarted)
            .ThenBy(entry => entry.InstanceId, StringComparer.OrdinalIgnoreCase)
            .ToList();
        if (explicitMtt.Count > 0)
        {
            return explicitMtt;
        }

        return rootDisplayDevices
            .Where(entry =>
                !IsKnownNonMttVirtualDisplayClassDevice(entry) &&
                entry.DeviceDescription.Contains("Virtual Display Driver", StringComparison.OrdinalIgnoreCase))
            .OrderByDescending(IsDisplayClassDeviceStarted)
            .ThenBy(entry => entry.InstanceId, StringComparer.OrdinalIgnoreCase)
            .ToList();
    }

    private static void TryCleanupDisconnectedMttMonitorDevices()
    {
        try
        {
            var monitorDevices = EnumeratePnpClassDevices("Monitor");
            foreach (var entry in monitorDevices)
            {
                if (string.IsNullOrWhiteSpace(entry.InstanceId) ||
                    !entry.InstanceId.StartsWith(@"DISPLAY\MTT1337\", StringComparison.OrdinalIgnoreCase) ||
                    IsDisplayClassDeviceStarted(entry))
                {
                    continue;
                }

                RunPnpUtil($"/remove-device \"{entry.InstanceId}\"", 15000);
                Thread.Sleep(50);
            }
        }
        catch
        {
            // Best effort; stale monitor entries should not block VDD recovery.
        }
    }

    private static void TryRefreshDisplayClassDevices()
    {
        try
        {
            RunPnpUtil("/scan-devices", 15000);
            Thread.Sleep(450);
        }
        catch
        {
            // Best effort.
        }
    }

    private static List<string> TryDisableCompetingDisplayClassDevicesForExclusiveMtt()
    {
        // Other display adapters must stay as the user/Windows left them.
        // Cloudgime stream ownership is enforced by MTT VDD primary selection
        // and Sunshine output_name, not by disabling competing devices.
        return [];
    }

    private static bool TryDisableDisplayClassDevice(string instanceId)
    {
        return false;
    }

    private static bool TryEnableDisplayClassDevice(string instanceId)
    {
        if (string.IsNullOrWhiteSpace(instanceId))
        {
            return false;
        }

        try
        {
            var result = RunPnpUtil($"/enable-device \"{instanceId}\"", 15000);
            if (LooksLikeDeviceEnableAccepted(result))
            {
                return true;
            }
        }
        catch
        {
            // Fall through to devcon.
        }

        var devconPath = ResolveBundledDevconPath();
        if (string.IsNullOrWhiteSpace(devconPath))
        {
            return false;
        }

        var devconWorkingDirectory = Path.GetDirectoryName(devconPath);
        var devconResult = RunProcess(devconPath, $"enable \"@{instanceId}\"", devconWorkingDirectory, 15000);
        return LooksLikeDeviceEnableAccepted(devconResult);
    }

    private static bool LooksLikeDeviceDisableAccepted(ProcessResult result)
    {
        var combined = $"{result.StdOut}\n{result.StdErr}";
        return result.Ok
            || combined.Contains("success", StringComparison.OrdinalIgnoreCase)
            || combined.Contains("disabled on reboot", StringComparison.OrdinalIgnoreCase)
            || combined.Contains("ready to be disabled", StringComparison.OrdinalIgnoreCase);
    }

    private static bool LooksLikeDeviceEnableAccepted(ProcessResult result)
    {
        var combined = $"{result.StdOut}\n{result.StdErr}";
        return result.Ok
            || combined.Contains("success", StringComparison.OrdinalIgnoreCase)
            || combined.Contains("enabled", StringComparison.OrdinalIgnoreCase)
            || combined.Contains("ready to be enabled", StringComparison.OrdinalIgnoreCase);
    }

    private static string? ResolveBundledDevconPath()
    {
        try
        {
            var bundleRoot = ResolveBundleRootFromHelper();
            if (string.IsNullOrWhiteSpace(bundleRoot))
            {
                return null;
            }

            var candidate = Path.Combine(bundleRoot, "drivers", "vdd-control", "Dependencies", "devcon.exe");
            return File.Exists(candidate) ? candidate : null;
        }
        catch
        {
            return null;
        }
    }

    private static void TryRestoreDisabledDisplayClassDevices(IEnumerable<string> instanceIds)
    {
        foreach (var instanceId in instanceIds.Where(id => !string.IsNullOrWhiteSpace(id)).Distinct(StringComparer.OrdinalIgnoreCase))
        {
            _ = TryEnableDisplayClassDevice(instanceId);
        }

        try
        {
            TryRefreshDisplayClassDevices();
        }
        catch
        {
            // Best effort.
        }
    }

    private static void TryRestartDisplayClassDeviceInstances(IEnumerable<DisplayClassDeviceEntry> entries)
    {
        foreach (var entry in entries)
        {
            if (string.IsNullOrWhiteSpace(entry.InstanceId))
            {
                continue;
            }

            try
            {
                RunPnpUtil($"/enable-device \"{entry.InstanceId}\"", 15000);
                RunPnpUtil($"/restart-device \"{entry.InstanceId}\"", 20000);
                Thread.Sleep(250);
            }
            catch
            {
                // Best effort.
            }
        }
    }

    private static void TryEnsureMttVddDeviceInstalled()
    {
        try
        {
            var bundleRoot = ResolveBundleRootFromHelper();
            if (string.IsNullOrWhiteSpace(bundleRoot))
            {
                return;
            }

            var devconPath = Path.Combine(bundleRoot, "drivers", "vdd-control", "Dependencies", "devcon.exe");
            var driverInfPath = Path.Combine(bundleRoot, "drivers", "virtual-display-driver", "MttVDD.inf");
            if (!File.Exists(devconPath) || !File.Exists(driverInfPath))
            {
                return;
            }

            TryCleanupDisconnectedMttMonitorDevices();

            var devconWorkingDirectory = Path.GetDirectoryName(devconPath);
            var status = RunProcess(devconPath, "status \"Root\\MttVDD\"", devconWorkingDirectory, 4000);
            var displayClassDevices = EnumerateDisplayClassDevices();
            var mttDisplayClassDevices = ResolveMttDisplayClassDevices(displayClassDevices);
            var missingDevice = LooksLikeProcessOutputFailure(status);
            var statusText = $"{status.StdOut}\n{status.StdErr}";
            var driverRunning = statusText.Contains("Driver is running", StringComparison.OrdinalIgnoreCase);
            if (!missingDevice &&
                driverRunning &&
                (mttDisplayClassDevices.Count == 0 || mttDisplayClassDevices.Any(IsDisplayClassDeviceStarted)))
            {
                return;
            }

            if (missingDevice)
            {
                RunProcess(devconPath, $"install \"{driverInfPath}\" \"Root\\MttVDD\"", devconWorkingDirectory, 15000);
                TryRefreshDisplayClassDevices();
                Thread.Sleep(500);
                status = RunProcess(devconPath, "status \"Root\\MttVDD\"", devconWorkingDirectory, 4000);
                missingDevice = LooksLikeProcessOutputFailure(status);
                statusText = $"{status.StdOut}\n{status.StdErr}";
                driverRunning = statusText.Contains("Driver is running", StringComparison.OrdinalIgnoreCase);
                displayClassDevices = EnumerateDisplayClassDevices();
                mttDisplayClassDevices = ResolveMttDisplayClassDevices(displayClassDevices);
            }

            if (missingDevice)
            {
                return;
            }

            if (!driverRunning)
            {
                RunProcess(devconPath, "enable \"Root\\MttVDD\"", devconWorkingDirectory, 4000);
                RunProcess(devconPath, "restart \"Root\\MttVDD\"", devconWorkingDirectory, 5000);
                Thread.Sleep(500);
            }

            if (mttDisplayClassDevices.Count > 0 && !mttDisplayClassDevices.Any(IsDisplayClassDeviceStarted))
            {
                TryRestartDisplayClassDeviceInstances(mttDisplayClassDevices);
                TryRefreshDisplayClassDevices();
            }
        }
        catch
        {
            // Best effort: display prepare can still fall back to the active host display.
        }
    }

    private static void TryRestartMttVddDevice()
    {
        try
        {
            var bundleRoot = ResolveBundleRootFromHelper();
            if (string.IsNullOrWhiteSpace(bundleRoot))
            {
                return;
            }

            var devconPath = Path.Combine(bundleRoot, "drivers", "vdd-control", "Dependencies", "devcon.exe");
            if (!File.Exists(devconPath))
            {
                return;
            }

            var devconWorkingDirectory = Path.GetDirectoryName(devconPath);
            RunProcess(devconPath, "restart \"Root\\MttVDD\"", devconWorkingDirectory, 5000);
            TryRestartDisplayClassDeviceInstances(ResolveMttDisplayClassDevices(EnumerateDisplayClassDevices()));
            TryRefreshDisplayClassDevices();
            Thread.Sleep(500);
        }
        catch
        {
            // Best effort: if restart is denied, normal activation still gets a chance below.
        }
    }

    private static string? ResolveParsecVddExecutablePath()
    {
        foreach (var candidate in new[]
        {
            Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles), "Parsec", "vdd", "parsec-vdd.exe"),
            Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ProgramFilesX86), "Parsec", "vdd", "parsec-vdd.exe"),
        })
        {
            if (File.Exists(candidate))
            {
                return candidate;
            }
        }

        return null;
    }

    private static void TryQuickAddParsecVddDisplay()
    {
        try
        {
            var parsecVddPath = ResolveParsecVddExecutablePath();
            if (string.IsNullOrWhiteSpace(parsecVddPath))
            {
                return;
            }

            var workingDirectory = Path.GetDirectoryName(parsecVddPath);
            foreach (var arguments in new[] { "-a", "--add", "/add" })
            {
                RunProcess(parsecVddPath, arguments, workingDirectory, 12000);
                Thread.Sleep(500);
                TryRefreshDisplayClassDevices();
            }
        }
        catch
        {
            // Best effort.
        }
    }

    private static List<DisplaySnapshot> EnsureParsecVddOnline(
        List<DisplaySnapshot> initialDisplays,
        StreamDisplayPreference preference)
    {
        var currentDisplays = initialDisplays;
        var activeParsec = FindParsecDisplay(currentDisplays, requireActive: true);
        if (activeParsec is not null)
        {
            return currentDisplays;
        }

        TryQuickAddParsecVddDisplay();
        Thread.Sleep(500);
        currentDisplays = EnumerateDisplays();
        var parsecDisplay = FindParsecDisplay(currentDisplays, requireActive: false);
        if (parsecDisplay is not null)
        {
            try
            {
                currentDisplays = TryActivateInactiveDisplay(currentDisplays, parsecDisplay);
            }
            catch
            {
                currentDisplays = EnumerateDisplays();
            }

            activeParsec = FindParsecDisplay(currentDisplays, requireActive: true);
            if (activeParsec is not null)
            {
                return currentDisplays;
            }
        }

        TryRunDisplaySwitchClone();
        for (var attempt = 0; attempt < 18; attempt++)
        {
            Thread.Sleep(250);
            currentDisplays = EnumerateDisplays();
            parsecDisplay = FindParsecDisplay(currentDisplays, requireActive: false);
            if (parsecDisplay is not null)
            {
                try
                {
                    currentDisplays = TryActivateInactiveDisplay(currentDisplays, parsecDisplay);
                }
                catch
                {
                    currentDisplays = EnumerateDisplays();
                }
            }

            activeParsec = FindParsecDisplay(currentDisplays, requireActive: true);
            if (activeParsec is not null)
            {
                return currentDisplays;
            }
        }

        throw new InvalidOperationException("Parsec VDA display did not come online for stream.");
    }

    private static bool IsCompetingVirtualDisplay(DisplaySnapshot display)
    {
        if (!display.Active || DisplayLooksLikeMttVdd(display))
        {
            return false;
        }

        return display.IsVdd || DisplayLooksLikeCompetingVirtual(display);
    }

    private static bool DisplayLooksLikeCompetingVirtual(DisplaySnapshot display)
    {
        var text = $"{display.DeviceString} {display.DeviceId}";
        return TextLooksLikeNonMttVirtualDisplay(text) ||
               text.Contains("QEMU", StringComparison.OrdinalIgnoreCase) ||
               text.Contains("VirtIO", StringComparison.OrdinalIgnoreCase) ||
               text.Contains("Red Hat", StringComparison.OrdinalIgnoreCase);
    }

    private static bool SavedDisplayLooksLikeCompetingVirtual(SavedDisplayState savedDisplay)
    {
        var text = $"{savedDisplay.DeviceString} {savedDisplay.DeviceId}";
        return text.Contains("Parsec", StringComparison.OrdinalIgnoreCase) ||
               text.Contains("PSCCDD", StringComparison.OrdinalIgnoreCase) ||
               text.Contains("QEMU", StringComparison.OrdinalIgnoreCase) ||
               text.Contains("VirtIO", StringComparison.OrdinalIgnoreCase) ||
               text.Contains("Red Hat", StringComparison.OrdinalIgnoreCase);
    }

    private static bool DisplayLooksLikeCloudgimeVdd(DisplaySnapshot display)
    {
        var text = $"{display.DeviceString} {display.DeviceId}";
        return display.IsVdd ||
               text.Contains("MttVDD", StringComparison.OrdinalIgnoreCase) ||
               text.Contains("MTT1337", StringComparison.OrdinalIgnoreCase) ||
               text.Contains("Virtual Display", StringComparison.OrdinalIgnoreCase);
    }

    private static bool SavedDisplayLooksLikeCloudgimeVdd(SavedDisplayState savedDisplay)
    {
        var text = $"{savedDisplay.DeviceString} {savedDisplay.DeviceId}";
        return text.Contains("MttVDD", StringComparison.OrdinalIgnoreCase) ||
               text.Contains("MTT1337", StringComparison.OrdinalIgnoreCase) ||
               text.Contains("Virtual Display", StringComparison.OrdinalIgnoreCase);
    }

    private static bool CanIgnoreDisableDisplayFailure(DisplaySnapshot display, Exception failure)
    {
        if (DisplayLooksLikeCompetingVirtual(display))
        {
            return true;
        }

        var lowered = failure.Message.ToLowerInvariant();
        return lowered.Contains("disp_change code -1") &&
               (display.IsVdd || DisplayLooksLikeCompetingVirtual(display) || DisplayLooksLikeCloudgimeVdd(display));
    }

    private static bool CanIgnoreEnableDisplayFailure(
        DisplaySnapshot currentDisplay,
        SavedDisplayState savedDisplay,
        Exception failure)
    {
        if (DisplayLooksLikeCompetingVirtual(currentDisplay) ||
            SavedDisplayLooksLikeCompetingVirtual(savedDisplay))
        {
            return true;
        }

        var lowered = failure.Message.ToLowerInvariant();
        return lowered.Contains("disp_change code -1") &&
               (DisplayLooksLikeCompetingVirtual(currentDisplay) ||
                SavedDisplayLooksLikeCompetingVirtual(savedDisplay) ||
                DisplayLooksLikeCloudgimeVdd(currentDisplay) ||
                SavedDisplayLooksLikeCloudgimeVdd(savedDisplay));
    }

    private static bool CanIgnorePrimaryRestoreFailure(DisplaySnapshot display, Exception failure)
    {
        var lowered = failure.Message.ToLowerInvariant();
        return lowered.Contains("disp_change code -1") &&
               (DisplayLooksLikeCompetingVirtual(display) || DisplayLooksLikeCloudgimeVdd(display));
    }

    private static List<DisplaySnapshot> TryDetachCompetingVirtualDisplays(
        List<DisplaySnapshot> displays,
        StreamDisplayPreference? displayPreference = null)
    {
        if (displayPreference is not null && PreferenceTargetsParsecVdd(displayPreference))
        {
            return displays;
        }

        var candidates = displays
            .Where(ShouldDetachCompetingVirtualDisplayForMttStream)
            .ToList();
        if (candidates.Count == 0)
        {
            return displays;
        }

        var changed = false;
        foreach (var display in candidates)
        {
            try
            {
                DetachDisplayFromDesktop(display);
                changed = true;
            }
            catch (Exception ex) when (CanIgnoreDisableDisplayFailure(display, ex))
            {
                // Some third-party virtual adapters reject topology changes while
                // their service is connected. Continue with the remaining displays.
            }
        }

        if (!changed)
        {
            return displays;
        }

        ApplyDisplayChanges();
        Thread.Sleep(650);
        return EnumerateDisplays();
    }

    private static bool ShouldDetachCompetingVirtualDisplayForMttStream(DisplaySnapshot display)
    {
        if (!display.Active || DisplayLooksLikeMttVdd(display))
        {
            return false;
        }

        if (IsQemuVirtioDisplay(display))
        {
            return false;
        }

        return IsParsecDisplay(display) || DisplayLooksLikeCompetingVirtual(display);
    }

    private static void DetachDisplayFromDesktop(DisplaySnapshot display)
    {
        var mode = GetCurrentModeOrDefault(display.DeviceName, allowRegistryFallback: true);
        mode.dmFields = DmPosition | DmPelsWidth | DmPelsHeight;
        mode.dmPositionX = display.PositionX;
        mode.dmPositionY = display.PositionY;
        mode.dmPelsWidth = 0;
        mode.dmPelsHeight = 0;

        EnsureDisplayResult(
            NativeMethods.ChangeDisplaySettingsEx(
                display.DeviceName,
                ref mode,
                IntPtr.Zero,
                NativeMethods.ChangeDisplaySettingsFlags.CDS_UPDATEREGISTRY |
                NativeMethods.ChangeDisplaySettingsFlags.CDS_NORESET,
                IntPtr.Zero
            ),
            $"detach competing virtual display {display.DeviceName}"
        );
    }

    private static HelperResult BuildRequestedStreamFallbackResult(Exception failure) => new()
    {
        Ok = true,
        Changed = false,
        Restored = false,
        Skipped = true,
        Reason = $"stream_display_unavailable_requested_stream_fallback:{CompactProcessOutput(failure.Message)}",
        Applied = null,
    };

    private static bool ShouldAllowRequestedStreamFallback(StreamDisplayPreference displayPreference) =>
        NormalizeStreamDisplayMode(displayPreference.Mode) is "auto";

    private static HelperResult PrepareDisplay(Options options)
    {
        var displayPreference = ReadStreamDisplayPreference(options.BundleRoot);
        var configureMttVdd = PreferenceMayUseMttVdd(displayPreference);
        SavedDisplayState? requestedMode = null;
        if (options.Width > 0 && options.Height > 0)
        {
            requestedMode = NormalizeRequestedMode(options.Width, options.Height, options.Fps, configureMttVdd);
        }

        var beforeDisplays = EnumerateDisplays();
        var previousPrimary = beforeDisplays.FirstOrDefault(display => display.Primary && display.Active);
        var previousVdd = beforeDisplays.FirstOrDefault(display => display.IsVdd);
        var previousOtherDisplays = beforeDisplays.Where(display => !display.IsVdd).Select(display => display.ToSavedState()).ToList();
        var previousCursor = GetCursorState();
        var remoteProcessMitigation = new RemoteProcessMitigationResult();
        var boostedProcesses = new List<PriorityBoostState>();
        var temperedRemoteProcesses = new List<PriorityBoostState>();
        var disabledDisplayClassDevices = new List<string>();

        try
        {
            disabledDisplayClassDevices = TryDisableCompetingDisplayClassDevicesForExclusiveMtt();

            if (SuspendCompetingRemoteAppsDuringStream)
            {
                remoteProcessMitigation = SuspendCompetingRemoteProcesses(new HashSet<int>());
            }

            List<DisplaySnapshot> currentDisplays;
            DisplaySnapshot streamDisplay;
            try
            {
                (currentDisplays, streamDisplay) = ResolveStreamDisplayForPrepare(beforeDisplays, requestedMode, displayPreference);
            }
            catch (Exception ex) when (requestedMode is not null && ShouldAllowRequestedStreamFallback(displayPreference))
            {
                if (remoteProcessMitigation.SuspendedProcesses.Count > 0)
                {
                    ResumeSuspendedRemoteProcesses(remoteProcessMitigation.SuspendedProcesses);
                }
                return BuildRequestedStreamFallbackResult(ex);
            }

            var displayOrCaptureChanged = false;
            var exactModeFallback = false;
            var primaryAuthorityBestEffortFailed = false;
            if (DisplayLooksLikeMttVdd(streamDisplay) &&
                currentDisplays.Any(ShouldDetachCompetingVirtualDisplayForMttStream))
            {
                currentDisplays = TryDetachCompetingVirtualDisplays(currentDisplays, displayPreference);
                streamDisplay = ResolvePreparedStreamDisplay(currentDisplays, displayPreference, streamDisplay)
                    ?? throw new InvalidOperationException("Stream display disappeared after detaching competing virtual displays.");
                displayOrCaptureChanged = true;
            }

            var useDuplicateMttVdd = UsesMttVddDuplicateAuthority(streamDisplay);
            if (useDuplicateMttVdd)
            {
                if (EnsureMttVddDuplicatedWithPrimary(
                    ref currentDisplays,
                    ref streamDisplay,
                    displayPreference,
                    displays => ResolvePreparedStreamDisplay(displays, displayPreference, streamDisplay),
                    "Stream display disappeared after enabling duplicate mode."))
                {
                    displayOrCaptureChanged = true;
                }
            }
            else
            {
                if (DisconnectOtherDisplaysBeforeMttPrimary(
                    ref currentDisplays,
                    ref streamDisplay,
                    displayPreference,
                    displays => ResolvePreparedStreamDisplay(displays, displayPreference, streamDisplay),
                    "Stream display disappeared after disconnecting other displays."))
                {
                    displayOrCaptureChanged = true;
                }

                if (!streamDisplay.Primary || streamDisplay.PositionX != 0 || streamDisplay.PositionY != 0)
                {
                    try
                    {
                        PromoteStreamDisplayToPrimaryWithRecovery(
                            ref currentDisplays,
                            ref streamDisplay,
                            requestedMode,
                            displayPreference,
                            displays => ResolvePreparedStreamDisplay(displays, displayPreference, streamDisplay),
                            "Stream display disappeared after setting it as primary.");
                        displayOrCaptureChanged = true;
                    }
                    catch when (DisplayLooksLikeMttVdd(streamDisplay))
                    {
                        primaryAuthorityBestEffortFailed = true;
                        currentDisplays = EnumerateDisplays();
                        streamDisplay = ResolvePreparedStreamDisplay(currentDisplays, displayPreference, streamDisplay)
                            ?? streamDisplay;
                    }
                }
            }

            if (!useDuplicateMttVdd &&
                requestedMode is not null &&
                (streamDisplay.Width != requestedMode.Width ||
                 streamDisplay.Height != requestedMode.Height ||
                 requestedMode.Frequency != 0 && streamDisplay.Frequency != requestedMode.Frequency))
            {
                var modeApplyResult = ApplyExactStreamDisplayModeWithRecovery(
                    ref currentDisplays,
                    ref streamDisplay,
                    requestedMode,
                    displayPreference,
                    displays => ResolvePreparedStreamDisplay(displays, displayPreference, streamDisplay),
                    "Stream display disappeared after prepare exact resize.");
                if (modeApplyResult.Applied)
                {
                    exactModeFallback = modeApplyResult.Fallback;
                }
                else
                {
                    exactModeFallback = false;
                }
                displayOrCaptureChanged = true;
            }

            var otherActiveDisplays = currentDisplays
                .Where(display => display.Active && !IsSameDisplay(display, streamDisplay))
                .ToList();
            if (!useDuplicateMttVdd &&
                ReassertPreparedStreamDisplayAuthority(
                ref currentDisplays,
                ref streamDisplay,
                displayPreference,
                "Stream display disappeared after display authority repair."))
            {
                displayOrCaptureChanged = true;
                otherActiveDisplays = currentDisplays
                    .Where(display => display.Active && !IsSameDisplay(display, streamDisplay))
                    .ToList();
            }
            if (DisableOtherDisplaysDuringStream && otherActiveDisplays.Count > 0)
            {
                DisableOtherActiveDisplaysForStreamAuthority(streamDisplay, currentDisplays);
                currentDisplays = EnumerateDisplays();
                streamDisplay = ResolvePreparedStreamDisplay(currentDisplays, displayPreference, streamDisplay)
                    ?? throw new InvalidOperationException("Stream display disappeared after disabling other displays.");
                displayOrCaptureChanged = true;
            }

            if (!useDuplicateMttVdd &&
                requestedMode is not null &&
                !DisplayMatchesRequestedMode(streamDisplay, requestedMode))
            {
                var postAuthorityModeApplyResult = ApplyExactStreamDisplayModeWithRecovery(
                    ref currentDisplays,
                    ref streamDisplay,
                    requestedMode,
                    displayPreference,
                    displays => ResolvePreparedStreamDisplay(displays, displayPreference, streamDisplay),
                    "Stream display disappeared after post-authority exact resize.");
                if (postAuthorityModeApplyResult.Applied)
                {
                    exactModeFallback = postAuthorityModeApplyResult.Fallback;
                }
                else
                {
                    exactModeFallback = false;
                }
                displayOrCaptureChanged = true;
            }

            var centeredCursor = CenterCursorOnStreamDisplay ? CenterCursor(streamDisplay) : previousCursor;

            boostedProcesses = BoostMoonlightHostProcesses(new HashSet<int>());

            temperedRemoteProcesses = LowerCompetingRemoteAppsDuringStream
                ? LowerCompetingRemoteProcessPriority(new HashSet<int>())
                : [];

            ShowSystemCursor();
            if (ArrangeWindowsOnStreamDisplay && ArrangeWindowsForStreamTarget(streamDisplay))
            {
                displayOrCaptureChanged = true;
            }

            var sunshineCaptureConfig = TryApplySunshineStreamCaptureConfigForBundle(options.BundleRoot, streamDisplay);
            if (sunshineCaptureConfig.Changed)
            {
                displayOrCaptureChanged = true;
            }

            var reportedAppliedMode = BuildReportedAppliedMode(streamDisplay, requestedMode, exactModeFallback);
            WriteState(new PrepareStateFile
            {
                SessionToken = options.SessionToken,
                Requested = requestedMode,
                PreviousPrimary = previousPrimary?.ToSavedState(),
                PreviousVdd = previousVdd?.ToSavedState(),
                PreviousOtherDisplays = previousOtherDisplays,
                PreviousCursor = previousCursor,
                CursorHidden = false,
                AppliedVdd = streamDisplay.IsVdd ? streamDisplay.ToSavedState() : previousVdd?.ToSavedState(),
                AppliedDisplay = streamDisplay.ToSavedState(),
                StreamDisplayMode = NormalizeStreamDisplayMode(displayPreference.Mode),
                SuspendedRemoteProcesses = remoteProcessMitigation.SuspendedProcesses,
                BoostedProcesses = boostedProcesses,
                TemperedRemoteProcesses = temperedRemoteProcesses,
                DisabledDisplayClassDevices = disabledDisplayClassDevices,
            });

            if (!displayOrCaptureChanged)
            {
                return new HelperResult
                {
                    Ok = true,
                    Changed = false,
                    Restored = false,
                    Skipped = false,
                    Reason = primaryAuthorityBestEffortFailed
                        ? requestedMode is null
                            ? "stream_display_session_ready_capture_pinned_primary_best_effort"
                            : "stream_display_session_ready_exact_mode_capture_pinned_primary_best_effort"
                        : requestedMode is null ? "stream_display_session_ready" : "stream_display_session_ready_exact_mode",
                    Applied = reportedAppliedMode,
                    SunshineCaptureChanged = sunshineCaptureConfig.Changed,
                    SunshineCaptureTargetChanged = sunshineCaptureConfig.TargetChanged,
                    SunshineCaptureDisplay = sunshineCaptureConfig.DisplayName,
                    SunshineCaptureConfigPath = sunshineCaptureConfig.ConfigPath,
                };
            }

            return new HelperResult
            {
                Ok = true,
                Changed = displayOrCaptureChanged,
                Restored = false,
                Skipped = false,
                Reason = exactModeFallback
                    ? "stream_display_session_surface_fallback"
                    : primaryAuthorityBestEffortFailed
                        ? requestedMode is null
                            ? "stream_display_session_prepared_capture_pinned_primary_best_effort"
                            : "stream_display_session_prepared_exact_mode_capture_pinned_primary_best_effort"
                        : requestedMode is null ? "stream_display_session_prepared" : "stream_display_session_prepared_exact_mode",
                Applied = reportedAppliedMode,
                SunshineCaptureChanged = sunshineCaptureConfig.Changed,
                SunshineCaptureTargetChanged = sunshineCaptureConfig.TargetChanged,
                SunshineCaptureDisplay = sunshineCaptureConfig.DisplayName,
                SunshineCaptureConfigPath = sunshineCaptureConfig.ConfigPath,
            };
        }
        catch
        {
            if (temperedRemoteProcesses.Count > 0)
            {
                RestoreBoostedMoonlightHostProcesses(temperedRemoteProcesses);
            }

            if (boostedProcesses.Count > 0)
            {
                RestoreBoostedMoonlightHostProcesses(boostedProcesses);
            }

            if (remoteProcessMitigation.SuspendedProcesses.Count > 0)
            {
                ResumeSuspendedRemoteProcesses(remoteProcessMitigation.SuspendedProcesses);
            }

            if (disabledDisplayClassDevices.Count > 0)
            {
                TryRestoreDisabledDisplayClassDevices(disabledDisplayClassDevices);
            }

            throw;
        }
    }

    private static HelperResult ResizeDisplay(Options options)
    {
        if (options.Width <= 0 || options.Height <= 0)
        {
            throw new ArgumentException("resize requires --width and --height");
        }

        var displayPreference = ReadStreamDisplayPreference(options.BundleRoot);
        var requested = NormalizeRequestedMode(options.Width, options.Height, options.Fps, PreferenceMayUseMttVdd(displayPreference));
        var state = ReadState();
        if (StateBelongsToDifferentSession(state, options.SessionToken))
        {
            return new HelperResult
            {
                Ok = true,
                Changed = false,
                Restored = false,
                Skipped = true,
                Reason = "superseded_by_newer_session",
            };
        }
        var beforeDisplays = EnumerateDisplays();
        var currentDisplays = EnsureStreamDisplayReadyForResize(beforeDisplays, requested, displayPreference, state);
        var streamDisplay = ResolveStreamDisplayForResize(currentDisplays, displayPreference, state)
            ?? throw new InvalidOperationException("Stream display is not active for resize.");

        var changed = false;
        var exactModeFallback = false;
        var primaryAuthorityBestEffortFailed = false;
        if (DisplayLooksLikeMttVdd(streamDisplay) &&
            currentDisplays.Any(ShouldDetachCompetingVirtualDisplayForMttStream))
        {
            currentDisplays = TryDetachCompetingVirtualDisplays(currentDisplays, displayPreference);
            streamDisplay = ResolveStreamDisplayForResize(currentDisplays, displayPreference, state)
                ?? throw new InvalidOperationException("Stream display disappeared after resize detached competing virtual displays.");
            changed = true;
        }

        var useDuplicateMttVdd = UsesMttVddDuplicateAuthority(streamDisplay);
        if (useDuplicateMttVdd)
        {
            if (EnsureMttVddDuplicatedWithPrimary(
                ref currentDisplays,
                ref streamDisplay,
                displayPreference,
                displays => ResolveStreamDisplayForResize(displays, displayPreference, state),
                "Stream display disappeared after enabling duplicate mode."))
            {
                changed = true;
            }
        }
        else
        {
            if (DisconnectOtherDisplaysBeforeMttPrimary(
                ref currentDisplays,
                ref streamDisplay,
                displayPreference,
                displays => ResolveStreamDisplayForResize(displays, displayPreference, state),
                "Stream display disappeared after resize disconnected other displays."))
            {
                changed = true;
            }

            if (!streamDisplay.Primary || streamDisplay.PositionX != 0 || streamDisplay.PositionY != 0)
            {
                try
                {
                    PromoteStreamDisplayToPrimaryWithRecovery(
                        ref currentDisplays,
                        ref streamDisplay,
                        requested,
                        displayPreference,
                        displays => ResolveStreamDisplayForResize(displays, displayPreference, state),
                        "Stream display disappeared after setting it as primary.");
                    changed = true;
                }
                catch when (DisplayLooksLikeMttVdd(streamDisplay))
                {
                    primaryAuthorityBestEffortFailed = true;
                    currentDisplays = EnumerateDisplays();
                    streamDisplay = ResolveStreamDisplayForResize(currentDisplays, displayPreference, state)
                        ?? streamDisplay;
                }
            }
        }

        if (!useDuplicateMttVdd &&
            (streamDisplay.Width != requested.Width ||
             streamDisplay.Height != requested.Height ||
             requested.Frequency != 0 && streamDisplay.Frequency != requested.Frequency))
        {
            var modeApplyResult = ApplyExactStreamDisplayModeWithRecovery(
                ref currentDisplays,
                ref streamDisplay,
                requested,
                displayPreference,
                displays => ResolveStreamDisplayForResize(displays, displayPreference, state),
                "Stream display disappeared after exact resize.");
            if (modeApplyResult.Applied)
            {
                exactModeFallback = modeApplyResult.Fallback;
            }
            else
            {
                exactModeFallback = false;
            }
            changed = true;
        }

        var targetDisplay = streamDisplay;
        var otherActiveDisplays = currentDisplays
            .Where(display => display.Active && !IsSameDisplay(display, targetDisplay))
            .ToList();
        if (!useDuplicateMttVdd &&
            ReassertResizeStreamDisplayAuthority(
            ref currentDisplays,
            ref streamDisplay,
            displayPreference,
            state,
            "Stream display disappeared after resize display authority repair."))
        {
            changed = true;
            otherActiveDisplays = currentDisplays
                .Where(display => display.Active && !IsSameDisplay(display, streamDisplay))
                .ToList();
        }
        if (DisableOtherDisplaysDuringStream && otherActiveDisplays.Count > 0)
        {
            DisableOtherActiveDisplaysForStreamAuthority(streamDisplay, currentDisplays);
            currentDisplays = EnumerateDisplays();
            streamDisplay = ResolveStreamDisplayForResize(currentDisplays, displayPreference, state)
                ?? throw new InvalidOperationException("Stream display disappeared after disabling other displays.");
            changed = true;
        }

        var centeredCursor = CenterCursorOnStreamDisplay ? CenterCursor(streamDisplay) : state?.PreviousCursor;
        if (CenterCursorOnStreamDisplay && centeredCursor is not null)
        {
            changed = true;
        }

        state ??= CreateInitialState(beforeDisplays, options.SessionToken);
        state.SessionToken = options.SessionToken;
        state.Requested = new SavedDisplayState
        {
            Width = requested.Width,
            Height = requested.Height,
            Frequency = requested.Frequency,
        };
        state.StreamDisplayMode = NormalizeStreamDisplayMode(displayPreference.Mode);
        ShowSystemCursor();
        if (ArrangeWindowsOnStreamDisplay && ArrangeWindowsForStreamTarget(streamDisplay))
        {
            changed = true;
        }

        var sunshineCaptureConfig = TryApplySunshineStreamCaptureConfigForBundle(options.BundleRoot, streamDisplay);
        if (sunshineCaptureConfig.Changed)
        {
            changed = true;
        }

        if (SuspendCompetingRemoteAppsDuringStream)
        {
            var existingSuspendedPids = state.SuspendedRemoteProcesses
                .Select(process => process.Pid)
                .Where(pid => pid > 0)
                .ToHashSet();
            var remoteProcessMitigation = SuspendCompetingRemoteProcesses(existingSuspendedPids);
            if (remoteProcessMitigation.Changed)
            {
                changed = true;
                state.SuspendedRemoteProcesses.AddRange(remoteProcessMitigation.SuspendedProcesses);
            }
        }

        if (LowerCompetingRemoteAppsDuringStream)
        {
            var existingTemperedPids = state.TemperedRemoteProcesses
                .Select(process => process.Pid)
                .Where(pid => pid > 0)
                .ToHashSet();
            var temperedRemoteProcesses = LowerCompetingRemoteProcessPriority(existingTemperedPids);
            if (temperedRemoteProcesses.Count > 0)
            {
                changed = true;
                state.TemperedRemoteProcesses.AddRange(temperedRemoteProcesses);
            }
        }

        var existingBoostedPids = state.BoostedProcesses
            .Select(process => process.Pid)
            .Where(pid => pid > 0)
            .ToHashSet();
        var boostedProcesses = BoostMoonlightHostProcesses(existingBoostedPids);
        if (boostedProcesses.Count > 0)
        {
            changed = true;
            state.BoostedProcesses.AddRange(boostedProcesses);
        }

        state.AppliedDisplay = streamDisplay.ToSavedState();
        if (streamDisplay.IsVdd)
        {
            state.AppliedVdd = streamDisplay.ToSavedState();
        }
        WriteState(state);
        var reportedAppliedMode = BuildReportedAppliedMode(streamDisplay, requested, exactModeFallback);

        return new HelperResult
        {
            Ok = true,
            Changed = changed,
            Restored = false,
            Skipped = !changed,
            Reason = exactModeFallback
                ? "stream_display_session_surface_fallback"
                : primaryAuthorityBestEffortFailed
                    ? changed
                        ? "stream_display_session_resized_exact_mode_capture_pinned_primary_best_effort"
                        : "stream_display_session_already_exact_mode_capture_pinned_primary_best_effort"
                    : changed ? "stream_display_session_resized_exact_mode" : "stream_display_session_already_exact_mode",
            Applied = reportedAppliedMode,
            SunshineCaptureChanged = sunshineCaptureConfig.Changed,
            SunshineCaptureTargetChanged = sunshineCaptureConfig.TargetChanged,
            SunshineCaptureDisplay = sunshineCaptureConfig.DisplayName,
            SunshineCaptureConfigPath = sunshineCaptureConfig.ConfigPath,
        };
    }

    private static HelperResult RestoreDisplay(Options options)
    {
        var state = ReadState();
        if (state is null)
        {
            return new HelperResult
            {
                Ok = true,
                Changed = false,
                Restored = false,
                Skipped = true,
                Reason = "no_saved_state",
            };
        }

        if (StateBelongsToDifferentSession(state, options.SessionToken))
        {
            return new HelperResult
            {
                Ok = true,
                Changed = false,
                Restored = false,
                Skipped = true,
                Reason = "superseded_by_newer_session",
            };
        }

        try
        {
            if (state.DisabledDisplayClassDevices.Count > 0)
            {
                TryRestoreDisabledDisplayClassDevices(state.DisabledDisplayClassDevices);
            }

            var currentDisplays = EnumerateDisplays();
            if (state.PreviousPrimary is not null)
            {
                var currentPreviousPrimary = FindDisplay(currentDisplays, state.PreviousPrimary);
                if (currentPreviousPrimary is null || !currentPreviousPrimary.Active)
                {
                    TryRunDisplaySwitchClone();
                    currentDisplays = EnumerateDisplays();
                }
            }

            var restoreTargets = state.PreviousOtherDisplays.Where(display => display.Active).ToList();
            foreach (var savedDisplay in restoreTargets)
            {
                var currentDisplay = FindDisplay(currentDisplays, savedDisplay);
                if (currentDisplay is null)
                {
                    continue;
                }

                try
                {
                    EnableDisplay(currentDisplay, savedDisplay);
                }
                catch (Exception ex) when (CanIgnoreEnableDisplayFailure(currentDisplay, savedDisplay, ex))
                {
                    // Some virtual displays cannot be restored on demand once their
                    // provider daemon changes ownership. Keep restoring the rest of
                    // the desktop instead of failing the whole session teardown.
                }
            }

            if (state.PreviousVdd is { Active: true } previousVdd)
            {
                var currentVddForRestore = FindDisplay(currentDisplays, previousVdd);
                if (currentVddForRestore is not null)
                {
                    try
                    {
                        EnableDisplay(currentVddForRestore, previousVdd);
                    }
                    catch (Exception ex) when (CanIgnoreEnableDisplayFailure(currentVddForRestore, previousVdd, ex))
                    {
                        // Best effort: if the saved virtual display can no longer
                        // be reattached cleanly, continue restoring the primary.
                    }
                }
            }

            if (restoreTargets.Count > 0 || state.PreviousVdd is { Active: true })
            {
                ApplyDisplayChanges();
                currentDisplays = EnumerateDisplays();
            }

            if (state.PreviousPrimary is not null)
            {
                currentDisplays = EnumerateDisplays();
                var previousPrimary = FindDisplay(currentDisplays, state.PreviousPrimary)
                    ?? throw new InvalidOperationException("previous primary display is no longer available.");
                if (!previousPrimary.Active)
                {
                    TryRunDisplaySwitchClone();
                    currentDisplays = EnumerateDisplays();
                    previousPrimary = FindDisplay(currentDisplays, state.PreviousPrimary)
                        ?? throw new InvalidOperationException("previous primary display is no longer available.");
                }

                if (!previousPrimary.Primary || previousPrimary.PositionX != 0 || previousPrimary.PositionY != 0)
                {
                    try
                    {
                        MakeDisplayPrimary(previousPrimary, currentDisplays, repositionOtherDisplays: false);
                    }
                    catch (Exception ex) when (CanIgnorePrimaryRestoreFailure(previousPrimary, ex))
                    {
                        TryRunDisplaySwitchClone();
                        currentDisplays = EnumerateDisplays();
                    }
                    catch when (!previousPrimary.IsVdd)
                    {
                        TryRunDisplaySwitchClone();
                        currentDisplays = EnumerateDisplays();
                        previousPrimary = FindDisplay(currentDisplays, state.PreviousPrimary)
                            ?? throw new InvalidOperationException("previous primary display is no longer available.");
                        if (!previousPrimary.Primary || previousPrimary.PositionX != 0 || previousPrimary.PositionY != 0)
                        {
                            MakeDisplayPrimary(previousPrimary, currentDisplays, repositionOtherDisplays: false);
                        }
                    }
                    currentDisplays = EnumerateDisplays();
                }
                RestoreWindowsToPreviousPrimaryBeforeStreamDisplayShutdown(state, currentDisplays);
            }

            if (state.PreviousCursor is not null)
            {
                NativeMethods.SetCursorPos(state.PreviousCursor.X, state.PreviousCursor.Y);
            }

            if (state.CursorHidden)
            {
                ShowSystemCursor();
            }

            File.Delete(StatePath);

            return new HelperResult
            {
                Ok = true,
                Changed = false,
                Restored = true,
                Skipped = false,
                Reason = "restored_previous_primary",
            };
        }
        finally
        {
            RestoreRuntimeProcessMitigations(state);
        }
    }

    private static void RestoreRuntimeProcessMitigations(PrepareStateFile state)
    {
        try
        {
            if (state.SuspendedRemoteProcesses.Count > 0)
            {
                ResumeSuspendedRemoteProcesses(state.SuspendedRemoteProcesses);
            }
        }
        catch
        {
            // Cleanup must not fail just because a remote-control process exited.
        }

        try
        {
            RestoreBoostedMoonlightHostProcesses(state.TemperedRemoteProcesses);
        }
        catch
        {
            // Best effort.
        }

        try
        {
            RestoreBoostedMoonlightHostProcesses(state.BoostedProcesses);
        }
        catch
        {
            // Best effort.
        }
    }

    private static HelperResult ProjectDisplay(Options options)
    {
        var state = ReadState();
        if (StateBelongsToDifferentSession(state, options.SessionToken))
        {
            return new HelperResult
            {
                Ok = true,
                Changed = false,
                Restored = false,
                Skipped = true,
                Reason = "superseded_by_newer_session",
            };
        }

        var mode = NormalizeProjectDisplayMode(options.ProjectMode);
        if (string.IsNullOrWhiteSpace(mode))
        {
            throw new ArgumentException("project-display requires --mode");
        }

        var changed = false;
        var displayPreference = ReadStreamDisplayPreference(options.BundleRoot);
        var currentDisplays = EnumerateDisplays();
        var targetResolution = ResolveProjectDisplayTarget(currentDisplays, displayPreference);
        currentDisplays = targetResolution.Displays;
        var streamDisplay = targetResolution.Target;

        switch (mode)
        {
            case "extend":
                TryRunDisplaySwitchExtend();
                Thread.Sleep(650);
                changed = true;
                break;
            case "duplicate":
                if (streamDisplay is not null && UsesMttVddDuplicateAuthority(streamDisplay))
                {
                    changed = EnsureMttVddDuplicatedWithPrimary(
                        ref currentDisplays,
                        ref streamDisplay,
                        displayPreference,
                        displays => ResolveProjectDisplayTarget(displays, displayPreference).Target,
                        "stream display disappeared after duplicate display mode.");
                }
                else if (!TryRunDisplaySwitchClone())
                {
                    return new HelperResult
                    {
                        Ok = false,
                        Changed = false,
                        Restored = false,
                        Skipped = false,
                        Reason = "display_mode_duplicate_failed",
                    };
                }
                changed = true;
                break;
            case "second_screen_only":
                if (streamDisplay is null)
                {
                    throw new InvalidOperationException("stream display is not available for second screen only mode.");
                }

                if (!streamDisplay.Primary || streamDisplay.PositionX != 0 || streamDisplay.PositionY != 0)
                {
                    PromoteStreamDisplayToPrimaryWithRecovery(
                        ref currentDisplays,
                        ref streamDisplay,
                        requestedMode: null,
                        displayPreference,
                        displays => ResolveProjectDisplayTarget(displays, displayPreference).Target,
                        "stream display disappeared after second screen only primary repair.",
                        repositionOtherDisplays: false);
                    changed = true;
                    targetResolution = ResolveProjectDisplayTarget(currentDisplays, displayPreference);
                    currentDisplays = targetResolution.Displays;
                    streamDisplay = targetResolution.Target
                        ?? throw new InvalidOperationException("stream display disappeared after second screen only primary repair.");
                }

                break;
            default:
                throw new ArgumentException($"unsupported display mode: {options.ProjectMode}");
        }

        currentDisplays = EnumerateDisplays();
        targetResolution = ResolveProjectDisplayTarget(currentDisplays, displayPreference);
        currentDisplays = targetResolution.Displays;
        streamDisplay = targetResolution.Target ?? FindPrimaryDisplay(currentDisplays);
        var sunshineCaptureConfig = streamDisplay is null
            ? new SunshineCaptureConfigApplyResult(false, false, null, null)
            : TryApplySunshineStreamCaptureConfigForBundle(options.BundleRoot, streamDisplay);

        return new HelperResult
        {
            Ok = true,
            Changed = changed || sunshineCaptureConfig.Changed,
            Restored = false,
            Skipped = !changed && !sunshineCaptureConfig.Changed,
            Reason = $"display_mode_{mode}",
            Applied = streamDisplay?.ToSavedState(),
            SunshineCaptureChanged = sunshineCaptureConfig.Changed,
            SunshineCaptureTargetChanged = sunshineCaptureConfig.TargetChanged,
            SunshineCaptureDisplay = sunshineCaptureConfig.DisplayName,
            SunshineCaptureConfigPath = sunshineCaptureConfig.ConfigPath,
        };
    }

    private static HelperResult ListDisplays(Options options)
    {
        var preference = ReadStreamDisplayPreference(options.BundleRoot);
        return BuildDisplayControlResult(
            changed: false,
            skipped: false,
            reason: "display_control_snapshot",
            preference: preference);
    }

    private static HelperResult SetStreamDisplay(Options options)
    {
        var normalizedMode = NormalizeManualDisplayMode(options.DisplayMode);
        if (string.IsNullOrWhiteSpace(normalizedMode))
        {
            throw new ArgumentException("set-stream-display requires --display-mode");
        }

        var preference = normalizedMode switch
        {
            "mtt_vdd" => new StreamDisplayPreference
            {
                SchemaVersion = 1,
                ManualOverride = false,
                Mode = "mtt_vdd",
            },
            "primary" => new StreamDisplayPreference
            {
                SchemaVersion = 1,
                ManualOverride = true,
                Mode = "primary",
            },
            "custom" => new StreamDisplayPreference
            {
                SchemaVersion = 1,
                ManualOverride = true,
                Mode = "custom",
                CustomDeviceName = options.DisplayDeviceName.Trim(),
                CustomDeviceId = options.DisplayDeviceId.Trim(),
                CustomLabel = options.DisplayLabel.Trim(),
            },
            _ => throw new ArgumentException($"unsupported display mode: {options.DisplayMode}"),
        };

        if (normalizedMode == "custom"
            && string.IsNullOrWhiteSpace(preference.CustomDeviceName)
            && string.IsNullOrWhiteSpace(preference.CustomDeviceId)
            && string.IsNullOrWhiteSpace(preference.CustomLabel))
        {
            throw new ArgumentException("custom display selection requires --device-name, --device-id, or --label");
        }

        WriteStreamDisplayPreference(options.BundleRoot, preference);
        return BuildDisplayControlResult(
            changed: true,
            skipped: false,
            reason: $"display_preference_{NormalizeStreamDisplayMode(preference.Mode)}",
            preference: preference);
    }

    private static string NormalizeProjectDisplayMode(string? mode)
    {
        var normalized = (mode ?? string.Empty).Trim().ToLowerInvariant().Replace('-', '_');
        return normalized switch
        {
            "extend" or "extended" or "perluas" => "extend",
            "duplicate" or "duplicated" or "clone" or "cloned" or "mirror" or "double" or "doble" or "duplikat" => "duplicate",
            "primary" or "utama" or "make_primary" or "stream_primary" or
            "second_screen_only" or "second_screen" or "stream_only" or "stream_display_only" or
            "layar_stream" or "layar_stream_saja" => "second_screen_only",
            _ => string.Empty,
        };
    }

    private static string NormalizeManualDisplayMode(string? mode)
    {
        var normalized = (mode ?? string.Empty).Trim().ToLowerInvariant().Replace('-', '_');
        return normalized switch
        {
            "cloud" or "cloud_only" or "mtt" or "mtt_vdd" or "vdd" or "virtual_display" => "mtt_vdd",
            "primary" or "host_primary" or "host_primary_only" or "current_primary" => "primary",
            "custom" or "device" or "display" => "custom",
            _ => string.Empty,
        };
    }

    private static (List<DisplaySnapshot> Displays, DisplaySnapshot? Target) ResolveProjectDisplayTarget(
        List<DisplaySnapshot> displays,
        StreamDisplayPreference displayPreference)
    {
        try
        {
            var resolved = ResolveStreamDisplayForPrepare(displays, requestedMode: null, displayPreference);
            return (resolved.Displays, resolved.Target);
        }
        catch
        {
            return (
                displays,
                FindPreferredStreamDisplay(displays, displayPreference, requireActive: true)
                    ?? FindPrimaryDisplay(displays)
            );
        }
    }

    private static HelperResult WatchWindowPrimary(Options options)
    {
        var seenWindowHandles = new Dictionary<nint, DateTimeOffset>();
        var sleepMs = Math.Max(350, options.PollMs);

        while (true)
        {
            try
            {
                EnforceMttVddAuthorityForActiveSession(options.SessionToken);
                ArrangeWindowsForWatchTarget(options.SessionToken, seenWindowHandles, TimeSpan.FromSeconds(4));
            }
            catch
            {
                // Keep the watcher alive; it is a best-effort helper only.
            }

            Thread.Sleep(sleepMs);
        }
    }

    private static HelperResult EnforcePersistentVddOnly(Options options)
    {
        using var singleInstance = new Mutex(true, @"Global\CloudgimeDisplayPreparePersistentVddOnly", out var ownsSingleInstance);
        if (!ownsSingleInstance)
        {
            Console.WriteLine("[PersistentVddOnly] Another background MTT VDD guard is already running.");
            return new HelperResult
            {
                Ok = true,
                Changed = false,
                Restored = false,
                Skipped = true,
                Reason = "persistent_vdd_guard_already_running",
            };
        }

        var sleepMs = Math.Clamp(options.PollMs, 5000, 30000);
        var primaryPromotionDeferredLogged = false;
        var captureRuntimeRefreshPending = false;
        var captureRuntimeRefreshDeferredLogged = false;
        var pendingCaptureTarget = string.Empty;
        var nextCaptureRuntimeRefreshAttempt = DateTimeOffset.MinValue;
        Console.WriteLine("[PersistentVddOnly] Starting background MTT VDD enforcement loop...");

        // Boot-time recovery: Clean up stale state files on cold boot
        try
        {
            long uptimeMs = Environment.TickCount64;
            if (uptimeMs < 180000 && File.Exists(StatePath))
            {
                Console.WriteLine($"[PersistentVddOnly] Cold boot detected (uptime: {uptimeMs / 1000}s) and stale display prepare state file exists. Cleaning up...");
                File.Delete(StatePath);
                
                // Memicu preflight / inisialisasi ulang jika bundle root tersedia
                if (!string.IsNullOrWhiteSpace(options.BundleRoot) && Directory.Exists(options.BundleRoot))
                {
                    Console.WriteLine("[PersistentVddOnly] Memicu preflight refresh setelah pembersihan boot...");
                    PreflightHost(options);
                }
            }
        }
        catch (Exception ex)
        {
            Console.WriteLine($"[PersistentVddOnly] Failed during boot-time recovery: {ex.Message}");
        }

        while (true)
        {
            try
            {
                var displays = EnumerateDisplays();
                var vddDisplay = FindVddDisplay(displays, requireActive: true);
                if (vddDisplay != null)
                {
                    var changed = false;

                    var sunshineCaptureConfig = TryApplySunshineStreamCaptureConfigForBundle(options.BundleRoot, vddDisplay);
                    if (sunshineCaptureConfig.Changed)
                    {
                        Console.WriteLine($"[PersistentVddOnly] Updated Sunshine capture target to {sunshineCaptureConfig.DisplayName}.");
                        changed = true;
                    }
                    if (sunshineCaptureConfig.TargetChanged ||
                        CaptureConfigIsNewerThanRunningSunshine(sunshineCaptureConfig.ConfigPath))
                    {
                        captureRuntimeRefreshPending = true;
                        captureRuntimeRefreshDeferredLogged = false;
                        pendingCaptureTarget = sunshineCaptureConfig.DisplayName ?? vddDisplay.DeviceName;
                    }

                    if (captureRuntimeRefreshPending)
                    {
                        if (HasActiveCloudgimeStreamSession())
                        {
                            if (!captureRuntimeRefreshDeferredLogged)
                            {
                                Console.WriteLine("[PersistentVddOnly] Capture runtime refresh deferred because a stream session is active.");
                                captureRuntimeRefreshDeferredLogged = true;
                            }
                        }
                        else if (DateTimeOffset.UtcNow >= nextCaptureRuntimeRefreshAttempt)
                        {
                            if (TryRefreshCaptureRuntimeForPersistentGuard(options.BundleRoot, pendingCaptureTarget, out var refreshMessage))
                            {
                                Console.WriteLine($"[PersistentVddOnly] Capture runtime refreshed while idle. {refreshMessage}");
                                captureRuntimeRefreshPending = false;
                                captureRuntimeRefreshDeferredLogged = false;
                                pendingCaptureTarget = string.Empty;
                                changed = true;
                            }
                            else
                            {
                                Console.WriteLine($"[PersistentVddOnly] Capture runtime refresh failed: {refreshMessage}");
                                nextCaptureRuntimeRefreshAttempt = DateTimeOffset.UtcNow.AddSeconds(60);
                            }
                        }
                    }

                    if (!vddDisplay.Primary || vddDisplay.PositionX != 0 || vddDisplay.PositionY != 0)
                    {
                        if (!primaryPromotionDeferredLogged)
                        {
                            Console.WriteLine("[PersistentVddOnly] MTT VDD is not primary; keeping capture pinned and deferring primary promotion to explicit stream preparation to avoid display flicker.");
                            primaryPromotionDeferredLogged = true;
                        }
                    }
                    else
                    {
                        primaryPromotionDeferredLogged = false;
                    }

                    if (changed)
                    {
                        Console.WriteLine("[PersistentVddOnly] MTT VDD primary/capture target repaired.");
                    }
                }
                else
                {
                    Console.WriteLine("[PersistentVddOnly] MTT VDD display is not active; passive guard will not change display topology.");
                }
            }
            catch (Exception ex)
            {
                Console.WriteLine($"[PersistentVddOnly] Warning during enforcement: {ex.Message}");
            }

            Thread.Sleep(sleepMs);
        }
    }

    private static bool HasActiveCloudgimeStreamSession()
    {
        foreach (var process in Process.GetProcesses())
        {
            try
            {
                var name = process.ProcessName;
                if (name.StartsWith("streamer-", StringComparison.OrdinalIgnoreCase) ||
                    name.StartsWith("mic_sidecar-", StringComparison.OrdinalIgnoreCase))
                {
                    return true;
                }
            }
            catch
            {
                // Ignore protected or transient processes.
            }
            finally
            {
                process.Dispose();
            }
        }

        return false;
    }

    private static bool CaptureConfigIsNewerThanRunningSunshine(string? configPath)
    {
        if (string.IsNullOrWhiteSpace(configPath) || !File.Exists(configPath))
        {
            return false;
        }

        var configWriteTime = File.GetLastWriteTimeUtc(configPath);
        foreach (var process in Process.GetProcessesByName("sunshine"))
        {
            try
            {
                if (process.HasExited)
                {
                    continue;
                }

                var processStartTime = process.StartTime.ToUniversalTime();
                if (configWriteTime > processStartTime.AddSeconds(2))
                {
                    return true;
                }
            }
            catch
            {
                // Ignore protected or transient processes.
            }
            finally
            {
                process.Dispose();
            }
        }

        return false;
    }

    private static bool TryRefreshCaptureRuntimeForPersistentGuard(string bundleRoot, string captureTarget, out string message)
    {
        try
        {
            var runtimeAgent = Path.Combine(bundleRoot, "moonlight", "system", "cloudgime-runtime-agent.exe");
            if (!File.Exists(runtimeAgent))
            {
                message = $"runtime agent missing at {runtimeAgent}";
                return false;
            }

            using var process = new Process();
            process.StartInfo = new ProcessStartInfo
            {
                FileName = runtimeAgent,
                WorkingDirectory = bundleRoot,
                UseShellExecute = false,
                CreateNoWindow = true,
            };
            process.StartInfo.ArgumentList.Add("--bundle-root");
            process.StartInfo.ArgumentList.Add(bundleRoot);
            process.StartInfo.ArgumentList.Add("restart-runtime");

            if (!process.Start())
            {
                message = "runtime agent did not start";
                return false;
            }

            if (!process.WaitForExit(90_000))
            {
                try
                {
                    process.Kill(entireProcessTree: true);
                }
                catch
                {
                    // Best effort.
                }

                message = "runtime refresh timed out";
                return false;
            }

            if (process.ExitCode != 0)
            {
                message = $"runtime agent exited with code {process.ExitCode}";
                return false;
            }

            message = $"target={captureTarget}";
            return true;
        }
        catch (Exception ex)
        {
            message = ex.Message;
            return false;
        }
    }

    private static Rectangle GetPrimaryWorkArea()
    {
        var primaryScreen = Screen.PrimaryScreen
            ?? throw new InvalidOperationException("primary screen is not available");
        return primaryScreen.WorkingArea;
    }

    private static bool ArrangeWindowsForWatchTarget(
        string sessionToken,
        Dictionary<nint, DateTimeOffset>? seenWindowHandles = null,
        TimeSpan? repeatCooldown = null)
    {
        var targetArea = TryResolveWindowWatchTargetArea(sessionToken, out var resolvedArea)
            ? resolvedArea
            : GetPrimaryWorkArea();
        return ArrangeWindowsToTargetArea(targetArea, seenWindowHandles, repeatCooldown);
    }

    private static bool ArrangeWindowsForStreamTarget(DisplaySnapshot display)
    {
        var targetArea = ResolveWindowArrangeTargetArea(display);
        return ArrangeWindowsToTargetArea(targetArea);
    }

    private static Rectangle ResolveWindowArrangeTargetArea(DisplaySnapshot display) =>
        UsesMttVddDuplicateAuthority(display)
            ? GetPrimaryWorkArea()
            : TryResolveDisplayWorkArea(display) ?? RectangleForDisplay(display);

    private static bool ArrangeWindowsToTargetArea(
        Rectangle targetArea,
        Dictionary<nint, DateTimeOffset>? seenWindowHandles = null,
        TimeSpan? repeatCooldown = null,
        Rectangle? sourceArea = null)
    {
        if (targetArea.Width <= 0 || targetArea.Height <= 0)
        {
            return false;
        }

        var changed = false;
        foreach (var handle in NativeMethods.EnumerateTopLevelWindows())
        {
            try
            {
                var candidate = TryGetWindowMoveCandidate(handle, targetArea, sourceArea);
                if (candidate is null)
                {
                    continue;
                }

                var key = candidate.Value.Handle;
                if (seenWindowHandles is not null &&
                    seenWindowHandles.TryGetValue(key, out var alreadyMovedAt) &&
                    DateTimeOffset.UtcNow - alreadyMovedAt < (repeatCooldown ?? TimeSpan.Zero))
                {
                    continue;
                }

                if (MoveWindowToTarget(candidate.Value.Handle, candidate.Value.Rect, targetArea))
                {
                    if (seenWindowHandles is not null)
                    {
                        seenWindowHandles[key] = DateTimeOffset.UtcNow;
                    }
                    changed = true;
                }
            }
            catch
            {
                // Best-effort helper only.
            }
        }

        return changed;
    }

    private static bool RestoreWindowsToPreviousPrimaryBeforeStreamDisplayShutdown(
        PrepareStateFile state,
        IReadOnlyList<DisplaySnapshot> currentDisplays)
    {
        if (state.PreviousPrimary is null)
        {
            return false;
        }

        var targetDisplay = FindDisplay(currentDisplays, state.PreviousPrimary);
        if (targetDisplay is null || !targetDisplay.Active)
        {
            return false;
        }

        var sourceDisplay = state.AppliedDisplay is not null
            ? FindDisplay(currentDisplays, state.AppliedDisplay)
            : null;
        if (sourceDisplay is null || !sourceDisplay.Active)
        {
            sourceDisplay = state.AppliedVdd is not null
                ? FindDisplay(currentDisplays, state.AppliedVdd)
                : null;
        }

        if (sourceDisplay is null || !sourceDisplay.Active || IsSameDisplay(sourceDisplay, targetDisplay))
        {
            return false;
        }

        var targetArea = TryResolveDisplayWorkArea(targetDisplay) ?? RectangleForDisplay(targetDisplay);
        var sourceArea = TryResolveDisplayWorkArea(sourceDisplay) ?? RectangleForDisplay(sourceDisplay);
        return ArrangeWindowsFromSourceToTarget(sourceArea, targetArea);
    }

    private static bool ArrangeWindowsFromSourceToTarget(Rectangle sourceArea, Rectangle targetArea)
        => ArrangeWindowsToTargetArea(targetArea, sourceArea: sourceArea);

    private static bool TryResolveWindowWatchTargetArea(string sessionToken, out Rectangle targetArea)
    {
        targetArea = Rectangle.Empty;

        try
        {
            var state = ReadState();
            if (!PrepareStateMatchesSession(state, sessionToken))
            {
                return false;
            }

            var displays = EnumerateDisplays();
            var displayPreference = ReadStreamDisplayPreference(string.Empty);
            var streamDisplay = ResolveStreamDisplayForResize(displays, displayPreference, state)
                ?? FindPreferredStreamDisplay(displays, displayPreference, requireActive: true)
                ?? FindPrimaryDisplay(displays);
            if (streamDisplay is null)
            {
                return false;
            }

            targetArea = ResolveWindowArrangeTargetArea(streamDisplay);
            if (targetArea.Width > 0 && targetArea.Height > 0)
            {
                return true;
            }

            targetArea = TryResolveDisplayWorkArea(streamDisplay)
                ?? new Rectangle(
                    streamDisplay.PositionX,
                    streamDisplay.PositionY,
                    Math.Max(320, streamDisplay.Width),
                    Math.Max(240, streamDisplay.Height));
            return targetArea.Width > 0 && targetArea.Height > 0;
        }
        catch
        {
            return false;
        }
    }

    private static Rectangle? TryResolveDisplayWorkArea(DisplaySnapshot display)
    {
        foreach (var screen in Screen.AllScreens)
        {
            if (string.Equals(screen.DeviceName, display.DeviceName, StringComparison.OrdinalIgnoreCase))
            {
                return screen.WorkingArea;
            }
        }

        return null;
    }

    private static (nint Handle, Rectangle Rect)? TryGetWindowMoveCandidate(
        nint handle,
        Rectangle targetArea,
        Rectangle? sourceArea = null)
    {
        if (handle == nint.Zero || handle == NativeMethods.GetShellWindow())
        {
            return null;
        }

        if (!NativeMethods.IsWindowVisible(handle) || NativeMethods.IsIconic(handle))
        {
            return null;
        }

        if (NativeMethods.IsWindowCloaked(handle))
        {
            return null;
        }

        NativeMethods.GetWindowThreadProcessId(handle, out var processId);
        if (processId == Environment.ProcessId)
        {
            return null;
        }

        var className = NativeMethods.GetWindowClassName(handle);
        if (className is "Shell_TrayWnd" or "Progman" or "WorkerW")
        {
            return null;
        }

        var exStyle = NativeMethods.GetWindowLongPtr(handle, WindowWatchExStyleIndex).ToInt64();
        if ((exStyle & WindowWatchToolWindow) != 0)
        {
            return null;
        }

        if (!NativeMethods.TryGetWindowRectangle(handle, out var rect))
        {
            return null;
        }

        if (rect.Width < 120 || rect.Height < 80)
        {
            return null;
        }

        if (sourceArea is not null && !WindowBelongsToSourceArea(rect, sourceArea.Value))
        {
            return null;
        }

        var area = rect.Width * rect.Height;
        if (area <= 0)
        {
            return null;
        }

        var overlap = Rectangle.Intersect(rect, targetArea);
        var overlapArea = overlap.Width > 0 && overlap.Height > 0 ? overlap.Width * overlap.Height : 0;
        var overlapRatio = (double)overlapArea / area;
        if (overlapRatio >= 0.55d &&
            !ShouldFillWindowToTarget(rect, targetArea, overlapRatio))
        {
            return null;
        }

        return (handle, rect);
    }

    private static bool WindowBelongsToSourceArea(Rectangle rect, Rectangle sourceArea)
    {
        var centerX = rect.Left + (rect.Width / 2);
        var centerY = rect.Top + (rect.Height / 2);
        if (sourceArea.Contains(centerX, centerY))
        {
            return true;
        }

        var area = Math.Max(1d, rect.Width * (double)rect.Height);
        var overlap = Rectangle.Intersect(rect, sourceArea);
        var overlapArea = overlap.Width > 0 && overlap.Height > 0 ? overlap.Width * overlap.Height : 0d;
        return overlapArea / area >= 0.22d;
    }

    private static bool ShouldFillWindowToTarget(Rectangle rect, Rectangle targetArea, double? overlapRatio = null)
    {
        if (targetArea.Width < 320 || targetArea.Height < 240 || rect.Width <= 0 || rect.Height <= 0)
        {
            return false;
        }

        var primaryPixels = Math.Max(1d, targetArea.Width * (double)targetArea.Height);
        var rectPixels = Math.Max(1d, rect.Width * (double)rect.Height);
        var largeWindow =
            rectPixels >= primaryPixels * 0.30d ||
            rect.Width >= targetArea.Width * 0.78d ||
            rect.Height >= targetArea.Height * 0.78d;
        if (!largeWindow)
        {
            return false;
        }

        var primaryPortrait = targetArea.Height > targetArea.Width;
        var rectPortrait = rect.Height > rect.Width;
        var orientationMismatch = primaryPortrait != rectPortrait;
        var widthDelta = Math.Abs(rect.Width - targetArea.Width) / (double)Math.Max(1, targetArea.Width);
        var heightDelta = Math.Abs(rect.Height - targetArea.Height) / (double)Math.Max(1, targetArea.Height);
        var significantSizeMismatch = widthDelta >= 0.14d || heightDelta >= 0.14d;
        var oversized = rect.Width > targetArea.Width * 1.03d || rect.Height > targetArea.Height * 1.03d;
        var nearPrimary = overlapRatio is null || overlapRatio >= 0.35d;

        return nearPrimary && (orientationMismatch || significantSizeMismatch || oversized);
    }

    private static bool MoveWindowToTarget(nint handle, Rectangle rect, Rectangle targetArea)
    {
        var fillPrimary = ShouldFillWindowToTarget(rect, targetArea);
        var width = fillPrimary
            ? targetArea.Width
            : Math.Min(rect.Width, Math.Max(320, targetArea.Width));
        var height = fillPrimary
            ? targetArea.Height
            : Math.Min(rect.Height, Math.Max(240, targetArea.Height));
        var targetX = targetArea.Left + ((targetArea.Width - width) / 2);
        var targetY = targetArea.Top + ((targetArea.Height - height) / 2);
        return NativeMethods.SetWindowPos(
            handle,
            nint.Zero,
            targetX,
            targetY,
            width,
            height,
            WindowWatchMoveFlags);
    }

    private static Rectangle RectangleForDisplay(DisplaySnapshot display) =>
        new(
            display.PositionX,
            display.PositionY,
            Math.Max(320, display.Width),
            Math.Max(240, display.Height));

    private static HelperResult Failure(string reason) => new()
    {
        Ok = false,
        Changed = false,
        Restored = false,
        Skipped = false,
        Reason = string.IsNullOrWhiteSpace(reason) ? "unknown_error" : reason,
    };

    private static void WriteState(PrepareStateFile state)
    {
        Directory.CreateDirectory(Path.GetDirectoryName(StatePath)!);
        state.Helper["updated_at"] = DateTimeOffset.UtcNow.ToString("O");
        File.WriteAllText(StatePath, JsonSerializer.Serialize(state, JsonOptions));
    }

    private static PrepareStateFile? ReadState() =>
        File.Exists(StatePath)
            ? JsonSerializer.Deserialize<PrepareStateFile>(File.ReadAllText(StatePath), JsonOptions)
            : null;

    private static PrepareStateFile CreateInitialState(List<DisplaySnapshot> displays, string sessionToken)
    {
        var previousPrimary = displays.FirstOrDefault(display => display.Primary && display.Active);
        var previousVdd = displays.FirstOrDefault(display => display.IsVdd);
        return new PrepareStateFile
        {
            SessionToken = sessionToken,
            PreviousPrimary = previousPrimary?.ToSavedState(),
            PreviousVdd = previousVdd?.ToSavedState(),
            PreviousOtherDisplays = displays.Where(display => !display.IsVdd).Select(display => display.ToSavedState()).ToList(),
            PreviousCursor = GetCursorState(),
            AppliedVdd = previousVdd?.ToSavedState(),
            AppliedDisplay = previousPrimary?.ToSavedState(),
            StreamDisplayMode = "mtt_vdd",
            SuspendedRemoteProcesses = [],
            TemperedRemoteProcesses = [],
            DisabledDisplayClassDevices = [],
        };
    }

    private static bool EnforceMttVddAuthorityForActiveSession(string sessionToken)
    {
        var state = ReadState();
        if (!PrepareStateMatchesSession(state, sessionToken))
        {
            return false;
        }

        var changed = false;
        var displayPreference = ReadStreamDisplayPreference(string.Empty);
        var currentDisplays = EnsureStreamDisplayReadyForResize(EnumerateDisplays(), state?.AppliedDisplay ?? state?.AppliedVdd, displayPreference, state);
        var streamDisplay = ResolveStreamDisplayForResize(currentDisplays, displayPreference, state)
            ?? throw new InvalidOperationException("Stream display is not active for stream authority enforcement.");

        if (UsesMttVddDuplicateAuthority(streamDisplay))
        {
            return false;
        }

        if (DisconnectOtherDisplaysBeforeMttPrimary(
            ref currentDisplays,
            ref streamDisplay,
            displayPreference,
            displays => ResolveStreamDisplayForResize(displays, displayPreference, state),
            "Stream display disappeared after stream authority disconnected other displays."))
        {
            changed = true;
        }

        if (!streamDisplay.Primary || streamDisplay.PositionX != 0 || streamDisplay.PositionY != 0)
        {
            try
            {
                PromoteStreamDisplayToPrimaryWithRecovery(
                    ref currentDisplays,
                    ref streamDisplay,
                    state?.Requested,
                    displayPreference,
                    displays => ResolveStreamDisplayForResize(displays, displayPreference, state),
                    "Stream display disappeared after stream authority primary repair.");
                changed = true;
            }
            catch when (DisplayLooksLikeMttVdd(streamDisplay))
            {
                currentDisplays = EnumerateDisplays();
                streamDisplay = ResolveStreamDisplayForResize(currentDisplays, displayPreference, state)
                    ?? streamDisplay;
            }
        }

        var streamDisplayIdentity = streamDisplay;
        var otherActiveDisplays = currentDisplays
            .Where(display => display.Active && !IsSameDisplay(display, streamDisplayIdentity))
            .ToList();
        if (ReassertResizeStreamDisplayAuthority(
            ref currentDisplays,
            ref streamDisplay,
            displayPreference,
            state,
            "Stream display disappeared after stream display authority repair."))
        {
            changed = true;
            otherActiveDisplays = currentDisplays
                .Where(display => display.Active && !IsSameDisplay(display, streamDisplay))
                .ToList();
        }
        if (DisableOtherDisplaysDuringStream && otherActiveDisplays.Count > 0)
        {
            DisableOtherActiveDisplaysForStreamAuthority(streamDisplay, currentDisplays);
            currentDisplays = EnumerateDisplays();
            streamDisplay = ResolveStreamDisplayForResize(currentDisplays, displayPreference, state)
                ?? throw new InvalidOperationException("Stream display disappeared after stream authority cleanup.");
            changed = true;
        }

        if (changed && state is not null)
        {
            state.AppliedDisplay = streamDisplay.ToSavedState();
            if (streamDisplay.IsVdd)
            {
                state.AppliedVdd = streamDisplay.ToSavedState();
            }
            WriteState(state);
        }

        return changed;
    }

    private static bool PrepareStateMatchesSession(PrepareStateFile? state, string sessionToken)
    {
        if (state is null)
        {
            return false;
        }

        if (string.IsNullOrWhiteSpace(sessionToken) || string.IsNullOrWhiteSpace(state.SessionToken))
        {
            return true;
        }

        return string.Equals(state.SessionToken, sessionToken, StringComparison.Ordinal);
    }

    private static bool StateBelongsToDifferentSession(PrepareStateFile? state, string sessionToken)
    {
        if (state is null)
        {
            return false;
        }

        if (string.IsNullOrWhiteSpace(sessionToken) || string.IsNullOrWhiteSpace(state.SessionToken))
        {
            return false;
        }

        return !string.Equals(state.SessionToken, sessionToken, StringComparison.Ordinal);
    }

    private static RemoteProcessMitigationResult SuspendCompetingRemoteProcesses(IReadOnlySet<int> alreadySuspendedPids)
    {
        var result = new RemoteProcessMitigationResult();
        var selfPid = Environment.ProcessId;

        foreach (var process in Process.GetProcesses())
        {
            try
            {
                if (process.HasExited || process.Id <= 0 || process.Id == selfPid)
                {
                    continue;
                }

                if (alreadySuspendedPids.Contains(process.Id) || !CompetingRemoteProcessNames.Contains(process.ProcessName))
                {
                    continue;
                }

                var handle = NativeMethods.OpenProcess(
                    NativeMethods.ProcessAccessRights.SuspendResume | NativeMethods.ProcessAccessRights.QueryLimitedInformation,
                    false,
                    process.Id
                );
                if (handle == IntPtr.Zero)
                {
                    continue;
                }

                try
                {
                    var status = NativeMethods.NtSuspendProcess(handle);
                    if (status >= 0)
                    {
                        result.SuspendedProcesses.Add(new SuspendedProcessState
                        {
                            Pid = process.Id,
                            Name = process.ProcessName,
                        });
                    }
                }
                finally
                {
                    NativeMethods.CloseHandle(handle);
                }
            }
            catch
            {
                // Best effort. Ignore protected or transient processes.
            }
            finally
            {
                process.Dispose();
            }
        }

        return result;
    }

    private static List<PriorityBoostState> BoostMoonlightHostProcesses(IReadOnlySet<int> alreadyBoostedPids)
    {
        var boosted = new List<PriorityBoostState>();
        var selfPid = Environment.ProcessId;

        foreach (var process in Process.GetProcesses())
        {
            try
            {
                if (process.HasExited || process.Id <= 0 || process.Id == selfPid)
                {
                    continue;
                }

                if (alreadyBoostedPids.Contains(process.Id) || !ShouldBoostMoonlightHostProcess(process.ProcessName))
                {
                    continue;
                }

                var previousPriority = process.PriorityClass;
                if (previousPriority == ProcessPriorityClass.High || previousPriority == ProcessPriorityClass.RealTime)
                {
                    continue;
                }

                process.PriorityClass = ProcessPriorityClass.High;
                boosted.Add(new PriorityBoostState
                {
                    Pid = process.Id,
                    Name = process.ProcessName,
                    PreviousPriority = previousPriority.ToString(),
                });
            }
            catch
            {
                // Best effort. Ignore protected or transient processes.
            }
            finally
            {
                process.Dispose();
            }
        }

        return boosted;
    }

    private static bool ShouldBoostMoonlightHostProcess(string processName)
    {
        if (string.IsNullOrWhiteSpace(processName))
        {
            return false;
        }

        if (PriorityBoostProcessNames.Contains(processName))
        {
            return true;
        }

        return processName.StartsWith("streamer-", StringComparison.OrdinalIgnoreCase)
            || processName.StartsWith("web-server-", StringComparison.OrdinalIgnoreCase)
            || processName.StartsWith("mic_sidecar-", StringComparison.OrdinalIgnoreCase);
    }

    private static List<PriorityBoostState> LowerCompetingRemoteProcessPriority(IReadOnlySet<int> alreadyTemperedPids)
    {
        var tempered = new List<PriorityBoostState>();
        var selfPid = Environment.ProcessId;

        foreach (var process in Process.GetProcesses())
        {
            try
            {
                if (process.HasExited || process.Id <= 0 || process.Id == selfPid)
                {
                    continue;
                }

                if (alreadyTemperedPids.Contains(process.Id) || !CompetingRemoteProcessNames.Contains(process.ProcessName))
                {
                    continue;
                }

                var previousPriority = process.PriorityClass;
                if (previousPriority == ProcessPriorityClass.Idle)
                {
                    continue;
                }

                process.PriorityClass = ProcessPriorityClass.Idle;
                tempered.Add(new PriorityBoostState
                {
                    Pid = process.Id,
                    Name = process.ProcessName,
                    PreviousPriority = previousPriority.ToString(),
                });
            }
            catch
            {
                // Best effort. Ignore protected or transient processes.
            }
            finally
            {
                process.Dispose();
            }
        }

        return tempered;
    }

    private static void ResumeSuspendedRemoteProcesses(IEnumerable<SuspendedProcessState> processes)
    {
        foreach (var process in processes)
        {
            if (process.Pid <= 0)
            {
                continue;
            }

            try
            {
                var handle = NativeMethods.OpenProcess(
                    NativeMethods.ProcessAccessRights.SuspendResume | NativeMethods.ProcessAccessRights.QueryLimitedInformation,
                    false,
                    process.Pid
                );
                if (handle == IntPtr.Zero)
                {
                    continue;
                }

                try
                {
                    _ = NativeMethods.NtResumeProcess(handle);
                }
                finally
                {
                    NativeMethods.CloseHandle(handle);
                }
            }
            catch
            {
                // Best effort. Ignore processes that already exited.
            }
        }
    }

    private static void RestoreBoostedMoonlightHostProcesses(IEnumerable<PriorityBoostState> processes)
    {
        foreach (var process in processes)
        {
            if (process.Pid <= 0 || string.IsNullOrWhiteSpace(process.PreviousPriority))
            {
                continue;
            }

            try
            {
                if (!Enum.TryParse<ProcessPriorityClass>(process.PreviousPriority, ignoreCase: true, out var previousPriority))
                {
                    continue;
                }

                using var currentProcess = Process.GetProcessById(process.Pid);
                if (currentProcess.HasExited)
                {
                    continue;
                }

                currentProcess.PriorityClass = previousPriority;
            }
            catch
            {
                // Best effort. Ignore processes that already exited or reject priority changes.
            }
        }
    }

    private static SavedDisplayState NormalizeRequestedMode(int width, int height, int fps, bool configureMttVdd)
    {
        var normalizedWidth = Math.Max(320, Math.Min(7680, width));
        var normalizedHeight = Math.Max(180, Math.Min(4320, height));
        if ((normalizedWidth & 1) != 0)
        {
            normalizedWidth -= 1;
        }

        if ((normalizedHeight & 1) != 0)
        {
            normalizedHeight -= 1;
        }

        var normalizedFps = fps >= 30 ? Math.Min(120, fps) : 60;
        if (!configureMttVdd)
        {
            return new SavedDisplayState
            {
                Width = normalizedWidth,
                Height = normalizedHeight,
                Frequency = normalizedFps,
            };
        }

        var vddSettingsChanged = EnsureConfiguredVddMode(normalizedWidth, normalizedHeight, normalizedFps);
        if (vddSettingsChanged)
        {
            TryRestartMttVddDevice();
        }

        var snapped = SnapRequestedModeToConfiguredVddModes(normalizedWidth, normalizedHeight, normalizedFps);

        return new SavedDisplayState
        {
            Width = snapped.Width,
            Height = snapped.Height,
            Frequency = snapped.Frequency,
        };
    }

    private static SavedDisplayState SnapRequestedModeToConfiguredVddModes(int width, int height, int fps)
    {
        var configuredModes = ReadConfiguredVddModes();
        if (configuredModes.Count == 0)
        {
            return new SavedDisplayState
            {
                Width = width,
                Height = height,
                Frequency = fps,
            };
        }

        var requestedLandscape = width >= height;
        var requestedAspect = width / (double)Math.Max(1, height);
        var orientationMatches = configuredModes
            .Where(mode => (mode.Width >= mode.Height) == requestedLandscape)
            .ToList();

        var candidates = orientationMatches.Count > 0 ? orientationMatches : configuredModes;
        var exact = candidates
            .Where(mode => mode.Width == width && mode.Height == height)
            .OrderBy(mode => Math.Abs(mode.Frequency - fps))
            .ThenByDescending(mode => mode.Frequency)
            .FirstOrDefault();
        if (exact is not null)
        {
            return new SavedDisplayState
            {
                Width = exact.Width,
                Height = exact.Height,
                Frequency = exact.Frequency > 0 ? exact.Frequency : fps,
            };
        }

        var ranked = candidates
            .Select(mode =>
            {
                var modeAspect = mode.Width / (double)Math.Max(1, mode.Height);
                var aspectDelta = Math.Abs(modeAspect - requestedAspect);
                var requestedArea = Math.Max(1d, width * height);
                var modeArea = Math.Max(1d, mode.Width * mode.Height);
                var areaScaleDelta = Math.Abs(Math.Log(modeArea / requestedArea));
                var widthDelta = Math.Abs(mode.Width - width) / (double)Math.Max(1, width);
                var heightDelta = Math.Abs(mode.Height - height) / (double)Math.Max(1, height);
                var score = (aspectDelta * 8d)
                    + (areaScaleDelta * 1.2d)
                    + (widthDelta * 0.4d)
                    + (heightDelta * 0.4d);

                return new
                {
                    Mode = mode,
                    AspectDelta = aspectDelta,
                    AreaScaleDelta = areaScaleDelta,
                    WidthDelta = widthDelta,
                    HeightDelta = heightDelta,
                    Score = score,
                };
            })
            .OrderBy(candidate => candidate.Score)
            .ThenBy(candidate => candidate.AreaScaleDelta)
            .ThenBy(candidate => candidate.AspectDelta)
            .ToList();
        var best = ranked
            .OrderBy(candidate => candidate.Score)
            .ThenBy(candidate => Math.Abs(candidate.Mode.Frequency - fps))
            .Select(candidate => candidate.Mode)
            .First();

        return new SavedDisplayState
        {
            Width = best.Width,
            Height = best.Height,
            Frequency = best.Frequency > 0 ? best.Frequency : fps,
        };
    }

    private static bool EnsureConfiguredVddMode(int width, int height, int fps)
    {
        EnsureVddRegistryPath();
        if (width < 320 || height < 180 || fps < 30)
        {
            // Health checks and non-stream preflight paths may call into the VDD
            // readiness flow without a concrete requested mode. In that case we
            // must not rewrite the VDD mode list back to the base set, because
            // doing so strips the current stream mode and forces another driver
            // restart / display re-enumeration on the next session.
            return false;
        }

        try
        {
            return NormalizeCloudgimeVddSettings(width, height, fps);
        }
        catch
        {
            return false;
        }
    }

    private static bool NormalizeCloudgimeVddSettings(int requestedWidth, int requestedHeight, int requestedFps)
    {
        var configDirectory = Path.GetDirectoryName(VddSettingsPath);
        if (string.IsNullOrWhiteSpace(configDirectory))
        {
            return false;
        }

        Directory.CreateDirectory(configDirectory);

        XDocument doc;
        XElement root;
        if (File.Exists(VddSettingsPath))
        {
            doc = XDocument.Load(VddSettingsPath, LoadOptions.PreserveWhitespace);
            root = doc.Root ?? new XElement("vdd_settings");
            if (doc.Root is null)
            {
                doc.Add(root);
            }
        }
        else
        {
            doc = new XDocument(new XDeclaration("1.0", "utf-8", null));
            root = new XElement("vdd_settings");
            doc.Add(root);
        }

        var changed = false;
        var monitors = EnsureChild(root, "monitors", ref changed);
        var monitorCount = File.Exists(Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "force-wgc.txt")) ? "2" : "1";
        changed |= SetElementValue(monitors, "count", monitorCount);

        var gpu = EnsureChild(root, "gpu", ref changed);
        changed |= SetElementValue(gpu, "friendlyname", ResolvePreferredVddGpuFriendlyName());

        foreach (var global in root.Elements("global").ToList())
        {
            global.Remove();
            changed = true;
        }

        var resolutions = EnsureChild(root, "resolutions", ref changed);
        var desiredModes = BuildCloudgimeVddModes(requestedWidth, requestedHeight, requestedFps);
        var currentModeKeys = resolutions.Elements("resolution")
            .Select(node => $"{ParseIntElement(node, "width")}x{ParseIntElement(node, "height")}@{ParseIntElement(node, "refresh_rate")}")
            .ToList();
        var desiredModeKeys = desiredModes
            .Select(mode => $"{mode.Width}x{mode.Height}@{mode.Fps}")
            .ToList();
        if (!currentModeKeys.SequenceEqual(desiredModeKeys))
        {
            resolutions.ReplaceNodes(desiredModes.Select(mode =>
                new XElement("resolution",
                    new XElement("width", mode.Width),
                    new XElement("height", mode.Height),
                    new XElement("refresh_rate", mode.Fps))));
            changed = true;
        }

        var options = EnsureChild(root, "options", ref changed);
        changed |= SetElementValue(options, "CustomEdid", "false");
        changed |= SetElementValue(options, "PreventSpoof", "false");
        changed |= SetElementValue(options, "EdidCeaOverride", "false");
        changed |= SetElementValue(options, "HardwareCursor", "false");
        changed |= SetElementValue(options, "SDR10bit", "false");
        changed |= SetElementValue(options, "HDRPlus", "false");
        changed |= SetElementValue(options, "logging", "false");
        changed |= SetElementValue(options, "debuglogging", "false");

        if (changed)
        {
            doc.Save(VddSettingsPath);
        }

        return changed;
    }

    private static string ResolvePreferredVddGpuFriendlyName()
    {
        var fallback = string.Empty;
        for (uint adapterIndex = 0; ; adapterIndex++)
        {
            var adapter = NativeMethods.CreateDisplayDevice();
            if (!NativeMethods.EnumDisplayDevices(null, adapterIndex, ref adapter, 0))
            {
                break;
            }

            var adapterName = adapter.DeviceString.Trim();
            var adapterId = adapter.DeviceID.Trim();
            if (string.IsNullOrWhiteSpace(adapterName))
            {
                continue;
            }

            var descriptor = $"{adapterName} {adapterId}";
            if (TextLooksLikeMttVdd(descriptor) ||
                TextLooksLikeNonMttVirtualDisplay(descriptor) ||
                descriptor.Contains("Microsoft Basic", StringComparison.OrdinalIgnoreCase))
            {
                continue;
            }

            if (descriptor.Contains("NVIDIA", StringComparison.OrdinalIgnoreCase))
            {
                return adapterName;
            }

            if (string.IsNullOrWhiteSpace(fallback) &&
                (descriptor.Contains("AMD", StringComparison.OrdinalIgnoreCase) ||
                 descriptor.Contains("Radeon", StringComparison.OrdinalIgnoreCase) ||
                 descriptor.Contains("Intel", StringComparison.OrdinalIgnoreCase)))
            {
                fallback = adapterName;
            }
        }

        return string.IsNullOrWhiteSpace(fallback) ? "auto" : fallback;
    }

    private static XElement EnsureChild(XElement parent, string name, ref bool changed)
    {
        var child = parent.Element(name);
        if (child is not null)
        {
            return child;
        }

        child = new XElement(name);
        parent.Add(child);
        changed = true;
        return child;
    }

    private static bool SetElementValue(XElement parent, string name, string value)
    {
        var element = parent.Element(name);
        if (element is null)
        {
            parent.Add(new XElement(name, value));
            return true;
        }

        if (string.Equals(element.Value, value, StringComparison.Ordinal))
        {
            return false;
        }

        element.Value = value;
        return true;
    }

    private static List<(int Width, int Height, int Fps)> BuildCloudgimeVddModes(int requestedWidth, int requestedHeight, int requestedFps)
    {
        var modes = new List<(int Width, int Height, int Fps)>();
        if (requestedWidth >= 320 && requestedHeight >= 180 && requestedFps >= 30)
        {
            modes.Add((requestedWidth, requestedHeight, Math.Min(120, requestedFps)));
        }
        modes.AddRange(CloudgimeVddBaseModes);

        var seen = new HashSet<string>(StringComparer.Ordinal);
        var distinct = new List<(int Width, int Height, int Fps)>();
        foreach (var mode in modes)
        {
            if (mode.Width < 320 || mode.Height < 180 || mode.Fps < 30)
            {
                continue;
            }

            var width = (mode.Width & 1) == 0 ? mode.Width : mode.Width - 1;
            var height = (mode.Height & 1) == 0 ? mode.Height : mode.Height - 1;
            var fps = Math.Min(120, mode.Fps);
            var key = $"{width}x{height}@{fps}";
            if (seen.Add(key))
            {
                distinct.Add((width, height, fps));
            }
        }

        return distinct;
    }

    private static List<SavedDisplayState> ReadConfiguredVddModes()
    {
        if (!File.Exists(VddSettingsPath))
        {
            return [];
        }

        try
        {
            var doc = XDocument.Load(VddSettingsPath);
            return doc.Descendants("resolution")
                .Select(node =>
                {
                    var width = ParseIntElement(node, "width");
                    var height = ParseIntElement(node, "height");
                    var refresh = ParseIntElement(node, "refresh_rate");
                    if (width < 320 || height < 180)
                    {
                        return null;
                    }

                    return new SavedDisplayState
                    {
                        Width = (width & 1) == 0 ? width : width - 1,
                        Height = (height & 1) == 0 ? height : height - 1,
                        Frequency = refresh >= 30 ? refresh : 60,
                    };
                })
                .Where(mode => mode is not null)
                .GroupBy(mode => $"{mode!.Width}x{mode.Height}@{mode.Frequency}")
                .Select(group => group.First()!)
                .ToList();
        }
        catch
        {
            return [];
        }
    }

    private static int ParseIntElement(XElement parent, string name)
    {
        var raw = parent.Element(name)?.Value;
        return int.TryParse(raw, out var parsed) ? parsed : 0;
    }

    private static bool IsRunningAsAdministrator()
    {
        try
        {
            using (var identity = System.Security.Principal.WindowsIdentity.GetCurrent())
            {
                var principal = new System.Security.Principal.WindowsPrincipal(identity);
                return principal.IsInRole(System.Security.Principal.WindowsBuiltInRole.Administrator);
            }
        }
        catch
        {
            return false;
        }
    }

    private static List<DisplaySnapshot> EnsureVddOnline(List<DisplaySnapshot> initialDisplays, SavedDisplayState? requestedMode = null)
    {
        var preferredVdd = FindVddDisplay(initialDisplays, requireActive: false);
        var requestedFps = requestedMode is null ? 0 : (int)Math.Round(requestedMode.Frequency);
        var isElevated = IsRunningAsAdministrator();

        // 1. Jika VDD sudah aktif dan resolusi sesuai, langsung kembalikan tanpa menyentuh driver
        if (preferredVdd is not null && preferredVdd.Active)
        {
            var modeMatches = requestedMode is null || 
                (preferredVdd.Width == requestedMode.Width && 
                 preferredVdd.Height == requestedMode.Height && 
                 (requestedFps == 0 || preferredVdd.Frequency == requestedFps));
            if (modeMatches)
            {
                return initialDisplays;
            }
        }

        if (isElevated)
        {
            EnsureVddRegistryPath();
        }

        var settingsChanged = isElevated && EnsureConfiguredVddMode(requestedMode?.Width ?? 0, requestedMode?.Height ?? 0, requestedFps);
        if (isElevated && !_driverActionAttempted)
        {
            _driverActionAttempted = true;
            TryDisableCompetingDisplayClassDevicesForExclusiveMtt();
            TryEnsureMttVddDeviceInstalled();
            if (settingsChanged)
            {
                TryRestartMttVddDevice();
            }
        }

        initialDisplays = EnumerateDisplays();
        preferredVdd = FindVddDisplay(initialDisplays, requireActive: false);

        if (preferredVdd is null || !preferredVdd.Active)
        {
            initialDisplays = TryDetachCompetingVirtualDisplays(initialDisplays);
            if (isElevated && !_driverActionAttempted)
            {
                _driverActionAttempted = true;
                TryDisableCompetingDisplayClassDevicesForExclusiveMtt();
                TryEnsureMttVddDeviceInstalled();
                TryRestartMttVddDevice();
                TryRefreshDisplayClassDevices();
                Thread.Sleep(500);
                initialDisplays = EnumerateDisplays();
                preferredVdd = FindVddDisplay(initialDisplays, requireActive: false);
            }
        }
        if (preferredVdd is not null && preferredVdd.Active)
        {
            return initialDisplays;
        }

        Exception? directActivationError = null;
        var activatedDisplays = initialDisplays;
        try
        {
            activatedDisplays = TryActivateInactiveVdd(initialDisplays, requestedMode);
        }
        catch (Exception ex)
        {
            directActivationError = ex;
        }

        if (FindVddDisplay(activatedDisplays, requireActive: true) is not null)
        {
            return activatedDisplays;
        }

        TryRunDisplaySwitchExtend();
        for (var attempt = 0; attempt < 10; attempt++)
        {
            Thread.Sleep(100);
            var displays = EnumerateDisplays();
            if (FindVddDisplay(displays, requireActive: true) is not null)
            {
                return displays;
            }
        }

        TryRunDisplaySwitchClone();
        for (var attempt = 0; attempt < 10; attempt++)
        {
            Thread.Sleep(100);
            var displays = EnumerateDisplays();
            if (FindVddDisplay(displays, requireActive: true) is not null)
            {
                return displays;
            }
        }

        try
        {
            if (isElevated && !_driverActionAttempted)
            {
                _driverActionAttempted = true;
                TryEnsureMttVddDeviceInstalled();
                TryRestartMttVddDevice();
                TryRefreshDisplayClassDevices();
            }
            activatedDisplays = TryActivateInactiveVdd(EnumerateDisplays(), requestedMode);
        }
        catch (Exception ex)
        {
            directActivationError ??= ex;
        }

        for (var attempt = 0; attempt < 10; attempt++)
        {
            Thread.Sleep(100);
            var displays = EnumerateDisplays();
            if (FindVddDisplay(displays, requireActive: true) is not null)
            {
                return displays;
            }
        }

        throw new InvalidOperationException(directActivationError is null
            ? "VDD display did not come online after duplicate recovery."
            : $"VDD display did not come online after duplicate recovery. Last activation error: {directActivationError.Message}");
    }

    private static void EnsureVddRegistryPath()
    {
        try
        {
            var configDirectory = Path.GetDirectoryName(VddSettingsPath);
            if (string.IsNullOrWhiteSpace(configDirectory))
            {
                return;
            }

            using var key = Registry.LocalMachine.CreateSubKey(@"SOFTWARE\MikeTheTech\VirtualDisplayDriver");
            key?.SetValue("VDDPATH", configDirectory, RegistryValueKind.String);
        }
        catch
        {
            // The driver still has its built-in fallback path; registry repair is best effort.
        }
    }

    private static List<DisplaySnapshot> TryActivateInactiveDisplay(
        List<DisplaySnapshot> displays,
        DisplaySnapshot targetDisplay,
        SavedDisplayState? requestedMode = null)
    {
        if (targetDisplay.Active)
        {
            return displays;
        }

        var activationState = BuildActivationStateForInactiveDisplay(targetDisplay, displays, requestedMode);
        EnableDisplay(targetDisplay, activationState);
        ApplyDisplayChanges();
        return EnumerateDisplays();
    }

    private static List<DisplaySnapshot> TryActivateInactiveVdd(
        List<DisplaySnapshot> displays,
        SavedDisplayState? requestedMode = null)
    {
        var inactiveVdd = FindVddDisplay(displays, requireActive: false);
        if (inactiveVdd is null)
        {
            return displays;
        }

        return TryActivateInactiveDisplay(displays, inactiveVdd, requestedMode);
    }

    private static SavedDisplayState BuildActivationStateForInactiveDisplay(
        DisplaySnapshot inactiveDisplay,
        IReadOnlyList<DisplaySnapshot> displays,
        SavedDisplayState? requestedMode = null)
    {
        var activationState = inactiveDisplay.ToSavedState();
        if (requestedMode is { Width: > 0, Height: > 0 })
        {
            activationState.Width = requestedMode.Width;
            activationState.Height = requestedMode.Height;
            activationState.Frequency = requestedMode.Frequency >= 30 ? requestedMode.Frequency : activationState.Frequency;
        }

        if (activationState.Width < 320)
        {
            activationState.Width = 1920;
        }

        if (activationState.Height < 180)
        {
            activationState.Height = 1080;
        }

        if (activationState.Frequency < 30)
        {
            activationState.Frequency = 60;
        }

        var rightEdge = displays
            .Where(display => display.Active)
            .Select(display => display.PositionX + Math.Max(display.Width, 1))
            .DefaultIfEmpty(0)
            .Max();

        activationState.PositionX = Math.Max(0, rightEdge);
        activationState.PositionY = 0;
        activationState.Primary = false;
        activationState.Active = true;
        return activationState;
    }

    private static void TryRunDisplaySwitchExtend()
    {
        try
        {
            using var process = Process.Start(new ProcessStartInfo
            {
                FileName = "DisplaySwitch.exe",
                Arguments = "/extend",
                UseShellExecute = false,
                CreateNoWindow = true,
            });
            process?.WaitForExit(5000);
        }
        catch
        {
            // Best effort.
        }
    }

    private static bool TryRunDisplaySwitchClone()
    {
        try
        {
            using var process = Process.Start(new ProcessStartInfo
            {
                FileName = "DisplaySwitch.exe",
                Arguments = "/clone",
                UseShellExecute = false,
                CreateNoWindow = true,
            });
            if (process is null)
            {
                return false;
            }
            process.WaitForExit(5000);
            Thread.Sleep(650);
            return process.ExitCode == 0;
        }
        catch
        {
            return false;
        }
    }

    private static bool StreamDisplayHasPrimaryAuthority(DisplaySnapshot streamDisplay) =>
        streamDisplay.Primary && streamDisplay.PositionX == 0 && streamDisplay.PositionY == 0;

    private static bool UsesMttVddDuplicateAuthority(DisplaySnapshot streamDisplay) =>
        DuplicateMttVddWithPrimary && DisplayLooksLikeMttVdd(streamDisplay);

    private static bool StreamDisplayAppearsDuplicatedWithPrimary(
        IReadOnlyList<DisplaySnapshot> currentDisplays,
        DisplaySnapshot streamDisplay)
    {
        var primaryDisplay = FindPrimaryDisplay(currentDisplays);
        if (primaryDisplay is null || !streamDisplay.Active)
        {
            return false;
        }

        if (IsSameDisplay(primaryDisplay, streamDisplay))
        {
            return true;
        }

        if (DisplayLooksLikeMttVdd(streamDisplay) && !DisplayIsVisibleOnDesktop(streamDisplay))
        {
            return true;
        }

        return primaryDisplay.Active &&
               primaryDisplay.PositionX == streamDisplay.PositionX &&
               primaryDisplay.PositionY == streamDisplay.PositionY &&
               Math.Abs(primaryDisplay.Width - streamDisplay.Width) <= 8 &&
               Math.Abs(primaryDisplay.Height - streamDisplay.Height) <= 8;
    }

    private static bool EnsureMttVddDuplicatedWithPrimary(
        ref List<DisplaySnapshot> currentDisplays,
        ref DisplaySnapshot streamDisplay,
        StreamDisplayPreference displayPreference,
        Func<List<DisplaySnapshot>, DisplaySnapshot?> resolveDisplay,
        string disappearanceReason)
    {
        if (!UsesMttVddDuplicateAuthority(streamDisplay) ||
            StreamDisplayAppearsDuplicatedWithPrimary(currentDisplays, streamDisplay))
        {
            return false;
        }

        if (FindPreferredNonMttCaptureDisplay(currentDisplays, streamDisplay) is not null)
        {
            // If a visible desktop display is already active, use it as the
            // Sunshine capture surface and leave MTT as a passive keep-awake
            // companion. Forcing clone/primary topology here can blank the VDD
            // or make Explorer move windows to an empty extended surface.
            return false;
        }

        var changed = EnsureNonVddPrimaryForDuplicate(
            ref currentDisplays,
            ref streamDisplay,
            resolveDisplay,
            disappearanceReason);
        var duplicateMode = ResolvePrimaryDuplicateMode(currentDisplays, streamDisplay);
        if (duplicateMode is null)
        {
            return changed;
        }

        var settingsChanged = EnsureConfiguredVddMode(
            duplicateMode.Width,
            duplicateMode.Height,
            (int)Math.Round(duplicateMode.Frequency));
        changed |= settingsChanged;
        if (settingsChanged)
        {
            TryRestartMttVddDevice();
            Thread.Sleep(750);
            currentDisplays = EnsureVddOnline(EnumerateDisplays(), duplicateMode);
            streamDisplay = resolveDisplay(currentDisplays)
                ?? FindPreferredStreamDisplay(currentDisplays, displayPreference, requireActive: true)
                ?? throw new InvalidOperationException(disappearanceReason);
        }

        var duplicateState = streamDisplay.ToSavedState();
        duplicateState.Width = duplicateMode.Width;
        duplicateState.Height = duplicateMode.Height;
        duplicateState.Frequency = duplicateMode.Frequency;
        duplicateState.PositionX = 0;
        duplicateState.PositionY = 0;
        duplicateState.Primary = false;
        duplicateState.Active = true;

        try
        {
            EnableDisplay(streamDisplay, duplicateState);
            ApplyDisplayChanges();
            changed = true;
        }
        catch (Exception ex) when (CanIgnoreEnableDisplayFailure(streamDisplay, duplicateState, ex))
        {
            // Some MTT VDD builds refuse overlap/clone-style topology changes.
            // Keep the VDD active as the stream companion and capture the real
            // primary display instead of failing the session or promoting VDD.
        }

        Thread.Sleep(500);
        currentDisplays = EnumerateDisplays();
        streamDisplay = resolveDisplay(currentDisplays)
            ?? FindPreferredStreamDisplay(currentDisplays, displayPreference, requireActive: true)
            ?? throw new InvalidOperationException(disappearanceReason);
        return changed;
    }

    private static bool EnsureNonVddPrimaryForDuplicate(
        ref List<DisplaySnapshot> currentDisplays,
        ref DisplaySnapshot streamDisplay,
        Func<List<DisplaySnapshot>, DisplaySnapshot?> resolveDisplay,
        string disappearanceReason)
    {
        var primaryDisplay = FindPrimaryDisplay(currentDisplays);
        if (primaryDisplay is not null &&
            !IsSameDisplay(primaryDisplay, streamDisplay) &&
            !DisplayLooksLikeMttVdd(primaryDisplay))
        {
            return false;
        }

        var changed = false;
        TryRunDisplaySwitchExtend();
        Thread.Sleep(650);
        currentDisplays = EnumerateDisplays();
        streamDisplay = resolveDisplay(currentDisplays)
            ?? throw new InvalidOperationException(disappearanceReason);
        primaryDisplay = FindPrimaryDisplay(currentDisplays);
        if (primaryDisplay is not null &&
            !IsSameDisplay(primaryDisplay, streamDisplay) &&
            !DisplayLooksLikeMttVdd(primaryDisplay))
        {
            return true;
        }

        var companionDisplay = FindDuplicateCompanionDisplay(currentDisplays, streamDisplay);
        if (companionDisplay is null)
        {
            return changed;
        }

        MakeDisplayPrimary(companionDisplay, currentDisplays, repositionOtherDisplays: false);
        Thread.Sleep(500);
        currentDisplays = EnumerateDisplays();
        streamDisplay = resolveDisplay(currentDisplays)
            ?? throw new InvalidOperationException(disappearanceReason);
        return true;
    }

    private static SavedDisplayState? ResolvePrimaryDuplicateMode(
        IReadOnlyList<DisplaySnapshot> currentDisplays,
        DisplaySnapshot streamDisplay)
    {
        var primaryDisplay = FindPrimaryDisplay(currentDisplays);
        if (primaryDisplay is null ||
            IsSameDisplay(primaryDisplay, streamDisplay) ||
            DisplayLooksLikeMttVdd(primaryDisplay))
        {
            primaryDisplay = FindDuplicateCompanionDisplay(currentDisplays, streamDisplay);
            if (primaryDisplay is null)
            {
                return null;
            }
        }

        return new SavedDisplayState
        {
            Width = Math.Max(320, primaryDisplay.Width),
            Height = Math.Max(180, primaryDisplay.Height),
            Frequency = primaryDisplay.Frequency >= 30 ? primaryDisplay.Frequency : 60,
        };
    }

    private static DisplaySnapshot? FindDuplicateCompanionDisplay(
        IEnumerable<DisplaySnapshot> currentDisplays,
        DisplaySnapshot streamDisplay) =>
        currentDisplays
            .Where(display => display.Active)
            .Where(display => !IsSameDisplay(display, streamDisplay))
            .Where(display => !DisplayLooksLikeMttVdd(display))
            .OrderByDescending(display => display.Primary)
            .ThenByDescending(IsQemuVirtioDisplay)
            .ThenBy(display => display.DisplayId)
            .FirstOrDefault();

    private static DisplaySnapshot? FindPreferredNonMttCaptureDisplay(
        IEnumerable<DisplaySnapshot> currentDisplays,
        DisplaySnapshot streamDisplay) =>
        currentDisplays
            .Where(display => display.Active)
            .Where(display => !IsSameDisplay(display, streamDisplay))
            .Where(display => !DisplayLooksLikeMttVdd(display))
            .OrderByDescending(display => display.Primary && display.PositionX == 0 && display.PositionY == 0)
            .ThenByDescending(display => display.Primary)
            .ThenByDescending(display => display.PositionX == 0 && display.PositionY == 0)
            .ThenByDescending(display => (long)Math.Max(1, display.Width) * Math.Max(1, display.Height))
            .ThenBy(display => display.DisplayId)
            .FirstOrDefault();

    private static bool DisplayMatchesRequestedMode(
        DisplaySnapshot display,
        SavedDisplayState requestedMode) =>
        display.Width == requestedMode.Width &&
        display.Height == requestedMode.Height &&
        (requestedMode.Frequency == 0 || display.Frequency == requestedMode.Frequency);

    private static bool ShouldRetryPrimaryPromotion(
        DisplaySnapshot streamDisplay,
        StreamDisplayPreference displayPreference,
        Exception failure)
    {
        if (!streamDisplay.IsVdd &&
            !PreferenceMayUseMttVdd(displayPreference) &&
            !(PreferenceTargetsParsecVdd(displayPreference) && IsParsecDisplay(streamDisplay)))
        {
            return false;
        }

        var lowered = failure.Message.ToLowerInvariant();
        return lowered.Contains("set primary using") ||
               lowered.Contains("authority was not applied") ||
               lowered.Contains("failed to read display mode") ||
               lowered.Contains("disp_change code -1") ||
               lowered.Contains("virtual display driver is not active");
    }

    private static List<DisplaySnapshot> TryCollapseDisplaysForPrimaryPromotionRecovery(
        List<DisplaySnapshot> currentDisplays,
        Func<List<DisplaySnapshot>, DisplaySnapshot?> resolveDisplay)
    {
        var streamDisplay = resolveDisplay(currentDisplays);
        if (streamDisplay is null ||
            !DisplayLooksLikeMttVdd(streamDisplay) ||
            UsesMttVddDuplicateAuthority(streamDisplay))
        {
            return currentDisplays;
        }

        var otherActiveDisplays = currentDisplays
            .Where(display => display.Active && !IsSameDisplay(display, streamDisplay))
            .ToList();
        if (otherActiveDisplays.Count == 0)
        {
            return currentDisplays;
        }

        try
        {
            DisableOtherActiveDisplaysForStreamAuthority(streamDisplay, currentDisplays);
            return EnumerateDisplays();
        }
        catch
        {
            return EnumerateDisplays();
        }
    }

    private static bool DisconnectOtherDisplaysBeforeMttPrimary(
        ref List<DisplaySnapshot> currentDisplays,
        ref DisplaySnapshot streamDisplay,
        StreamDisplayPreference displayPreference,
        Func<List<DisplaySnapshot>, DisplaySnapshot?> resolveDisplay,
        string disappearanceReason)
    {
        _ = displayPreference;
        if (!DisableOtherDisplaysDuringStream ||
            !DisplayLooksLikeMttVdd(streamDisplay) ||
            UsesMttVddDuplicateAuthority(streamDisplay))
        {
            return false;
        }

        var streamDisplayIdentity = streamDisplay;
        var otherActiveDisplays = currentDisplays
            .Where(display => display.Active && !IsSameDisplay(display, streamDisplayIdentity))
            .ToList();
        if (otherActiveDisplays.Count == 0)
        {
            return false;
        }

        DisableOtherActiveDisplaysForStreamAuthority(streamDisplay, currentDisplays);
        Thread.Sleep(350);
        currentDisplays = EnumerateDisplays();
        streamDisplay = resolveDisplay(currentDisplays)
            ?? throw new InvalidOperationException(disappearanceReason);
        return true;
    }

    private static List<DisplaySnapshot> RecoverDisplaysAfterPrimaryPromotionFailure(
        SavedDisplayState? requestedMode,
        StreamDisplayPreference displayPreference,
        int attempt,
        Func<List<DisplaySnapshot>, DisplaySnapshot?>? resolveDisplay = null)
    {
        Thread.Sleep(350 + (attempt * 250));

        List<DisplaySnapshot> recoveredDisplays;
        try
        {
            if (PreferenceMayUseMttVdd(displayPreference))
            {
                recoveredDisplays = EnsureVddOnline(EnumerateDisplays(), requestedMode);
            }
            else if (PreferenceTargetsParsecVdd(displayPreference))
            {
                recoveredDisplays = EnsureParsecVddOnline(EnumerateDisplays(), displayPreference);
            }
            else
            {
                recoveredDisplays = EnumerateDisplays();
            }
        }
        catch
        {
            // Best effort: if VDD recovery is still converging, continue with the
            // latest display snapshot so the next retry can re-resolve the target.
            recoveredDisplays = EnumerateDisplays();
        }

        recoveredDisplays = TryDetachCompetingVirtualDisplays(recoveredDisplays, displayPreference);

        if (resolveDisplay is not null)
        {
            recoveredDisplays = TryCollapseDisplaysForPrimaryPromotionRecovery(
                recoveredDisplays,
                resolveDisplay);
        }

        return recoveredDisplays;
    }

    private static void PromoteStreamDisplayToPrimaryWithRecovery(
        ref List<DisplaySnapshot> currentDisplays,
        ref DisplaySnapshot streamDisplay,
        SavedDisplayState? requestedMode,
        StreamDisplayPreference displayPreference,
        Func<List<DisplaySnapshot>, DisplaySnapshot?> resolveDisplay,
        string disappearanceReason,
        bool repositionOtherDisplays = true)
    {
        Exception? lastError = null;
        var maxAttempts = streamDisplay.IsVdd ||
            PreferenceMayUseMttVdd(displayPreference) ||
            (PreferenceTargetsParsecVdd(displayPreference) && IsParsecDisplay(streamDisplay))
                ? 4
                : 1;

        for (var attempt = 0; attempt < maxAttempts; attempt++)
        {
            try
            {
                MakeDisplayPrimary(streamDisplay, currentDisplays, repositionOtherDisplays, requestedMode);
                Thread.Sleep(250);
                currentDisplays = EnumerateDisplays();
                streamDisplay = resolveDisplay(currentDisplays)
                    ?? throw new InvalidOperationException(disappearanceReason);
                if (StreamDisplayHasPrimaryAuthority(streamDisplay))
                {
                    return;
                }

                var authorityFailure = new InvalidOperationException(
                    $"Stream display authority was not applied: {streamDisplay.DeviceName} primary={streamDisplay.Primary} position={streamDisplay.PositionX},{streamDisplay.PositionY}");
                if (attempt + 1 >= maxAttempts ||
                    !ShouldRetryPrimaryPromotion(streamDisplay, displayPreference, authorityFailure))
                {
                    lastError = authorityFailure;
                    break;
                }

                lastError = authorityFailure;
                currentDisplays = RecoverDisplaysAfterPrimaryPromotionFailure(
                    requestedMode,
                    displayPreference,
                    attempt,
                    resolveDisplay);
                streamDisplay = resolveDisplay(currentDisplays)
                    ?? throw new InvalidOperationException(disappearanceReason);
                if (StreamDisplayHasPrimaryAuthority(streamDisplay))
                {
                    return;
                }
                continue;
            }
            catch (Exception ex) when (attempt + 1 < maxAttempts &&
                ShouldRetryPrimaryPromotion(streamDisplay, displayPreference, ex))
            {
                lastError = ex;
                currentDisplays = RecoverDisplaysAfterPrimaryPromotionFailure(
                    requestedMode,
                    displayPreference,
                    attempt,
                    resolveDisplay);
                streamDisplay = resolveDisplay(currentDisplays)
                    ?? throw new InvalidOperationException(disappearanceReason);
                if (StreamDisplayHasPrimaryAuthority(streamDisplay))
                {
                    return;
                }
            }
            catch (Exception ex)
            {
                lastError = ex;
                break;
            }
        }

        throw lastError ?? new InvalidOperationException("Stream display primary promotion failed.");
    }

    private static DisplayModeApplyResult ApplyExactStreamDisplayModeWithRecovery(
        ref List<DisplaySnapshot> currentDisplays,
        ref DisplaySnapshot streamDisplay,
        SavedDisplayState requestedMode,
        StreamDisplayPreference displayPreference,
        Func<List<DisplaySnapshot>, DisplaySnapshot?> resolveDisplay,
        string disappearanceReason)
    {
        var maxAttempts = streamDisplay.IsVdd ||
            PreferenceMayUseMttVdd(displayPreference) ||
            (PreferenceTargetsParsecVdd(displayPreference) && IsParsecDisplay(streamDisplay))
                ? 3
                : 1;
        var lastResult = new DisplayModeApplyResult(false, true, false);

        for (var attempt = 0; attempt < maxAttempts; attempt++)
        {
            lastResult = SetExactDisplayMode(streamDisplay, requestedMode);
            if (lastResult.Applied && lastResult.RequiresApply)
            {
                ApplyDisplayChanges();
            }

            currentDisplays = EnumerateDisplays();
            streamDisplay = resolveDisplay(currentDisplays)
                ?? throw new InvalidOperationException(disappearanceReason);

            if (DisplayMatchesRequestedMode(streamDisplay, requestedMode) || attempt + 1 >= maxAttempts)
            {
                return lastResult;
            }

            currentDisplays = RecoverDisplaysAfterPrimaryPromotionFailure(
                requestedMode,
                displayPreference,
                attempt,
                resolveDisplay);
            streamDisplay = resolveDisplay(currentDisplays)
                ?? throw new InvalidOperationException(disappearanceReason);
        }

        return lastResult;
    }

    private static bool ReassertPreparedStreamDisplayAuthority(
        ref List<DisplaySnapshot> currentDisplays,
        ref DisplaySnapshot streamDisplay,
        StreamDisplayPreference displayPreference,
        string disappearanceReason)
    {
        if (UsesMttVddDuplicateAuthority(streamDisplay))
        {
            return false;
        }

        if (StreamDisplayHasPrimaryAuthority(streamDisplay))
        {
            return false;
        }

        try
        {
            var streamDisplayIdentity = streamDisplay.ToSavedState();
            PromoteStreamDisplayToPrimaryWithRecovery(
                ref currentDisplays,
                ref streamDisplay,
                requestedMode: null,
                displayPreference,
                displays => FindDisplay(displays, streamDisplayIdentity)
                    ?? FindPreferredStreamDisplay(displays, displayPreference, requireActive: true),
                disappearanceReason,
                repositionOtherDisplays: false);
        }
        catch
        {
            return false;
        }
        if (!StreamDisplayHasPrimaryAuthority(streamDisplay) && DisableOtherDisplaysWhenStreamAuthorityFails)
        {
            DisableOtherActiveDisplaysForStreamAuthority(streamDisplay, currentDisplays);
            currentDisplays = EnumerateDisplays();
            streamDisplay = ResolvePreparedStreamDisplay(currentDisplays, displayPreference, streamDisplay)
                ?? throw new InvalidOperationException(disappearanceReason);
            if (!StreamDisplayHasPrimaryAuthority(streamDisplay))
            {
                MakeDisplayPrimary(streamDisplay, currentDisplays, repositionOtherDisplays: false);
                currentDisplays = EnumerateDisplays();
                streamDisplay = ResolvePreparedStreamDisplay(currentDisplays, displayPreference, streamDisplay)
                    ?? throw new InvalidOperationException(disappearanceReason);
            }
        }
        return StreamDisplayHasPrimaryAuthority(streamDisplay);
    }

    private static bool ReassertResizeStreamDisplayAuthority(
        ref List<DisplaySnapshot> currentDisplays,
        ref DisplaySnapshot streamDisplay,
        StreamDisplayPreference displayPreference,
        PrepareStateFile? state,
        string disappearanceReason)
    {
        if (UsesMttVddDuplicateAuthority(streamDisplay))
        {
            return false;
        }

        if (StreamDisplayHasPrimaryAuthority(streamDisplay))
        {
            return false;
        }

        try
        {
            var streamDisplayIdentity = streamDisplay.ToSavedState();
            PromoteStreamDisplayToPrimaryWithRecovery(
                ref currentDisplays,
                ref streamDisplay,
                state?.Requested,
                displayPreference,
                displays => FindDisplay(displays, streamDisplayIdentity)
                    ?? ResolveStreamDisplayForResize(displays, displayPreference, state),
                disappearanceReason,
                repositionOtherDisplays: false);
        }
        catch
        {
            return false;
        }
        if (!StreamDisplayHasPrimaryAuthority(streamDisplay) && DisableOtherDisplaysWhenStreamAuthorityFails)
        {
            DisableOtherActiveDisplaysForStreamAuthority(streamDisplay, currentDisplays);
            currentDisplays = EnumerateDisplays();
            streamDisplay = ResolveStreamDisplayForResize(currentDisplays, displayPreference, state)
                ?? throw new InvalidOperationException(disappearanceReason);
            if (!StreamDisplayHasPrimaryAuthority(streamDisplay))
            {
                MakeDisplayPrimary(streamDisplay, currentDisplays, repositionOtherDisplays: false);
                currentDisplays = EnumerateDisplays();
                streamDisplay = ResolveStreamDisplayForResize(currentDisplays, displayPreference, state)
                    ?? throw new InvalidOperationException(disappearanceReason);
            }
        }
        return StreamDisplayHasPrimaryAuthority(streamDisplay);
    }

    private static void DisableOtherActiveDisplaysForStreamAuthority(
        DisplaySnapshot streamDisplay,
        IReadOnlyList<DisplaySnapshot> currentDisplays)
    {
        if (!DisplayLooksLikeMttVdd(streamDisplay))
        {
            return;
        }

        var candidates = currentDisplays
            .Where(display =>
                display.Active &&
                !IsSameDisplay(display, streamDisplay) &&
                !DisplayLooksLikeMttVdd(display))
            .ToList();
        if (candidates.Count == 0)
        {
            return;
        }

        var changed = false;
        var failures = new List<string>();
        foreach (var display in candidates)
        {
            try
            {
                DetachDisplayFromDesktop(display);
                changed = true;
            }
            catch (Exception ex) when (CanIgnoreDisableDisplayFailure(display, ex))
            {
                // Some virtual adapters reject a desktop detach while their
                // service is still connected. Keep preparing the MTT VDD lane.
            }
            catch (Exception ex)
            {
                failures.Add($"{display.DeviceName}:{CompactProcessOutput(ex.Message)}");
            }
        }

        if (changed)
        {
            ApplyDisplayChanges();
            Thread.Sleep(650);
        }

        if (failures.Count > 0)
        {
            throw new InvalidOperationException(
                $"Tidak semua display non-MTT bisa di-disconnect dari desktop: {string.Join("; ", failures)}");
        }
    }

    private static void EnsureStreamDisplayHasPrimaryAuthority(DisplaySnapshot streamDisplay)
    {
        if (StreamDisplayHasPrimaryAuthority(streamDisplay))
        {
            return;
        }

        throw new InvalidOperationException(
            $"Stream display authority was not applied: {streamDisplay.DeviceName} primary={streamDisplay.Primary} position={streamDisplay.PositionX},{streamDisplay.PositionY}");
    }

    private static void MakeDisplayPrimary(
        DisplaySnapshot targetDisplay,
        IReadOnlyList<DisplaySnapshot> displays,
        bool repositionOtherDisplays = true,
        SavedDisplayState? targetOverride = null)
    {
        var targetWidth = Math.Max(320, targetOverride?.Width ?? targetDisplay.Width);
        var targetHeight = Math.Max(180, targetOverride?.Height ?? targetDisplay.Height);
        var targetFrequency = targetOverride?.Frequency >= 30
            ? (int)Math.Round(targetOverride.Frequency)
            : targetDisplay.Frequency;

        var shouldDetachOthers = DisableOtherDisplaysDuringStream && DisplayLooksLikeMttVdd(targetDisplay);

        if (shouldDetachOthers)
        {
            foreach (var display in displays.Where(item => item.Active && !string.Equals(item.DeviceName, targetDisplay.DeviceName, StringComparison.OrdinalIgnoreCase) && !DisplayLooksLikeMttVdd(item)))
            {
                try
                {
                    var mode = GetCurrentModeOrDefault(display.DeviceName, allowRegistryFallback: true);
                    mode.dmFields = DmPosition | DmPelsWidth | DmPelsHeight;
                    mode.dmPositionX = display.PositionX;
                    mode.dmPositionY = display.PositionY;
                    mode.dmPelsWidth = 0;
                    mode.dmPelsHeight = 0;

                    NativeMethods.ChangeDisplaySettingsEx(
                        display.DeviceName,
                        ref mode,
                        IntPtr.Zero,
                        NativeMethods.ChangeDisplaySettingsFlags.CDS_UPDATEREGISTRY |
                        NativeMethods.ChangeDisplaySettingsFlags.CDS_NORESET,
                        IntPtr.Zero
                    );
                }
                catch
                {
                    // Keep the primary transaction alive even if an unrelated adapter rejects detaching
                }
            }
        }
        else if (repositionOtherDisplays && RepositionOtherDisplaysWhenMakingPrimary)
        {
            var nextX = Math.Max(1, targetWidth);
            foreach (var display in displays.Where(item => item.Active && !string.Equals(item.DeviceName, targetDisplay.DeviceName, StringComparison.OrdinalIgnoreCase)))
            {
                var mode = GetCurrentMode(display.DeviceName);
                mode.dmFields = DmPosition | DmPelsWidth | DmPelsHeight;
                mode.dmPositionX = nextX;
                mode.dmPositionY = 0;
                mode.dmPelsWidth = display.Width;
                mode.dmPelsHeight = display.Height;
                if (display.Frequency > 0)
                {
                    mode.dmFields |= DmDisplayFrequency;
                    mode.dmDisplayFrequency = display.Frequency;
                }

                try
                {
                    EnsureDisplayResult(
                        NativeMethods.ChangeDisplaySettingsEx(
                            display.DeviceName,
                            ref mode,
                            IntPtr.Zero,
                            NativeMethods.ChangeDisplaySettingsFlags.CDS_UPDATEREGISTRY |
                            NativeMethods.ChangeDisplaySettingsFlags.CDS_NORESET,
                            IntPtr.Zero
                        ),
                        $"reposition non-primary display {display.DeviceName}"
                    );
                }
                catch
                {
                    // Keep the primary transaction alive even if an unrelated
                    // adapter rejects repositioning.
                }

                nextX += Math.Max(1, display.Width);
            }
        }

        var targetMode = GetCurrentModeOrDefault(targetDisplay.DeviceName, allowRegistryFallback: true);
        targetMode.dmFields = DmPosition | DmPelsWidth | DmPelsHeight;
        targetMode.dmPositionX = 0;
        targetMode.dmPositionY = 0;
        targetMode.dmPelsWidth = targetWidth;
        targetMode.dmPelsHeight = targetHeight;
        if (targetFrequency > 0)
        {
            targetMode.dmFields |= DmDisplayFrequency;
            targetMode.dmDisplayFrequency = targetFrequency;
        }

        EnsureDisplayResult(
            NativeMethods.ChangeDisplaySettingsEx(
                targetDisplay.DeviceName,
                ref targetMode,
                IntPtr.Zero,
                NativeMethods.ChangeDisplaySettingsFlags.CDS_UPDATEREGISTRY |
                NativeMethods.ChangeDisplaySettingsFlags.CDS_NORESET |
                NativeMethods.ChangeDisplaySettingsFlags.CDS_SET_PRIMARY,
                IntPtr.Zero
            ),
            $"set primary using {targetDisplay.DeviceName}"
        );

        ApplyDisplayChanges();
    }

    private static void DisableDisplay(DisplaySnapshot display)
    {
        // Kept for older call paths, but intentionally inert. Display disable
        // policy is no longer used by Cloudgime Host.
    }

    private static void EnableDisplay(DisplaySnapshot currentDisplay, SavedDisplayState savedDisplay)
    {
        var mode = GetCurrentModeOrDefault(currentDisplay.DeviceName, allowRegistryFallback: true);
        mode.dmFields = DmPosition | DmPelsWidth | DmPelsHeight;
        mode.dmPositionX = savedDisplay.PositionX;
        mode.dmPositionY = savedDisplay.PositionY;
        mode.dmPelsWidth = Math.Max(320, savedDisplay.Width);
        mode.dmPelsHeight = Math.Max(180, savedDisplay.Height);
        if (savedDisplay.Frequency >= 30)
        {
            mode.dmFields |= DmDisplayFrequency;
            mode.dmDisplayFrequency = (int)Math.Round(savedDisplay.Frequency);
        }

        var flags = NativeMethods.ChangeDisplaySettingsFlags.CDS_UPDATEREGISTRY |
                    NativeMethods.ChangeDisplaySettingsFlags.CDS_NORESET;
        if (savedDisplay.Primary)
        {
            flags |= NativeMethods.ChangeDisplaySettingsFlags.CDS_SET_PRIMARY;
        }

        EnsureDisplayResult(
            NativeMethods.ChangeDisplaySettingsEx(currentDisplay.DeviceName, ref mode, IntPtr.Zero, flags, IntPtr.Zero),
            $"enable display {currentDisplay.DeviceName}"
        );
    }

    private static SavedDisplayState BuildReportedAppliedMode(
        DisplaySnapshot display,
        SavedDisplayState? requestedMode,
        bool exactModeFallback)
    {
        if (exactModeFallback &&
            requestedMode is not null &&
            DisplayCanContainRequestedStreamSurface(display, requestedMode))
        {
            return new SavedDisplayState
            {
                Width = requestedMode.Width,
                Height = requestedMode.Height,
                Frequency = requestedMode.Frequency > 0 ? requestedMode.Frequency : display.Frequency,
            };
        }

        return display.ToSavedState();
    }

    private static bool DisplayCanContainRequestedStreamSurface(
        DisplaySnapshot display,
        SavedDisplayState requestedMode)
    {
        if (display.Width <= 0 || display.Height <= 0 || requestedMode.Width <= 0 || requestedMode.Height <= 0)
        {
            return false;
        }

        var sameOrientation = (display.Width >= display.Height) == (requestedMode.Width >= requestedMode.Height);
        return sameOrientation &&
               display.Width >= requestedMode.Width &&
               display.Height >= requestedMode.Height;
    }

    private static DisplayModeApplyResult SetExactDisplayMode(DisplaySnapshot display, SavedDisplayState requestedMode)
    {
        var mode = GetCurrentMode(display.DeviceName, allowRegistryFallback: true);
        var result = TryApplyDisplayMode(display, requestedMode, mode);

        if (result.Result == DispChangeSuccessful)
        {
            return new DisplayModeApplyResult(true, false, result.RequiresApply);
        }

        var requestedWithoutFrequency = new SavedDisplayState
        {
            Width = requestedMode.Width,
            Height = requestedMode.Height,
            Frequency = 0,
        };
        result = TryApplyDisplayMode(display, requestedWithoutFrequency, mode);

        if (result.Result != DispChangeSuccessful)
        {
            foreach (var compatibleMode in SelectCompatibleFallbackDisplayModes(display.DeviceName, requestedMode))
            {
                result = TryApplyDisplayMode(display, compatibleMode, mode);

                if (result.Result == DispChangeSuccessful)
                {
                    return new DisplayModeApplyResult(true, true, result.RequiresApply);
                }
            }

            return new DisplayModeApplyResult(false, true, false);
        }

        return new DisplayModeApplyResult(true, false, result.RequiresApply);
    }

    private static DisplayModeAttempt TryApplyDisplayMode(
        DisplaySnapshot display,
        SavedDisplayState requestedMode,
        NativeMethods.DEVMODE baseMode)
    {
        var currentOrientation = NormalizeOrientation(baseMode.dmDisplayOrientation);
        var targetOrientation = DetermineRequestedOrientation(currentOrientation, requestedMode.Width, requestedMode.Height);
        var preferOrientationFallback = RequiresQuarterTurn(currentOrientation, targetOrientation);
        var attempts = new List<(bool UseOrientationFallback, bool SwapForOrientationFallback)>
        {
            (false, false),
        };
        if (preferOrientationFallback)
        {
            attempts.Add((true, false));
            attempts.Add((true, true));
        }
        else
        {
            attempts.Add((true, false));
            if (currentOrientation is Dmdo90 or Dmdo270)
            {
                attempts.Add((true, true));
            }
        }

        var lastResult = DispChangeBadMode;
        foreach (var attempt in attempts)
        {
            var immediateResult = TryApplyExactDisplayMode(
                display,
                requestedMode,
                baseMode,
                attempt.UseOrientationFallback,
                attempt.SwapForOrientationFallback,
                NativeMethods.ChangeDisplaySettingsFlags.CDS_NONE);
            if (immediateResult == DispChangeSuccessful)
            {
                return new DisplayModeAttempt(immediateResult, false);
            }

            lastResult = immediateResult;
        }

        foreach (var attempt in attempts)
        {
            var stagedResult = TryApplyExactDisplayMode(
                display,
                requestedMode,
                baseMode,
                attempt.UseOrientationFallback,
                attempt.SwapForOrientationFallback,
                NativeMethods.ChangeDisplaySettingsFlags.CDS_UPDATEREGISTRY |
                NativeMethods.ChangeDisplaySettingsFlags.CDS_NORESET);
            if (stagedResult == DispChangeSuccessful)
            {
                return new DisplayModeAttempt(stagedResult, true);
            }

            lastResult = stagedResult;
        }

        return new DisplayModeAttempt(lastResult, false);
    }

    private static List<SavedDisplayState> SelectCompatibleFallbackDisplayModes(string deviceName, SavedDisplayState requestedMode)
    {
        var nativeModes = EnumerateAvailableDisplayModes(deviceName);
        var modes = nativeModes
            .Concat(nativeModes.Select(mode => new SavedDisplayState
            {
                Width = mode.Height,
                Height = mode.Width,
                Frequency = mode.Frequency,
            }))
            .Where(mode => mode.Width > 0 && mode.Height > 0)
            .GroupBy(mode => $"{mode.Width}x{mode.Height}@{mode.Frequency}")
            .Select(group => group.First())
            .ToList();
        if (modes.Count == 0)
        {
            return [];
        }

        var requestedLandscape = requestedMode.Width >= requestedMode.Height;
        var requestedAspect = requestedMode.Width / (double)Math.Max(1, requestedMode.Height);
        var requestedArea = Math.Max(1d, requestedMode.Width * requestedMode.Height);
        var orientationMatches = modes
            .Where(mode => (mode.Width >= mode.Height) == requestedLandscape)
            .ToList();
        var candidates = orientationMatches.Count > 0 ? orientationMatches : modes;
        var containingCandidates = candidates
            .Where(mode => mode.Width >= requestedMode.Width && mode.Height >= requestedMode.Height)
            .ToList();
        if (containingCandidates.Count > 0)
        {
            candidates = containingCandidates;
        }
        else
        {
            var boundedCandidates = candidates
                .Where(mode => mode.Width <= requestedMode.Width && mode.Height <= requestedMode.Height)
                .ToList();
            if (boundedCandidates.Count > 0)
            {
                candidates = boundedCandidates;
            }
        }

        return candidates
            .Select(mode =>
            {
                var modeAspect = mode.Width / (double)Math.Max(1, mode.Height);
                var modeArea = Math.Max(1d, mode.Width * mode.Height);
                var aspectDelta = Math.Abs(modeAspect - requestedAspect);
                var areaScaleDelta = Math.Abs(Math.Log(modeArea / requestedArea));
                var undersizedPenalty = modeArea < requestedArea ? 0.08d : 0d;
                var frequencyDelta = requestedMode.Frequency >= 30
                    ? Math.Abs(mode.Frequency - requestedMode.Frequency) / 240d
                    : 0d;
                var exactRejectedCandidatePenalty =
                    mode.Width == requestedMode.Width &&
                    mode.Height == requestedMode.Height &&
                    (requestedMode.Frequency <= 0 || mode.Frequency == requestedMode.Frequency)
                        ? 10d
                        : 0d;
                var score = (aspectDelta * 24d) + (areaScaleDelta * 0.5d) + undersizedPenalty + frequencyDelta + exactRejectedCandidatePenalty;
                return new { Mode = mode, Score = score, AreaScaleDelta = areaScaleDelta, AspectDelta = aspectDelta };
            })
            .OrderBy(candidate => candidate.Score)
            .ThenBy(candidate => candidate.AreaScaleDelta)
            .ThenBy(candidate => candidate.AspectDelta)
            .Select(candidate => candidate.Mode)
            .ToList();
    }

    private static List<SavedDisplayState> EnumerateAvailableDisplayModes(string deviceName)
    {
        var modes = new List<SavedDisplayState>();
        for (var index = 0; ; index++)
        {
            var mode = NativeMethods.CreateDevMode();
            if (!NativeMethods.EnumDisplaySettingsEx(deviceName, index, ref mode, 0))
            {
                break;
            }

            if (mode.dmPelsWidth <= 0 || mode.dmPelsHeight <= 0)
            {
                continue;
            }

            modes.Add(new SavedDisplayState
            {
                Width = mode.dmPelsWidth,
                Height = mode.dmPelsHeight,
                Frequency = mode.dmDisplayFrequency,
            });
        }

        return modes
            .GroupBy(mode => $"{mode.Width}x{mode.Height}@{mode.Frequency}")
            .Select(group => group.First())
            .ToList();
    }

    private static int TryApplyExactDisplayMode(
        DisplaySnapshot display,
        SavedDisplayState requestedMode,
        NativeMethods.DEVMODE baseMode,
        bool useOrientationFallback,
        bool swapForOrientationFallback,
        NativeMethods.ChangeDisplaySettingsFlags flags)
    {
        var mode = baseMode;
        mode.dmFields = DmPosition | DmPelsWidth | DmPelsHeight;
        if (baseMode.dmBitsPerPel > 0)
        {
            mode.dmFields |= DmBitsPerPel;
            mode.dmBitsPerPel = baseMode.dmBitsPerPel;
        }

        mode.dmPositionX = display.PositionX;
        mode.dmPositionY = display.PositionY;

        var targetWidth = requestedMode.Width;
        var targetHeight = requestedMode.Height;

        if (useOrientationFallback)
        {
            var currentOrientation = NormalizeOrientation(baseMode.dmDisplayOrientation);
            var targetOrientation = DetermineRequestedOrientation(currentOrientation, requestedMode.Width, requestedMode.Height);
            if (swapForOrientationFallback && targetOrientation is Dmdo90 or Dmdo270)
            {
                (targetWidth, targetHeight) = (targetHeight, targetWidth);
            }

            mode.dmFields |= DmDisplayOrientation;
            mode.dmDisplayOrientation = targetOrientation;
        }

        mode.dmPelsWidth = targetWidth;
        mode.dmPelsHeight = targetHeight;

        if (requestedMode.Frequency >= 30)
        {
            mode.dmFields |= DmDisplayFrequency;
            mode.dmDisplayFrequency = (int)Math.Round(requestedMode.Frequency);
        }

        return NativeMethods.ChangeDisplaySettingsEx(
            display.DeviceName,
            ref mode,
            IntPtr.Zero,
            flags,
            IntPtr.Zero
        );
    }

    private static int NormalizeOrientation(int orientation) =>
        orientation switch
        {
            DmdoDefault or Dmdo90 or Dmdo180 or Dmdo270 => orientation,
            _ => DmdoDefault,
        };

    private static int DetermineRequestedOrientation(int currentOrientation, int requestedWidth, int requestedHeight)
    {
        var requestedLandscape = requestedWidth >= requestedHeight;
        var currentLandscape = currentOrientation is DmdoDefault or Dmdo180;
        if (requestedLandscape == currentLandscape)
        {
            return currentOrientation;
        }

        return currentOrientation switch
        {
            DmdoDefault => Dmdo90,
            Dmdo90 => DmdoDefault,
            Dmdo180 => Dmdo270,
            Dmdo270 => Dmdo180,
            _ => requestedLandscape ? DmdoDefault : Dmdo90,
        };
    }

    private static bool RequiresQuarterTurn(int currentOrientation, int targetOrientation)
    {
        var currentLandscape = currentOrientation is DmdoDefault or Dmdo180;
        var targetLandscape = targetOrientation is DmdoDefault or Dmdo180;
        return currentLandscape != targetLandscape;
    }

    private static void ApplyDisplayChanges() =>
        EnsureDisplayResult(NativeMethods.ChangeDisplaySettingsEx(null, IntPtr.Zero, IntPtr.Zero, 0, IntPtr.Zero), "apply display changes");

    private static CursorState? CenterCursor(DisplaySnapshot display)
    {
        if (!display.Active || display.Width <= 0 || display.Height <= 0)
        {
            return null;
        }

        var x = display.PositionX + (display.Width / 2);
        var y = display.PositionY + (display.Height / 2);
        if (!NativeMethods.SetCursorPos(x, y))
        {
            return null;
        }

        // Force a tiny visible desktop change so capture pipelines produce an
        // initial frame even right after a display-mode transition.
        var jiggleX = Math.Min(display.PositionX + display.Width - 2, Math.Max(display.PositionX + 1, x + 12));
        var jiggleY = Math.Min(display.PositionY + display.Height - 2, Math.Max(display.PositionY + 1, y + 12));
        if (jiggleX != x || jiggleY != y)
        {
            Thread.Sleep(25);
            NativeMethods.SetCursorPos(jiggleX, jiggleY);
            Thread.Sleep(25);
            NativeMethods.SetCursorPos(x, y);
        }

        return new CursorState { X = x, Y = y };
    }

    private static bool HideSystemCursor()
    {
        for (var i = 0; i < 32; i++)
        {
            if (NativeMethods.ShowCursor(false) < 0)
            {
                return true;
            }
        }

        return false;
    }

    private static void ShowSystemCursor()
    {
        for (var i = 0; i < 32; i++)
        {
            if (NativeMethods.ShowCursor(true) >= 0)
            {
                return;
            }
        }
    }

    private static CursorState? GetCursorState() =>
        NativeMethods.GetCursorPos(out var point)
            ? new CursorState { X = point.X, Y = point.Y }
            : null;

    private static HelperResult BuildDisplayControlResult(
        bool changed,
        bool skipped,
        string reason,
        StreamDisplayPreference preference)
    {
        var displays = EnumerateDisplays();
        var selectedDisplay = FindPreferredStreamDisplay(displays, preference, requireActive: false);
        var activeTarget = FindPreferredStreamDisplay(displays, preference, requireActive: true)
            ?? FindPrimaryDisplay(displays);

        return new HelperResult
        {
            Ok = true,
            Changed = changed,
            Restored = false,
            Skipped = skipped,
            Reason = reason,
            StreamDisplayPreference = ToDisplayControlPreferenceInfo(preference),
            SelectedDisplayLabel = selectedDisplay is null ? BuildPreferenceFallbackLabel(preference) : BuildDisplayChoiceLabel(selectedDisplay),
            ActiveDisplayLabel = activeTarget is null ? "Tidak ada display aktif" : BuildDisplayChoiceLabel(activeTarget),
            Displays = displays
                .OrderByDescending(display => display.Active)
                .ThenByDescending(display => display.Primary)
                .ThenBy(display => display.StreamDisplayPriority)
                .ThenBy(display => display.DisplayId)
                .Select(display => new DisplayControlDisplayInfo
                {
                    DisplayId = display.DisplayId,
                    DeviceName = display.DeviceName,
                    DeviceId = display.DeviceId,
                    DeviceString = display.DeviceString,
                    Label = BuildDisplayChoiceLabel(display),
                    Width = display.Width,
                    Height = display.Height,
                    Frequency = display.Frequency,
                    Active = display.Active,
                    Primary = display.Primary,
                    IsVirtualDisplay = display.IsVdd || IsQemuVirtioDisplay(display) || IsParsecDisplay(display),
                    IsMttVdd = display.IsVdd,
                    SelectedPreference = selectedDisplay is not null && IsSameDisplay(display, selectedDisplay),
                    CurrentStreamTarget = activeTarget is not null && IsSameDisplay(display, activeTarget),
                })
                .ToList(),
        };
    }

    private static DisplayControlPreferenceInfo ToDisplayControlPreferenceInfo(StreamDisplayPreference preference) => new()
    {
        Mode = NormalizeStreamDisplayMode(preference.Mode),
        ManualOverride = preference.ManualOverride,
        CustomDeviceName = preference.CustomDeviceName,
        CustomDeviceId = preference.CustomDeviceId,
        CustomLabel = preference.CustomLabel,
    };

    private static string BuildPreferenceFallbackLabel(StreamDisplayPreference preference)
    {
        var mode = NormalizeStreamDisplayMode(preference.Mode);
        return mode switch
        {
            "mtt_vdd" => "Cloud Display (MTT VDD)",
            "primary" => "Display utama host",
            "custom" when !string.IsNullOrWhiteSpace(preference.CustomLabel) => preference.CustomLabel.Trim(),
            "custom" when !string.IsNullOrWhiteSpace(preference.CustomDeviceName) => preference.CustomDeviceName.Trim(),
            _ => "Belum dipilih",
        };
    }

    private static string BuildDisplayChoiceLabel(DisplaySnapshot display)
    {
        var flags = new List<string>();
        if (display.IsVdd)
        {
            flags.Add("Cloud");
        }

        if (display.Primary)
        {
            flags.Add("Primary");
        }

        flags.Add(display.Active ? "Active" : "Detached");

        if (IsParsecDisplay(display))
        {
            flags.Add("Parsec");
        }
        else if (IsQemuVirtioDisplay(display))
        {
            flags.Add("Virtio");
        }

        var descriptor = string.IsNullOrWhiteSpace(display.DeviceString)
            ? display.DeviceName
            : display.DeviceString;
        var modeText = display.Width > 0 && display.Height > 0
            ? $"{display.Width}x{display.Height}"
            : "mode tidak diketahui";
        if (display.Frequency > 0)
        {
            modeText = $"{modeText} @{display.Frequency}Hz";
        }

        return $"{descriptor} · {modeText} · {string.Join(" · ", flags)}";
    }

    private static StreamDisplayPreference ReadStreamDisplayPreference(string bundleRoot)
    {
        var preference = new StreamDisplayPreference();
        foreach (var path in ResolveStreamDisplayPreferencePaths(bundleRoot))
        {
            try
            {
                if (!File.Exists(path))
                {
                    continue;
                }

                var loaded = JsonSerializer.Deserialize<StreamDisplayPreference>(File.ReadAllText(path), JsonOptions);
                if (loaded is not null)
                {
                    preference = loaded;
                    break;
                }
            }
            catch
            {
                // Invalid preference files should not prevent a stream from starting.
            }
        }

        preference.Mode = NormalizeStreamDisplayMode(preference.Mode);
        preference.CustomDeviceName = preference.CustomDeviceName.Trim();
        preference.CustomDeviceId = preference.CustomDeviceId.Trim();
        preference.CustomLabel = preference.CustomLabel.Trim();
        if (!preference.ManualOverride && preference.Mode != "mtt_vdd")
        {
            preference.Mode = "mtt_vdd";
            preference.CustomDeviceName = string.Empty;
            preference.CustomDeviceId = string.Empty;
            preference.CustomLabel = string.Empty;
        }

        if (string.IsNullOrWhiteSpace(preference.Mode))
        {
            preference.Mode = "mtt_vdd";
        }

        return preference;
    }

    private static void WriteStreamDisplayPreference(string bundleRoot, StreamDisplayPreference preference)
    {
        var path = ResolveWritableStreamDisplayPreferencePath(bundleRoot);
        var normalized = new StreamDisplayPreference
        {
            SchemaVersion = Math.Max(1, preference.SchemaVersion),
            ManualOverride = preference.ManualOverride,
            Mode = NormalizeStreamDisplayMode(preference.Mode),
            CustomDeviceName = preference.CustomDeviceName.Trim(),
            CustomDeviceId = preference.CustomDeviceId.Trim(),
            CustomLabel = preference.CustomLabel.Trim(),
        };
        if (!normalized.ManualOverride && normalized.Mode != "mtt_vdd")
        {
            normalized.Mode = "mtt_vdd";
        }
        if (!normalized.ManualOverride || normalized.Mode != "custom")
        {
            normalized.CustomDeviceName = normalized.Mode == "custom" ? normalized.CustomDeviceName : string.Empty;
            normalized.CustomDeviceId = normalized.Mode == "custom" ? normalized.CustomDeviceId : string.Empty;
            normalized.CustomLabel = normalized.Mode == "custom" ? normalized.CustomLabel : string.Empty;
        }

        var parent = Path.GetDirectoryName(path);
        if (!string.IsNullOrWhiteSpace(parent))
        {
            Directory.CreateDirectory(parent);
        }

        File.WriteAllText(path, JsonSerializer.Serialize(normalized, JsonOptions));
    }

    private static IEnumerable<string> ResolveStreamDisplayPreferencePaths(string bundleRoot)
    {
        var roots = new List<string>();
        if (!string.IsNullOrWhiteSpace(bundleRoot))
        {
            roots.Add(Path.GetFullPath(bundleRoot));
        }

        var helperBundleRoot = ResolveBundleRootFromHelper();
        if (!string.IsNullOrWhiteSpace(helperBundleRoot))
        {
            roots.Add(helperBundleRoot);
        }

        foreach (var root in roots.Distinct(StringComparer.OrdinalIgnoreCase))
        {
            yield return Path.Combine(root, "moonlight", "server", StreamDisplayPreferencesFileName);
        }

        yield return Path.Combine(AppContext.BaseDirectory, StreamDisplayPreferencesFileName);
    }

    private static string ResolveWritableStreamDisplayPreferencePath(string bundleRoot)
    {
        foreach (var path in ResolveStreamDisplayPreferencePaths(bundleRoot))
        {
            if (!string.IsNullOrWhiteSpace(path))
            {
                return path;
            }
        }

        throw new InvalidOperationException("stream display preference path is not available");
    }

    private static string NormalizeStreamDisplayMode(string? mode)
    {
        var normalized = (mode ?? string.Empty).Trim().ToLowerInvariant().Replace('-', '_');
        return normalized switch
        {
            "auto" => "auto",
            "mtt" or "mtt_vdd" or "vdd" or "virtual_display" => "mtt_vdd",
            "qemu" or "virtio" or "qemu_virtio" => "qemu_virtio",
            // Parsec may stay installed for unrelated apps, but Cloudgime stream
            // must always route back to MTT VDD.
            "parsec" or "parsec_vda" => "mtt_vdd",
            "primary" or "current_primary" => "primary",
            "custom" or "device" => "custom",
            _ => "mtt_vdd",
        };
    }

    private static bool PreferenceMayUseMttVdd(StreamDisplayPreference preference)
    {
        var mode = NormalizeStreamDisplayMode(preference.Mode);
        if (mode is "mtt_vdd" or "auto")
        {
            return true;
        }

        var custom = $"{preference.CustomDeviceName} {preference.CustomDeviceId}";
        return custom.Contains("mtt", StringComparison.OrdinalIgnoreCase)
            || custom.Contains("vdd", StringComparison.OrdinalIgnoreCase)
            || custom.Contains("MTT1337", StringComparison.OrdinalIgnoreCase)
            || TextLooksLikeCloudgimeVirtualDisplay(custom);
    }

    private static bool PreferenceTargetsParsecVdd(StreamDisplayPreference preference)
    {
        var mode = NormalizeStreamDisplayMode(preference.Mode);
        if (mode is "parsec_vda")
        {
            return true;
        }

        var custom = $"{preference.CustomDeviceName} {preference.CustomDeviceId}";
        return custom.Contains("parsec", StringComparison.OrdinalIgnoreCase)
            || custom.Contains("PSCCDD", StringComparison.OrdinalIgnoreCase)
            || custom.Contains(@"Root\Parsec\VDA", StringComparison.OrdinalIgnoreCase);
    }

    private static (List<DisplaySnapshot> Displays, DisplaySnapshot Target) ResolveStreamDisplayForPrepare(
        List<DisplaySnapshot> displays,
        SavedDisplayState? requestedMode,
        StreamDisplayPreference preference)
    {
        var mode = NormalizeStreamDisplayMode(preference.Mode);
        if (mode is "mtt_vdd")
        {
            var currentDisplays = EnsureVddOnline(displays, requestedMode);
            var target = FindVddDisplay(currentDisplays, requireActive: true)
                ?? throw new InvalidOperationException("Cloudgime VDD display is not active for stream.");
            return (currentDisplays, target);
        }

        if (mode is "parsec_vda")
        {
            var currentDisplays = EnsureParsecVddOnline(displays, preference);
            var target = currentDisplays
                .Where(display => display.Active)
                .Where(IsParsecDisplay)
                .OrderBy(display => display.DisplayId)
                .FirstOrDefault()
                ?? throw new InvalidOperationException("Parsec VDA display is not active for stream.");
            return (currentDisplays, target);
        }

        if (mode is "auto")
        {
            try
            {
                var currentDisplays = EnsureVddOnline(displays, requestedMode);
                var target = FindVddDisplay(currentDisplays, requireActive: true);
                if (target is not null)
                {
                    return (currentDisplays, target);
                }
            }
            catch
            {
                // Auto mode may fall back to the best already-active display.
            }
        }

        var preferred = FindPreferredStreamDisplay(displays, preference, requireActive: true)
            ?? (mode is "auto" ? FindPrimaryDisplay(displays) : null);
        if (preferred is null)
        {
            throw new InvalidOperationException($"No active stream display matched mode '{mode}'.");
        }

        return (displays, preferred);
    }

    private static List<DisplaySnapshot> EnsureStreamDisplayReadyForResize(
        List<DisplaySnapshot> displays,
        SavedDisplayState? requestedMode,
        StreamDisplayPreference preference,
        PrepareStateFile? state)
    {
        var stateMode = state is null
            ? NormalizeStreamDisplayMode(preference.Mode)
            : NormalizeStreamDisplayMode(state.StreamDisplayMode);
        var appliedIsSavedVdd = state?.AppliedDisplay is not null &&
            state.AppliedVdd is not null &&
            IsSameSavedDisplay(state.AppliedDisplay, state.AppliedVdd);
        if (stateMode is "parsec_vda" || PreferenceTargetsParsecVdd(preference))
        {
            try
            {
                return EnsureParsecVddOnline(displays, preference);
            }
            catch when (stateMode is not "parsec_vda")
            {
                return displays;
            }
        }

        if (stateMode is "mtt_vdd" || appliedIsSavedVdd || PreferenceMayUseMttVdd(preference))
        {
            try
            {
                return EnsureVddOnline(displays, requestedMode);
            }
            catch when (stateMode is not "mtt_vdd")
            {
                return displays;
            }
        }

        return displays;
    }

    private static DisplaySnapshot? ResolveStreamDisplayForResize(
        List<DisplaySnapshot> displays,
        StreamDisplayPreference preference,
        PrepareStateFile? state)
    {
        if (state?.AppliedDisplay is not null)
        {
            var applied = FindDisplay(displays, state.AppliedDisplay);
            if (applied is { Active: true })
            {
                return applied;
            }
        }

        if (state?.AppliedVdd is not null)
        {
            var appliedVdd = FindDisplay(displays, state.AppliedVdd);
            if (appliedVdd is { Active: true })
            {
                return appliedVdd;
            }
        }

        return FindPreferredStreamDisplay(displays, preference, requireActive: true)
            ?? FindPrimaryDisplay(displays);
    }

    private static DisplaySnapshot? ResolvePreparedStreamDisplay(
        List<DisplaySnapshot> displays,
        StreamDisplayPreference preference,
        DisplaySnapshot previousTarget)
    {
        var saved = previousTarget.ToSavedState();
        var same = FindDisplay(displays, saved);
        if (same is { Active: true })
        {
            return same;
        }

        return FindPreferredStreamDisplay(displays, preference, requireActive: true);
    }

    private static DisplaySnapshot? FindPreferredStreamDisplay(
        IEnumerable<DisplaySnapshot> displays,
        StreamDisplayPreference preference,
        bool requireActive)
    {
        var candidates = displays.Where(display => !requireActive || display.Active).ToList();
        var mode = NormalizeStreamDisplayMode(preference.Mode);
        return mode switch
        {
            "mtt_vdd" => FindVddDisplay(candidates, requireActive),
            "qemu_virtio" => candidates.Where(IsQemuVirtioDisplay).OrderBy(display => display.DisplayId).FirstOrDefault(),
            "parsec_vda" => FindParsecDisplay(candidates, requireActive),
            "primary" => FindPrimaryDisplay(candidates),
            "custom" => candidates.FirstOrDefault(display => MatchesCustomDisplayPreference(display, preference)),
            "auto" => FindVddDisplay(candidates, requireActive)
                ?? candidates.Where(IsQemuVirtioDisplay).OrderBy(display => display.DisplayId).FirstOrDefault()
                ?? FindPrimaryDisplay(candidates),
            _ => FindVddDisplay(candidates, requireActive),
        };
    }

    private static DisplaySnapshot? FindPrimaryDisplay(IEnumerable<DisplaySnapshot> displays) =>
        displays.FirstOrDefault(display => display.Active && display.Primary)
        ?? displays.FirstOrDefault(display => display.Active);

    private static bool MatchesCustomDisplayPreference(DisplaySnapshot display, StreamDisplayPreference preference)
    {
        if (!string.IsNullOrWhiteSpace(preference.CustomDeviceName) &&
            string.Equals(display.DeviceName, preference.CustomDeviceName, StringComparison.OrdinalIgnoreCase))
        {
            return true;
        }

        if (!string.IsNullOrWhiteSpace(preference.CustomDeviceId) &&
            string.Equals(display.DeviceId, preference.CustomDeviceId, StringComparison.OrdinalIgnoreCase))
        {
            return true;
        }

        var customLabel = preference.CustomLabel.Trim();
        return !string.IsNullOrWhiteSpace(customLabel) &&
            ($"{display.DeviceName} {display.DeviceString} {display.DeviceId}")
                .Contains(customLabel, StringComparison.OrdinalIgnoreCase);
    }

    private static bool IsQemuVirtioDisplay(DisplaySnapshot display)
    {
        var text = $"{display.DeviceString} {display.DeviceId}";
        return text.Contains("qemu", StringComparison.OrdinalIgnoreCase)
            || text.Contains("virtio", StringComparison.OrdinalIgnoreCase)
            || text.Contains("red hat", StringComparison.OrdinalIgnoreCase)
            || text.Contains("rht1234", StringComparison.OrdinalIgnoreCase)
            || text.Contains("VEN_1AF4", StringComparison.OrdinalIgnoreCase);
    }

    private static bool IsParsecDisplay(DisplaySnapshot display)
    {
        var text = $"{display.DeviceString} {display.DeviceId}";
        return text.Contains("parsec", StringComparison.OrdinalIgnoreCase)
            || text.Contains("PSCCDD", StringComparison.OrdinalIgnoreCase);
    }

    private static bool IsSameDisplay(DisplaySnapshot display, DisplaySnapshot other) =>
        string.Equals(display.DeviceName, other.DeviceName, StringComparison.OrdinalIgnoreCase)
        || (!string.IsNullOrWhiteSpace(display.DeviceId) &&
            string.Equals(display.DeviceId, other.DeviceId, StringComparison.OrdinalIgnoreCase))
        || display.DisplayId == other.DisplayId;

    private static bool IsSameSavedDisplay(SavedDisplayState first, SavedDisplayState second) =>
        (!string.IsNullOrWhiteSpace(first.DeviceName) &&
            string.Equals(first.DeviceName, second.DeviceName, StringComparison.OrdinalIgnoreCase))
        || (!string.IsNullOrWhiteSpace(first.DeviceId) &&
            string.Equals(first.DeviceId, second.DeviceId, StringComparison.OrdinalIgnoreCase))
        || first.DisplayId != 0 && first.DisplayId == second.DisplayId;

    private static DisplaySnapshot? FindVddDisplay(IEnumerable<DisplaySnapshot> displays, bool requireActive)
    {
        var configuredOutputName = TryReadConfiguredSunshineOutputNameFromHelperBundle();
        return displays
            .Where(display => DisplayLooksLikeMttVdd(display) && (!requireActive || display.Active))
            .OrderByDescending(display =>
                !string.IsNullOrWhiteSpace(configuredOutputName)
                && (
                    string.Equals(display.DeviceName, configuredOutputName, StringComparison.OrdinalIgnoreCase)
                    || (!string.IsNullOrWhiteSpace(display.DeviceId)
                        && string.Equals(display.DeviceId, configuredOutputName, StringComparison.OrdinalIgnoreCase))
                ))
            .ThenBy(display => display.StreamDisplayPriority)
            .ThenByDescending(DisplayIsVisibleOnDesktop)
            .ThenByDescending(display => display.Active)
            .ThenByDescending(display => display.DisplayId)
            .FirstOrDefault();
    }

    private static bool DisplayIsVisibleOnDesktop(DisplaySnapshot display) =>
        Screen.AllScreens.Any(screen =>
            string.Equals(screen.DeviceName, display.DeviceName, StringComparison.OrdinalIgnoreCase));

    private static DisplaySnapshot? FindParsecDisplay(IEnumerable<DisplaySnapshot> displays, bool requireActive) =>
        displays
            .Where(display => IsParsecDisplay(display) && (!requireActive || display.Active))
            .OrderByDescending(display => display.Active)
            .ThenByDescending(display => display.Width > 0 && display.Height > 0)
            .ThenByDescending(display => display.Primary)
            .ThenByDescending(display => display.DisplayId)
            .FirstOrDefault();

    private static DisplaySnapshot? FindDisplay(IEnumerable<DisplaySnapshot> displays, SavedDisplayState savedDisplay) =>
        displays.FirstOrDefault(display =>
            !string.IsNullOrWhiteSpace(savedDisplay.DeviceId) &&
            string.Equals(display.DeviceId, savedDisplay.DeviceId, StringComparison.OrdinalIgnoreCase))
        ?? displays.FirstOrDefault(display =>
            !string.IsNullOrWhiteSpace(savedDisplay.DeviceName) &&
            string.Equals(display.DeviceName, savedDisplay.DeviceName, StringComparison.OrdinalIgnoreCase))
        ?? displays.FirstOrDefault(display => display.DisplayId == savedDisplay.DisplayId);

    private static List<DisplaySnapshot> EnumerateDisplays()
    {
        var displays = new List<DisplaySnapshot>();
        for (uint adapterIndex = 0; ; adapterIndex++)
        {
            var adapter = NativeMethods.CreateDisplayDevice();
            if (!NativeMethods.EnumDisplayDevices(null, adapterIndex, ref adapter, 0))
            {
                break;
            }

            if (string.IsNullOrWhiteSpace(adapter.DeviceName) ||
                adapter.StateFlags.HasFlag(NativeMethods.DisplayDeviceStateFlags.MirroringDriver))
            {
                continue;
            }

            var (monitorString, monitorId) = EnumerateMonitorInfo(adapter.DeviceName);
            var hasMode = TryGetMode(adapter.DeviceName, EnumCurrentSettings, out var mode) ||
                          TryGetMode(adapter.DeviceName, EnumRegistrySettings, out mode);

            var deviceName = adapter.DeviceName.Trim();
            var adapterString = adapter.DeviceString.Trim();
            var adapterId = adapter.DeviceID.Trim();
            var adapterText = $"{adapterString} {adapterId}";
            var monitorText = $"{monitorString} {monitorId}";
            var adapterLooksLikeVdd =
                TextLooksLikeMttVdd(adapterText) ||
                (adapterString.Contains("Virtual Display Driver", StringComparison.OrdinalIgnoreCase) &&
                 adapterId.Contains("MttVDD", StringComparison.OrdinalIgnoreCase));
            var adapterLooksLikeCompetingVirtualDisplay =
                TextLooksLikeNonMttVirtualDisplay(adapterText);
            var adapterLooksLikeParsecVda =
                adapterString.Contains("Parsec Virtual Display Adapter", StringComparison.OrdinalIgnoreCase) ||
                adapterId.Contains(@"Parsec\VDA", StringComparison.OrdinalIgnoreCase);
            var monitorLooksLikeVdd =
                TextLooksLikeMttVdd(monitorText);
            var authoritativeAdapterLooksLikeVdd =
                adapterLooksLikeVdd && !adapterLooksLikeCompetingVirtualDisplay;
            var isVdd =
                authoritativeAdapterLooksLikeVdd;
            var streamDisplayPriority = authoritativeAdapterLooksLikeVdd
                ? 0
                : monitorLooksLikeVdd ? 5 : adapterLooksLikeParsecVda ? 200 : 100;
            var deviceString = CombineDisplayDescriptor(monitorString, adapterString);
            var deviceId = CombineDisplayDescriptor(monitorId, adapterId);
            var attached = adapter.StateFlags.HasFlag(NativeMethods.DisplayDeviceStateFlags.AttachedToDesktop);
            var primary = adapter.StateFlags.HasFlag(NativeMethods.DisplayDeviceStateFlags.PrimaryDevice);

            displays.Add(new DisplaySnapshot(
                ParseDisplayId(deviceName),
                deviceName,
                deviceId,
                deviceString,
                hasMode ? mode.dmPelsWidth : 0,
                hasMode ? mode.dmPelsHeight : 0,
                hasMode ? mode.dmDisplayFrequency : 0,
                hasMode ? mode.dmPositionX : 0,
                hasMode ? mode.dmPositionY : 0,
                primary,
                attached && hasMode && mode.dmPelsWidth > 0 && mode.dmPelsHeight > 0,
                isVdd,
                hasMode ? NormalizeOrientation(mode.dmDisplayOrientation) : DmdoDefault,
                streamDisplayPriority
            ));
        }

        return displays;
    }

    private static string CombineDisplayDescriptor(string primary, string secondary)
    {
        if (string.IsNullOrWhiteSpace(primary))
        {
            return secondary.Trim();
        }

        if (string.IsNullOrWhiteSpace(secondary) ||
            primary.Contains(secondary, StringComparison.OrdinalIgnoreCase))
        {
            return primary.Trim();
        }

        return $"{primary.Trim()} | {secondary.Trim()}";
    }

    private static (string MonitorString, string MonitorId) EnumerateMonitorInfo(string adapterDeviceName)
    {
        for (uint monitorIndex = 0; ; monitorIndex++)
        {
            var monitor = NativeMethods.CreateDisplayDevice();
            if (!NativeMethods.EnumDisplayDevices(adapterDeviceName, monitorIndex, ref monitor, 0))
            {
                break;
            }

            if (!string.IsNullOrWhiteSpace(monitor.DeviceString) || !string.IsNullOrWhiteSpace(monitor.DeviceID))
            {
                return (monitor.DeviceString.Trim(), monitor.DeviceID.Trim());
            }
        }

        return (string.Empty, string.Empty);
    }

    private static NativeMethods.DEVMODE GetCurrentMode(string deviceName, bool allowRegistryFallback = false)
    {
        if (TryGetMode(deviceName, EnumCurrentSettings, out var mode))
        {
            return mode;
        }

        if (allowRegistryFallback && TryGetMode(deviceName, EnumRegistrySettings, out mode))
        {
            return mode;
        }

        throw new InvalidOperationException($"failed to read display mode for {deviceName}");
    }

    private static NativeMethods.DEVMODE GetCurrentModeOrDefault(string deviceName, bool allowRegistryFallback = false)
    {
        try
        {
            return GetCurrentMode(deviceName, allowRegistryFallback);
        }
        catch
        {
            return NativeMethods.CreateDevMode();
        }
    }

    private static bool TryGetMode(string deviceName, int modeIndex, out NativeMethods.DEVMODE mode)
    {
        mode = NativeMethods.CreateDevMode();
        return NativeMethods.EnumDisplaySettingsEx(deviceName, modeIndex, ref mode, 0);
    }

    private static void EnsureDisplayResult(int result, string action)
    {
        if (result == NativeMethods.DispChangeSuccessful)
        {
            return;
        }

        throw new Win32Exception($"{action} failed with DISP_CHANGE code {result}");
    }

    private static int ParseDisplayId(string deviceName)
    {
        var digits = new string(deviceName.Where(char.IsDigit).ToArray());
        return int.TryParse(digits, out var value) ? value : 0;
    }
}

internal static class NativeMethods
{
    internal const int DispChangeSuccessful = 0;
    private const int DwmwaCloaked = 14;

    internal delegate bool EnumWindowsProc(nint hWnd, nint lParam);

    [Flags]
    internal enum DisplayDeviceStateFlags : int
    {
        AttachedToDesktop = 0x00000001,
        PrimaryDevice = 0x00000004,
        MirroringDriver = 0x00000008,
    }

    [Flags]
    internal enum ChangeDisplaySettingsFlags : uint
    {
        CDS_NONE = 0x00000000,
        CDS_UPDATEREGISTRY = 0x00000001,
        CDS_SET_PRIMARY = 0x00000010,
        CDS_NORESET = 0x10000000,
    }

    [Flags]
    internal enum ProcessAccessRights : uint
    {
        SuspendResume = 0x0800,
        QueryLimitedInformation = 0x1000,
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    internal struct DISPLAY_DEVICE
    {
        public int cb;

        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)]
        public string DeviceName;

        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 128)]
        public string DeviceString;

        public DisplayDeviceStateFlags StateFlags;

        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 128)]
        public string DeviceID;

        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 128)]
        public string DeviceKey;
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    internal struct DEVMODE
    {
        private const int CchDevicename = 32;
        private const int CchFormname = 32;

        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = CchDevicename)]
        public string dmDeviceName;
        public short dmSpecVersion;
        public short dmDriverVersion;
        public short dmSize;
        public short dmDriverExtra;
        public int dmFields;
        public int dmPositionX;
        public int dmPositionY;
        public int dmDisplayOrientation;
        public int dmDisplayFixedOutput;
        public short dmColor;
        public short dmDuplex;
        public short dmYResolution;
        public short dmTTOption;
        public short dmCollate;

        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = CchFormname)]
        public string dmFormName;

        public short dmLogPixels;
        public int dmBitsPerPel;
        public int dmPelsWidth;
        public int dmPelsHeight;
        public int dmDisplayFlags;
        public int dmDisplayFrequency;
        public int dmICMMethod;
        public int dmICMIntent;
        public int dmMediaType;
        public int dmDitherType;
        public int dmReserved1;
        public int dmReserved2;
        public int dmPanningWidth;
        public int dmPanningHeight;
    }

    [StructLayout(LayoutKind.Sequential)]
    internal struct POINT
    {
        public int X;
        public int Y;
    }

    [StructLayout(LayoutKind.Sequential)]
    internal struct RECT
    {
        public int Left;
        public int Top;
        public int Right;
        public int Bottom;
    }

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    internal static extern bool EnumDisplayDevices(string? lpDevice, uint iDevNum, ref DISPLAY_DEVICE lpDisplayDevice, uint dwFlags);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    internal static extern bool EnumDisplaySettingsEx(string lpszDeviceName, int iModeNum, ref DEVMODE lpDevMode, uint dwFlags);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    internal static extern int ChangeDisplaySettingsEx(string? lpszDeviceName, ref DEVMODE lpDevMode, IntPtr hwnd, ChangeDisplaySettingsFlags dwflags, IntPtr lParam);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    internal static extern int ChangeDisplaySettingsEx(string? lpszDeviceName, IntPtr lpDevMode, IntPtr hwnd, uint dwflags, IntPtr lParam);

    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool SetProcessDpiAwarenessContext(IntPtr dpiContext);

    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool SetProcessDPIAware();

    [DllImport("user32.dll")]
    internal static extern bool GetCursorPos(out POINT point);

    [DllImport("user32.dll")]
    internal static extern bool SetCursorPos(int x, int y);

    [DllImport("user32.dll")]
    internal static extern int ShowCursor(bool bShow);

    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool IsWindowVisible(nint hWnd);

    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool IsIconic(nint hWnd);

    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool GetWindowRect(nint hWnd, out RECT lpRect);

    [DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    private static extern int GetClassNameW(nint hWnd, System.Text.StringBuilder lpClassName, int nMaxCount);

    [DllImport("user32.dll", SetLastError = true)]
    internal static extern nint GetShellWindow();

    [DllImport("user32.dll", EntryPoint = "GetWindowLongPtrW", SetLastError = true)]
    internal static extern nint GetWindowLongPtr(nint hWnd, int nIndex);

    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool SetWindowPos(
        nint hWnd,
        nint hWndInsertAfter,
        int x,
        int y,
        int cx,
        int cy,
        uint uFlags);

    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool EnumWindows(EnumWindowsProc lpEnumFunc, nint lParam);

    [DllImport("user32.dll", SetLastError = true)]
    internal static extern uint GetWindowThreadProcessId(nint hWnd, out int lpdwProcessId);

    [DllImport("dwmapi.dll", SetLastError = true)]
    private static extern int DwmGetWindowAttribute(nint hwnd, int dwAttribute, out int pvAttribute, int cbAttribute);

    [DllImport("kernel32.dll", SetLastError = true)]
    internal static extern IntPtr OpenProcess(ProcessAccessRights dwDesiredAccess, bool bInheritHandle, int dwProcessId);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool CloseHandle(IntPtr hObject);

    [DllImport("ntdll.dll")]
    internal static extern int NtSuspendProcess(IntPtr processHandle);

    [DllImport("ntdll.dll")]
    internal static extern int NtResumeProcess(IntPtr processHandle);

    internal static bool TryGetWindowRectangle(nint hWnd, out Rectangle rect)
    {
        rect = Rectangle.Empty;
        if (!GetWindowRect(hWnd, out var rawRect))
        {
            return false;
        }

        rect = Rectangle.FromLTRB(rawRect.Left, rawRect.Top, rawRect.Right, rawRect.Bottom);
        return true;
    }

    internal static string GetWindowClassName(nint hWnd)
    {
        var builder = new System.Text.StringBuilder(256);
        _ = GetClassNameW(hWnd, builder, builder.Capacity);
        return builder.ToString();
    }

    internal static bool IsWindowCloaked(nint hWnd)
    {
        try
        {
            return DwmGetWindowAttribute(hWnd, DwmwaCloaked, out var cloaked, Marshal.SizeOf<int>()) == 0 &&
                   cloaked != 0;
        }
        catch
        {
            return false;
        }
    }

    internal static IReadOnlyList<nint> EnumerateTopLevelWindows()
    {
        var windows = new List<nint>();
        _ = EnumWindows((hWnd, _) =>
        {
            windows.Add(hWnd);
            return true;
        }, nint.Zero);
        return windows;
    }

    internal static DISPLAY_DEVICE CreateDisplayDevice() => new()
    {
        cb = Marshal.SizeOf<DISPLAY_DEVICE>(),
        DeviceName = string.Empty,
        DeviceString = string.Empty,
        DeviceID = string.Empty,
        DeviceKey = string.Empty,
    };

    internal static DEVMODE CreateDevMode() => new()
    {
        dmDeviceName = string.Empty,
        dmFormName = string.Empty,
        dmSize = (short)Marshal.SizeOf<DEVMODE>(),
    };
}
