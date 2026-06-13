using System.Diagnostics;
using System.Net.Http.Headers;
using System.Net.Sockets;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Windows.Forms;

namespace CloudgimeHostKeepAwakeAgent;

internal static class Program
{
    private const uint EsContinuous = 0x80000000;
    private const uint EsSystemRequired = 0x00000001;
    private const uint EsDisplayRequired = 0x00000002;
    private const uint MonitorDefaultToNearest = 0x00000002;
    private const string HostWindowsServiceName = "CloudgimeHost-Host";
    private const string RuntimeWindowsServiceName = "CloudgimeRuntime-Host";
    private const string HostKeeperTunnelTaskName = "CloudgimeHostKeeperTunnelAgent";
    private const string KeeperTunnelProcessName = "KeeperTunnelAgent";
    private const int LocalWebPort = 18080;
    private const int SunshinePort = 49000;
    private static readonly TimeSpan MinimumCycleDelay = TimeSpan.FromMinutes(2);
    private static readonly TimeSpan MaximumCycleDelay = TimeSpan.FromMinutes(5);
    private static readonly TimeSpan MinimumInputIdleBeforeNudge = TimeSpan.FromSeconds(120);
    private static readonly TimeSpan RecentlyActiveStreamWindow = TimeSpan.FromMinutes(3);
    private static readonly TimeSpan HostStartupGraceWindow = TimeSpan.FromSeconds(90);
    private static readonly TimeSpan SelfHealActionCooldown = TimeSpan.FromSeconds(90);
    private static readonly TimeSpan SelfHealRecoveryBudgetWindow = TimeSpan.FromMinutes(20);
    private static readonly TimeSpan HostSupervisorStateFreshWindow = TimeSpan.FromMinutes(10);
    private static readonly TimeSpan StartServiceTimeout = TimeSpan.FromSeconds(50);
    private static readonly TimeSpan RestartRuntimeTimeout = TimeSpan.FromSeconds(90);
    private static readonly TimeSpan PrepareHostTimeout = TimeSpan.FromMinutes(4);
    private static readonly TimeSpan PortProbeTimeout = TimeSpan.FromSeconds(3);
    private const int SelfHealRecoveryBudgetMaxAttempts = 5;
    private const int SelfHealEscalationCycleThreshold = 3;
    private static readonly string[] CaptureRecoveryHints =
    [
        "capture",
        "display",
        "virtual display",
        "virtual_display",
        "vdd",
        "mttvdd",
        "safe_default",
        "disp_change",
        "output_name",
        "sunshine_capture_init_failed",
        "failed to initialize video capture",
        "stream display disappeared",
        "current capture route",
        "duplicate recovery",
        "capture host belum bisa disegarkan"
    ];
    private static readonly string[] MttVddControllerHints =
    [
        "virtual display driver",
        "mttvdd",
        "mike the tech"
    ];
    private static readonly string[] MttVddDisplayHints =
    [
        "virtual display driver",
        "mtt",
        "mtt1337",
        "mike the tech",
        "iddsampledriver"
    ];
    private static readonly string[] ParsecDisplayHints =
    [
        "parsec",
        "psccdd",
        @"root\parsec\vda"
    ];
    private static readonly string[] QemuDisplayHints =
    [
        "red hat",
        "virtio",
        "qemu"
    ];
    private const int DisplayDeviceAttachedToDesktop = 0x00000001;
    private const int DisplayDevicePrimaryDevice = 0x00000004;
    private const int DisplayDeviceMirroringDriver = 0x00000008;
    private static readonly JsonSerializerOptions JsonOptions = new(JsonSerializerDefaults.Web)
    {
        WriteIndented = true,
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull
    };

    private static readonly HttpClient Http = new()
    {
        Timeout = TimeSpan.FromSeconds(8)
    };

    private static string _logPath = Path.Combine(Path.GetTempPath(), "cloudgime-keep-awake-agent.log");

    private static async Task<int> Main(string[] args)
    {
        var options = AgentOptions.Parse(args);
        var bundleRoot = ResolveBundleRoot(options.BundleRoot);
        var serverRoot = ResolveServerRoot(bundleRoot);
        Directory.CreateDirectory(serverRoot);
        _logPath = Path.Combine(serverRoot, options.Mode == AgentMode.User ? "keep-awake-agent-user.log" : "keep-awake-agent.log");

        var mutexName = $"CloudgimeHostKeepAwakeAgent-{options.Mode.ToString().ToLowerInvariant()}";
        using var mutex = new Mutex(initiallyOwned: true, mutexName, out var ownsMutex);
        if (!ownsMutex)
        {
            return 0;
        }

        Log($"start mode={options.Mode} bundleRoot={bundleRoot} allowNudge={options.AllowNudge}");

        using var cancellation = new CancellationTokenSource();
        Console.CancelKeyPress += (_, eventArgs) =>
        {
            eventArgs.Cancel = true;
            cancellation.Cancel();
        };

        try
        {
            await RunAsync(options, bundleRoot, serverRoot, cancellation.Token);
            return 0;
        }
        catch (OperationCanceledException)
        {
            Log("stop requested");
            return 0;
        }
        catch (Exception ex)
        {
            Log($"fatal: {ex}");
            return 1;
        }
        finally
        {
            _ = SetThreadExecutionState(EsContinuous);
            Log("exit");
        }
    }

    private static async Task RunAsync(AgentOptions options, string bundleRoot, string serverRoot, CancellationToken cancellationToken)
    {
        var heartbeatPath = Path.Combine(serverRoot, "keep_awake_heartbeat.json");
        var statePath = Path.Combine(serverRoot, "keep_awake_state.json");
        var selfHealStatePath = Path.Combine(serverRoot, "host_self_heal_state.json");
        var heartbeatUrl = ResolveHeartbeatUrl(options.HeartbeatUrl);
        var lastNetworkHeartbeat = DateTimeOffset.MinValue;

        while (!cancellationToken.IsCancellationRequested)
        {
            var now = DateTimeOffset.UtcNow;
            var cycle = RandomDelay();
            var nextDue = now.Add(cycle);
            var powerResult = SetThreadExecutionState(EsContinuous | EsSystemRequired | EsDisplayRequired);
            var idleSeconds = TryGetInputIdle(out var idle) ? (double?)idle.TotalSeconds : null;
            var nudge = NudgeResult.NotAttempted("disabled");

            if (options.Mode == AgentMode.User && options.AllowNudge)
            {
                nudge = TryNudgeSafely(bundleRoot, idle);
            }

            var network = "skipped";
            if (options.Mode == AgentMode.System && now - lastNetworkHeartbeat >= TimeSpan.FromMinutes(2))
            {
                network = await SendNetworkHeartbeatAsync(heartbeatUrl, cancellationToken);
                lastNetworkHeartbeat = now;
            }

            var selfHeal = HostSelfHealCycleResult.NotApplicable();
            var keeperTunnel = KeeperTunnelKeepaliveResult.NotApplicable();
            if (options.Mode == AgentMode.System)
            {
                selfHeal = await RunHostSelfHealCycleAsync(options, bundleRoot, serverRoot, selfHealStatePath, cancellationToken);
                keeperTunnel = TryEnsureKeeperTunnelAgentRunning(bundleRoot);
            }

            var state = new KeepAwakeState(
                Version: 2,
                Mode: options.Mode.ToString().ToLowerInvariant(),
                UpdatedAtUnixMs: DateTimeOffset.UtcNow.ToUnixTimeMilliseconds(),
                NextCycleAtUnixMs: nextDue.ToUnixTimeMilliseconds(),
                CycleSeconds: (int)cycle.TotalSeconds,
                PowerRequestRefreshed: powerResult != 0,
                NetworkHeartbeat: network,
                InputIdleSeconds: idleSeconds,
                NudgeStatus: nudge.Status,
                NudgeReason: nudge.Reason,
                SelfHealStatus: selfHeal.Status,
                SelfHealAction: selfHeal.Action,
                SelfHealReason: selfHeal.Reason,
                SelfHealConsecutiveUnhealthyCycles: selfHeal.ConsecutiveUnhealthyCycles,
                SelfHealHostServiceState: selfHeal.HostServiceState,
                SelfHealRuntimeServiceState: selfHeal.RuntimeServiceState,
                SelfHealLocalHttpReady: selfHeal.LocalHttpReady,
                SelfHealSunshineReady: selfHeal.SunshineReady,
                SelfHealLifecyclePhase: selfHeal.LifecyclePhase);

            WriteJsonAtomically(heartbeatPath, state);
            WriteJsonAtomically(statePath, state);
            Log(
                $"cycle mode={state.Mode} power={state.PowerRequestRefreshed} network={network} idleSeconds={idleSeconds?.ToString("F0") ?? "unknown"} " +
                $"nudge={nudge.Status}:{nudge.Reason} selfHeal={selfHeal.Status}:{selfHeal.Action}:{selfHeal.Reason} " +
                $"keeperTunnel={keeperTunnel.Status}:{keeperTunnel.Reason} " +
                $"service={selfHeal.HostServiceState}/{selfHeal.RuntimeServiceState} localHttp={selfHeal.LocalHttpReady} sunshine={selfHeal.SunshineReady} " +
                $"lifecycle={selfHeal.LifecyclePhase} nextSeconds={state.CycleSeconds}");

            await Task.Delay(cycle, cancellationToken);
        }
    }

    private static KeeperTunnelKeepaliveResult TryEnsureKeeperTunnelAgentRunning(string bundleRoot)
    {
        var keeperRoot = Path.Combine(bundleRoot, "keeper-tunnel");
        var agentPath = Path.Combine(keeperRoot, "KeeperTunnelAgent.exe");
        var envPath = Path.Combine(keeperRoot, "data", "cloudrental.env");

        if (!File.Exists(agentPath))
        {
            return KeeperTunnelKeepaliveResult.Skipped("agent_missing");
        }

        if (!File.Exists(envPath))
        {
            return KeeperTunnelKeepaliveResult.Skipped("env_missing");
        }

        if (IsProcessRunningForExecutable(KeeperTunnelProcessName, agentPath))
        {
            return KeeperTunnelKeepaliveResult.Healthy("running");
        }

        var taskResult = RunProcessCaptured(
            "powershell.exe",
            $"-NoProfile -NonInteractive -ExecutionPolicy Bypass -Command \"Start-ScheduledTask -TaskName '{HostKeeperTunnelTaskName}' -ErrorAction SilentlyContinue\"",
            Environment.SystemDirectory,
            TimeSpan.FromSeconds(20));
        Thread.Sleep(TimeSpan.FromSeconds(3));

        if (IsProcessRunningForExecutable(KeeperTunnelProcessName, agentPath))
        {
            return KeeperTunnelKeepaliveResult.ActionOk("start_task");
        }

        if (TryStartProcessDetached(agentPath, string.Empty, keeperRoot, out var directStartError))
        {
            Thread.Sleep(TimeSpan.FromSeconds(2));
            if (IsProcessRunningForExecutable(KeeperTunnelProcessName, agentPath))
            {
                return KeeperTunnelKeepaliveResult.ActionOk("spawn_direct");
            }
        }

        var taskDetail = taskResult.TimedOut
            ? "task_start_timeout"
            : taskResult.ExitCode == 0
                ? "task_start_no_process"
                : $"task_start_exit_{taskResult.ExitCode}";
        var directDetail = string.IsNullOrWhiteSpace(directStartError)
            ? "direct_start_no_process"
            : directStartError;
        return KeeperTunnelKeepaliveResult.ActionFailed($"{taskDetail}; {directDetail}");
    }

    private static async Task<HostSelfHealCycleResult> RunHostSelfHealCycleAsync(
        AgentOptions options,
        string bundleRoot,
        string serverRoot,
        string statePath,
        CancellationToken cancellationToken)
    {
        var state = ReadHostSelfHealState(statePath);
        if (!IsActivated(serverRoot))
        {
            state.UpdatedAtUnixMs = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();
            state.LastAssessment = "wait_activation";
            state.LastAssessmentReason = "host_not_activated_yet";
            state.LastAction = "none";
            state.LastActionReason = "waiting_for_setup_token_claim";
            state.LastActionSucceeded = true;
            state.ConsecutiveUnhealthyCycles = 0;
            WriteJsonAtomically(statePath, state);
            return new HostSelfHealCycleResult(
                "wait_activation",
                "none",
                "host_not_activated_yet",
                0,
                "stopped",
                "stopped",
                false,
                false,
                "idle");
        }
        var snapshot = await CollectHostHealthSnapshotAsync(bundleRoot, serverRoot, cancellationToken);

        // Unlock locked console session if Sunshine is down and we are in system mode
        if (!snapshot.SunshineReady && options.Mode == AgentMode.System)
        {
            uint consoleSessionId = GetActiveConsoleSessionId();
            if (consoleSessionId != 0xFFFFFFFF && consoleSessionId > 0)
            {
                var connState = GetSessionConnectState(consoleSessionId);
                if (connState == WTS_CONNECTSTATE_CLASS.WTSDisconnected)
                {
                    Log($"detected disconnected/locked console session {consoleSessionId}. Attempting to redirect to console via tscon...");
                    var tsconResult = RunProcessCaptured(
                        "tscon.exe",
                        $"{consoleSessionId} /dest:console",
                        Environment.SystemDirectory,
                        TimeSpan.FromSeconds(15));
                    Log($"tscon redirect exitCode={tsconResult.ExitCode} timedOut={tsconResult.TimedOut}");
                    
                    // Wait a moment for display/session to initialize, and recollect snapshot
                    await Task.Delay(TimeSpan.FromSeconds(3), cancellationToken);
                    snapshot = await CollectHostHealthSnapshotAsync(bundleRoot, serverRoot, cancellationToken);
                }
            }
        }

        var assessment = AssessHostHealth(snapshot, state);
        var nowMs = snapshot.ObservedAtUnixMs;

        state.UpdatedAtUnixMs = nowMs;
        state.LastAssessment = assessment.Assessment;
        state.LastAssessmentReason = TrimDetail(assessment.Reason, 400);
        state.LastObservedLifecyclePhase = snapshot.LifecyclePhase;
        state.LastObservedLifecycleReason = snapshot.LifecycleReason;
        state.LastObservedHostServiceState = snapshot.HostServiceState;
        state.LastObservedRuntimeServiceState = snapshot.RuntimeServiceState;
        state.LastObservedLocalHttpReady = snapshot.LocalHttpReady;
        state.LastObservedSunshineReady = snapshot.SunshineReady;
        state.LastObservedMttVddSatisfied = snapshot.MttVddSatisfied;
        state.LastObservedConfiguredOutputName = snapshot.ConfiguredOutputName;
        state.LastObservedDisplayTopologySummary = snapshot.DisplayTopologySummary;
        state.LastObservedDisplayStableForMttVdd = snapshot.DisplayStableForMttVdd;

        if (assessment.Healthy)
        {
            state.LastHealthyAtUnixMs = nowMs;
            state.ConsecutiveUnhealthyCycles = 0;
            state.RecoveryWindowAttemptCount = 0;
            state.RecoveryWindowStartedAtUnixMs = null;
            state.LastAction = "none";
            state.LastActionReason = "host_ready";
            state.LastActionSucceeded = true;
            state.LastActionAtUnixMs = nowMs;
            AddSelfHealHistory(state, nowMs, "healthy", "none", assessment.Reason, true, snapshot);
            WriteJsonAtomically(statePath, state);
            return new HostSelfHealCycleResult(
                "healthy",
                "none",
                assessment.Reason,
                state.ConsecutiveUnhealthyCycles,
                snapshot.HostServiceState,
                snapshot.RuntimeServiceState,
                snapshot.LocalHttpReady,
                snapshot.SunshineReady,
                snapshot.LifecyclePhase);
        }

        state.ConsecutiveUnhealthyCycles += 1;
        var effectiveAction = ChooseEffectiveAction(snapshot, assessment, state);
        if (effectiveAction == HostSelfHealAction.None)
        {
            state.LastAction = "none";
            state.LastActionReason = assessment.Reason;
            state.LastActionSucceeded = false;
            AddSelfHealHistory(state, nowMs, "wait", "none", assessment.Reason, false, snapshot);
            WriteJsonAtomically(statePath, state);
            return new HostSelfHealCycleResult(
                "wait",
                "none",
                assessment.Reason,
                state.ConsecutiveUnhealthyCycles,
                snapshot.HostServiceState,
                snapshot.RuntimeServiceState,
                snapshot.LocalHttpReady,
                snapshot.SunshineReady,
                snapshot.LifecyclePhase);
        }

        if (snapshot.StreamRecentlyActive)
        {
            var streamActiveReason = $"stream_active:{assessment.Reason}";
            state.LastAction = ActionName(effectiveAction);
            state.LastActionReason = streamActiveReason;
            state.LastActionSucceeded = false;
            AddSelfHealHistory(state, nowMs, "stream_active", ActionName(effectiveAction), streamActiveReason, false, snapshot);
            WriteJsonAtomically(statePath, state);
            return new HostSelfHealCycleResult(
                "stream_active",
                ActionName(effectiveAction),
                streamActiveReason,
                state.ConsecutiveUnhealthyCycles,
                snapshot.HostServiceState,
                snapshot.RuntimeServiceState,
                snapshot.LocalHttpReady,
                snapshot.SunshineReady,
                snapshot.LifecyclePhase);
        }

        if (state.LastActionAtUnixMs is long lastActionAt
            && nowMs - lastActionAt < (long)SelfHealActionCooldown.TotalMilliseconds)
        {
            var cooldownReason = $"cooldown_active:{assessment.Reason}";
            state.LastAction = ActionName(effectiveAction);
            state.LastActionReason = cooldownReason;
            state.LastActionSucceeded = false;
            AddSelfHealHistory(state, nowMs, "cooldown", ActionName(effectiveAction), cooldownReason, false, snapshot);
            WriteJsonAtomically(statePath, state);
            return new HostSelfHealCycleResult(
                "cooldown",
                ActionName(effectiveAction),
                cooldownReason,
                state.ConsecutiveUnhealthyCycles,
                snapshot.HostServiceState,
                snapshot.RuntimeServiceState,
                snapshot.LocalHttpReady,
                snapshot.SunshineReady,
                snapshot.LifecyclePhase);
        }

        NormalizeRecoveryBudget(state, nowMs);
        if (state.RecoveryWindowAttemptCount >= SelfHealRecoveryBudgetMaxAttempts)
        {
            var budgetReason = $"budget_exhausted:{assessment.Reason}";
            state.LastAction = ActionName(effectiveAction);
            state.LastActionReason = budgetReason;
            state.LastActionSucceeded = false;
            AddSelfHealHistory(state, nowMs, "budget_exhausted", ActionName(effectiveAction), budgetReason, false, snapshot);
            WriteJsonAtomically(statePath, state);
            return new HostSelfHealCycleResult(
                "budget_exhausted",
                ActionName(effectiveAction),
                budgetReason,
                state.ConsecutiveUnhealthyCycles,
                snapshot.HostServiceState,
                snapshot.RuntimeServiceState,
                snapshot.LocalHttpReady,
                snapshot.SunshineReady,
                snapshot.LifecyclePhase);
        }

        state.RecoveryWindowAttemptCount += 1;
        state.LastActionAtUnixMs = nowMs;
        state.LastAction = ActionName(effectiveAction);
        state.LastActionReason = assessment.Reason;

        var execution = await ExecuteSelfHealActionAsync(bundleRoot, effectiveAction, cancellationToken);
        state.LastActionSucceeded = execution.Success;

        if (execution.Attempted)
        {
            try
            {
                await Task.Delay(TimeSpan.FromSeconds(3), cancellationToken);
            }
            catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
            {
            }
        }

        var actionReason = execution.Success
            ? $"{assessment.Reason}; {execution.Detail}"
            : $"{assessment.Reason}; {execution.Detail}";
        AddSelfHealHistory(state, nowMs, execution.Status, execution.Action, actionReason, execution.Success, snapshot);
        WriteJsonAtomically(statePath, state);

        return new HostSelfHealCycleResult(
            execution.Status,
            execution.Action,
            actionReason,
            state.ConsecutiveUnhealthyCycles,
            snapshot.HostServiceState,
            snapshot.RuntimeServiceState,
            snapshot.LocalHttpReady,
            snapshot.SunshineReady,
            snapshot.LifecyclePhase);
    }

    private static async Task<HostHealthSnapshot> CollectHostHealthSnapshotAsync(
        string bundleRoot,
        string serverRoot,
        CancellationToken cancellationToken)
    {
        var supervisorStatePath = Path.Combine(serverRoot, "host_supervisor_state.json");
        var capabilityPath = Path.Combine(serverRoot, "host_capability_profile.json");
        var taskHealthPath = Path.Combine(serverRoot, "host_user_daemon_task_health.json");
        var hostInstallerPath = Path.Combine(bundleRoot, "host-installer.exe");
        var runtimeAgentPath = Path.Combine(bundleRoot, "moonlight", "system", "cloudgime-runtime-agent.exe");

        var snapshot = new HostHealthSnapshot
        {
            ObservedAtUnixMs = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds(),
            HostInstallerPresent = File.Exists(hostInstallerPath),
            RuntimeAgentPresent = File.Exists(runtimeAgentPath),
            HostServiceState = QueryWindowsServiceState(HostWindowsServiceName),
            RuntimeServiceState = QueryWindowsServiceState(RuntimeWindowsServiceName),
            StreamRecentlyActive = IsStreamRecentlyActive(bundleRoot)
        };

        var probes = await Task.WhenAll(
            ProbeTcpPortAsync("127.0.0.1", LocalWebPort, PortProbeTimeout, cancellationToken),
            ProbeTcpPortAsync("127.0.0.1", SunshinePort, PortProbeTimeout, cancellationToken));
        snapshot.LocalHttpReady = probes[0];
        snapshot.SunshineReady = probes[1];

        snapshot.Supervisor = ReadHostSupervisorSnapshot(supervisorStatePath);
        snapshot.Capability = ReadCapabilityProfileSnapshot(capabilityPath);
        snapshot.TaskHealth = ReadTaskHealthSnapshot(taskHealthPath);

        snapshot.LifecyclePhase = FirstNonEmpty(snapshot.Supervisor.LifecyclePhase, "unknown");
        snapshot.LifecycleReason = snapshot.Supervisor.LifecycleReason;
        snapshot.SupervisorStateFresh = snapshot.Supervisor.UpdatedAtUnixMs is long updatedAt
            && snapshot.ObservedAtUnixMs - updatedAt <= (long)HostSupervisorStateFreshWindow.TotalMilliseconds;
        snapshot.WithinStartupGrace = snapshot.Supervisor.DaemonStartedAtUnixMs is long daemonStartedAt
            && snapshot.ObservedAtUnixMs - daemonStartedAt <= (long)HostStartupGraceWindow.TotalMilliseconds;

        var (mttVddSatisfied, mttVddReason) = EvaluateMttVddInvariant(snapshot.Capability);
        snapshot.MttVddSatisfied = mttVddSatisfied;
        snapshot.MttVddReason = mttVddReason;
        snapshot.ConfiguredOutputName = snapshot.Capability.ConfiguredOutputName;
        snapshot.DisplayTopology = ReadDesktopDisplayTopology();
        snapshot.DisplayTopologySummary = snapshot.DisplayTopology.Summary;
        snapshot.DisplayStableForMttVdd = snapshot.MttVddSatisfied
            && snapshot.DisplayTopology.ReadSucceeded
            && snapshot.DisplayTopology.PrimaryActiveDisplayCount == 1
            && (snapshot.DisplayTopology.PrimaryActiveDisplayLooksLikeMttVdd
                || snapshot.DisplayTopology.UsedScreenFallback);
        snapshot.CaptureIssueHint = FindCaptureIssueHint(snapshot);

        return snapshot;
    }

    private static HostHealthAssessment AssessHostHealth(HostHealthSnapshot snapshot, HostSelfHealState state)
    {
        if (snapshot.LocalHttpReady
            && snapshot.SunshineReady
            && !string.Equals(snapshot.LifecyclePhase, "failed", StringComparison.OrdinalIgnoreCase))
        {
            return new HostHealthAssessment(true, "ready", "host_ready", HostSelfHealAction.None);
        }

        if (snapshot.WithinStartupGrace
            && (snapshot.LocalHttpReady
                || snapshot.SunshineReady
                || string.Equals(snapshot.LifecyclePhase, "starting", StringComparison.OrdinalIgnoreCase)
                || string.Equals(snapshot.LifecyclePhase, "recovering", StringComparison.OrdinalIgnoreCase)
                || string.Equals(snapshot.LifecyclePhase, "unknown", StringComparison.OrdinalIgnoreCase)))
        {
            return new HostHealthAssessment(false, "startup_grace", "host_startup_grace_window", HostSelfHealAction.None);
        }

        if (!snapshot.HostInstallerPresent && snapshot.HostServiceState != "running")
        {
            return new HostHealthAssessment(false, "installer_missing", "host_installer_missing", HostSelfHealAction.StartService);
        }

        if (snapshot.HostServiceState != "running")
        {
            return new HostHealthAssessment(false, "service_missing", "host_service_not_running", HostSelfHealAction.StartService);
        }

        if (!snapshot.MttVddSatisfied)
        {
            return new HostHealthAssessment(false, "mtt_vdd_invariant", snapshot.MttVddReason, HostSelfHealAction.PrepareHost);
        }

        if (!string.IsNullOrWhiteSpace(snapshot.CaptureIssueHint))
        {
            return new HostHealthAssessment(false, "capture_issue", snapshot.CaptureIssueHint, HostSelfHealAction.PrepareHost);
        }

        if (!snapshot.LocalHttpReady && !snapshot.SunshineReady)
        {
            return new HostHealthAssessment(false, "ports_down", "local_http_and_sunshine_not_ready", HostSelfHealAction.RestartRuntime);
        }

        if (!snapshot.LocalHttpReady)
        {
            return new HostHealthAssessment(false, "local_http_down", "local_http_not_ready", HostSelfHealAction.RestartRuntime);
        }

        if (!snapshot.SunshineReady)
        {
            return new HostHealthAssessment(false, "sunshine_down", "sunshine_not_ready", HostSelfHealAction.RestartRuntime);
        }

        if (string.Equals(snapshot.LifecyclePhase, "failed", StringComparison.OrdinalIgnoreCase))
        {
            return new HostHealthAssessment(
                false,
                "lifecycle_failed",
                FirstNonEmpty(snapshot.LifecycleReason, "lifecycle_failed"),
                HostSelfHealAction.RestartRuntime);
        }

        if (!snapshot.SupervisorStateFresh)
        {
            return new HostHealthAssessment(false, "state_stale", "host_supervisor_state_stale", HostSelfHealAction.RestartRuntime);
        }

        if (state.ConsecutiveUnhealthyCycles >= SelfHealEscalationCycleThreshold)
        {
            return new HostHealthAssessment(false, "persistent_unhealthy", "persistent_unhealthy_host_state", HostSelfHealAction.PrepareHost);
        }

        return new HostHealthAssessment(false, "runtime_not_ready", "runtime_not_ready", HostSelfHealAction.RestartRuntime);
    }

    private static HostSelfHealAction ChooseEffectiveAction(
        HostHealthSnapshot snapshot,
        HostHealthAssessment assessment,
        HostSelfHealState state)
    {
        var action = assessment.RecommendedAction;
        if (action == HostSelfHealAction.PrepareHost
            && snapshot.DisplayStableForMttVdd)
        {
            if (snapshot.HostServiceState != "running" || snapshot.RuntimeServiceState != "running")
            {
                return HostSelfHealAction.StartService;
            }

            return HostSelfHealAction.RestartRuntime;
        }

        if (action == HostSelfHealAction.RestartRuntime
            && (state.ConsecutiveUnhealthyCycles >= SelfHealEscalationCycleThreshold
                || (!state.LastActionSucceeded
                    && string.Equals(state.LastAction, "restart_runtime", StringComparison.OrdinalIgnoreCase)
                    && string.Equals(state.LastAssessment, assessment.Assessment, StringComparison.OrdinalIgnoreCase))))
        {
            if (snapshot.DisplayStableForMttVdd)
            {
                return HostSelfHealAction.RestartRuntime;
            }

            return HostSelfHealAction.PrepareHost;
        }

        if (action == HostSelfHealAction.StartService
            && snapshot.HostInstallerPresent
            && state.ConsecutiveUnhealthyCycles >= SelfHealEscalationCycleThreshold
            && !snapshot.LocalHttpReady)
        {
            if (snapshot.DisplayStableForMttVdd)
            {
                return HostSelfHealAction.StartService;
            }

            return HostSelfHealAction.PrepareHost;
        }

        return action;
    }

    private static async Task<HostActionExecutionResult> ExecuteSelfHealActionAsync(
        string bundleRoot,
        HostSelfHealAction action,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();

        return action switch
        {
            HostSelfHealAction.StartService => await ExecuteStartServiceAsync(bundleRoot),
            HostSelfHealAction.RestartRuntime => await ExecuteRestartRuntimeAsync(bundleRoot),
            HostSelfHealAction.PrepareHost => await ExecutePrepareHostAsync(bundleRoot),
            _ => HostActionExecutionResult.NotAttempted("none", "no action requested")
        };
    }

    private static Task<HostActionExecutionResult> ExecuteStartServiceAsync(string bundleRoot)
    {
        var installerPath = Path.Combine(bundleRoot, "host-installer.exe");
        if (File.Exists(installerPath))
        {
            var result = RunProcessCaptured(
                installerPath,
                $"--bundle-root {QuoteArgument(bundleRoot)} start-service",
                bundleRoot,
                StartServiceTimeout);
            return Task.FromResult(ToActionExecutionResult("start_service", result, "host-installer start-service"));
        }

        var hostResult = RunProcessCaptured(
            "sc.exe",
            $"start \"{HostWindowsServiceName}\"",
            Environment.SystemDirectory,
            StartServiceTimeout);
        var runtimeResult = RunProcessCaptured(
            "sc.exe",
            $"start \"{RuntimeWindowsServiceName}\"",
            Environment.SystemDirectory,
            StartServiceTimeout);
        var output = CombineOutputs(hostResult.Output, runtimeResult.Output);
        var success = IsScStartSuccessful(hostResult) || IsScStartSuccessful(runtimeResult);
        return Task.FromResult(new HostActionExecutionResult(
            "start_service",
            true,
            success,
            success ? "action_ok" : "action_failed",
            TrimDetail(output, 600)));
    }

    private static Task<HostActionExecutionResult> ExecuteRestartRuntimeAsync(string bundleRoot)
    {
        var runtimeAgentPath = Path.Combine(bundleRoot, "moonlight", "system", "cloudgime-runtime-agent.exe");
        if (File.Exists(runtimeAgentPath))
        {
            var result = RunProcessCaptured(
                runtimeAgentPath,
                $"--bundle-root {QuoteArgument(bundleRoot)} restart-runtime",
                bundleRoot,
                RestartRuntimeTimeout);
            return Task.FromResult(ToActionExecutionResult("restart_runtime", result, "runtime-agent restart-runtime"));
        }

        var installerPath = Path.Combine(bundleRoot, "host-installer.exe");
        if (File.Exists(installerPath))
        {
            var result = RunProcessCaptured(
                installerPath,
                $"--bundle-root {QuoteArgument(bundleRoot)} restart-runtime",
                bundleRoot,
                RestartRuntimeTimeout);
            return Task.FromResult(ToActionExecutionResult("restart_runtime", result, "host-installer restart-runtime"));
        }

        return Task.FromResult(HostActionExecutionResult.NotAttempted("restart_runtime", "runtime restart helper missing"));
    }

    private static Task<HostActionExecutionResult> ExecutePrepareHostAsync(string bundleRoot)
    {
        var installerPath = Path.Combine(bundleRoot, "host-installer.exe");
        if (!File.Exists(installerPath))
        {
            return Task.FromResult(HostActionExecutionResult.NotAttempted("prepare_host", "host-installer missing"));
        }

        var result = RunProcessCaptured(
            installerPath,
            $"--bundle-root {QuoteArgument(bundleRoot)} prepare-host",
            bundleRoot,
            PrepareHostTimeout);
        return Task.FromResult(ToActionExecutionResult("prepare_host", result, "host-installer prepare-host"));
    }

    private static HostActionExecutionResult ToActionExecutionResult(string action, ProcessCaptureResult result, string label)
    {
        var success = !result.TimedOut && result.ExitCode == 0;
        var detail = success
            ? $"{label} ok"
            : result.TimedOut
                ? $"{label} timed out"
                : $"{label} exit={result.ExitCode}";
        var output = TrimDetail(result.Output, 600);
        if (!string.IsNullOrWhiteSpace(output))
        {
            detail = $"{detail}; output={output}";
        }

        return new HostActionExecutionResult(
            action,
            true,
            success,
            success ? "action_ok" : "action_failed",
            detail);
    }

    private static (bool Satisfied, string Reason) EvaluateMttVddInvariant(CapabilityProfileSnapshot capability)
    {
        if (!capability.Exists)
        {
            return (false, "host_capability_profile_missing");
        }

        if (!capability.HasMttVddController)
        {
            return (false, "mtt_vdd_controller_missing");
        }

        if (!string.Equals(capability.SelectedCapture, "ddx", StringComparison.OrdinalIgnoreCase))
        {
            return (false, $"selected_capture={capability.SelectedCapture}");
        }

        if (!capability.SelectedCaptureReason.Contains("virtual_display_driver", StringComparison.OrdinalIgnoreCase))
        {
            return (false, $"selected_capture_reason={capability.SelectedCaptureReason}");
        }

        if (string.IsNullOrWhiteSpace(capability.ConfiguredOutputName))
        {
            return (true, "mtt_vdd_ready_primary_capture_auto");
        }

        return (true, "mtt_vdd_ready_pinned_capture");
    }

    private static DesktopDisplayTopologySnapshot ReadDesktopDisplayTopology()
    {
        var snapshot = new DesktopDisplayTopologySnapshot();
        try
        {
            for (var index = 0; index < 64; index++)
            {
                var device = new DISPLAY_DEVICE
                {
                    cb = Marshal.SizeOf<DISPLAY_DEVICE>()
                };

                if (!EnumDisplayDevices(null, index, ref device, 0))
                {
                    break;
                }

                if (string.IsNullOrWhiteSpace(device.DeviceName))
                {
                    continue;
                }

                var display = new DesktopDisplaySnapshot
                {
                    DeviceName = device.DeviceName.Trim(),
                    DeviceString = device.DeviceString.Trim(),
                    DeviceId = device.DeviceID.Trim(),
                    AttachedToDesktop = (device.StateFlags & DisplayDeviceAttachedToDesktop) != 0,
                    Primary = (device.StateFlags & DisplayDevicePrimaryDevice) != 0,
                    MirroringDriver = (device.StateFlags & DisplayDeviceMirroringDriver) != 0
                };
                snapshot.AllDisplays.Add(display);
            }

            FinalizeDesktopDisplayTopology(snapshot);
        }
        catch (Exception ex)
        {
            PopulateScreenFallback(snapshot, $"enum_error:{ex.GetType().Name}");
        }

        return snapshot;
    }

    private static void FinalizeDesktopDisplayTopology(DesktopDisplayTopologySnapshot snapshot)
    {
        var activeDisplays = snapshot.AllDisplays
            .Where(display => display.AttachedToDesktop && !display.MirroringDriver)
            .ToList();
        if (activeDisplays.Count == 0)
        {
            PopulateScreenFallback(snapshot, "screen_fallback");
            return;
        }

        snapshot.ReadSucceeded = true;
        snapshot.ActiveDisplayCount = activeDisplays.Count;
        snapshot.PrimaryActiveDisplayCount = activeDisplays.Count(display => display.Primary);
        var primary = activeDisplays.FirstOrDefault(display => display.Primary);
        if (primary is not null)
        {
            var primaryDescriptor = $"{primary.DeviceName} {primary.DeviceString} {primary.DeviceId}";
            var primaryLooksLikeParsec = TextContainsAnyHint(primaryDescriptor, ParsecDisplayHints);
            var primaryLooksLikeQemu = TextContainsAnyHint(primaryDescriptor, QemuDisplayHints);
            snapshot.PrimaryActiveDisplayName = FirstNonEmpty(primary.DeviceName, primary.DeviceString, primary.DeviceId);
            snapshot.PrimaryActiveDisplayLooksLikeMttVdd =
                TextContainsAnyHint(primaryDescriptor, MttVddDisplayHints) &&
                !primaryLooksLikeParsec &&
                !primaryLooksLikeQemu;
            snapshot.PrimaryActiveDisplayLooksLikeParsec = primaryLooksLikeParsec;
            snapshot.PrimaryActiveDisplayLooksLikeQemu = primaryLooksLikeQemu;
        }
        if (activeDisplays.Count == 1)
        {
            var only = activeDisplays[0];
            var descriptor = $"{only.DeviceName} {only.DeviceString} {only.DeviceId}";
            var singleLooksLikeParsec = TextContainsAnyHint(descriptor, ParsecDisplayHints);
            var singleLooksLikeQemu = TextContainsAnyHint(descriptor, QemuDisplayHints);
            snapshot.SingleActiveDisplayName = FirstNonEmpty(only.DeviceName, only.DeviceString, only.DeviceId);
            snapshot.SingleActiveDisplayLooksLikeMttVdd =
                TextContainsAnyHint(descriptor, MttVddDisplayHints) &&
                !singleLooksLikeParsec &&
                !singleLooksLikeQemu;
            snapshot.SingleActiveDisplayLooksLikeParsec = singleLooksLikeParsec;
            snapshot.SingleActiveDisplayLooksLikeQemu = singleLooksLikeQemu;
        }

        snapshot.Summary = string.Join(
            " | ",
            activeDisplays.Select(display =>
                $"{display.DeviceName} primary={display.Primary} desc={TrimDetail(display.DeviceString, 80)} id={TrimDetail(display.DeviceId, 80)}"));
    }

    private static void PopulateScreenFallback(DesktopDisplayTopologySnapshot snapshot, string reason)
    {
        var screens = Screen.AllScreens;
        snapshot.UsedScreenFallback = true;
        snapshot.ReadSucceeded = screens.Length > 0;
        snapshot.ActiveDisplayCount = screens.Length;
        snapshot.PrimaryActiveDisplayCount = screens.Count(screen => screen.Primary);
        if (screens.Length == 1)
        {
            snapshot.SingleActiveDisplayName = screens[0].DeviceName ?? string.Empty;
        }
        var primary = screens.FirstOrDefault(screen => screen.Primary);
        if (primary is not null)
        {
            snapshot.PrimaryActiveDisplayName = primary.DeviceName ?? string.Empty;
        }

        snapshot.Summary = screens.Length == 0
            ? $"{reason}:no_active_display"
            : $"{reason}:{string.Join(" | ", screens.Select(screen => $"{screen.DeviceName} primary={screen.Primary} bounds={screen.Bounds.Width}x{screen.Bounds.Height}"))}";
    }

    private static string FindCaptureIssueHint(HostHealthSnapshot snapshot)
    {
        foreach (var value in new[]
                 {
                     snapshot.LifecycleReason,
                     snapshot.Supervisor.LastFailureRecoveryReason,
                     snapshot.Supervisor.LastServiceWatchdogReason,
                     snapshot.Supervisor.LastCommandError,
                     snapshot.Capability.SelectedCaptureReason
                 })
        {
            if (string.IsNullOrWhiteSpace(value))
            {
                continue;
            }

            var lowered = value.Trim().ToLowerInvariant();
            if (CaptureRecoveryHints.Any(hint => lowered.Contains(hint, StringComparison.Ordinal)))
            {
                return value.Trim();
            }
        }

        return string.Empty;
    }

    private static HostSupervisorSnapshot ReadHostSupervisorSnapshot(string path)
    {
        var snapshot = new HostSupervisorSnapshot();
        if (!File.Exists(path))
        {
            return snapshot;
        }

        snapshot.Exists = true;
        snapshot.UpdatedAtUnixMs = ToUnixMilliseconds(File.GetLastWriteTimeUtc(path));

        try
        {
            using var document = JsonDocument.Parse(File.ReadAllBytes(path));
            var root = document.RootElement;
            snapshot.LifecyclePhase = JsonString(root, "lifecycle_phase");
            snapshot.LifecycleReason = JsonString(root, "lifecycle_reason");
            snapshot.LastFailureRecoveryReason = JsonString(root, "last_failure_recovery_reason");
            snapshot.LastServiceWatchdogReason = JsonString(root, "last_service_watchdog_reason");
            snapshot.LastCommandError = JsonString(root, "last_command_error");
            snapshot.DaemonStartedAtUnixMs = JsonLong(root, "daemon_started_at_unix_ms");
            snapshot.UpdatedAtUnixMs = JsonLong(root, "updated_at_unix_ms") ?? snapshot.UpdatedAtUnixMs;
        }
        catch (Exception ex)
        {
            snapshot.ParseError = ex.GetType().Name;
        }

        return snapshot;
    }

    private static bool IsActivated(string serverRoot)
    {
        var path = Path.Combine(serverRoot, "host_activation_state.json");
        if (!File.Exists(path))
        {
            return true;
        }

        try
        {
            using var document = JsonDocument.Parse(File.ReadAllBytes(path));
            var root = document.RootElement;
            if (root.TryGetProperty("ActivationState", out var prop))
            {
                var val = prop.GetString();
                if (string.Equals(val, "prepared_local", StringComparison.OrdinalIgnoreCase)
                    || string.Equals(val, "locked_waiting_token", StringComparison.OrdinalIgnoreCase))
                {
                    return false;
                }
            }
        }
        catch
        {
            // Ignore parse errors, default to true
        }

        return true;
    }

    private static CapabilityProfileSnapshot ReadCapabilityProfileSnapshot(string path)
    {
        var snapshot = new CapabilityProfileSnapshot();
        if (!File.Exists(path))
        {
            return snapshot;
        }

        snapshot.Exists = true;
        try
        {
            using var document = JsonDocument.Parse(File.ReadAllBytes(path));
            var root = document.RootElement;
            snapshot.SelectedCapture = JsonString(root, "selected_capture");
            snapshot.SelectedCaptureReason = JsonString(root, "selected_capture_reason");
            snapshot.SelectedRuntimeKey = JsonString(root, "selected_runtime_key");
            snapshot.SelectedEncoder = JsonString(root, "selected_encoder");
            snapshot.ConfiguredOutputName = ReadConfiguredOutputName(JsonString(root, "config_path"));
            snapshot.GpuControllerNames = JsonArrayObjects(root, "gpu_controllers")
                .Select(item => JsonString(item, "name"))
                .Where(value => !string.IsNullOrWhiteSpace(value))
                .ToList();
            snapshot.HasMttVddController = snapshot.GpuControllerNames.Any(IsMttVddControllerName);
        }
        catch (Exception ex)
        {
            snapshot.ParseError = ex.GetType().Name;
        }

        return snapshot;
    }

    private static TaskHealthSnapshot ReadTaskHealthSnapshot(string path)
    {
        var snapshot = new TaskHealthSnapshot();
        if (!File.Exists(path))
        {
            return snapshot;
        }

        snapshot.Exists = true;
        try
        {
            using var document = JsonDocument.Parse(File.ReadAllBytes(path));
            var root = document.RootElement;
            snapshot.PolicyValid = JsonBool(root, "policyValid");
            snapshot.DaemonRunning = JsonBool(root, "daemonRunning");
        }
        catch (Exception ex)
        {
            snapshot.ParseError = ex.GetType().Name;
        }

        return snapshot;
    }

    private static HostSelfHealState ReadHostSelfHealState(string path)
    {
        if (!File.Exists(path))
        {
            return new HostSelfHealState();
        }

        try
        {
            var state = JsonSerializer.Deserialize<HostSelfHealState>(File.ReadAllBytes(path), JsonOptions);
            return state ?? new HostSelfHealState();
        }
        catch (Exception ex)
        {
            Log($"self-heal state reset because parse failed: {ex.GetType().Name}");
            return new HostSelfHealState();
        }
    }

    private static void NormalizeRecoveryBudget(HostSelfHealState state, long nowMs)
    {
        if (state.RecoveryWindowStartedAtUnixMs is null
            || nowMs - state.RecoveryWindowStartedAtUnixMs > (long)SelfHealRecoveryBudgetWindow.TotalMilliseconds)
        {
            state.RecoveryWindowStartedAtUnixMs = nowMs;
            state.RecoveryWindowAttemptCount = 0;
        }
    }

    private static void AddSelfHealHistory(
        HostSelfHealState state,
        long atUnixMs,
        string status,
        string action,
        string reason,
        bool success,
        HostHealthSnapshot snapshot)
    {
        state.RecentActions.Add(new HostSelfHealHistoryItem
        {
            AtUnixMs = atUnixMs,
            Status = status,
            Action = action,
            Reason = TrimDetail(reason, 400),
            Success = success,
            HostServiceState = snapshot.HostServiceState,
            RuntimeServiceState = snapshot.RuntimeServiceState,
            LocalHttpReady = snapshot.LocalHttpReady,
            SunshineReady = snapshot.SunshineReady,
            LifecyclePhase = snapshot.LifecyclePhase,
            ConfiguredOutputName = snapshot.ConfiguredOutputName
        });

        while (state.RecentActions.Count > 16)
        {
            state.RecentActions.RemoveAt(0);
        }
    }

    private static string ActionName(HostSelfHealAction action) =>
        action switch
        {
            HostSelfHealAction.StartService => "start_service",
            HostSelfHealAction.RestartRuntime => "restart_runtime",
            HostSelfHealAction.PrepareHost => "prepare_host",
            _ => "none"
        };

    private static bool IsScStartSuccessful(ProcessCaptureResult result)
    {
        if (result.TimedOut)
        {
            return false;
        }

        if (result.ExitCode == 0)
        {
            return true;
        }

        var output = result.Output;
        return output.Contains("service has already been started", StringComparison.OrdinalIgnoreCase)
               || output.Contains("already running", StringComparison.OrdinalIgnoreCase);
    }

    private static string QueryWindowsServiceState(string serviceName)
    {
        var result = RunProcessCaptured(
            "sc.exe",
            $"query \"{serviceName}\"",
            Environment.SystemDirectory,
            TimeSpan.FromSeconds(15));

        if (result.TimedOut)
        {
            return "timeout";
        }

        var output = result.Output;
        if (output.Contains("does not exist", StringComparison.OrdinalIgnoreCase))
        {
            return "missing";
        }

        foreach (var line in output.Split(['\r', '\n'], StringSplitOptions.RemoveEmptyEntries))
        {
            if (!line.Contains("STATE", StringComparison.OrdinalIgnoreCase))
            {
                continue;
            }

            var colonIndex = line.IndexOf(':');
            if (colonIndex < 0)
            {
                continue;
            }

            var right = line[(colonIndex + 1)..].Trim();
            var parts = right.Split(' ', StringSplitOptions.RemoveEmptyEntries);
            if (parts.Length >= 2)
            {
                return parts[1].Trim().ToLowerInvariant();
            }
        }

        return result.ExitCode == 0 ? "unknown" : "error";
    }

    private static bool IsProcessRunningForExecutable(string processName, string executablePath)
    {
        var expectedPath = NormalizeExecutablePath(executablePath);
        foreach (var process in Process.GetProcessesByName(processName))
        {
            try
            {
                var actualPath = NormalizeExecutablePath(GetProcessExecutablePathSafe(process));
                if (string.Equals(actualPath, expectedPath, StringComparison.OrdinalIgnoreCase))
                {
                    return true;
                }
            }
            catch
            {
            }
            finally
            {
                process.Dispose();
            }
        }

        return false;
    }

    private static string GetProcessExecutablePathSafe(Process process)
    {
        try
        {
            return process.MainModule?.FileName ?? string.Empty;
        }
        catch
        {
            IntPtr hProcess = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process.Id);
            if (hProcess != IntPtr.Zero)
            {
                try
                {
                    int size = 1024;
                    StringBuilder builder = new StringBuilder(size);
                    if (QueryFullProcessImageName(hProcess, 0, builder, ref size))
                    {
                        return builder.ToString();
                    }
                }
                catch
                {
                }
                finally
                {
                    CloseHandle(hProcess);
                }
            }
        }
        return string.Empty;
    }

    private static string NormalizeExecutablePath(string path)
    {
        if (string.IsNullOrWhiteSpace(path))
        {
            return string.Empty;
        }

        try
        {
            return Path.GetFullPath(path).TrimEnd('\\', '/');
        }
        catch
        {
            return path.Trim().TrimEnd('\\', '/');
        }
    }

    private static bool TryStartProcessDetached(string fileName, string arguments, string workingDirectory, out string error)
    {
        error = string.Empty;
        try
        {
            using var process = new Process
            {
                StartInfo = new ProcessStartInfo
                {
                    FileName = fileName,
                    Arguments = arguments,
                    WorkingDirectory = workingDirectory,
                    UseShellExecute = false,
                    CreateNoWindow = true
                }
            };
            process.Start();
            return true;
        }
        catch (Exception ex)
        {
            error = ex.GetType().Name;
            return false;
        }
    }

    private static async Task<bool> ProbeTcpPortAsync(
        string host,
        int port,
        TimeSpan timeout,
        CancellationToken cancellationToken)
    {
        try
        {
            using var client = new TcpClient();
            using var probeCancellation = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
            probeCancellation.CancelAfter(timeout);
            await client.ConnectAsync(host, port, probeCancellation.Token);
            return client.Connected;
        }
        catch (OperationCanceledException) when (!cancellationToken.IsCancellationRequested)
        {
            return false;
        }
        catch
        {
            return false;
        }
    }

    private static ProcessCaptureResult RunProcessCaptured(
        string fileName,
        string arguments,
        string workingDirectory,
        TimeSpan timeout)
    {
        using var process = new Process
        {
            StartInfo = new ProcessStartInfo
            {
                FileName = fileName,
                Arguments = arguments,
                WorkingDirectory = workingDirectory,
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                UseShellExecute = false,
                CreateNoWindow = true,
                StandardOutputEncoding = Encoding.UTF8,
                StandardErrorEncoding = Encoding.UTF8
            }
        };

        process.Start();
        var stdoutTask = process.StandardOutput.ReadToEndAsync();
        var stderrTask = process.StandardError.ReadToEndAsync();

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

            try
            {
                process.WaitForExit();
            }
            catch
            {
            }
        }

        Task.WaitAll(stdoutTask, stderrTask);
        var output = CombineOutputs(stdoutTask.Result, stderrTask.Result);
        return new ProcessCaptureResult(exited ? process.ExitCode : -1, !exited, output);
    }

    private static string CombineOutputs(params string[] values)
    {
        return string.Join(
            Environment.NewLine,
            values.Where(value => !string.IsNullOrWhiteSpace(value)).Select(value => value.Trim()));
    }

    private static string ReadConfiguredOutputName(string configPath)
    {
        if (string.IsNullOrWhiteSpace(configPath) || !File.Exists(configPath))
        {
            return string.Empty;
        }

        try
        {
            foreach (var line in File.ReadLines(configPath))
            {
                var trimmed = line.Trim();
                if (!trimmed.StartsWith("output_name", StringComparison.OrdinalIgnoreCase))
                {
                    continue;
                }

                var separatorIndex = trimmed.IndexOf('=');
                if (separatorIndex < 0 || separatorIndex == trimmed.Length - 1)
                {
                    return string.Empty;
                }

                return trimmed[(separatorIndex + 1)..].Trim();
            }
        }
        catch
        {
        }

        return string.Empty;
    }

    private static bool IsMttVddControllerName(string value)
    {
        if (string.IsNullOrWhiteSpace(value))
        {
            return false;
        }

        var lowered = value.Trim().ToLowerInvariant();
        return MttVddControllerHints.Any(hint => lowered.Contains(hint, StringComparison.Ordinal));
    }

    private static string QuoteArgument(string value)
    {
        if (string.IsNullOrWhiteSpace(value))
        {
            return "\"\"";
        }

        if (!value.Contains('"') && !value.Any(char.IsWhiteSpace))
        {
            return value;
        }

        return $"\"{value.Replace("\"", "\\\"", StringComparison.Ordinal)}\"";
    }

    private static long ToUnixMilliseconds(DateTime utcTime)
    {
        if (utcTime.Kind != DateTimeKind.Utc)
        {
            utcTime = utcTime.ToUniversalTime();
        }

        return new DateTimeOffset(utcTime).ToUnixTimeMilliseconds();
    }

    private static string JsonString(JsonElement element, string propertyName)
    {
        if (!element.TryGetProperty(propertyName, out var value) || value.ValueKind != JsonValueKind.String)
        {
            return string.Empty;
        }

        return value.GetString()?.Trim() ?? string.Empty;
    }

    private static bool JsonBool(JsonElement element, string propertyName)
    {
        if (!element.TryGetProperty(propertyName, out var value))
        {
            return false;
        }

        return value.ValueKind switch
        {
            JsonValueKind.True => true,
            JsonValueKind.False => false,
            _ => false
        };
    }

    private static long? JsonLong(JsonElement element, string propertyName)
    {
        if (!element.TryGetProperty(propertyName, out var value))
        {
            return null;
        }

        return value.ValueKind switch
        {
            JsonValueKind.Number when value.TryGetInt64(out var number) => number,
            JsonValueKind.String when long.TryParse(value.GetString(), out var parsed) => parsed,
            _ => null
        };
    }

    private static IEnumerable<JsonElement> JsonArrayObjects(JsonElement element, string propertyName)
    {
        if (!element.TryGetProperty(propertyName, out var value) || value.ValueKind != JsonValueKind.Array)
        {
            return [];
        }

        return value.EnumerateArray().Where(item => item.ValueKind == JsonValueKind.Object).ToArray();
    }

    private static string TrimDetail(string value, int maxLength)
    {
        var trimmed = value?.Trim() ?? string.Empty;
        if (trimmed.Length <= maxLength)
        {
            return trimmed;
        }

        return $"{trimmed[..maxLength]}...";
    }

    private static TimeSpan RandomDelay()
    {
        var minimumSeconds = (int)MinimumCycleDelay.TotalSeconds;
        var maximumSeconds = (int)MaximumCycleDelay.TotalSeconds;
        return TimeSpan.FromSeconds(Random.Shared.Next(minimumSeconds, maximumSeconds + 1));
    }

    private static string ResolveHeartbeatUrl(string? configured)
    {
        var value = FirstNonEmpty(
            configured,
            Environment.GetEnvironmentVariable("CLOUDGIME_KEEP_AWAKE_HEARTBEAT_URL"),
            "https://api.cloudgime.my.id/api/v1/config/global");

        return Uri.TryCreate(value, UriKind.Absolute, out var uri)
            && (uri.Scheme == Uri.UriSchemeHttp || uri.Scheme == Uri.UriSchemeHttps)
                ? uri.ToString()
                : string.Empty;
    }

    private static async Task<string> SendNetworkHeartbeatAsync(string heartbeatUrl, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(heartbeatUrl))
        {
            return "disabled";
        }

        try
        {
            using var request = new HttpRequestMessage(HttpMethod.Get, heartbeatUrl);
            request.Headers.UserAgent.Add(new ProductInfoHeaderValue("CloudgimeHostKeepAwakeAgent", "1.0"));
            request.Headers.CacheControl = new CacheControlHeaderValue
            {
                NoCache = true
            };

            using var response = await Http.SendAsync(request, HttpCompletionOption.ResponseHeadersRead, cancellationToken);
            return $"http-{(int)response.StatusCode}";
        }
        catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
        {
            throw;
        }
        catch (Exception ex)
        {
            return $"error-{ex.GetType().Name}";
        }
    }

    private static NudgeResult TryNudgeSafely(string bundleRoot, TimeSpan inputIdle)
    {
        if (inputIdle < MinimumInputIdleBeforeNudge)
        {
            return NudgeResult.Skipped("recent-input");
        }

        if (IsStreamRecentlyActive(bundleRoot))
        {
            return NudgeResult.Skipped("stream-active");
        }

        if (IsForegroundFullscreen())
        {
            return NudgeResult.Skipped("fullscreen");
        }

        if (!GetCursorPos(out var current))
        {
            return NudgeResult.Skipped("cursor-unavailable");
        }

        var dx = Random.Shared.Next(0, 2) == 0 ? -1 : 1;
        var dy = Random.Shared.Next(0, 2) == 0 ? -1 : 1;
        var moved = SetCursorPos(current.X + dx, current.Y + dy);
        Thread.Sleep(Random.Shared.Next(80, 180));
        var restored = SetCursorPos(current.X, current.Y);
        return moved && restored ? NudgeResult.Performed("one-pixel-restore") : NudgeResult.Skipped("set-cursor-failed");
    }

    private static bool IsStreamRecentlyActive(string bundleRoot)
    {
        var serverRoot = ResolveServerRoot(bundleRoot);
        var candidateFiles = new[]
        {
            Path.Combine(serverRoot, "host_supervisor_state.json"),
            Path.Combine(serverRoot, "runtime_state.json"),
            Path.Combine(serverRoot, "stream_state.json")
        };

        foreach (var file in candidateFiles)
        {
            if (!File.Exists(file))
            {
                continue;
            }

            try
            {
                var lastWrite = File.GetLastWriteTimeUtc(file);
                if (DateTime.UtcNow - lastWrite > RecentlyActiveStreamWindow)
                {
                    continue;
                }

                using var document = JsonDocument.Parse(File.ReadAllBytes(file));
                if (JsonContainsActiveStreamSignal(document.RootElement))
                {
                    return true;
                }
            }
            catch
            {
            }
        }

        return false;
    }

    private static bool JsonContainsActiveStreamSignal(JsonElement element)
    {
        switch (element.ValueKind)
        {
            case JsonValueKind.Object:
                foreach (var property in element.EnumerateObject())
                {
                    if (PropertyLooksActive(property.Name, property.Value))
                    {
                        return true;
                    }

                    if (JsonContainsActiveStreamSignal(property.Value))
                    {
                        return true;
                    }
                }

                break;
            case JsonValueKind.Array:
                foreach (var item in element.EnumerateArray())
                {
                    if (JsonContainsActiveStreamSignal(item))
                    {
                        return true;
                    }
                }

                break;
        }

        return false;
    }

    private static bool PropertyLooksActive(string name, JsonElement value)
    {
        var normalized = name.Replace("_", string.Empty, StringComparison.Ordinal).Replace("-", string.Empty, StringComparison.Ordinal).ToLowerInvariant();
        if (value.ValueKind == JsonValueKind.True
            && (normalized.Contains("stream", StringComparison.Ordinal)
                || normalized.Contains("session", StringComparison.Ordinal)
                || normalized.Contains("connection", StringComparison.Ordinal)))
        {
            return true;
        }

        if (value.ValueKind != JsonValueKind.String)
        {
            return false;
        }

        var text = value.GetString()?.Trim().ToLowerInvariant();
        if (string.IsNullOrWhiteSpace(text))
        {
            return false;
        }

        if (normalized.Contains("phase", StringComparison.Ordinal)
            || normalized.Contains("status", StringComparison.Ordinal)
            || normalized.Contains("state", StringComparison.Ordinal))
        {
            return text is "streaming" or "connected" or "connection_complete" or "client_connected" or "active";
        }

        return false;
    }

    private static bool TryGetInputIdle(out TimeSpan idle)
    {
        var info = new LASTINPUTINFO
        {
            cbSize = (uint)Marshal.SizeOf<LASTINPUTINFO>()
        };

        if (!GetLastInputInfo(ref info))
        {
            idle = TimeSpan.MaxValue;
            return false;
        }

        var currentTick = unchecked((uint)Environment.TickCount);
        var elapsedMs = unchecked(currentTick - info.dwTime);
        idle = TimeSpan.FromMilliseconds(elapsedMs);
        return true;
    }

    private static bool IsForegroundFullscreen()
    {
        var window = GetForegroundWindow();
        if (window == IntPtr.Zero)
        {
            return false;
        }

        if (!GetWindowRect(window, out var windowRect))
        {
            return false;
        }

        var monitor = MonitorFromWindow(window, MonitorDefaultToNearest);
        if (monitor == IntPtr.Zero)
        {
            return false;
        }

        var info = new MONITORINFO
        {
            cbSize = Marshal.SizeOf<MONITORINFO>()
        };

        if (!GetMonitorInfo(monitor, ref info))
        {
            return false;
        }

        var widthDelta = Math.Abs((windowRect.Right - windowRect.Left) - (info.rcMonitor.Right - info.rcMonitor.Left));
        var heightDelta = Math.Abs((windowRect.Bottom - windowRect.Top) - (info.rcMonitor.Bottom - info.rcMonitor.Top));
        var leftDelta = Math.Abs(windowRect.Left - info.rcMonitor.Left);
        var topDelta = Math.Abs(windowRect.Top - info.rcMonitor.Top);
        return widthDelta <= 2 && heightDelta <= 2 && leftDelta <= 2 && topDelta <= 2;
    }

    private static string ResolveBundleRoot(string? configured)
    {
        if (!string.IsNullOrWhiteSpace(configured))
        {
            return Path.GetFullPath(configured);
        }

        var baseDirectory = Path.GetFullPath(AppContext.BaseDirectory);
        var directory = new DirectoryInfo(baseDirectory);
        for (var i = 0; directory is not null && i < 5; i++, directory = directory.Parent)
        {
            if (File.Exists(Path.Combine(directory.FullName, "host-installer.exe"))
                || Directory.Exists(Path.Combine(directory.FullName, "moonlight", "server")))
            {
                return directory.FullName;
            }
        }

        return baseDirectory;
    }

    private static string ResolveServerRoot(string bundleRoot)
    {
        return Path.Combine(bundleRoot, "moonlight", "server");
    }

    private static string FirstNonEmpty(params string?[] values)
    {
        foreach (var value in values)
        {
            if (!string.IsNullOrWhiteSpace(value))
            {
                return value;
            }
        }

        return string.Empty;
    }

    private static bool TextContainsAnyHint(string text, IEnumerable<string> hints)
    {
        if (string.IsNullOrWhiteSpace(text))
        {
            return false;
        }

        return hints.Any(hint => text.Contains(hint, StringComparison.OrdinalIgnoreCase));
    }

    private static void WriteJsonAtomically(string path, object value)
    {
        var directory = Path.GetDirectoryName(path);
        if (!string.IsNullOrWhiteSpace(directory))
        {
            Directory.CreateDirectory(directory);
        }

        var tempPath = $"{path}.tmp";
        File.WriteAllText(tempPath, JsonSerializer.Serialize(value, value.GetType(), JsonOptions));
        File.Copy(tempPath, path, overwrite: true);
        File.Delete(tempPath);
    }

    private static void Log(string message)
    {
        try
        {
            var directory = Path.GetDirectoryName(_logPath);
            if (!string.IsNullOrWhiteSpace(directory))
            {
                Directory.CreateDirectory(directory);
            }

            File.AppendAllText(_logPath, $"[{DateTimeOffset.Now:O}] {message}{Environment.NewLine}");
        }
        catch
        {
        }
    }

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern uint SetThreadExecutionState(uint esFlags);

    [DllImport("kernel32.dll")]
    private static extern uint WTSGetActiveConsoleSessionId();

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern IntPtr OpenProcess(uint processAccess, bool bInheritHandle, int processId);

    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    private static extern bool QueryFullProcessImageName(IntPtr hProcess, uint dwFlags, StringBuilder lpExeName, ref int lpdwSize);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CloseHandle(IntPtr hObject);

    private const uint PROCESS_QUERY_LIMITED_INFORMATION = 0x1000;

    [DllImport("wtsapi32.dll", SetLastError = true)]
    private static extern bool WTSQuerySessionInformation(
        IntPtr hServer,
        uint sessionId,
        int wtsInfoClass,
        out IntPtr ppBuffer,
        out uint pBytesReturned);

    [DllImport("wtsapi32.dll")]
    private static extern void WTSFreeMemory(IntPtr pMemory);

    private const int WTSConnectState = 8;
    private const int WTS_CURRENT_SERVER_HANDLE = 0;

    private enum WTS_CONNECTSTATE_CLASS
    {
        WTSActive,
        WTSConnected,
        WTSConnectQuery,
        WTSShadow,
        WTSDisconnected,
        WTSIdle,
        WTSListen,
        WTSReset,
        WTSDown,
        WTSInit
    }

    private static uint GetActiveConsoleSessionId()
    {
        try
        {
            return WTSGetActiveConsoleSessionId();
        }
        catch (Exception ex)
        {
            Log($"failed to get active console session id: {ex}");
            return 0xFFFFFFFF;
        }
    }

    private static WTS_CONNECTSTATE_CLASS GetSessionConnectState(uint sessionId)
    {
        IntPtr buffer = IntPtr.Zero;
        uint bytesReturned = 0;
        try
        {
            if (WTSQuerySessionInformation((IntPtr)WTS_CURRENT_SERVER_HANDLE, sessionId, WTSConnectState, out buffer, out bytesReturned))
            {
                if (bytesReturned >= sizeof(int))
                {
                    int stateVal = Marshal.ReadInt32(buffer);
                    return (WTS_CONNECTSTATE_CLASS)stateVal;
                }
            }
        }
        catch (Exception ex)
        {
            Log($"failed to query session connect state for session {sessionId}: {ex}");
        }
        finally
        {
            if (buffer != IntPtr.Zero)
            {
                WTSFreeMemory(buffer);
            }
        }
        return WTS_CONNECTSTATE_CLASS.WTSDisconnected;
    }

    [DllImport("user32.dll")]
    private static extern bool GetLastInputInfo(ref LASTINPUTINFO plii);

    [DllImport("user32.dll")]
    private static extern bool GetCursorPos(out POINT lpPoint);

    [DllImport("user32.dll")]
    private static extern bool SetCursorPos(int x, int y);

    [DllImport("user32.dll")]
    private static extern IntPtr GetForegroundWindow();

    [DllImport("user32.dll")]
    private static extern bool GetWindowRect(IntPtr hWnd, out RECT lpRect);

    [DllImport("user32.dll")]
    private static extern IntPtr MonitorFromWindow(IntPtr hwnd, uint dwFlags);

    [DllImport("user32.dll", CharSet = CharSet.Auto)]
    private static extern bool GetMonitorInfo(IntPtr hMonitor, ref MONITORINFO lpmi);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern bool EnumDisplayDevices(string? lpDevice, int iDevNum, ref DISPLAY_DEVICE lpDisplayDevice, int dwFlags);

    private sealed record AgentOptions(AgentMode Mode, string? BundleRoot, string? HeartbeatUrl, bool AllowNudge)
    {
        public static AgentOptions Parse(string[] args)
        {
            var mode = AgentMode.System;
            string? bundleRoot = null;
            string? heartbeatUrl = null;
            var allowNudge = false;

            for (var i = 0; i < args.Length; i++)
            {
                var arg = args[i];
                switch (arg)
                {
                    case "--bundle-root" when i + 1 < args.Length:
                        bundleRoot = args[++i];
                        break;
                    case "--heartbeat-url" when i + 1 < args.Length:
                        heartbeatUrl = args[++i];
                        break;
                    case "--mode" when i + 1 < args.Length:
                        mode = string.Equals(args[++i], "user", StringComparison.OrdinalIgnoreCase)
                            ? AgentMode.User
                            : AgentMode.System;
                        break;
                    case "--allow-nudge":
                        allowNudge = true;
                        break;
                    case "--no-nudge":
                        allowNudge = false;
                        break;
                }
            }

            return new AgentOptions(mode, bundleRoot, heartbeatUrl, allowNudge);
        }
    }

    private enum AgentMode
    {
        System,
        User
    }

    private enum HostSelfHealAction
    {
        None,
        StartService,
        RestartRuntime,
        PrepareHost
    }

    private sealed record KeepAwakeState(
        int Version,
        string Mode,
        long UpdatedAtUnixMs,
        long NextCycleAtUnixMs,
        int CycleSeconds,
        bool PowerRequestRefreshed,
        string NetworkHeartbeat,
        double? InputIdleSeconds,
        string NudgeStatus,
        string NudgeReason,
        string? SelfHealStatus,
        string? SelfHealAction,
        string? SelfHealReason,
        int? SelfHealConsecutiveUnhealthyCycles,
        string? SelfHealHostServiceState,
        string? SelfHealRuntimeServiceState,
        bool? SelfHealLocalHttpReady,
        bool? SelfHealSunshineReady,
        string? SelfHealLifecyclePhase);

    private sealed record NudgeResult(string Status, string Reason)
    {
        public static NudgeResult NotAttempted(string reason) => new("not_attempted", reason);

        public static NudgeResult Skipped(string reason) => new("skipped", reason);

        public static NudgeResult Performed(string reason) => new("performed", reason);
    }

    private sealed record KeeperTunnelKeepaliveResult(string Status, string Reason)
    {
        public static KeeperTunnelKeepaliveResult NotApplicable() => new("not_applicable", "user_mode");

        public static KeeperTunnelKeepaliveResult Skipped(string reason) => new("skipped", reason);

        public static KeeperTunnelKeepaliveResult Healthy(string reason) => new("healthy", reason);

        public static KeeperTunnelKeepaliveResult ActionOk(string reason) => new("action_ok", reason);

        public static KeeperTunnelKeepaliveResult ActionFailed(string reason) => new("action_failed", reason);
    }

    private sealed record HostHealthAssessment(
        bool Healthy,
        string Assessment,
        string Reason,
        HostSelfHealAction RecommendedAction);

    private sealed record HostSelfHealCycleResult(
        string Status,
        string Action,
        string Reason,
        int ConsecutiveUnhealthyCycles,
        string HostServiceState,
        string RuntimeServiceState,
        bool LocalHttpReady,
        bool SunshineReady,
        string LifecyclePhase)
    {
        public static HostSelfHealCycleResult NotApplicable() => new(
            "not_applicable",
            "none",
            "user_mode",
            0,
            string.Empty,
            string.Empty,
            false,
            false,
            string.Empty);
    }

    private sealed record HostActionExecutionResult(
        string Action,
        bool Attempted,
        bool Success,
        string Status,
        string Detail)
    {
        public static HostActionExecutionResult NotAttempted(string action, string detail) => new(
            action,
            false,
            false,
            "not_attempted",
            detail);
    }

    private sealed record ProcessCaptureResult(int ExitCode, bool TimedOut, string Output);

    private sealed class HostHealthSnapshot
    {
        public long ObservedAtUnixMs { get; set; }
        public bool HostInstallerPresent { get; set; }
        public bool RuntimeAgentPresent { get; set; }
        public string HostServiceState { get; set; } = string.Empty;
        public string RuntimeServiceState { get; set; } = string.Empty;
        public bool LocalHttpReady { get; set; }
        public bool SunshineReady { get; set; }
        public bool StreamRecentlyActive { get; set; }
        public bool SupervisorStateFresh { get; set; }
        public bool WithinStartupGrace { get; set; }
        public bool MttVddSatisfied { get; set; }
        public string MttVddReason { get; set; } = string.Empty;
        public string CaptureIssueHint { get; set; } = string.Empty;
        public string LifecyclePhase { get; set; } = string.Empty;
        public string LifecycleReason { get; set; } = string.Empty;
        public string ConfiguredOutputName { get; set; } = string.Empty;
        public DesktopDisplayTopologySnapshot DisplayTopology { get; set; } = new();
        public string DisplayTopologySummary { get; set; } = string.Empty;
        public bool DisplayStableForMttVdd { get; set; }
        public HostSupervisorSnapshot Supervisor { get; set; } = new();
        public CapabilityProfileSnapshot Capability { get; set; } = new();
        public TaskHealthSnapshot TaskHealth { get; set; } = new();
    }

    private sealed class DesktopDisplayTopologySnapshot
    {
        public bool ReadSucceeded { get; set; }
        public bool UsedScreenFallback { get; set; }
        public int ActiveDisplayCount { get; set; }
        public int PrimaryActiveDisplayCount { get; set; }
        public string PrimaryActiveDisplayName { get; set; } = string.Empty;
        public bool PrimaryActiveDisplayLooksLikeMttVdd { get; set; }
        public bool PrimaryActiveDisplayLooksLikeParsec { get; set; }
        public bool PrimaryActiveDisplayLooksLikeQemu { get; set; }
        public string SingleActiveDisplayName { get; set; } = string.Empty;
        public bool SingleActiveDisplayLooksLikeMttVdd { get; set; }
        public bool SingleActiveDisplayLooksLikeParsec { get; set; }
        public bool SingleActiveDisplayLooksLikeQemu { get; set; }
        public string Summary { get; set; } = string.Empty;
        public List<DesktopDisplaySnapshot> AllDisplays { get; set; } = [];
    }

    private sealed class DesktopDisplaySnapshot
    {
        public string DeviceName { get; set; } = string.Empty;
        public string DeviceString { get; set; } = string.Empty;
        public string DeviceId { get; set; } = string.Empty;
        public bool AttachedToDesktop { get; set; }
        public bool Primary { get; set; }
        public bool MirroringDriver { get; set; }
    }

    private sealed class HostSupervisorSnapshot
    {
        public bool Exists { get; set; }
        public string ParseError { get; set; } = string.Empty;
        public string LifecyclePhase { get; set; } = string.Empty;
        public string LifecycleReason { get; set; } = string.Empty;
        public string LastFailureRecoveryReason { get; set; } = string.Empty;
        public string LastServiceWatchdogReason { get; set; } = string.Empty;
        public string LastCommandError { get; set; } = string.Empty;
        public long? DaemonStartedAtUnixMs { get; set; }
        public long? UpdatedAtUnixMs { get; set; }
    }

    private sealed class CapabilityProfileSnapshot
    {
        public bool Exists { get; set; }
        public string ParseError { get; set; } = string.Empty;
        public string SelectedCapture { get; set; } = string.Empty;
        public string SelectedCaptureReason { get; set; } = string.Empty;
        public string SelectedRuntimeKey { get; set; } = string.Empty;
        public string SelectedEncoder { get; set; } = string.Empty;
        public string ConfiguredOutputName { get; set; } = string.Empty;
        public bool HasMttVddController { get; set; }
        public List<string> GpuControllerNames { get; set; } = [];
    }

    private sealed class TaskHealthSnapshot
    {
        public bool Exists { get; set; }
        public string ParseError { get; set; } = string.Empty;
        public bool PolicyValid { get; set; }
        public bool DaemonRunning { get; set; }
    }

    private sealed class HostSelfHealState
    {
        public int SchemaVersion { get; set; } = 1;
        public long UpdatedAtUnixMs { get; set; }
        public long? LastHealthyAtUnixMs { get; set; }
        public string LastAssessment { get; set; } = "unknown";
        public string LastAssessmentReason { get; set; } = string.Empty;
        public string LastAction { get; set; } = "none";
        public string LastActionReason { get; set; } = string.Empty;
        public bool LastActionSucceeded { get; set; }
        public long? LastActionAtUnixMs { get; set; }
        public int ConsecutiveUnhealthyCycles { get; set; }
        public long? RecoveryWindowStartedAtUnixMs { get; set; }
        public int RecoveryWindowAttemptCount { get; set; }
        public string LastObservedLifecyclePhase { get; set; } = string.Empty;
        public string LastObservedLifecycleReason { get; set; } = string.Empty;
        public string LastObservedHostServiceState { get; set; } = string.Empty;
        public string LastObservedRuntimeServiceState { get; set; } = string.Empty;
        public bool LastObservedLocalHttpReady { get; set; }
        public bool LastObservedSunshineReady { get; set; }
        public bool LastObservedMttVddSatisfied { get; set; }
        public string LastObservedConfiguredOutputName { get; set; } = string.Empty;
        public string LastObservedDisplayTopologySummary { get; set; } = string.Empty;
        public bool LastObservedDisplayStableForMttVdd { get; set; }
        public List<HostSelfHealHistoryItem> RecentActions { get; set; } = [];
    }

    private sealed class HostSelfHealHistoryItem
    {
        public long AtUnixMs { get; set; }
        public string Status { get; set; } = string.Empty;
        public string Action { get; set; } = string.Empty;
        public string Reason { get; set; } = string.Empty;
        public bool Success { get; set; }
        public string HostServiceState { get; set; } = string.Empty;
        public string RuntimeServiceState { get; set; } = string.Empty;
        public bool LocalHttpReady { get; set; }
        public bool SunshineReady { get; set; }
        public string LifecyclePhase { get; set; } = string.Empty;
        public string ConfiguredOutputName { get; set; } = string.Empty;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct LASTINPUTINFO
    {
        public uint cbSize;
        public uint dwTime;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct POINT
    {
        public int X;
        public int Y;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct RECT
    {
        public int Left;
        public int Top;
        public int Right;
        public int Bottom;
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Auto)]
    private struct MONITORINFO
    {
        public int cbSize;
        public RECT rcMonitor;
        public RECT rcWork;
        public uint dwFlags;
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct DISPLAY_DEVICE
    {
        public int cb;

        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)]
        public string DeviceName;

        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 128)]
        public string DeviceString;

        public int StateFlags;

        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 128)]
        public string DeviceID;

        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 128)]
        public string DeviceKey;
    }
}
