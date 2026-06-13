import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { openPath, openUrl } from "@tauri-apps/plugin-opener";
import {
  Activity,
  ArrowUpRight,
  Copy,
  FolderOpen,
  Globe,
  HeartPulse,
  KeyRound,
  LaptopMinimal,
  LifeBuoy,
  Lock,
  Play,
  RefreshCw,
  ServerCog,
  Settings2,
  ShieldCheck,
  ShieldEllipsis,
  Square,
  Volume2,
  Wifi,
  Wrench,
} from "lucide-react";
import "./App.css";
import {
  changeAdminPassword,
  getShellState,
  lockApp,
  launchEmergencyUninstaller,
  claimSetupToken,
  recoverHostActivation,
  resetLocalHostIdentity,
  runPreflightHost,
  runHostAction,
  saveAudioPreferences,
  saveDisplayPreferences,
  savePreferences,
  sendHeartbeat,
  setAdminPassword,
  syncHostBinding,
  toggleDualStream,
  uninstallInstalledHost,
  unlockApp,
  uploadHostDiagnostic,
} from "./api";
import type {
  ActionOutcome,
  HostUserDaemonTaskHealth,
  RailKey,
  ShellState,
  WindowsNativeDiagnosticReportEntry,
} from "./types";

type UiLanguage = "id" | "en";

const UI_LANGUAGE_STORAGE_KEY = "cloudgime.host-control.ui-language";
let activeUiLanguage: UiLanguage = "id";

function buildRailItems(): { key: RailKey; label: string; icon: typeof Settings2 }[] {
  return [
    { key: "operator", label: bi("Operator", "Operator"), icon: Settings2 },
    { key: "access", label: bi("Akses", "Access"), icon: Globe },
    { key: "audio", label: bi("Audio", "Audio"), icon: Volume2 },
    { key: "display", label: bi("Display", "Display"), icon: LaptopMinimal },
    { key: "health", label: bi("Kesehatan", "Health"), icon: HeartPulse },
    { key: "maintenance", label: bi("Perawatan", "Maintenance"), icon: Wrench },
    { key: "support", label: bi("Bantuan", "Support"), icon: LifeBuoy },
    { key: "admin", label: bi("Admin", "Admin"), icon: ShieldEllipsis },
  ];
}

type HeroTone = "success" | "warning" | "critical" | "neutral";
type RecommendedAction =
  | "setup"
  | "open_token"
  | "start_host"
  | "send_heartbeat"
  | "open_public_url"
  | "none";

type ActivationProgressMode = "submitting" | "confirming";
type ActivationProgressStage =
  | "verify_token"
  | "binding_host"
  | "starting_runtime"
  | "ready_for_stream";

type ActivationProgress = {
  mode: ActivationProgressMode;
  stage: ActivationProgressStage;
  title: string;
  detail: string;
  secondsRemaining: number;
};

function App() {
  const [language, setLanguage] = useState<UiLanguage>(() => {
    try {
      const stored = window.localStorage.getItem(UI_LANGUAGE_STORAGE_KEY);
      return stored === "en" ? "en" : "id";
    } catch {
      return "id";
    }
  });
  activeUiLanguage = language;
  const [shell, setShell] = useState<ShellState | null>(null);
  const [bootError, setBootError] = useState("");
  const [activeRail, setActiveRail] = useState<RailKey>("operator");
  const [busyLabel, setBusyLabel] = useState("");
  const [lastRefreshedAt, setLastRefreshedAt] = useState(() => new Date().toISOString());
  const [activity, setActivity] = useState<string[]>([
    bi("Menunggu shell Host Control dimuat.", "Waiting for the Host Control shell to load."),
  ]);
  const [toast, setToast] = useState("");
  const [setupTokenInput, setSetupTokenInput] = useState("");
  const [displayNameInput, setDisplayNameInput] = useState("");
  const [controlPlaneInput, setControlPlaneInput] = useState("https://cloudgime.my.id");
  const [audioModeInput, setAudioModeInput] = useState("auto");
  const [audioSinkInput, setAudioSinkInput] = useState("");
  const [virtualSinkInput, setVirtualSinkInput] = useState("");
  const [microphoneInput, setMicrophoneInput] = useState("");
  const [displayModeInput, setDisplayModeInput] = useState("mtt_vdd");
  const [displayCustomDeviceNameInput, setDisplayCustomDeviceNameInput] = useState("");
  const [displayCustomDeviceIdInput, setDisplayCustomDeviceIdInput] = useState("");
  const [displayCustomLabelInput, setDisplayCustomLabelInput] = useState("");
  const [unlockPasswordInput, setUnlockPasswordInput] = useState("");
  const [setupPasswordInput, setSetupPasswordInput] = useState("");
  const [setupPasswordConfirmInput, setSetupPasswordConfirmInput] = useState("");
  const [changePasswordInput, setChangePasswordInput] = useState("");
  const [changePasswordConfirmInput, setChangePasswordConfirmInput] = useState("");
  const [uninstallPasswordInput, setUninstallPasswordInput] = useState("");
  const [showUninstallConfirm, setShowUninstallConfirm] = useState(false);
  const [showEmergencyKillConfirm, setShowEmergencyKillConfirm] = useState(false);
  const [emergencyKillPhrase, setEmergencyKillPhrase] = useState("");
  const [activationProgress, setActivationProgress] = useState<ActivationProgress | null>(null);
  const lastBindingSyncKey = useRef("");
  const handledLaunchIntent = useRef("");
  const toastTimer = useRef<number | null>(null);

  useEffect(() => {
    void refreshState("Host Control shell ready.");
  }, []);

  useEffect(() => {
    try {
      window.localStorage.setItem(UI_LANGUAGE_STORAGE_KEY, language);
    } catch {}
  }, [language]);

  useEffect(() => {
    if (!shell?.auth.unlocked || busyLabel) {
      return;
    }

    const timer = window.setInterval(() => {
      void refreshState(undefined, true);
    }, 15000);

    return () => window.clearInterval(timer);
  }, [shell?.auth.unlocked, busyLabel]);

  useEffect(() => {
    if (!shell) {
      return;
    }

    setDisplayNameInput(shell.activation.displayName);
    setControlPlaneInput(shell.activation.controlPlaneUrl || "https://cloudgime.my.id");
  }, [shell?.activation.displayName, shell?.activation.controlPlaneUrl]);

  useEffect(() => {
    if (!shell) {
      return;
    }

    setAudioModeInput(shell.audio.mode || "auto");
    setAudioSinkInput(shell.audio.selectedAudioSinkName || "");
    setVirtualSinkInput(shell.audio.selectedVirtualSinkName || "");
    setMicrophoneInput(shell.audio.selectedMicrophoneName || "");
  }, [
    shell?.audio.mode,
    shell?.audio.selectedAudioSinkName,
    shell?.audio.selectedVirtualSinkName,
    shell?.audio.selectedMicrophoneName,
  ]);

  useEffect(() => {
    if (!shell) {
      return;
    }

    setDisplayModeInput(shell.display.mode || "mtt_vdd");
    setDisplayCustomDeviceNameInput(shell.display.customDeviceName || "");
    setDisplayCustomDeviceIdInput(shell.display.customDeviceId || "");
    setDisplayCustomLabelInput(shell.display.customLabel || "");
  }, [
    shell?.display.mode,
    shell?.display.customDeviceName,
    shell?.display.customDeviceId,
    shell?.display.customLabel,
  ]);

  useEffect(() => {
    const el = document.querySelector(".content");
    if (el) {
      el.scrollTop = 0;
    }
  }, [activeRail]);

  useEffect(() => {
    if (!shell?.install.launchIntent || handledLaunchIntent.current === shell.install.launchIntent) {
      return;
    }

    handledLaunchIntent.current = shell.install.launchIntent;
    if (shell.install.launchIntent === "uninstall") {
      setActiveRail("admin");
      appendActivity(
        bi(
          "Sesi ini dibuka dari Apps & Features untuk uninstall.",
          "This session was opened from Apps & Features for uninstall.",
        ),
      );
      pushToast(
        bi(
          "Buka kunci app, lalu konfirmasi uninstall dari menu Admin.",
          "Unlock the app, then confirm uninstall from Admin.",
        ),
      );
    }
  }, [shell?.install.launchIntent]);

  useEffect(() => {
    if (!activationProgress) {
      return;
    }

    const timer = window.setInterval(() => {
      setActivationProgress((current) =>
        current
          ? {
              ...current,
              secondsRemaining: Math.max(0, current.secondsRemaining - 1),
            }
          : null,
      );
    }, 1000);

    return () => window.clearInterval(timer);
  }, [activationProgress]);

  useEffect(() => {
    if (!shell?.auth.unlocked || !shell.install.installedMode) {
      return;
    }

    function handleEmergencyShortcut(event: KeyboardEvent) {
      if (!(event.ctrlKey && event.shiftKey && event.key === "Delete")) {
        return;
      }

      event.preventDefault();
      setEmergencyKillPhrase("");
      setShowUninstallConfirm(false);
      setShowEmergencyKillConfirm(true);
      setActiveRail("admin");
      pushToast(
        bi(
          "Shortcut EmergencyKill aktif. Ketik EMERGENCYKILL untuk lanjut.",
          "EmergencyKill shortcut opened. Type EMERGENCYKILL to continue.",
        ),
      );
    }

    window.addEventListener("keydown", handleEmergencyShortcut);
    return () => window.removeEventListener("keydown", handleEmergencyShortcut);
  }, [shell?.auth.unlocked, shell?.install.installedMode]);

  const hero = useMemo(() => buildHero(shell), [shell, language]);
  const stageState = useMemo(() => buildStageState(shell), [shell]);
  const recommendedStep = useMemo(() => buildRecommendedStep(shell), [shell, language]);
  const railItems = useMemo(() => buildRailItems(), [language]);
  const showLockGate = Boolean(shell && (shell.auth.needsPasswordSetup || !shell.auth.unlocked));

  const languageSwitch = (
    <div
      className="language-switch"
      role="group"
      aria-label={bi("Ganti bahasa", "Switch language")}
    >
      <button
        type="button"
        className={`language-button ${language === "id" ? "active" : ""}`}
        onClick={() => setLanguage("id")}
      >
        ID
      </button>
      <button
        type="button"
        className={`language-button ${language === "en" ? "active" : ""}`}
        onClick={() => setLanguage("en")}
      >
        EN
      </button>
    </div>
  );

  function applyShellState(next: ShellState) {
    setShell(next);
    setLastRefreshedAt(new Date().toISOString());
  }

  function buildBindingSyncKey(next: ShellState | null) {
    if (!next) return "";
    return [
      next.activation.hostId,
      next.activation.displayName,
      next.activation.sentinelPcId,
      next.activation.sentinelDeviceId,
      next.activation.keeperEntryId,
      next.activation.controlPlaneUrl,
    ].join("|");
  }

  async function syncBindingIfNeeded(next: ShellState | null, force = false) {
    if (!next || !next.activation.hostId) return;
    if (
      !next.activation.displayName &&
      !next.activation.sentinelPcId &&
      !next.activation.sentinelDeviceId &&
      !next.activation.keeperEntryId
    ) {
      return;
    }

    const syncKey = buildBindingSyncKey(next);
    if (!force && syncKey && lastBindingSyncKey.current === syncKey) {
      return;
    }

    const synced = await syncHostBinding();
    applyShellState(synced);
    lastBindingSyncKey.current = buildBindingSyncKey(synced);
  }

  async function refreshState(note?: string, silent = false) {
    try {
      setBootError("");
      const next = await getShellState();
      applyShellState(next);
      try {
        await syncBindingIfNeeded(next, false);
      } catch {}
      if (note && !silent) {
        appendActivity(note);
      }
    } catch (error) {
      const message = extractErrorMessage(error);
      if (!shell) {
        setBootError(message);
      }
      if (!silent) {
        pushToast(message);
      }
    }
  }

  async function runAction(action: Parameters<typeof runHostAction>[0], busyText: string) {
    try {
      setBusyLabel(busyText);
      const outcome = await runHostAction(action);
      handleOutcome(outcome);
    } catch (error) {
      pushToast(extractErrorMessage(error));
    } finally {
      setBusyLabel("");
    }
  }

  async function runPreflight(fix: boolean) {
    try {
      setBusyLabel(
        fix
          ? bi("Menjalankan perbaikan preflight...", "Running preflight fixes...")
          : bi("Menjalankan pemeriksaan preflight...", "Running preflight checks..."),
      );
      const outcome = await runPreflightHost({ fix });
      handleOutcome(outcome);
    } catch (error) {
      pushToast(extractErrorMessage(error));
    } finally {
      setBusyLabel("");
    }
  }

  async function handleSavePreferences() {
    try {
      setBusyLabel(bi("Menyimpan pengaturan host...", "Saving host settings..."));
      const next = await savePreferences({
        displayName: displayNameInput,
        controlPlaneUrl: controlPlaneInput,
      });
      applyShellState(next);
      appendActivity(
        bi("Identitas host dan control plane tersimpan.", "Host identity and control plane saved."),
      );
      pushToast(bi("Pengaturan host tersimpan.", "Host settings saved."));
    } catch (error) {
      pushToast(extractErrorMessage(error));
    } finally {
      setBusyLabel("");
    }
  }

  async function handleSaveAudioRoute(modeOverride?: "auto" | "manual") {
    const resolvedMode = modeOverride ?? (audioModeInput === "manual" ? "manual" : "auto");

    if (resolvedMode === "manual") {
      if (!audioSinkInput.trim()) {
        pushToast(bi("Pilih output audio dulu.", "Pick an audio output first."));
        return;
      }
      if (!virtualSinkInput.trim()) {
        pushToast(bi("Pilih virtual sink dulu.", "Pick a virtual sink first."));
        return;
      }
      if (!microphoneInput.trim()) {
        pushToast(bi("Pilih input mikrofon dulu.", "Pick a microphone/input first."));
        return;
      }
    }

    try {
      setBusyLabel(bi("Menyimpan route audio...", "Saving audio route..."));
      const outcome = await saveAudioPreferences({
        mode: resolvedMode,
        selectedAudioSinkName: resolvedMode === "manual" ? audioSinkInput : "",
        selectedVirtualSinkName: resolvedMode === "manual" ? virtualSinkInput : "",
        selectedMicrophoneName: resolvedMode === "manual" ? microphoneInput : "",
      });
      handleOutcome(outcome);
    } catch (error) {
      pushToast(extractErrorMessage(error));
    } finally {
      setBusyLabel("");
    }
  }

  async function handleSaveDisplayRoute(modeOverride?: string) {
    const resolvedMode = modeOverride ?? displayModeInput;
    if (
      resolvedMode === "custom" &&
      !displayCustomDeviceNameInput.trim() &&
      !displayCustomDeviceIdInput.trim() &&
      !displayCustomLabelInput.trim()
    ) {
      pushToast(
        bi(
          "Custom display butuh Device Name, Device ID, atau label.",
          "Custom display needs a device name, device id, or label.",
        ),
      );
      return;
    }

    try {
      setBusyLabel(bi("Menyimpan target display...", "Saving display target..."));
      const outcome = await saveDisplayPreferences({
        mode: resolvedMode,
        customDeviceName: resolvedMode === "custom" ? displayCustomDeviceNameInput : "",
        customDeviceId: resolvedMode === "custom" ? displayCustomDeviceIdInput : "",
        customLabel: resolvedMode === "custom" ? displayCustomLabelInput : "",
      });
      handleOutcome(outcome);
    } catch (error) {
      pushToast(extractErrorMessage(error));
    } finally {
      setBusyLabel("");
    }
  }

  async function handleToggleDualStream() {
    if (!shell) return;
    const targetVal = !shell.display.dualStreamEnabled;
    const labelText = targetVal
      ? bi("Mengaktifkan Mode Sewa...", "Enabling Rental Mode...")
      : bi("Menonaktifkan Mode Sewa...", "Disabling Rental Mode...");
    try {
      setBusyLabel(labelText);
      const outcome = await toggleDualStream(targetVal);
      handleOutcome(outcome);
    } catch (error) {
      pushToast(extractErrorMessage(error));
    } finally {
      setBusyLabel("");
    }
  }

  async function handleOpenTokenPage() {
    try {
      setBusyLabel(bi("Menyimpan identitas host...", "Saving host identity..."));
      const next = await savePreferences({
        displayName: displayNameInput,
        controlPlaneUrl: controlPlaneInput,
      });
      applyShellState(next);
      try {
        await syncBindingIfNeeded(next, true);
      } catch {}
      appendActivity(bi("Identitas host tersimpan.", "Host identity saved."));
      await openMaybe(
        buildTokenPageUrl(next, displayNameInput, controlPlaneInput),
        bi("halaman token", "token page"),
      );
    } catch (error) {
      pushToast(extractErrorMessage(error));
    } finally {
      setBusyLabel("");
    }
  }

  function clearSetupTokenInputs() {
    setSetupTokenInput("");
  }

  async function handleClaimSetupToken() {
    const raw = setupTokenInput;
    const normalized = raw.replace(/\s+/g, "").trim();
    setSetupTokenInput(normalized);
    if (normalized.length < 12) {
      pushToast(bi("Paste token host yang valid dulu.", "Paste a valid host token first."));
      return;
    }
    if (activationProgress) {
      return;
    }

    setActivationProgress(createActivationProgress("verify_token", 18));

    let submitState = shell;
    let submitError = "";
    try {
      setBusyLabel(bi("Mengklaim setup token...", "Claiming setup token..."));
      const outcome = await claimSetupToken({ setupToken: normalized, expectedTokenKind: "" });
      submitState = outcome.state;
      applyShellState(outcome.state);
      appendActivity(outcome.message);
      clearSetupTokenInputs();
      setActivationProgress(createActivationProgress("binding_host", 4));
    } catch (error) {
      try {
        submitState = await getShellState();
        applyShellState(submitState);
      } catch {}
      submitError = extractErrorMessage(error);
    } finally {
      setBusyLabel("");
    }

    const completed =
      finishActivationIfAccepted(submitState, submitError, "setup") ||
      (shouldMonitorActivationFlow(submitState, submitError)
        ? await monitorActivationFlow(submitState, submitError)
        : false);
    if (completed) {
      await autoCompleteHostAfterToken();
      return;
    }
    if (!completed) {
      setActivationProgress(null);
      void handleUploadDiagnostic(
        submitError
          ? `Setup token claim did not complete: ${submitError}`
          : "Setup token claim did not complete after monitor flow.",
        true,
      );
      pushToast(
        submitError ||
          bi(
            "Claim setup token belum selesai. Tunggu beberapa detik lalu klik Refresh Status bila perlu.",
            "Setup token claim did not finish yet. Wait a few seconds, then click Refresh Status if needed.",
          ),
      );
    }
  }

  function renderSetupTokenClaimControls(buttonTone: "ghost" | "success") {
    const disabled = Boolean(activationProgress);
    return (
      <div className="compact-top-gap">
        <div className="token-lane-row">
          <input
            className="token-input"
            value={setupTokenInput}
            onChange={(event) => setSetupTokenInput(event.currentTarget.value)}
            disabled={disabled}
            onKeyDown={(event) => {
              if (event.key === "Enter" && setupTokenInput.trim().length >= 12) {
                void handleClaimSetupToken();
              }
            }}
            placeholder={bi("Paste Setup / Pairing Token disini", "Paste Setup / Pairing Token here")}
          />
          <button
            type="button"
            className={`action-button ${buttonTone}`}
            onClick={() => void handleClaimSetupToken()}
            disabled={setupTokenInput.trim().length < 12 || disabled}
          >
            <KeyRound size={16} />
            {bi("Claim Token", "Claim Token")}
          </button>
        </div>
      </div>
    );
  }

  async function handleResetLocalHostIdentity() {
    if (activationProgress || busyLabel) {
      return;
    }
    const confirmed = window.confirm(
      bi(
        "Reset identitas lokal host ini? Token/runtime lama akan dikosongkan, route publik lama dibersihkan, lalu host harus memakai token Instance Pair atau Always-On Host baru.",
        "Reset this host's local identity? The old token/runtime state will be cleared, the old public route will be emptied, and this host must use a fresh Instance Pair or Always-On Host token.",
      ),
    );
    if (!confirmed) {
      return;
    }

    setBusyLabel(bi("Mereset identitas lokal...", "Resetting local identity..."));
    try {
      const outcome = await resetLocalHostIdentity();
      clearSetupTokenInputs();
      applyShellState(outcome.state);
      appendActivity(outcome.message);
      pushToast(bi("Identitas lokal direset. Paste token host baru.", "Local identity reset. Paste a fresh host token."));
    } catch (error) {
      pushToast(extractErrorMessage(error));
    } finally {
      setBusyLabel("");
    }
  }

  async function handleRecoverHostActivation() {
    if (activationProgress || busyLabel) {
      return;
    }
    setBusyLabel(bi("Memulihkan aktivasi...", "Recovering activation..."));
    try {
      const outcome = await recoverHostActivation();
      applyShellState(outcome.state);
      appendActivity(outcome.message);
      pushToast(outcome.message);
    } catch (error) {
      const message = extractErrorMessage(error);
      appendActivity(`${bi("Recovery aktivasi gagal", "Activation recovery failed")}: ${message}`);
      void handleUploadDiagnostic(`Activation recovery failed: ${message}`, true);
      pushToast(message);
    } finally {
      setBusyLabel("");
    }
  }

  async function handleUploadDiagnostic(reason?: string, silent = false) {
    const summary = reason?.trim() || bi("Manual diagnostic dari operator Host App.", "Manual diagnostic from Host App operator.");
    if (!silent) {
      setBusyLabel(bi("Mengirim diagnostic...", "Uploading diagnostic..."));
    }
    try {
      const outcome = await uploadHostDiagnostic({ reason: summary });
      if (outcome.state?.bundleRoot) {
        applyShellState(outcome.state);
      }
      appendActivity(outcome.message);
      if (!silent) {
        pushToast(outcome.message);
      }
      return true;
    } catch (error) {
      const message = extractErrorMessage(error);
      appendActivity(`${bi("Diagnostic gagal dikirim", "Diagnostic upload failed")}: ${message}`);
      if (!silent) {
        pushToast(message);
      }
      return false;
    } finally {
      if (!silent) {
        setBusyLabel("");
      }
    }
  }

  function shouldMonitorActivationFlow(
    next: ShellState | null,
    submitError: string,
  ) {
    if (next?.activation.phase === "activated") {
      return true;
    }

    if (!submitError) {
      return true;
    }

    return submitError.toLowerCase().includes("already activated");
  }

  function finishActivationIfAccepted(
    next: ShellState | null,
    submitError: string,
    source: "setup" | "manual",
  ) {
    const alreadyActivated = submitError.toLowerCase().includes("already activated");
    if (next?.activation.phase !== "activated" && !alreadyActivated) {
      return false;
    }

    clearSetupTokenInputs();
    setActivationProgress(null);
    const message =
      source === "setup"
        ? bi(
            "Setup token diterima. Host Control sedang menyiapkan runtime otomatis.",
            "Setup token accepted. Host Control is preparing the runtime automatically.",
          )
        : bi(
            "Aktivasi selesai. Host Control sedang menyiapkan runtime otomatis.",
            "Activation complete. Host Control is preparing the runtime automatically.",
          );
    appendActivity(message);
    pushToast(message);
    return true;
  }

  async function autoCompleteHostAfterToken() {
    setActivationProgress(createActivationProgress("starting_runtime", 20));
    try {
      let latestState = await getShellState();
      applyShellState(latestState);

      if (
        latestState.activation.phase === "activated" &&
        !(
          latestState.runtime.lifecyclePhase === "ready" &&
          latestState.runtime.requiredProcessesReady &&
          latestState.network.publicUrl
        ) &&
        canOperateLocalRuntime(latestState)
      ) {
        try {
          const startOutcome = await runHostAction("start_host");
          latestState = startOutcome.state;
          applyShellState(startOutcome.state);
          appendActivity(startOutcome.message);
        } catch (error) {
          appendActivity(
            `${bi("Start otomatis belum selesai", "Automatic start is not finished yet")}: ${extractErrorMessage(error)}`,
          );
        }
      }

      await waitForRuntimeReadiness(
        bi(
          "Runtime host sudah hidup otomatis dan readiness sudah dikirim.",
          "Host runtime started automatically and readiness was sent.",
        ),
        bi(
          "Runtime host masih warming up otomatis.",
          "Host runtime is still warming up automatically.",
        ),
      );
    } catch (error) {
      const message = `${bi("Auto-start host belum selesai", "Host auto-start is not finished yet")}: ${extractErrorMessage(error)}`;
      appendActivity(message);
      pushToast(message);
    } finally {
      setActivationProgress(null);
    }
  }

  async function monitorActivationFlow(
    initialState: ShellState | null,
    submitError: string,
  ) {
    let latestState = initialState;
    const totalAttempts = 15;

    for (let attempt = 0; attempt < totalAttempts; attempt += 1) {
      if (attempt > 0 || !latestState) {
        await delayMs(2000);
        try {
          latestState = await getShellState();
          applyShellState(latestState);
        } catch {
          continue;
        }
      } else {
        applyShellState(latestState);
      }

      const secondsRemaining = Math.max(0, (totalAttempts - attempt - 1) * 2);
      if (latestState.activation.phase === "activated") {
          clearSetupTokenInputs();
        if (
          latestState.runtime.lifecyclePhase === "ready" &&
          latestState.runtime.requiredProcessesReady &&
          latestState.network.publicUrl
        ) {
          setActivationProgress(
            createActivationProgress("ready_for_stream", Math.max(secondsRemaining, 4)),
          );
          try {
            const heartbeatOutcome = await sendHeartbeat();
            applyShellState(heartbeatOutcome.state);
          } catch {}
          setActivationProgress(null);
          appendActivity(
            bi(
              "Siap untuk stream. Runtime aktif dan heartbeat terakhir sudah dikirim.",
              "Ready for stream. Runtime is active and the latest heartbeat was sent.",
            ),
          );
          pushToast(bi("Siap untuk stream. Host siap dipakai.", "Ready for stream. Host is ready."));
          return true;
        }

        setActivationProgress(
          createActivationProgress("starting_runtime", Math.max(secondsRemaining, 6)),
        );
        continue;
      }

      setActivationProgress(
        createActivationProgress(
          resolveActivationProgressStage(latestState, attempt),
          Math.max(secondsRemaining, 6),
        ),
      );
    }

    if (latestState?.activation.phase === "activated") {
      clearSetupTokenInputs();
      setActivationProgress(null);
      appendActivity(
        bi(
          "Aktivasi selesai. Runtime masih menyiapkan stream.",
          "Activation complete. Runtime is still preparing the stream.",
        ),
      );
      pushToast(
        bi(
          "Aktivasi selesai. Runtime masih menyiapkan stream.",
          "Activation complete. Runtime is still preparing the stream.",
        ),
      );
      return true;
    }

    if (
      submitError &&
      submitError.toLowerCase().includes("already activated") &&
      latestState?.activation.phase === "activated"
    ) {
        clearSetupTokenInputs();
      setActivationProgress(null);
      pushToast(
        bi(
          "Host ini sudah aktif. Tidak perlu paste token lagi.",
          "This host is already activated. No need to paste the token again.",
        ),
      );
      return true;
    }

    return false;
  }

  async function handleStartHostFlow() {
    try {
      setBusyLabel(bi("Menyalakan runtime host...", "Starting host runtime..."));
      const outcome = await runHostAction("start_host");
      applyShellState(outcome.state);
      appendActivity(outcome.message);
      await waitForRuntimeReadiness(
        bi(
          "Runtime host sudah hidup dan heartbeat readiness sudah dikirim.",
          "Host runtime started and readiness heartbeat was sent.",
        ),
        bi(
          "Runtime host sudah hidup. Kirim heartbeat saat route publik sudah siap.",
          "Host runtime started. Send heartbeat when the public route is ready.",
        ),
      );
    } catch (error) {
      pushToast(extractErrorMessage(error));
    } finally {
      setBusyLabel("");
    }
  }

  async function waitForRuntimeReadiness(successMessage: string, warmingMessage: string) {
    let latestState = await getShellState();
    applyShellState(latestState);

    for (let attempt = 0; attempt < 8; attempt += 1) {
      if (
        latestState.runtime.lifecyclePhase === "ready" &&
        latestState.runtime.requiredProcessesReady &&
        latestState.network.publicUrl
      ) {
        break;
      }

      await new Promise((resolve) => window.setTimeout(resolve, 2000));
      latestState = await getShellState();
      applyShellState(latestState);
    }

    if (
      latestState.runtime.lifecyclePhase === "ready" &&
      latestState.runtime.requiredProcessesReady &&
      latestState.network.publicUrl
    ) {
      const heartbeatOutcome = await sendHeartbeat();
      applyShellState(heartbeatOutcome.state);
      appendActivity(successMessage);
      pushToast(successMessage);
      return;
    }

    const diagnostic = describeRuntimeBlockingState(latestState);
    const message = diagnostic ? `${warmingMessage} ${diagnostic}` : warmingMessage;
    appendActivity(message);
    pushToast(message);
  }

  async function handleSendHeartbeat() {
    try {
      setBusyLabel(bi("Mengirim heartbeat host...", "Sending host heartbeat..."));
      const outcome = await sendHeartbeat();
      handleOutcome(outcome);
    } catch (error) {
      pushToast(extractErrorMessage(error));
    } finally {
      setBusyLabel("");
    }
  }

  async function handleSetupPassword() {
    if (setupPasswordInput.trim().length < 6) {
      pushToast(
        bi(
          "Password admin minimal 6 karakter.",
          "Admin password must be at least 6 characters.",
        ),
      );
      return;
    }

    if (setupPasswordInput !== setupPasswordConfirmInput) {
      pushToast(
        bi("Konfirmasi password tidak cocok.", "Password confirmation does not match."),
      );
      return;
    }

    try {
      setBusyLabel(bi("Membuat password admin...", "Creating admin password..."));
      const next = await setAdminPassword(setupPasswordInput);
      applyShellState(next);
      setSetupPasswordInput("");
      setSetupPasswordConfirmInput("");
      appendActivity(bi("Password admin dibuat.", "Admin password created."));
      pushToast(bi("Password admin dibuat.", "Admin password created."));
    } catch (error) {
      pushToast(extractErrorMessage(error));
    } finally {
      setBusyLabel("");
    }
  }

  async function handleUnlock() {
    if (!unlockPasswordInput.trim()) {
      pushToast(bi("Masukkan password admin.", "Enter the admin password."));
      return;
    }

    try {
      setBusyLabel(bi("Membuka Host Control...", "Unlocking host control..."));
      const next = await unlockApp(unlockPasswordInput);
      applyShellState(next);
      setUnlockPasswordInput("");
      appendActivity(bi("Host Control terbuka.", "Host Control unlocked."));
      pushToast(bi("Host Control terbuka.", "Host Control unlocked."));
    } catch (error) {
      pushToast(extractErrorMessage(error));
    } finally {
      setBusyLabel("");
    }
  }

  async function handleChangePassword() {
    if (changePasswordInput.trim().length < 6) {
      pushToast(
        bi(
          "Password admin baru minimal 6 karakter.",
          "New admin password must be at least 6 characters.",
        ),
      );
      return;
    }

    if (changePasswordInput !== changePasswordConfirmInput) {
      pushToast(
        bi(
          "Konfirmasi password baru tidak cocok.",
          "New password confirmation does not match.",
        ),
      );
      return;
    }

    try {
      setBusyLabel(bi("Memperbarui password admin...", "Updating admin password..."));
      const next = await changeAdminPassword(changePasswordInput);
      applyShellState(next);
      setChangePasswordInput("");
      setChangePasswordConfirmInput("");
      appendActivity(bi("Password admin diperbarui.", "Admin password updated."));
      pushToast(bi("Password admin diperbarui.", "Admin password updated."));
    } catch (error) {
      pushToast(extractErrorMessage(error));
    } finally {
      setBusyLabel("");
    }
  }

  async function handleUninstallInstalledHost() {
    if (!shell?.install.installedMode) {
      pushToast(
        bi(
          "Mode terpasang tidak aktif untuk host ini.",
          "Installed mode is not active for this host.",
        ),
      );
      return;
    }

    setUninstallPasswordInput("");
    setShowUninstallConfirm(true);
  }

  function handleEmergencyKillPrompt() {
    if (!shell?.install.installedMode) {
      pushToast(
        bi(
          "Mode terpasang tidak aktif untuk host ini.",
          "Installed mode is not active for this host.",
        ),
      );
      return;
    }

    setEmergencyKillPhrase("");
    setShowUninstallConfirm(false);
    setShowEmergencyKillConfirm(true);
  }

  async function handleConfirmUninstallInstalledHost() {
    if (!shell?.install.installedMode) {
      pushToast(
        bi(
          "Mode terpasang tidak aktif untuk host ini.",
          "Installed mode is not active for this host.",
        ),
      );
      setShowUninstallConfirm(false);
      return;
    }
    if (!uninstallPasswordInput.trim()) {
      pushToast(
        bi(
          "Masukkan password admin untuk uninstall.",
          "Enter the admin password to uninstall.",
        ),
      );
      return;
    }

    try {
      setBusyLabel(bi("Menghentikan host dan uninstall...", "Stopping host and uninstalling..."));
      const message = await uninstallInstalledHost({ password: uninstallPasswordInput });
      setShowUninstallConfirm(false);
      setUninstallPasswordInput("");
      appendActivity(message);
      pushToast(message);
      window.setTimeout(() => {
        void closeWindowForReal();
      }, 600);
    } catch (error) {
      pushToast(extractErrorMessage(error));
    } finally {
      setBusyLabel("");
    }
  }

  async function handleConfirmEmergencyKill() {
    if (!shell?.install.installedMode) {
      pushToast(
        bi(
          "Mode terpasang tidak aktif untuk host ini.",
          "Installed mode is not active for this host.",
        ),
      );
      setShowEmergencyKillConfirm(false);
      return;
    }

    if (emergencyKillPhrase.trim().toUpperCase() !== "EMERGENCYKILL") {
      pushToast(
        bi(
          "Ketik EMERGENCYKILL untuk konfirmasi uninstall paksa.",
          "Type EMERGENCYKILL to confirm the forced uninstall.",
        ),
      );
      return;
    }

    try {
      setBusyLabel(
        bi(
          "Menjalankan EmergencyKill...",
          "Launching EmergencyKill...",
        ),
      );
      const message = await launchEmergencyUninstaller();
      setShowEmergencyKillConfirm(false);
      setEmergencyKillPhrase("");
      appendActivity(message);
      pushToast(message);
      window.setTimeout(() => {
        void closeWindowForReal();
      }, 600);
    } catch (error) {
      pushToast(extractErrorMessage(error));
    } finally {
      setBusyLabel("");
    }
  }

  async function handleLock() {
    try {
      const next = await lockApp();
      applyShellState(next);
      appendActivity(bi("Host Control terkunci.", "Host Control locked."));
      pushToast(bi("Host Control terkunci.", "Host Control locked."));
    } catch (error) {
      pushToast(extractErrorMessage(error));
    }
  }

  async function closeWindowForReal() {
    try {
      await getCurrentWindow().destroy();
    } catch {
      await getCurrentWindow().close();
    }
  }

  async function handleRecommendedAction() {
    switch (recommendedStep.action) {
      case "setup":
        await runAction("setup_host", bi("Menyiapkan host...", "Setting up host..."));
        break;
      case "open_token":
        await handleOpenTokenPage();
        break;
      case "start_host":
        await handleStartHostFlow();
        break;
      case "send_heartbeat":
        await handleSendHeartbeat();
        break;
      case "open_public_url":
        await openMaybe(shell?.network.publicUrl || "", bi("URL publik", "public URL"));
        break;
      default:
        break;
    }
  }

  function handleOutcome(outcome: ActionOutcome) {
    applyShellState(outcome.state);
    appendActivity(outcome.message);
    pushToast(outcome.message);
  }

  function appendActivity(message: string) {
    const stamped = `${new Date().toLocaleTimeString("en-GB", {
      hour: "2-digit",
      minute: "2-digit",
    })} · ${message}`;
    setActivity((current) => [stamped, ...current].slice(0, 6));
  }

  function pushToast(message: string) {
    setToast(message);
    if (toastTimer.current) {
      window.clearTimeout(toastTimer.current);
    }
    toastTimer.current = window.setTimeout(() => {
      setToast("");
    }, 4200);
  }

  async function copyText(value: string, label: string) {
    if (!value.trim()) {
      pushToast(bi(`${label} belum siap.`, `${label} is not ready yet.`));
      return;
    }

    try {
      await navigator.clipboard.writeText(value);
      pushToast(bi(`${label} disalin.`, `${label} copied.`));
    } catch {
      pushToast(
        bi(
          `Tidak bisa menyalin ${label.toLowerCase()}.`,
          `Could not copy ${label.toLowerCase()}.`,
        ),
      );
    }
  }

  async function openMaybe(value: string, label: string) {
    if (!value.trim()) {
      pushToast(bi(`${label} belum siap.`, `${label} is not ready yet.`));
      return;
    }

    try {
      if (/^[a-z]+:\/\//i.test(value)) {
        await openUrl(value);
      } else {
        await openPath(value);
      }
    } catch {
      pushToast(
        bi(
          `Tidak bisa membuka ${label.toLowerCase()}.`,
          `Could not open ${label.toLowerCase()}.`,
        ),
      );
    }
  }

  if (!shell) {
    return (
      <div className="splash-screen">
        <div className="splash-card glass-card">
          <div className="dialog-switch-row">{languageSwitch}</div>
          <span className="badge beta">BETA</span>
          <h1>Cloudgime Host Control</h1>
          <p>{bootError || bi("Menyiapkan shell operator...", "Preparing the operator shell...")}</p>
          {bootError && (
            <button
              type="button"
              className="action-button success wide"
              onClick={() => void refreshState()}
            >
              <RefreshCw size={16} />
              {bi("Coba Lagi", "Retry")}
            </button>
          )}
        </div>
      </div>
    );
  }

  if (showLockGate) {
    return (
      <div className="splash-screen">
        <div className="lock-card glass-card">
          <div className="dialog-switch-row">{languageSwitch}</div>
          <span className="badge beta">BETA</span>
          <h3>
            {shell.auth.needsPasswordSetup
              ? bi("Buat password admin", "Create admin password")
              : bi("Buka kunci Cloudgime Host Control", "Unlock Cloudgime Host Control")}
          </h3>
          <p>
            {shell.auth.needsPasswordSetup
              ? bi(
                  "Host ini membutuhkan password admin lokal sebelum shell operator bisa dipakai.",
                  "This host needs a local admin password before the operator shell can be used.",
                )
              : bi(
                  "Masukkan password admin lokal untuk membuka bundle host ini.",
                  "Enter the local admin password to unlock this host bundle.",
                )}
          </p>

          {shell.auth.needsPasswordSetup ? (
            <>
              <label className="field-label">
                <span>{bi("Password admin", "Admin password")}</span>
                <input
                  type="password"
                  value={setupPasswordInput}
                  onChange={(event) => setSetupPasswordInput(event.currentTarget.value)}
                />
              </label>
              <label className="field-label">
                <span>{bi("Konfirmasi password", "Confirm password")}</span>
                <input
                  type="password"
                  value={setupPasswordConfirmInput}
                  onChange={(event) =>
                    setSetupPasswordConfirmInput(event.currentTarget.value)
                  }
                />
              </label>
              <button
                type="button"
                className="action-button success wide"
                onClick={() => void handleSetupPassword()}
              >
                <ShieldCheck size={16} />
                {bi("Buat Password", "Create Password")}
              </button>
            </>
          ) : (
            <>
              <label className="field-label">
                <span>{bi("Password admin", "Admin password")}</span>
                <input
                  type="password"
                  value={unlockPasswordInput}
                  onChange={(event) => setUnlockPasswordInput(event.currentTarget.value)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") {
                      void handleUnlock();
                    }
                  }}
                />
              </label>
              <button
                type="button"
                className="action-button success wide"
                onClick={() => void handleUnlock()}
              >
                <Lock size={16} />
                {bi("Buka Kunci", "Unlock")}
              </button>
            </>
          )}
        </div>
      </div>
    );
  }

  return (
    <div className="app-frame">
      <div className="ambient ambient-a" />
      <div className="ambient ambient-b" />
      <div className="ambient ambient-c" />

      <main className="shell">
        <div className="beta-ribbon">BETA</div>

        <aside className="rail">
          <div className="rail-header">
            <span className="rail-kicker">{bi("Khusus internal", "Internal Use Only")}</span>
            <h1>Cloudgime Host Control</h1>
            <p>{bi("Operator Beta", "Beta Operator")}</p>
          </div>

          <nav className="rail-nav">
            {railItems.map((item) => {
              const Icon = item.icon;
              return (
                <button
                  key={item.key}
                  className={`rail-button ${activeRail === item.key ? "active" : ""}`}
                  onClick={() => setActiveRail(item.key)}
                  type="button"
                >
                  <Icon size={18} />
                  <span>{item.label}</span>
                </button>
              );
            })}
          </nav>

          <div className="rail-footer">
            <span className={`mini-pill ${shell.auth.unlocked ? "success" : "warning"}`}>
              {shell.auth.unlocked
                ? bi("Terbuka", "Unlocked")
                : shell.auth.needsPasswordSetup
                  ? bi("Buat password", "Set password")
                  : bi("Terkunci", "Locked")}
            </span>
            <span className="mini-pill neutral">{normalizePhase(shell.activation.phase)}</span>
          </div>
        </aside>

        <section className="content">
          <header className="page-header">
            <div>
              <span className="page-kicker">{bi("Konsol host operator", "Operator host console")}</span>
              <h2>{hero.title}</h2>
              <p>{hero.subtitle}</p>
            </div>
            <div className="header-status-stack">
              {languageSwitch}
              <div className={`hero-badge ${hero.tone}`}>
                <span className="hero-dot" />
                {hero.badge}
              </div>
              <span className="mini-pill neutral">
                {bi("Sinkron terakhir", "Last synced")} {formatSyncStamp(lastRefreshedAt)}
              </span>
              {shell.runtime.runtimeLabel || shell.runtime.encoder ? (
                <span className={`mini-pill ${isCompatibilityRuntime(shell.runtime) ? "warning" : "success"}`}>
                  {formatRuntimeMode(shell.runtime)} · {formatRuntimeShort(shell.runtime)}
                </span>
              ) : null}
            </div>
          </header>

          {activeRail === "operator" && (
            <>
              <div className="hero-strip">
            <div className={`hero-card ${hero.tone}`}>
              <div className="hero-headline">
                <span className="hero-led" />
                <span>{hero.title}</span>
              </div>
              <div className="hero-meta">{hero.subtitle}</div>
            </div>

            <div className="identity-card glass-card">
              <div className="identity-row">
                <span className="identity-label">{bi("Host ID", "Host ID")}</span>
                <strong>{shell.activation.hostId}</strong>
                <button
                  type="button"
                  className="link-button"
                  onClick={() => void copyText(shell.activation.hostId, "Host ID")}
                >
                  <Copy size={15} />
                  {bi("Salin", "Copy")}
                </button>
              </div>
              <div className="identity-row">
                <span className="identity-label">{bi("Nama PC", "PC Name")}</span>
                <input
                  className="inline-input"
                  value={displayNameInput}
                  onChange={(event) => setDisplayNameInput(event.currentTarget.value)}
                  placeholder={bi("Host Cloudgime", "Cloudgime Host")}
                />
                <button
                  type="button"
                  className="link-button"
                  onClick={() => void handleSavePreferences()}
                >
                  {bi("Simpan", "Save")}
                </button>
              </div>
              <div className="identity-row">
                <span className="identity-label">{bi("Status", "State")}</span>
                <strong>{normalizePhase(shell.activation.phase)}</strong>
                <span
                  className={`mini-pill ${shell.activation.readyForStream ? "success" : "neutral"}`}
                >
                  {describeReadinessBadge(shell)}
                </span>
              </div>
              <div className="identity-row">
                <span className="identity-label">{bi("Lane", "Lane")}</span>
                <strong>{describeActivationLane(shell)}</strong>
                <span
                  className={`mini-pill ${
                    shell.activation.tokenKind.trim().toLowerCase() === "always_on_host"
                      ? "success"
                      : shell.activation.tokenKind.trim()
                        ? "neutral"
                        : "ghost"
                  }`}
                >
                  {shell.activation.instanceType.trim().toLowerCase() === "always-on"
                    ? bi("24 Jam", "24/7")
                    : shell.activation.instanceType.trim().toLowerCase() === "power-managed"
                      ? bi("Panel + Host", "Panel + Host")
                      : bi("Belum Ada", "Unassigned")}
                </span>
              </div>
              <div className="identity-row">
                <span className="identity-label">{bi("Tautan Sentinel", "Sentinel Binding")}</span>
                <strong>
                  {shell.activation.sentinelPcId || shell.activation.sentinelDeviceId
                    ? [
                        shell.activation.sentinelPcId || "",
                        shell.activation.sentinelDeviceId || "",
                      ]
                        .filter(Boolean)
                        .join(" · ")
                    : bi("Belum terdeteksi", "Not detected")}
                </strong>
                <span
                  className={`mini-pill ${
                    shell.activation.sentinelPcId || shell.activation.sentinelDeviceId
                      ? "success"
                      : "neutral"
                  }`}
                >
                  {shell.activation.sentinelPcId || shell.activation.sentinelDeviceId
                    ? bi("Tertaut", "Linked")
                    : bi("Tidak ada", "Missing")}
                </span>
              </div>
              <div className="identity-row identity-row-note">
                <span className="identity-label">{bi("Alur", "Flow")}</span>
                <strong>{describeActivationLaneNote(shell)}</strong>
              </div>
            </div>
          </div>

          <div className="stage-strip">
            <StageCard
              title={bi("Siapkan", "Set up")}
              value={stageState.setup}
              description={bi(
                "Bundle lokal, runtime, dan cek kesehatan.",
                "Local bundle, runtime, and health checks.",
              )}
            />
            <StageCard
              title={bi("Aktif", "Activated")}
              value={stageState.activated}
              description={bi(
                "Token sudah dipakai dari control plane.",
                "Token redeemed from control plane.",
              )}
            />
            <StageCard
              title={bi("Berjalan", "Running")}
              value={stageState.running}
              description={bi(
                "Runtime hidup dan seharusnya siap menerima sesi.",
                "Runtime is up and should accept sessions.",
              )}
            />
          </div>

          <section className="glass-card next-step-card">
            <div>
              <span className="page-kicker">{bi("Langkah berikutnya", "Recommended next step")}</span>
              <h3>{recommendedStep.title}</h3>
              <p>{recommendedStep.description}</p>
            </div>
            <div className="next-step-actions">
              {recommendedStep.action !== "none" && (
                <ActionButton
                  icon={recommendedStep.icon}
                  label={recommendedStep.label}
                  variant={recommendedStep.variant}
                  onClick={() => void handleRecommendedAction()}
                  disabled={recommendedStep.disabled}
                />
              )}
            </div>
          </section>

          <div className="page-grid operator-grid">
              <section className="glass-card section-card">
                <div className="section-heading">
                  <span>{bi("1 · Mode otomatis", "1 · Automatic mode")}</span>
                  <p>
                    {bi(
                      "Tidak perlu klik setup atau start manual. Paste token Instance Pair atau Always-On Host, lalu Host Control menyiapkan bundle, binding, service, runtime, dan heartbeat otomatis.",
                      "No manual setup or start click is required. Paste the Instance Pair or Always-On Host token and Host Control prepares the bundle, binding, service, runtime, and heartbeat automatically.",
                    )}
                  </p>
                </div>
                <div className="hint-row compact-top-gap">
                  <span className="mini-pill neutral">{describeActivationPhase(shell.activation.phase)}</span>
                  <span>
                    {shell.runtime.lifecyclePhase === "ready"
                      ? bi("Runtime siap.", "Runtime is ready.")
                      : bi("Runtime akan dinyalakan otomatis setelah token valid.", "Runtime will start automatically after a valid token.")}
                  </span>
                </div>
              </section>

              <section className="glass-card section-card">
                <div className="section-heading">
                  <span>{bi("2 · Aktifkan host ini", "2 · Activate this host")}</span>
                  <p>
                    {shell.activation.phase === "activated"
                      ? bi(
                          "Host ini sudah aktif. Buka lagi Host Control hanya saat perlu menerbitkan ulang atau memeriksa token.",
                          "This host is already activated. Reopen Host Control only when you need to reissue or inspect the token.",
                        )
                      : bi(
                          "Paste token Instance Pair atau Always-On Host dari CloudRental. Host app akan claim binding dan redeem token aktivasi secara otomatis.",
                          "Paste the Instance Pair or Always-On Host token from CloudRental. The host app will claim the binding and redeem the activation token automatically.",
                        )}
                  </p>
                </div>

                {shell.activation.phase === "activated" ? (
                  <>
                    <div className="rounded-2xl border border-emerald-500/20 bg-emerald-500/5 px-4 py-4 mb-4">
                      <div className="text-xs uppercase tracking-[0.22em] text-emerald-300">
                        {bi("Setup Token Repair", "Setup Token Repair")}
                      </div>
                      <p className="mt-2 text-sm text-slate-300">
                        {bi(
                          "Jika host ini perlu di-bind ulang setelah reinstall, paste token Instance Pair atau Always-On Host baru di sini. Host app akan claim dan redeem otomatis.",
                          "If this host needs to be rebound after reinstall, paste a fresh Instance Pair or Always-On Host token here. The host app will claim and redeem automatically.",
                        )}
                      </p>
                      {renderSetupTokenClaimControls("ghost")}
                    </div>
                    {activationProgress && (
                      <div className="activation-progress">
                        <div className="activation-progress-head">
                          <div className="busy-spinner inline-spinner" />
                          <div className="activation-progress-copy">
                            <strong>{activationProgress.title}</strong>
                            <p>{activationProgress.detail}</p>
                          </div>
                          <span className="mini-pill neutral">
                            {activationProgress.secondsRemaining > 0
                              ? `~${activationProgress.secondsRemaining} detik`
                              : bi("menyelesaikan", "finalizing")}
                          </span>
                        </div>
                        <div className="activation-progress-steps">
                          {buildActivationProgressSteps(activationProgress.stage).map((step) => (
                            <div
                              key={step.key}
                              className={`activation-progress-step ${step.state}`}
                            >
                              <span className="activation-progress-step-index">
                                {step.order}
                              </span>
                              <span>{step.label}</span>
                            </div>
                          ))}
                        </div>
                      </div>
                    )}
                    <div className="activation-summary-grid">
                      <div className="summary-tile">
                        <span className="stage-title">{bi("Sudah aktif", "Activated")}</span>
                        <strong>{formatTimestampShort(shell.activation.activatedAtUtc)}</strong>
                      </div>
                      <div className="summary-tile">
                        <span className="stage-title">{bi("Heartbeat terakhir", "Last heartbeat")}</span>
                        <strong>{formatTimestampShort(shell.activation.lastHeartbeatAtUtc)}</strong>
                      </div>
                      <div className="summary-tile">
                        <span className="stage-title">{bi("Control plane", "Control plane")}</span>
                        <strong>{normalizeControlPlane(controlPlaneInput)}</strong>
                      </div>
                    </div>
                  </>
                ) : (
                  <>
                    <div className="rounded-2xl border border-sky-500/20 bg-sky-500/5 px-4 py-4 mb-4">
                      <div className="text-xs uppercase tracking-[0.22em] text-sky-300">
                        {bi("Setup Token", "Setup Token")}
                      </div>
                      <p className="mt-2 text-sm text-slate-300">
                        {bi(
                          "Flow baru yang direkomendasikan. Paste token Instance Pair atau Always-On Host dari Cloudrental dan host app akan claim lalu redeem sendiri.",
                          "Recommended new flow. Paste the Cloudrental Instance Pair or Always-On Host token and the host app will claim and redeem automatically.",
                        )}
                      </p>
                      {renderSetupTokenClaimControls("success")}
                    </div>
                    {activationProgress && (
                      <div className="activation-progress">
                        <div className="activation-progress-head">
                          <div className="busy-spinner inline-spinner" />
                          <div className="activation-progress-copy">
                            <strong>{activationProgress.title}</strong>
                            <p>{activationProgress.detail}</p>
                          </div>
                          <span className="mini-pill neutral">
                            {activationProgress.secondsRemaining > 0
                              ? `~${activationProgress.secondsRemaining} detik`
                              : bi("menyelesaikan", "finalizing")}
                          </span>
                        </div>
                        <div className="activation-progress-steps">
                          {buildActivationProgressSteps(activationProgress.stage).map((step) => (
                            <div
                              key={step.key}
                              className={`activation-progress-step ${step.state}`}
                            >
                              <span className="activation-progress-step-index">
                                {step.order}
                              </span>
                              <span>{step.label}</span>
                            </div>
                          ))}
                        </div>
                      </div>
                    )}
                  </>
                )}

                <div className="hint-row">
                  <span className="mini-pill neutral">
                    {normalizeControlPlane(controlPlaneInput)}
                  </span>
                  <span>{describeActivationPhase(shell.activation.phase)}</span>
                </div>
              </section>

              <section className="glass-card section-card">
                <div className="section-heading">
                  <span>{bi("3 · Runtime otomatis", "3 · Automatic runtime")}</span>
                  <p>
                    {bi(
                      "Service dan runtime dipicu otomatis setelah token valid. Kontrol manual dipindah ke Perawatan untuk teknisi saja.",
                      "Service and runtime are triggered automatically after a valid token. Manual controls are moved to Maintenance for technicians only.",
                    )}
                  </p>
                </div>
                <div className="activation-summary-grid">
                  <div className="summary-tile">
                    <span className="stage-title">{bi("Runtime", "Runtime")}</span>
                    <strong>{shell.runtime.lifecyclePhase}</strong>
                  </div>
                  <div className="summary-tile">
                    <span className="stage-title">{bi("Proses wajib", "Required processes")}</span>
                    <strong>{shell.runtime.requiredProcessesReady ? "OK" : bi("Menunggu", "Waiting")}</strong>
                  </div>
                  <div className="summary-tile">
                    <span className="stage-title">{bi("Route publik", "Public route")}</span>
                    <strong>{shell.network.publicUrl ? bi("Ada", "Available") : bi("Menunggu", "Waiting")}</strong>
                  </div>
                </div>
              </section>

              <section className="glass-card section-card">
                <div className="section-heading">
                  <span>{bi("Akses stream", "Stream access")}</span>
                  <p>{bi("Buka URL, kirim readiness, dan kumpulkan dukungan.", "Open URLs, signal readiness, and collect support.")}</p>
                </div>
                <div className="action-grid compact">
                  <ActionButton
                    icon={ArrowUpRight}
                    label={bi("Buka URL Publik", "Open Public URL")}
                    onClick={() => void openMaybe(shell.network.publicUrl, bi("URL publik", "public URL"))}
                    disabled={!shell.network.publicUrl}
                  />
                  <ActionButton
                    icon={Copy}
                    label={bi("Salin URL Publik", "Copy Public URL")}
                    onClick={() => void copyText(shell.network.publicUrl, "Public URL")}
                    disabled={!shell.network.publicUrl}
                  />
                  <ActionButton
                    icon={Activity}
                    label={bi("Kirim Heartbeat", "Send Heartbeat")}
                    onClick={() => void handleSendHeartbeat()}
                    disabled={shell.activation.phase !== "activated"}
                  />
                  <ActionButton
                    icon={FolderOpen}
                    label={bi("Kumpulkan Dukungan", "Collect Support")}
                    onClick={() =>
                      void runAction("collect_support", bi("Mengumpulkan bundle dukungan...", "Collecting support bundle..."))
                    }
                  />
                </div>
                <p className="section-note">
                  {bi(
                    "Menutup Host Control hanya menutup jendela ini. Host tetap berjalan lewat layanan latar belakang, dan saat dibuka lagi aplikasi akan meminta password admin.",
                    "Closing Host Control only closes this window. The host keeps running through the background service, and reopening this app asks for the admin password again.",
                  )}
                </p>
              </section>

              <section className="glass-card section-card activity-card">
                <div className="section-heading">
                  <span>{bi("Aktivitas terbaru", "Recent activity")}</span>
                  <p>{bi("Umpan balik operator, bukan console penuh.", "Operator feedback, not a full console.")}</p>
                </div>
                <div className="activity-list">
                  {activity.map((line) => (
                    <div key={line} className="activity-line">
                      {line}
                    </div>
                  ))}
                </div>
              </section>
            </div>
            </>
          )}

          {activeRail === "access" && (
            <div className="page-grid two-column">
              <GlassInfoCard
                title={bi("Route publik", "Public route")}
                value={shell.network.publicUrl || bi("Belum dipublikasikan", "Not published yet")}
                subtitle={bi("Ini adalah route stream yang dilihat pelanggan.", "This is the customer-facing stream route.")}
                actions={
                  <>
                    <button
                      type="button"
                      className="action-button ghost"
                      onClick={() => void openMaybe(shell.network.publicUrl, bi("URL publik", "public URL"))}
                      disabled={!shell.network.publicUrl}
                    >
                      <ArrowUpRight size={16} />
                      {bi("Buka", "Open")}
                    </button>
                    <button
                      type="button"
                      className="action-button ghost"
                      onClick={() => void copyText(shell.network.publicUrl, "Public URL")}
                      disabled={!shell.network.publicUrl}
                    >
                      <Copy size={16} />
                      {bi("Salin", "Copy")}
                    </button>
                  </>
                }
              />
              <GlassInfoCard
                title={bi("UI lokal", "Local UI")}
                value={shell.network.localUrl || bi("Belum siap", "Not ready")}
                subtitle={bi("Pakai ini hanya untuk pengecekan operator lokal.", "Use this only for local operator checks.")}
                actions={
                  <>
                    <button
                      type="button"
                      className="action-button ghost"
                      onClick={() => void openMaybe(shell.network.localUrl, "local URL")}
                      disabled={!shell.network.localUrl}
                    >
                      <ArrowUpRight size={16} />
                      {bi("Buka", "Open")}
                    </button>
                    <button
                      type="button"
                      className="action-button ghost"
                      onClick={() => void copyText(shell.network.localUrl, "Local URL")}
                      disabled={!shell.network.localUrl}
                    >
                      <Copy size={16} />
                      {bi("Salin", "Copy")}
                    </button>
                  </>
                }
              />
              <section className="glass-card section-card">
                <div className="section-heading">
                  <span>{bi("Pengaturan akses terkelola", "Managed access settings")}</span>
                  <p>
                    {bi(
                      "Admin master menentukan slot PC lewat setup token. Operator lokal hanya menyimpan nama PC dan control plane saat migrasi.",
                      "Master admin decides the PC slot through the setup token. The local operator only saves the PC name and control plane during migration.",
                    )}
                  </p>
                </div>
                <label className="field-label">
                  <span>{bi("Nama PC", "PC Name")}</span>
                  <input
                    value={displayNameInput}
                    onChange={(event) => setDisplayNameInput(event.currentTarget.value)}
                  />
                </label>
                <label className="field-label">
                  <span>{bi("Control Plane", "Control Plane")}</span>
                  <input
                    value={controlPlaneInput}
                    onChange={(event) => setControlPlaneInput(event.currentTarget.value)}
                  />
                </label>
                <div className="action-grid compact">
                  <ActionButton
                    icon={ShieldCheck}
                    label={bi("Simpan", "Save")}
                    onClick={() => void handleSavePreferences()}
                  />
                  <ActionButton
                    icon={RefreshCw}
                    label={bi("Muat Ulang", "Reload")}
                    onClick={() => void refreshState(bi("Nilai akses dimuat ulang.", "Access values reloaded."))}
                  />
                </div>
                <p className="field-helper">
                  {bi(
                    "Mode resmi sekarang: token Instance Pair atau Always-On Host dari control plane, lalu runtime publish route lewat control plane dan keeper tunnel. FRP manual sudah bukan jalur normal.",
                    "The official mode is now: an Instance Pair or Always-On Host token from the control plane, then the runtime publishes the route through the control plane and keeper tunnel. Manual FRP is no longer the normal path.",
                  )}
                </p>
              </section>
            </div>
          )}

          {activeRail === "audio" && (
            <div className="page-grid two-column">
              <section className="glass-card section-card">
                <div className="section-heading">
                  <span>{bi("Route audio", "Audio routing")}</span>
                  <p>
                    {bi(
                      "Pilih auto atau manual. Saat host aktif, apply akan refresh capability lalu restart runtime bila perlu.",
                      "Choose automatic or manual routing. When the host is active, apply refreshes capability and restarts the runtime if needed.",
                    )}
                  </p>
                </div>

                <label className="field-label">
                  <span>{bi("Mode", "Mode")}</span>
                  <select
                    value={audioModeInput}
                    onChange={(event) => setAudioModeInput(event.currentTarget.value)}
                  >
                    <option value="auto">{bi("Otomatis", "Automatic")}</option>
                    <option value="manual">{bi("Manual", "Manual")}</option>
                  </select>
                </label>

                <label className="field-label">
                  <span>{bi("Speaker / Output", "Speaker / Output")}</span>
                  <select
                    value={audioSinkInput}
                    onChange={(event) => setAudioSinkInput(event.currentTarget.value)}
                    disabled={audioModeInput !== "manual" || shell.audio.availableOutputs.length === 0}
                  >
                    <option value="">
                      {shell.audio.availableOutputs.length === 0
                        ? bi("Belum terdeteksi", "Not detected yet")
                        : bi("Pilih output", "Pick an output")}
                    </option>
                    {shell.audio.availableOutputs.map((name) => (
                      <option key={`audio-output-${name}`} value={name}>
                        {name}
                      </option>
                    ))}
                  </select>
                </label>

                <label className="field-label">
                  <span>{bi("Virtual Sink", "Virtual Sink")}</span>
                  <select
                    value={virtualSinkInput}
                    onChange={(event) => setVirtualSinkInput(event.currentTarget.value)}
                    disabled={audioModeInput !== "manual" || shell.audio.availableOutputs.length === 0}
                  >
                    <option value="">
                      {shell.audio.availableOutputs.length === 0
                        ? bi("Belum terdeteksi", "Not detected yet")
                        : bi("Pilih virtual sink", "Pick a virtual sink")}
                    </option>
                    {shell.audio.availableOutputs.map((name) => (
                      <option key={`virtual-output-${name}`} value={name}>
                        {name}
                      </option>
                    ))}
                  </select>
                </label>

                <label className="field-label">
                  <span>{bi("Microphone / Input", "Microphone / Input")}</span>
                  <select
                    value={microphoneInput}
                    onChange={(event) => setMicrophoneInput(event.currentTarget.value)}
                    disabled={audioModeInput !== "manual" || shell.audio.availableInputs.length === 0}
                  >
                    <option value="">
                      {shell.audio.availableInputs.length === 0
                        ? bi("Belum terdeteksi", "Not detected yet")
                        : bi("Pilih input", "Pick an input")}
                    </option>
                    {shell.audio.availableInputs.map((name) => (
                      <option key={`audio-input-${name}`} value={name}>
                        {name}
                      </option>
                    ))}
                  </select>
                </label>

                <div className="action-grid compact">
                  <ActionButton
                    icon={Volume2}
                    label={bi("Terapkan Audio", "Apply Audio")}
                    onClick={() => void handleSaveAudioRoute()}
                  />
                  <ActionButton
                    icon={RefreshCw}
                    label={bi("Muat Ulang Status", "Refresh Status")}
                    onClick={() =>
                      void refreshState(
                        bi("Status audio dimuat ulang.", "Audio state reloaded."),
                      )
                    }
                  />
                  <ActionButton
                    icon={Wrench}
                    label={bi("Kembalikan Otomatis", "Restore Auto")}
                    onClick={() => void handleSaveAudioRoute("auto")}
                  />
                  <ActionButton
                    icon={FolderOpen}
                    label={bi("Audio INF", "Audio INF")}
                    onClick={() => void openMaybe(shell.paths.audioInfPath, bi("audio INF", "audio INF"))}
                    disabled={!shell.paths.audioInfPath}
                  />
                </div>

                {audioModeInput === "manual" &&
                  (shell.audio.availableOutputs.length === 0 ||
                    shell.audio.availableInputs.length === 0) && (
                    <p className="section-note">
                      {bi(
                        "Manual mode butuh endpoint audio sudah terdeteksi dulu. Jalankan Setup Host atau Refresh Status bila daftar masih kosong.",
                        "Manual mode needs detected audio endpoints first. Run Setup Host or Refresh Status if the lists are still empty.",
                      )}
                    </p>
                  )}
              </section>

              <section className="glass-card section-card">
                <div className="section-heading">
                  <span>{bi("Status route saat ini", "Current route status")}</span>
                  <p>
                    {bi(
                      "Panel ini menunjukkan route yang sedang dipakai runtime dan alasan pemilihannya.",
                      "This panel shows the route currently used by the runtime and why it was chosen.",
                    )}
                  </p>
                </div>

                <div className="info-pairs">
                  <div>
                    <strong>{bi("Mode route", "Routing mode")}</strong>
                    <span>{normalizePhase(shell.audio.mode || "auto")}</span>
                  </div>
                  <div>
                    <strong>{bi("Status route", "Routing status")}</strong>
                    <span>{normalizePhase(shell.audio.routingStatus || "unknown")}</span>
                  </div>
                  <div>
                    <strong>{bi("Output aktif", "Selected output")}</strong>
                    <span>{shell.audio.selectedAudioSinkName || bi("Belum ada", "Not selected yet")}</span>
                  </div>
                  <div>
                    <strong>{bi("Virtual sink aktif", "Selected virtual sink")}</strong>
                    <span>
                      {shell.audio.selectedVirtualSinkName || bi("Belum ada", "Not selected yet")}
                    </span>
                  </div>
                  <div>
                    <strong>{bi("Mic aktif", "Selected microphone")}</strong>
                    <span>
                      {shell.audio.selectedMicrophoneName || bi("Belum ada", "Not selected yet")}
                    </span>
                  </div>
                  <div>
                    <strong>{bi("Inventory", "Inventory")}</strong>
                    <span>
                      {bi(
                        `${shell.audio.availableOutputs.length} output · ${shell.audio.availableInputs.length} input`,
                        `${shell.audio.availableOutputs.length} outputs · ${shell.audio.availableInputs.length} inputs`,
                      )}
                    </span>
                  </div>
                </div>

                <p className="section-note">
                  <strong>{bi("Alasan seleksi:", "Selection reason:")}</strong>{" "}
                  {shell.audio.selectionReason || bi("Belum ada detail.", "No detail yet.")}
                </p>
                <p className="section-note">
                  <strong>{bi("Alasan status runtime:", "Runtime status reason:")}</strong>{" "}
                  {shell.audio.routingReason || bi("Belum ada detail.", "No detail yet.")}
                </p>
              </section>
            </div>
          )}

          {activeRail === "display" && (
            <div className="page-grid three-column">
              <section className="glass-card section-card">
                <div className="section-heading">
                  <span>{bi("Target display stream", "Stream display target")}</span>
                  <p>
                    {bi(
                      "Pilih display yang akan dijadikan primary saat sesi Cloudgime masuk. Runtime akan restart bila sedang aktif.",
                      "Pick the display that becomes primary when a Cloudgime session starts. The runtime restarts if it is active.",
                    )}
                  </p>
                </div>

                <label className="field-label">
                  <span>{bi("Mode target", "Target mode")}</span>
                  <select
                    value={displayModeInput}
                    onChange={(event) => setDisplayModeInput(event.currentTarget.value)}
                  >
                    <option value="mtt_vdd">{bi("MTT VDD", "MTT VDD")}</option>
                    <option value="qemu_virtio">{bi("QEMU / VirtIO", "QEMU / VirtIO")}</option>
                    <option value="parsec_vda">{bi("Parsec VDA", "Parsec VDA")}</option>
                    <option value="primary">{bi("Primary saat ini", "Current primary")}</option>
                    <option value="auto">{bi("Auto", "Auto")}</option>
                    <option value="custom">{bi("Custom", "Custom")}</option>
                  </select>
                </label>

                {displayModeInput === "custom" && (
                  <>
                    <label className="field-label">
                      <span>{bi("Device Name", "Device Name")}</span>
                      <input
                        value={displayCustomDeviceNameInput}
                        onChange={(event) => setDisplayCustomDeviceNameInput(event.currentTarget.value)}
                        placeholder="\\\\.\\DISPLAY2"
                      />
                    </label>
                    <label className="field-label">
                      <span>{bi("Device ID", "Device ID")}</span>
                      <input
                        value={displayCustomDeviceIdInput}
                        onChange={(event) => setDisplayCustomDeviceIdInput(event.currentTarget.value)}
                        placeholder="DISPLAY\\MTT1337..."
                      />
                    </label>
                    <label className="field-label">
                      <span>{bi("Label match", "Label match")}</span>
                      <input
                        value={displayCustomLabelInput}
                        onChange={(event) => setDisplayCustomLabelInput(event.currentTarget.value)}
                        placeholder="QEMU, MTT1337, PSCCDD"
                      />
                    </label>
                  </>
                )}

                <div className="action-grid compact">
                  <ActionButton
                    icon={ShieldCheck}
                    label={bi("Terapkan Display", "Apply Display")}
                    onClick={() => void handleSaveDisplayRoute()}
                  />
                  <ActionButton
                    icon={LaptopMinimal}
                    label={bi("Pakai MTT VDD", "Use MTT VDD")}
                    onClick={() => void handleSaveDisplayRoute("mtt_vdd")}
                  />
                  <ActionButton
                    icon={RefreshCw}
                    label={bi("Muat Ulang", "Reload")}
                    onClick={() =>
                      void refreshState(
                        bi("Status display dimuat ulang.", "Display state reloaded."),
                      )
                    }
                  />
                </div>

                <p className="section-note">
                  {bi(
                    "Untuk test adil QEMU vs MTT, pilih mode display lalu jalankan stream dengan preset resolusi yang sama.",
                    "For a fair QEMU vs MTT test, pick a display mode and stream with the same resolution preset.",
                  )}
                </p>
              </section>

              <section className="glass-card section-card">
                <div className="section-heading">
                  <span>{bi("Status target saat ini", "Current target status")}</span>
                  <p>
                    {bi(
                      "Cloudgime akan fokus ke target ini hanya selama sesi stream. Display lain tetap dibiarkan aktif.",
                      "Cloudgime focuses this target only during a stream session. Other displays remain active.",
                    )}
                  </p>
                </div>

                <div className="info-pairs">
                  <div>
                    <strong>{bi("Mode tersimpan", "Saved mode")}</strong>
                    <span>{formatDisplayMode(shell.display.mode)}</span>
                  </div>
                  <div>
                    <strong>{bi("Target efektif", "Effective target")}</strong>
                    <span>{shell.display.effectiveLabel || formatDisplayMode(shell.display.mode)}</span>
                  </div>
                  <div>
                    <strong>{bi("Custom name", "Custom name")}</strong>
                    <span>{shell.display.customDeviceName || bi("Tidak dipakai", "Not used")}</span>
                  </div>
                  <div>
                    <strong>{bi("Custom ID", "Custom ID")}</strong>
                    <span>{shell.display.customDeviceId || bi("Tidak dipakai", "Not used")}</span>
                  </div>
                  <div>
                    <strong>{bi("Label match", "Label match")}</strong>
                    <span>{shell.display.customLabel || bi("Tidak dipakai", "Not used")}</span>
                  </div>
                  <div>
                    <strong>{bi("Encoder / capture", "Encoder / capture")}</strong>
                    <span>{formatEncoderCapture(shell.runtime)}</span>
                  </div>
                </div>
              </section>

              <section className="glass-card section-card">
                <div className="section-heading">
                  <span>{bi("Mode Sewa PC (Anti-Flicker)", "PC Rental Mode (Anti-Flicker)")}</span>
                  <p>
                    {bi(
                      "Gunakan mode ini jika PC disewakan dan memiliki aplikasi client streaming lain yang berjalan bersamaan. Ini mengaktifkan Windows Graphics Capture (WGC) dan Dual Display untuk mencegah layar berkedip/flickering. Pengaturan ini persisten setelah restart.",
                      "Use this mode if the PC is rented and has other streaming clients running concurrently. This enables Windows Graphics Capture (WGC) and Dual Display to prevent screen flickering/blinking. This setting is persistent across restarts.",
                    )}
                  </p>
                </div>

                <div className="info-pairs">
                  <div>
                    <strong>{bi("Status Mode Sewa", "Rental Mode Status")}</strong>
                    <span className={`mini-pill ${shell.display.dualStreamEnabled ? "success" : "neutral"}`}>
                      {shell.display.dualStreamEnabled
                        ? bi("Start (Aktif)", "Start (Active)")
                        : bi("Stop (Non-Aktif)", "Stop (Inactive)")}
                    </span>
                  </div>
                </div>

                <div className="action-grid compact" style={{ marginTop: '16px' }}>
                  <ActionButton
                    icon={shell.display.dualStreamEnabled ? Square : Play}
                    label={shell.display.dualStreamEnabled 
                      ? bi("Stop Mode Sewa", "Stop Rental Mode") 
                      : bi("Start Mode Sewa", "Start Rental Mode")
                    }
                    variant={shell.display.dualStreamEnabled ? "danger" : "success"}
                    onClick={() => void handleToggleDualStream()}
                  />
                </div>
              </section>
            </div>
          )}

          {activeRail === "health" && (
            <div className="page-grid three-column">
              <MetricCard
                title={bi("Siklus hidup", "Lifecycle")}
                value={normalizePhase(shell.runtime.lifecyclePhase)}
                icon={LaptopMinimal}
              />
              <MetricCard
                title={bi("Kesehatan", "Health")}
                value={normalizePhase(shell.runtime.healthGrade)}
                icon={HeartPulse}
              />
              <MetricCard
                title={bi("Audio", "Audio")}
                value={normalizePhase(shell.runtime.audioStatus)}
                icon={Wifi}
              />
              <MetricCard
                title={bi("Layanan", "Service")}
                value={normalizePhase(shell.runtime.serviceState)}
                icon={ServerCog}
              />
              <MetricCard
                title={bi("Runtime", "Runtime")}
                value={formatRuntimeShort(shell.runtime)}
                icon={Activity}
              />
              <MetricCard
                title={bi("Mode Runtime", "Runtime Mode")}
                value={formatRuntimeMode(shell.runtime)}
                icon={ServerCog}
              />
              <MetricCard
                title={bi("Encoder", "Encoder")}
                value={formatEncoderCapture(shell.runtime)}
                icon={LaptopMinimal}
              />
              <MetricCard
                title={bi("Siap", "Ready")}
                value={
                  shell.activation.readyForStream
                    ? bi("Siap untuk stream", "Ready for stream")
                    : bi("Belum siap", "Not ready")
                }
                icon={ShieldCheck}
              />
              <MetricCard
                title={bi("Task Daemon", "Daemon Task")}
                value={describeHostUserDaemonTaskStatus(shell.hostUserDaemonTaskHealth)}
                icon={ServerCog}
              />
              <MetricCard
                title={bi("Laporan Client", "Client Reports")}
                value={describeWindowsNativeDiagnosticFeed(shell)}
                icon={LifeBuoy}
              />
              <section className="glass-card section-card span-all">
                <div className="section-heading">
                  <span>{bi("Identitas runtime aktif", "Active runtime identity")}</span>
                  <p>
                    {bi(
                      "Bagian ini membedakan runtime modern dari fallback compatibility agar tidak tertukar dengan istilah Moonlight legacy.",
                      "This separates the modern runtime from the compatibility fallback so it is not confused with Moonlight legacy wording.",
                    )}
                  </p>
                </div>

                <div className="info-pairs">
                  <div>
                    <strong>{bi("Mode", "Mode")}</strong>
                    <span>{formatRuntimeMode(shell.runtime)}</span>
                  </div>
                  <div>
                    <strong>{bi("Runtime aktif", "Active runtime")}</strong>
                    <span>{formatRuntimeShort(shell.runtime)}</span>
                  </div>
                  <div>
                    <strong>{bi("Slot / profil", "Slot / profile")}</strong>
                    <span>{formatRuntimeKeys(shell.runtime)}</span>
                  </div>
                  <div>
                    <strong>{bi("Encoder / capture", "Encoder / capture")}</strong>
                    <span>{formatEncoderCapture(shell.runtime)}</span>
                  </div>
                  <div>
                    <strong>{bi("FFmpeg", "FFmpeg")}</strong>
                    <span>{shell.runtime.ffmpegSource || bi("Tidak tercatat", "Not recorded")}</span>
                  </div>
                  <div>
                    <strong>{bi("Fallback tersedia", "Available fallback")}</strong>
                    <span>{formatFallbackRuntime(shell.runtime)}</span>
                  </div>
                  <div>
                    <strong>{bi("Alasan seleksi", "Selection reason")}</strong>
                    <span>{shell.runtime.selectionReason || bi("Belum ada detail.", "No detail yet.")}</span>
                  </div>
                  <div>
                    <strong>{bi("Peringatan", "Warnings")}</strong>
                    <span>{formatRuntimeWarnings(shell.runtime)}</span>
                  </div>
                </div>
              </section>
              <section className="glass-card section-card span-all">
                <div className="section-heading">
                  <span>{bi("Penjaga daemon user-session", "User-session daemon guard")}</span>
                  <p>
                    {bi(
                      "Audit ini memastikan task CloudgimeHostUser-Host tetap memakai policy anti-stale dan restart otomatis.",
                      "This audit confirms the CloudgimeHostUser-Host task still uses the anti-stale policy and automatic restart profile.",
                    )}
                  </p>
                </div>

                <div className="info-pairs">
                  <div>
                    <strong>{bi("Status task", "Task status")}</strong>
                    <span>{describeHostUserDaemonTaskStatus(shell.hostUserDaemonTaskHealth)}</span>
                  </div>
                  <div>
                    <strong>{bi("Validasi policy", "Policy validation")}</strong>
                    <span>{describeHostUserDaemonTaskPolicy(shell.hostUserDaemonTaskHealth)}</span>
                  </div>
                  <div>
                    <strong>{bi("Task / PID", "Task / PID")}</strong>
                    <span>{formatHostUserDaemonTaskIdentity(shell.hostUserDaemonTaskHealth)}</span>
                  </div>
                  <div>
                    <strong>{bi("Restart profile", "Restart profile")}</strong>
                    <span>{formatHostUserDaemonTaskRestart(shell.hostUserDaemonTaskHealth)}</span>
                  </div>
                  <div>
                    <strong>{bi("Last run", "Last run")}</strong>
                    <span>{formatTimestampShort(shell.hostUserDaemonTaskHealth?.lastRunTimeUtc || "")}</span>
                  </div>
                  <div>
                    <strong>{bi("Audit terakhir", "Last audit")}</strong>
                    <span>{formatTimestampShort(shell.hostUserDaemonTaskHealth?.checkedAtUtc || "")}</span>
                  </div>
                  <div>
                    <strong>{bi("Multiple instance", "Multiple instance")}</strong>
                    <span>{shell.hostUserDaemonTaskHealth?.taskSettings.multipleInstancesPolicy || bi("Belum ada data", "No data yet")}</span>
                  </div>
                  <div>
                    <strong>{bi("Idle stop", "Idle stop")}</strong>
                    <span>{formatIdleGuard(shell.hostUserDaemonTaskHealth)}</span>
                  </div>
                  <div>
                    <strong>{bi("Issues", "Issues")}</strong>
                    <span>{formatHostUserDaemonTaskIssues(shell.hostUserDaemonTaskHealth)}</span>
                  </div>
                </div>
              </section>
              <section className="glass-card section-card span-all">
                <div className="section-heading">
                  <span>{bi("Laporan gagal dari app Windows", "Windows app failure reports")}</span>
                  <p>
                    {bi(
                      "Saat user menekan tombol kirim diagnostik di app Windows, ringkasannya muncul di sini agar admin cepat membaca kenapa sesi gagal.",
                      "When a user presses send diagnostics in the Windows app, the summary appears here so admins can quickly read why the session failed.",
                    )}
                  </p>
                </div>

                {getRecentWindowsNativeDiagnosticReports(shell).length ? (
                  <div className="stack-list">
                    {getRecentWindowsNativeDiagnosticReports(shell).map((report) => (
                      <details key={`${report.sessionId}-${report.sequence}`} className="diagnostic-report-card">
                        <summary className="diagnostic-report-summary">
                          <div className="diagnostic-report-summary-copy">
                            <strong>{formatDiagnosticSummary(report)}</strong>
                            <span>{formatDiagnosticMeta(report)}</span>
                          </div>
                          <span>{formatDiagnosticTimestamp(report)}</span>
                        </summary>
                        <div className="info-pairs compact-top-gap">
                          <div>
                            <strong>{bi("Mesin user", "User machine")}</strong>
                            <span>{formatDiagnosticMachine(report)}</span>
                          </div>
                          <div>
                            <strong>{bi("Jalur dipilih", "Chosen route")}</strong>
                            <span>{formatDiagnosticRoute(report)}</span>
                          </div>
                          <div>
                            <strong>{bi("Tahap", "Stage")}</strong>
                            <span>{formatDiagnosticStage(report)}</span>
                          </div>
                          <div>
                            <strong>{bi("Sesi", "Session")}</strong>
                            <span>{report.sessionId || bi("Tidak tercatat", "Not recorded")}</span>
                          </div>
                          <div>
                            <strong>{bi("Versi app", "App version")}</strong>
                            <span>{report.detailJson?.app?.version || bi("Tidak tercatat", "Not recorded")}</span>
                          </div>
                          <div>
                            <strong>{bi("Alamat host", "Host address")}</strong>
                            <span>{formatDiagnosticHostAddress(report)}</span>
                          </div>
                          <div>
                            <strong>{bi("Ringkasan user", "User-facing summary")}</strong>
                            <span>{report.detailJson?.detail || report.detailText || bi("Tidak ada detail tambahan.", "No extra detail.")}</span>
                          </div>
                          <div>
                            <strong>{bi("Profil WebView", "WebView profile")}</strong>
                            <span>{formatDiagnosticWebView(report)}</span>
                          </div>
                        </div>
                        <details className="diagnostic-raw-block">
                          <summary>{bi("Lihat log singkat", "View short log")}</summary>
                          <pre>{formatDiagnosticLogTail(report)}</pre>
                        </details>
                      </details>
                    ))}
                  </div>
                ) : (
                  <div className="status-note">
                    {bi(
                      "Belum ada laporan dari app Windows. Setelah user menekan tombol kirim diagnostik saat gagal, daftar ini akan terisi.",
                      "There are no Windows app reports yet. After a user presses send diagnostics on a failure screen, this list will populate.",
                    )}
                  </div>
                )}
              </section>
              <section className="glass-card section-card span-all">
                <div className="section-heading">
                  <span>{bi("Aksi kesehatan", "Health actions")}</span>
                  <p>{bi("Jalankan validasi, firewall, atau cek heartbeat.", "Run validation, firewall, or heartbeat checks.")}</p>
                </div>
                <div className="action-grid compact">
                  <ActionButton
                    icon={ShieldCheck}
                    label={bi("Preflight (Perbaiki)", "Preflight (Fix)")}
                    onClick={() => void runPreflight(true)}
                  />
                  <ActionButton
                    icon={ShieldCheck}
                    label={bi("Verifikasi Startup", "Verify Startup")}
                    onClick={() =>
                      void runAction("verify_startup", bi("Menjalankan validasi startup...", "Running startup validation..."))
                    }
                  />
                  <ActionButton
                    icon={Activity}
                    label={bi("Kirim Heartbeat", "Send Heartbeat")}
                    onClick={() => void handleSendHeartbeat()}
                    disabled={shell.activation.phase !== "activated"}
                  />
                  <ActionButton
                    icon={Wrench}
                    label={bi("Atur Firewall", "Configure Firewall")}
                    onClick={() =>
                      void runAction("configure_firewall", bi("Mengatur firewall...", "Configuring firewall..."))
                    }
                  />
                  <ActionButton
                    icon={FolderOpen}
                    label={bi("Kumpulkan Dukungan", "Collect Support")}
                    onClick={() =>
                      void runAction("collect_support", bi("Mengumpulkan bundle dukungan...", "Collecting support bundle..."))
                    }
                  />
                </div>
              </section>
            </div>
          )}

          {activeRail === "maintenance" && (
            <div className="page-grid two-column">
              <section className="glass-card section-card">
                <div className="section-heading">
                  <span>{bi("Aksi runtime dan layanan", "Runtime and service actions")}</span>
                  <p>{bi("Kontrol berat tetap di sini, di luar lane operator.", "Heavy controls stay here, outside the operator lane.")}</p>
                </div>
                <div className="action-grid">
                  <ActionButton
                    icon={ShieldCheck}
                    label={bi("Pasang Layanan", "Install Service")}
                    onClick={() => void runAction("install_service", bi("Memasang layanan...", "Installing service..."))}
                  />
                  <ActionButton
                    icon={RefreshCw}
                    label={bi("Reset Identitas Lokal", "Reset Local Identity")}
                    variant="danger"
                    onClick={() => void handleResetLocalHostIdentity()}
                    disabled={Boolean(activationProgress) || Boolean(busyLabel)}
                  />
                  <ActionButton
                    icon={ShieldCheck}
                    label={bi("Pulihkan Aktivasi", "Recover Activation")}
                    onClick={() => void handleRecoverHostActivation()}
                    disabled={Boolean(activationProgress) || Boolean(busyLabel)}
                  />
                  <ActionButton
                    icon={Play}
                    label={bi("Jalankan Layanan", "Start Service")}
                    onClick={() => void runAction("start_service", bi("Menyalakan layanan...", "Starting service..."))}
                    disabled={!canOperateLocalRuntime(shell)}
                  />
                  <ActionButton
                    icon={Square}
                    label={bi("Hentikan Layanan", "Stop Service")}
                    onClick={() => void runAction("stop_service", bi("Menghentikan layanan...", "Stopping service..."))}
                  />
                  <ActionButton
                    icon={RefreshCw}
                    label={bi("Restart Runtime", "Restart Runtime")}
                    variant="warning"
                    onClick={() =>
                      void runAction("restart_runtime", bi("Merestart runtime...", "Restarting runtime..."))
                    }
                    disabled={!canOperateLocalRuntime(shell)}
                  />
                  <ActionButton
                    icon={Play}
                    label={bi("Jalankan Host", "Start Host")}
                    variant="success"
                    onClick={() => void runAction("start_host", bi("Menyalakan runtime host...", "Starting host runtime..."))}
                    disabled={!canOperateLocalRuntime(shell)}
                  />
                  <ActionButton
                    icon={Square}
                    label={bi("Hapus Layanan", "Remove Service")}
                    variant="danger"
                    onClick={() => void runAction("remove_service", bi("Menghapus layanan...", "Removing service..."))}
                  />
                </div>
              </section>
              <section className="glass-card section-card">
                <div className="section-heading">
                  <span>{bi("File teknis", "Technical files")}</span>
                  <p>{bi("Buka artefak bundle host langsung.", "Open the host bundle artifacts directly.")}</p>
                </div>
                <div className="action-grid compact">
                  <ActionButton
                    icon={FolderOpen}
                    label={bi("File Runtime", "Runtime File")}
                    onClick={() => void openMaybe(shell.paths.runtimeFilePath, bi("file runtime", "runtime file"))}
                  />
                  <ActionButton
                    icon={FolderOpen}
                    label={bi("Info Rilis", "Release Info")}
                    onClick={() => void openMaybe(shell.paths.releaseInfoPath, bi("info rilis", "release info"))}
                  />
                  <ActionButton
                    icon={FolderOpen}
                    label={bi("Profil Kapabilitas", "Capability Profile")}
                    onClick={() =>
                      void openMaybe(shell.paths.capabilityProfilePath, bi("profil kapabilitas", "capability profile"))
                    }
                  />
                  <ActionButton
                    icon={FolderOpen}
                    label={bi("Paket Audio", "Audio Package")}
                    onClick={() =>
                      void openMaybe(shell.paths.audioPackagePath, bi("paket audio", "audio package"))
                    }
                    disabled={!shell.paths.audioPackagePath}
                  />
                  <ActionButton
                    icon={FolderOpen}
                    label={bi("Audio INF", "Audio INF")}
                    onClick={() => void openMaybe(shell.paths.audioInfPath, bi("audio INF", "audio INF"))}
                    disabled={!shell.paths.audioInfPath}
                  />
                  <ActionButton
                    icon={FolderOpen}
                    label={bi("Status Display", "Display State")}
                    onClick={() => void openMaybe(shell.paths.displayStatePath, bi("status display", "display state"))}
                  />
                </div>
              </section>
            </div>
          )}

          {activeRail === "support" && (
            <div className="page-grid two-column">
              <GlassInfoCard
                title={bi("Root bundle", "Bundle root")}
                value={shell.bundleRoot}
                subtitle={bi("Path utama bundle host.", "Primary host bundle path.")}
                actions={
                  <>
                    <button
                      type="button"
                      className="action-button ghost"
                      onClick={() => void openMaybe(shell.bundleRoot, "bundle root")}
                    >
                      <FolderOpen size={16} />
                      {bi("Buka", "Open")}
                    </button>
                    <button
                      type="button"
                      className="action-button ghost"
                      onClick={() => void copyText(shell.bundleRoot, "Bundle root")}
                    >
                      <Copy size={16} />
                      {bi("Salin", "Copy")}
                    </button>
                  </>
                }
              />
              <GlassInfoCard
                title={bi("Bundle dukungan", "Support bundles")}
                value={bi(
                  `${shell.support.supportBundleCount} bundle`,
                  `${shell.support.supportBundleCount} bundle(s)`,
                )}
                subtitle={shell.support.lastSupportBundleId || bi("Belum ada bundle dukungan.", "No support bundle collected yet.")}
                actions={
                  <>
                    <button
                      type="button"
                      className="action-button ghost"
                      onClick={() => void openMaybe(shell.paths.supportFolderPath, "support folder")}
                    >
                      <FolderOpen size={16} />
                      {bi("Buka folder", "Open folder")}
                    </button>
                    <button
                      type="button"
                      className="action-button ghost"
                      onClick={() =>
                        void openMaybe(shell.support.lastSupportBundlePath, "last support bundle")
                      }
                      disabled={!shell.support.lastSupportBundlePath}
                    >
                      <ArrowUpRight size={16} />
                      {bi("Buka terbaru", "Open latest")}
                    </button>
                  </>
                }
              />
              <section className="glass-card section-card span-all">
                <div className="section-heading">
                  <span>{bi("Status host mentah", "Raw host status")}</span>
                  <p>{bi("Berguna saat operator eskalasi ke support.", "Useful when operators escalate to support.")}</p>
                </div>
                <pre className="status-dump">
                  {shell.support.rawStatusJson || bi("Status JSON belum siap.", "Status JSON is not ready yet.")}
                </pre>
              </section>
            </div>
          )}

          {activeRail === "admin" && (
            <div className="page-grid two-column">
              <section className="glass-card section-card">
                <div className="section-heading">
                  <span>{bi("Kontrol admin", "Admin controls")}</span>
                  <p>{bi("Keamanan lokal dan pintasan control plane.", "Local security and control plane shortcuts.")}</p>
                </div>
                <div className="action-grid compact">
                  <ActionButton
                    icon={ArrowUpRight}
                    label={bi("Buka Halaman Token", "Open Token Page")}
                    onClick={() => void handleOpenTokenPage()}
                  />
                  <ActionButton icon={Lock} label={bi("Kunci App", "Lock App")} onClick={() => void handleLock()} />
                </div>
              </section>
              <section className="glass-card section-card">
                <div className="section-heading">
                  <span>{bi("Host terpasang", "Installed host")}</span>
                  <p>{bi("Layout Program Files dan ProgramData untuk PC ini.", "Program Files and ProgramData layout for this PC.")}</p>
                </div>
                <div className="info-pairs">
                  <div>
                    <strong>{bi("Mode terpasang", "Installed mode")}</strong>
                    <span>{shell.install.installedMode ? bi("Aktif", "Enabled") : bi("Tidak aktif", "Not active")}</span>
                  </div>
                  <div>
                    <strong>{bi("Root instalasi", "Install root")}</strong>
                    <span>{shell.install.installRoot || bi("Belum diatur", "Not set")}</span>
                  </div>
                  <div>
                    <strong>{bi("Root data", "Data root")}</strong>
                    <span>{shell.install.dataRoot || bi("Belum diatur", "Not set")}</span>
                  </div>
                  <div>
                    <strong>{bi("Apps & Features", "Apps & Features")}</strong>
                    <span>{shell.install.uninstallRegistered ? bi("Terdaftar", "Registered") : bi("Belum terdaftar", "Not registered")}</span>
                  </div>
                </div>
                <div className="action-grid compact">
                  <ActionButton
                    icon={Square}
                    label={bi("Uninstall Host", "Uninstall Host")}
                    variant="danger"
                    disabled={!shell.install.installedMode}
                    onClick={() => void handleUninstallInstalledHost()}
                  />
                  <ActionButton
                    icon={ShieldEllipsis}
                    label="EmergencyKill"
                    variant="danger"
                    disabled={!shell.install.installedMode}
                    onClick={() => handleEmergencyKillPrompt()}
                  />
                </div>
                <p className="section-note">
                  {bi(
                    "EmergencyKill untuk uninstall paksa. Shortcut keyboard: Ctrl + Shift + Delete.",
                    "EmergencyKill triggers a forced uninstall. Keyboard shortcut: Ctrl + Shift + Delete.",
                  )}
                </p>
              </section>
              <section className="glass-card section-card">
                <div className="section-heading">
                  <span>{bi("Ganti password admin", "Change admin password")}</span>
                  <p>{bi("Timpa password admin lokal untuk bundle host ini.", "Overwrite the local admin password for this host bundle.")}</p>
                </div>
                <label className="field-label">
                  <span>{bi("Password baru", "New password")}</span>
                  <input
                    type="password"
                    value={changePasswordInput}
                    onChange={(event) => setChangePasswordInput(event.currentTarget.value)}
                  />
                </label>
                <label className="field-label">
                  <span>{bi("Konfirmasi password baru", "Confirm new password")}</span>
                  <input
                    type="password"
                    value={changePasswordConfirmInput}
                    onChange={(event) => setChangePasswordConfirmInput(event.currentTarget.value)}
                  />
                </label>
                <div className="action-grid compact">
                  <ActionButton
                    icon={KeyRound}
                    label={bi("Perbarui Password", "Update Password")}
                    onClick={() => void handleChangePassword()}
                  />
                </div>
              </section>
            </div>
          )}
        </section>

      </main>

      {busyLabel && (
        <div className="busy-overlay">
          <div className="busy-card glass-card">
            <div className="busy-spinner" />
            <strong>{busyLabel}</strong>
            <p>
              {bi(
                "Shell operator menunggu perintah host selesai dijalankan.",
                "The operator shell is waiting for the host command to finish.",
              )}
            </p>
          </div>
        </div>
      )}

      {showUninstallConfirm && (
        <div className="lock-overlay">
          <div className="lock-card glass-card">
            <h3>{bi("Konfirmasi uninstall host", "Confirm host uninstall")}</h3>
            <p>
              {bi(
                "Aksi ini akan menghentikan host yang berjalan, menghapus aplikasi terpasang, dan menghapus data host lokal dari PC ini. Masukkan lagi password admin untuk melanjutkan.",
                "This will stop the running host, remove the installed app, and delete the local host data from this PC. Enter the admin password again to continue.",
              )}
            </p>
            <label className="field-label">
              <span>{bi("Password admin", "Admin password")}</span>
              <input
                type="password"
                value={uninstallPasswordInput}
                onChange={(event) => setUninstallPasswordInput(event.currentTarget.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    event.preventDefault();
                    void handleConfirmUninstallInstalledHost();
                  }
                }}
                autoFocus
              />
            </label>
            <div className="action-grid compact">
              <ActionButton
                icon={Square}
                label={bi("Uninstall Sekarang", "Uninstall Now")}
                variant="danger"
                onClick={() => void handleConfirmUninstallInstalledHost()}
              />
              <ActionButton
                icon={Lock}
                label={bi("Batal", "Cancel")}
                onClick={() => {
                  setShowUninstallConfirm(false);
                  setUninstallPasswordInput("");
                }}
              />
            </div>
          </div>
        </div>
      )}

      {showEmergencyKillConfirm && (
        <div className="lock-overlay">
          <div className="lock-card glass-card">
            <h3>EmergencyKill</h3>
            <p>
              {bi(
                "Ini adalah uninstall paksa pamungkas. Host akan diputus, service dan task akan disapu, lalu folder instalasi dan data host akan dibersihkan. Ketik EMERGENCYKILL untuk melanjutkan.",
                "This is the last-resort forced uninstall. The host will be cut off, services and tasks will be swept, then the install and host data folders will be cleaned. Type EMERGENCYKILL to continue.",
              )}
            </p>
            <label className="field-label">
              <span>{bi("Ketik konfirmasi", "Type the confirmation")}</span>
              <input
                type="text"
                value={emergencyKillPhrase}
                onChange={(event) => setEmergencyKillPhrase(event.currentTarget.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    event.preventDefault();
                    void handleConfirmEmergencyKill();
                  }
                }}
                placeholder="EMERGENCYKILL"
                autoFocus
              />
            </label>
            <div className="action-grid compact">
              <ActionButton
                icon={ShieldEllipsis}
                label="EmergencyKill"
                variant="danger"
                onClick={() => void handleConfirmEmergencyKill()}
              />
              <ActionButton
                icon={Lock}
                label={bi("Batal", "Cancel")}
                onClick={() => {
                  setShowEmergencyKillConfirm(false);
                  setEmergencyKillPhrase("");
                }}
              />
            </div>
          </div>
        </div>
      )}

      {toast && <div className="toast">{toast}</div>}
    </div>
  );
}

function ActionButton({
  icon: Icon,
  label,
  onClick,
  variant = "neutral",
  disabled,
}: {
  icon: typeof Settings2;
  label: string;
  onClick: () => void;
  variant?: "success" | "warning" | "danger" | "neutral" | "ghost";
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      className={`action-button ${variant}`}
      onClick={onClick}
      disabled={disabled}
    >
      <Icon size={16} />
      <span>{label}</span>
    </button>
  );
}

function StageCard({
  title,
  value,
  description,
}: {
  title: string;
  value: "done" | "next" | "waiting";
  description: string;
}) {
  return (
    <div className={`stage-card ${value}`}>
      <span className="stage-title">{title}</span>
      <strong>
        {value === "done"
          ? bi("Siap", "Ready")
          : value === "next"
            ? bi("Berikutnya", "Next")
            : bi("Menunggu", "Waiting")}
      </strong>
      <p>{description}</p>
    </div>
  );
}

function GlassInfoCard({
  title,
  value,
  subtitle,
  actions,
}: {
  title: string;
  value: string;
  subtitle: string;
  actions: ReactNode;
}) {
  return (
    <section className="glass-card section-card">
      <div className="section-heading">
        <span>{title}</span>
        <p>{subtitle}</p>
      </div>
      <div className="info-value">{value}</div>
      <div className="action-grid compact">{actions}</div>
    </section>
  );
}

function MetricCard({
  title,
  value,
  icon: Icon,
}: {
  title: string;
  value: string;
  icon: typeof Activity;
}) {
  return (
    <div className="metric-card glass-card">
      <div className="metric-head">
        <Icon size={18} />
        <span>{title}</span>
      </div>
      <strong>{value}</strong>
    </div>
  );
}

function hasBoundSetupToken(shell: ShellState | null) {
  if (!shell) {
    return false;
  }

  const phase = shell.activation.phase.trim().toLowerCase();
  const setupBoundPhase = phase === "prepared_local" || phase === "locked_waiting_token";
  if (!setupBoundPhase) {
    return false;
  }

  return Boolean(
    shell.activation.hostId &&
      shell.activation.sentinelPcId &&
      shell.activation.sentinelDeviceId &&
      shell.activation.keeperEntryId,
  );
}

function describeActivationLane(shell: ShellState | null) {
  const tokenKind = shell?.activation.tokenKind?.trim().toLowerCase() || "";
  if (tokenKind === "always_on_host") {
    return bi("Always-On Host", "Always-On Host");
  }
  if (tokenKind === "instance_pair") {
    return bi("Instance Pair", "Instance Pair");
  }
  if (tokenKind === "control_node") {
    return bi("Control Node", "Control Node");
  }
  if (tokenKind === "legacy_slot") {
    return bi("Legacy Slot", "Legacy Slot");
  }
  return bi("Belum ditentukan", "Not assigned yet");
}

function describeActivationLaneNote(shell: ShellState | null) {
  const tokenKind = shell?.activation.tokenKind?.trim().toLowerCase() || "";
  if (tokenKind === "always_on_host") {
    return bi(
      "Host ini memakai lane 24 jam tanpa Power Panel. Lobby akan masuk lewat claim lalu stream langsung saat route siap.",
      "This host uses the 24/7 lane without Power Panel. The lobby will go through claim and then open the stream directly once the route is ready.",
    );
  }
  if (tokenKind === "instance_pair") {
    return bi(
      "Host ini memakai lane Power Panel + Host. Slot mengikuti flow claim, power, lalu stream.",
      "This host uses the Power Panel + Host lane. The slot follows the claim, power, then stream flow.",
    );
  }
  return bi(
    "Host ini belum punya lane provisioning yang terbaca dari token terakhir.",
    "This host does not yet have a provisioning lane detected from the last claimed token.",
  );
}

function canOperateLocalRuntime(shell: ShellState | null) {
  if (!shell) {
    return false;
  }

  return shell.activation.phase === "activated" || hasBoundSetupToken(shell);
}

function buildRecommendedStep(shell: ShellState | null): {
  title: string;
  description: string;
  label: string;
  icon: typeof Settings2;
  variant: "neutral" | "success" | "warning" | "danger" | "ghost";
  action: RecommendedAction;
  disabled?: boolean;
} {
  if (!shell) {
    return {
      title: bi("Memuat status host", "Load host state"),
      description: bi(
        "Shell operator masih mengumpulkan status bundle.",
        "The operator shell is still collecting bundle status.",
      ),
      label: bi("Muat Ulang Status", "Refresh Status"),
      icon: RefreshCw,
      variant: "neutral",
      action: "none",
      disabled: true,
    };
  }

  if (shell.activation.phase === "installed_unprepared") {
    return {
      title: bi("Menunggu setup token", "Waiting for setup token"),
      description: bi(
        "Paste token host. Host Control akan menyiapkan bundle lokal, runtime, dan health check otomatis.",
        "Paste the host token. Host Control will prepare the local bundle, runtime, and health checks automatically.",
      ),
      label: bi("Otomatis", "Automatic"),
      icon: Settings2,
      variant: "neutral",
      action: "none",
    };
  }

  if (
    shell.activation.phase === "prepared_local" ||
    shell.activation.phase === "locked_waiting_token"
  ) {
    if (
      hasBoundSetupToken(shell) &&
      shell.runtime.lifecyclePhase !== "ready" &&
      !shell.runtime.requiredProcessesReady
    ) {
      return {
        title: bi("Binding masuk, runtime otomatis", "Binding applied, runtime automatic"),
        description: bi(
          "Setup token sudah mengikat slot PC ini. Host Control akan menyalakan route web stream lokal otomatis.",
          "The setup token already bound this PC slot. Host Control will start the local web stream route automatically.",
        ),
        label: bi("Otomatis", "Automatic"),
        icon: Play,
        variant: "success",
        action: "none",
      };
    }

    return {
      title: bi("Paste host token", "Paste host token"),
      description: bi(
        "Token dibuat dari master admin. Setelah ditempel, binding dan aktivasi berjalan satu arah otomatis.",
        "The token is generated from master admin. After pasting it, binding and activation run automatically in one direction.",
      ),
      label: bi("Menunggu Token", "Waiting Token"),
      icon: ArrowUpRight,
      variant: "neutral",
      action: "none",
    };
  }

  if (
    shell.activation.phase === "activated" &&
    shell.runtime.lifecyclePhase !== "ready" &&
    !shell.runtime.requiredProcessesReady
  ) {
    return {
      title: bi("Runtime sedang otomatis", "Runtime is automatic"),
      description: bi(
        "Aktivasi selesai. Host Control sedang menyalakan runtime dan mempublikasikan readiness otomatis.",
        "Activation is complete. Host Control is starting the runtime and publishing readiness automatically.",
      ),
      label: bi("Otomatis", "Automatic"),
      icon: Play,
      variant: "success",
      action: "none",
    };
  }

  if (
    shell.activation.phase === "activated" &&
    shell.runtime.requiredProcessesReady &&
    shell.runtime.lifecyclePhase === "ready" &&
    !shell.activation.readyForStream
  ) {
    return {
      title: bi("Readiness siap dikirim", "Readiness is ready to send"),
      description: bi(
        "Runtime sudah hidup. Host Control akan mengirim heartbeat otomatis saat flow token selesai.",
        "The runtime is up. Host Control will send the heartbeat automatically when the token flow finishes.",
      ),
      label: bi("Otomatis", "Automatic"),
      icon: Activity,
      variant: "warning",
      action: "none",
    };
  }

  if (shell.activation.readyForStream && shell.network.publicUrl) {
    return {
      title: bi("Host siap", "Host is ready"),
      description: bi(
        "Control plane sudah melihat host ini siap untuk stream. User masuk lewat CloudRental wrapper.",
        "The control plane already sees this host as ready for stream. Users enter through the CloudRental wrapper.",
      ),
      label: bi("Siap", "Ready"),
      icon: ArrowUpRight,
      variant: "success",
      action: "none",
    };
  }

  if (shell.activation.phase === "suspended") {
    return {
      title: bi("Ditangguhkan oleh MASTER admin", "Suspended by MASTER admin"),
      description: bi(
        "Kontrol runtime tetap terkunci sampai host ini diaktifkan lagi dari control plane.",
        "Runtime controls stay locked until this host is reactivated from the control plane.",
      ),
      label: bi("Ditangguhkan", "Suspended"),
      icon: ShieldEllipsis,
      variant: "danger",
      action: "none",
    };
  }

  if (shell.activation.phase === "revoked") {
    return {
      title: bi("Token harus diterbitkan ulang", "Token must be reissued"),
      description: bi(
        "Host ini sudah direvoke. Buka Host Control dan buat token baru sebelum dijalankan lagi.",
        "This host was revoked. Open Host Control and issue a fresh token before running it again.",
      ),
      label: bi("Butuh Token Baru", "Needs New Token"),
      icon: ArrowUpRight,
      variant: "danger",
      action: "none",
    };
  }

  return {
    title: bi("Periksa status host", "Check host status"),
    description: bi(
      "Refresh status bundle jika host ini berubah dari luar lane operator.",
      "Refresh bundle status if this host was changed outside the operator lane.",
    ),
    label: bi("Muat Ulang Status", "Refresh Status"),
    icon: RefreshCw,
    variant: "neutral",
    action: "none",
    disabled: true,
  };
}

function buildHero(shell: ShellState | null): {
  title: string;
  subtitle: string;
  badge: string;
  tone: HeroTone;
} {
  if (!shell) {
    return {
      title: bi("MEMUAT", "LOADING"),
      subtitle: bi("Mengumpulkan detail bundle host.", "Collecting host bundle details."),
      badge: bi("Memuat", "Loading"),
      tone: "neutral",
    };
  }

  const phase = shell.activation.phase;
  const running = shell.runtime.lifecyclePhase === "ready" || shell.runtime.requiredProcessesReady;

  if (phase === "activated" && running) {
    return {
      title: bi("AKTIF / BERJALAN", "ACTIVATED / RUNNING"),
      subtitle: shell.activation.readyForStream
        ? bi("Host aktif dan saat ini siap untuk stream.", "Host is active and currently ready for stream.")
        : bi(
            "Runtime sudah hidup. Kirim heartbeat jika control plane masih menampilkan readiness lama.",
            "Runtime is up. Send heartbeat if the control plane still shows stale readiness.",
          ),
      badge: shell.activation.readyForStream
        ? bi("Siap untuk stream", "Ready for stream")
        : bi("Runtime hidup", "Runtime up"),
      tone: "success",
    };
  }

  if (phase === "activated") {
    return {
      title: bi("AKTIF / SIAGA", "ACTIVATED / STANDBY"),
      subtitle: bi(
        "Token sudah dipakai. Jalankan runtime host saat PC ini harus menerima sesi.",
        "Token is redeemed. Start the host runtime when this PC should accept sessions.",
      ),
      badge: bi("Aktivasi terbuka", "Activation unlocked"),
      tone: "warning",
    };
  }

  if (phase === "prepared_local" || phase === "locked_waiting_token") {
    if (hasBoundSetupToken(shell) && running) {
      return {
        title: bi("BINDING OK / BERJALAN", "BOUND / RUNNING"),
        subtitle: bi(
          "Binding setup token sudah lengkap dan runtime lokal sedang hidup. Jalur open web stream seharusnya bisa dipakai.",
          "The setup token binding is complete and the local runtime is up. The open web stream route should now be usable.",
        ),
        badge: bi("Runtime lokal hidup", "Local runtime up"),
        tone: "success",
      };
    }

    if (hasBoundSetupToken(shell)) {
      return {
        title: bi("BINDING OK / SIAGA", "BOUND / STANDBY"),
        subtitle: bi(
          "Identity slot PC sudah terikat dari setup token. Jalankan host untuk menghidupkan route web stream lokal.",
          "The PC slot identity is already bound from the setup token. Start the host to bring the local web stream route online.",
        ),
        badge: bi("Binding lengkap", "Binding complete"),
        tone: "warning",
      };
    }

    return {
      title: bi("SIAP LOKAL / MENUNGGU TOKEN", "SET UP / WAITING TOKEN"),
      subtitle: bi(
        "Setup lokal selesai. Buat token di Host Control lalu paste di sini.",
        "Local setup is done. Generate a token in Host Control and paste it here.",
      ),
      badge: bi("Siap lokal", "Prepare only"),
      tone: "neutral",
    };
  }

  if (phase === "suspended") {
    return {
      title: bi("DITANGGUHKAN", "SUSPENDED"),
      subtitle: bi(
        "MASTER admin harus mengaktifkan ulang host ini sebelum aksi runtime terbuka lagi.",
        "MASTER admin must reactivate this host before runtime actions unlock again.",
      ),
      badge: bi("Diblokir", "Blocked"),
      tone: "critical",
    };
  }

  if (phase === "revoked") {
    return {
      title: bi("DICABUT", "REVOKED"),
      subtitle: bi(
        "Host ini membutuhkan token aktivasi baru dari control plane.",
        "This host needs a new activation token from the control plane.",
      ),
      badge: bi("Dicabut", "Revoked"),
      tone: "critical",
    };
  }

  return {
    title: bi("BELUM SIAP / LOKAL", "NOT READY / LOCAL"),
    subtitle: bi(
      "Jalankan setup host dulu, lalu lanjut ke aktivasi.",
      "Run host setup first, then continue to activation.",
    ),
    badge: bi("Perlu setup", "Needs setup"),
    tone: "neutral",
  };
}

function buildStageState(shell: ShellState | null) {
  if (!shell) {
    return { setup: "waiting", activated: "waiting", running: "waiting" } as const;
  }

  const bindingReady = hasBoundSetupToken(shell);
  const setup = shell.activation.phase === "installed_unprepared" ? "next" : "done";
  const activated =
    shell.activation.phase === "activated" || bindingReady
      ? "done"
      : setup === "done"
        ? "next"
        : "waiting";
  const running =
    canOperateLocalRuntime(shell) &&
    (shell.runtime.lifecyclePhase === "ready" || shell.runtime.requiredProcessesReady)
      ? "done"
      : canOperateLocalRuntime(shell)
        ? "next"
        : "waiting";

  return { setup, activated, running } as const;
}

function buildTokenPageUrl(
  shell: ShellState | null,
  displayName: string,
  controlPlaneUrl: string,
) {
  const base = normalizeControlPlane(
    controlPlaneUrl || shell?.activation.controlPlaneUrl || "https://cloudgime.my.id",
  );
  const url = new URL(base);
  url.searchParams.set("openHostControl", "1");
  url.searchParams.set("adminTab", "SETTINGS");
  url.searchParams.set("settingsSection", "HOST_CONTROL");

  if (shell?.activation.hostId) {
    url.searchParams.set("hostId", shell.activation.hostId);
    url.searchParams.set("lookupHostId", shell.activation.hostId);
  }

  if (shell?.activation.sentinelPcId) {
    url.searchParams.set("sentinelPcId", shell.activation.sentinelPcId);
  }
  if (shell?.activation.sentinelDeviceId) {
    url.searchParams.set("sentinelDeviceId", shell.activation.sentinelDeviceId);
  }
  if (shell?.activation.keeperEntryId) {
    url.searchParams.set("keeperEntryId", shell.activation.keeperEntryId);
  }

  const name = displayName.trim() || shell?.activation.displayName || "";
  if (name) {
    url.searchParams.set("displayName", name);
  }

  return url.toString();
}

function delayMs(value: number) {
  return new Promise((resolve) => window.setTimeout(resolve, value));
}

function bi(id: string, en: string) {
  return activeUiLanguage === "en" ? en : id;
}

function createActivationProgress(
  stage: ActivationProgressStage,
  secondsRemaining: number,
): ActivationProgress {
  switch (stage) {
    case "verify_token":
      return {
        stage,
        mode: "submitting",
        title: bi("Memverifikasi token", "Verifying token"),
        detail: bi(
          "Memeriksa token baru ke control plane. Klik Activate sekali saja, lalu tunggu prosesnya.",
          "Checking the new token with the control plane. Click Activate once, then wait for the process to finish.",
        ),
        secondsRemaining,
      };
    case "binding_host":
      return {
        stage,
        mode: "confirming",
        title: bi("Menautkan host", "Binding host"),
        detail: bi(
          "Token sudah diterima. Cloudgime sedang menautkan host ini. Jangan paste token baru selama proses berjalan.",
          "The token was accepted. Cloudgime is binding this host now. Do not paste a new token while the process is running.",
        ),
        secondsRemaining,
      };
    case "starting_runtime":
      return {
        stage,
        mode: "confirming",
        title: bi("Menyalakan runtime", "Starting runtime"),
        detail: bi(
          "Aktivasi sudah diterima. Host sedang menyalakan runtime lokal dan menunggu pemeriksaan kesehatan selesai.",
          "Activation was accepted. The host is starting the local runtime and waiting for health checks to finish.",
        ),
        secondsRemaining,
      };
    case "ready_for_stream":
      return {
        stage,
        mode: "confirming",
        title: bi("Siap untuk stream", "Ready for stream"),
        detail: bi(
          "Runtime dan route stream sudah siap. Menyinkronkan status akhir ke control plane.",
          "The runtime and stream route are ready. Syncing the final status to the control plane.",
        ),
        secondsRemaining,
      };
  }
}

function resolveActivationProgressStage(
  state: ShellState | null,
  attempt: number,
): ActivationProgressStage {
  if (!state) {
    return "verify_token";
  }

  const tokenAccepted = Boolean(
    state.activation.redeemedAtUtc ||
      state.activation.runtimeTokenPresent ||
      state.activation.activationRecordIdPresent,
  );

  if (state.activation.phase !== "activated") {
    return tokenAccepted || attempt >= 2 ? "binding_host" : "verify_token";
  }

  if (
    state.runtime.lifecyclePhase === "ready" &&
    state.runtime.localHttpReady &&
    state.runtime.requiredProcessesReady &&
    state.network.publicUrl
  ) {
    return "ready_for_stream";
  }

  return "starting_runtime";
}

function buildActivationProgressSteps(stage: ActivationProgressStage) {
  const steps: { key: ActivationProgressStage; label: string }[] = [
    { key: "verify_token", label: bi("Memverifikasi token", "Verifying token") },
    { key: "binding_host", label: bi("Menautkan host", "Binding host") },
    { key: "starting_runtime", label: bi("Menyalakan runtime", "Starting runtime") },
    { key: "ready_for_stream", label: bi("Siap untuk stream", "Ready for stream") },
  ];
  const currentIndex = steps.findIndex((step) => step.key === stage);

  return steps.map((step, index) => ({
    ...step,
    order: index + 1,
    state:
      index < currentIndex
        ? "done"
        : index === currentIndex
          ? "current"
          : "pending",
  }));
}

function normalizePhase(phase: string) {
  const normalized = (phase || "unknown").trim().toLowerCase().replace(/\s+/g, "_");
  const dictionary: Record<string, [string, string]> = {
    activated: ["Aktif", "Activated"],
    running: ["Berjalan", "Running"],
    ready: ["Siap", "Ready"],
    waiting: ["Menunggu", "Waiting"],
    next: ["Berikutnya", "Next"],
    auto: ["Otomatis", "Automatic"],
    manual: ["Manual", "Manual"],
    unknown: ["Tidak diketahui", "Unknown"],
    suspended: ["Ditangguhkan", "Suspended"],
    revoked: ["Dicabut", "Revoked"],
    enabled: ["Aktif", "Enabled"],
    disabled: ["Nonaktif", "Disabled"],
    missing: ["Tidak ada", "Missing"],
    installed_unprepared: ["Terpasang, belum disiapkan", "Installed, unprepared"],
    prepared_local: ["Siap lokal", "Prepared local"],
    locked_waiting_token: ["Terkunci, menunggu token", "Locked, waiting token"],
    starting_runtime: ["Menyalakan runtime", "Starting runtime"],
    configuring_firewall: ["Mengatur firewall", "Configuring firewall"],
    not_ready: ["Belum siap", "Not ready"],
  };
  const translated = dictionary[normalized];
  if (translated) {
    return bi(translated[0], translated[1]);
  }
  return normalized
    .replace(/_/g, " ")
    .replace(/\b\w/g, (segment) => segment.toUpperCase());
}

function normalizeControlPlane(url: string) {
  const trimmed = (url || "https://cloudgime.my.id").trim();
  return trimmed ? trimmed.replace(/\/+$/, "") : "https://cloudgime.my.id";
}

function describeReadinessBadge(shell: ShellState) {
  if (shell.activation.readyForStream) {
    return bi("Siap untuk stream", "Ready for stream");
  }

  if (shell.activation.phase !== "activated") {
    return bi("Menunggu aktivasi", "Waiting activation");
  }

  if (
    shell.runtime.lifecyclePhase !== "ready" ||
    !shell.runtime.localHttpReady ||
    !shell.runtime.requiredProcessesReady
  ) {
    return bi("Menyalakan runtime", "Starting runtime");
  }

  if (!shell.network.publicUrl) {
    return bi("Menyiapkan route", "Preparing route");
  }

  return bi("Mempublikasikan readiness", "Publishing readiness");
}

function isCompatibilityRuntime(runtime: ShellState["runtime"]) {
  const haystack = [
    runtime.runtimeKey,
    runtime.runtimeProfileKey,
    runtime.runtimeLabel,
    runtime.runtimeVersion,
  ]
    .join(" ")
    .toLowerCase();

  return (
    haystack.includes("legacy") ||
    haystack.includes("compatibility") ||
    haystack.includes("0.20.")
  );
}

function formatRuntimeMode(runtime: ShellState["runtime"]) {
  if (!runtime.runtimeLabel && !runtime.runtimeKey && !runtime.runtimeProfileKey) {
    return bi("Belum dipilih", "Not selected");
  }

  return isCompatibilityRuntime(runtime)
    ? bi("Compatibility fallback", "Compatibility fallback")
    : bi("Modern runtime", "Modern runtime");
}

function formatRuntimeShort(runtime: ShellState["runtime"]) {
  const label = runtime.runtimeLabel || bi("Belum dipilih", "Not selected");
  const version = runtime.runtimeVersion ? ` ${runtime.runtimeVersion}` : "";
  return `${label}${version}`;
}

function formatRuntimeKeys(runtime: ShellState["runtime"]) {
  const keys = [runtime.runtimeKey, runtime.runtimeProfileKey]
    .map((value) => String(value || "").trim())
    .filter(Boolean)
    .filter((value, index, values) => values.indexOf(value) === index);
  return keys.length ? keys.join(" / ") : bi("Belum tercatat", "Not recorded");
}

function formatEncoderCapture(runtime: ShellState["runtime"]) {
  const encoder = runtime.encoder ? runtime.encoder.toUpperCase() : bi("Belum dipilih", "Not selected");
  const capture = runtime.capture ? runtime.capture.toUpperCase() : bi("capture belum tercatat", "capture not recorded");
  const reason = runtime.captureReason ? ` (${runtime.captureReason})` : "";
  return `${encoder} / ${capture}${reason}`;
}

function formatDisplayMode(mode: string) {
  switch ((mode || "mtt_vdd").replace(/-/g, "_").toLowerCase()) {
    case "auto":
      return bi("Auto", "Auto");
    case "qemu_virtio":
    case "qemu":
    case "virtio":
      return "QEMU / VirtIO";
    case "parsec_vda":
    case "parsec":
      return "Parsec VDA";
    case "primary":
    case "current_primary":
      return bi("Primary saat ini", "Current primary");
    case "custom":
      return bi("Custom", "Custom");
    case "mtt_vdd":
    default:
      return "MTT VDD";
  }
}

function formatFallbackRuntime(runtime: ShellState["runtime"]) {
  if (!runtime.fallbackRuntimeLabel) {
    return bi("Tidak ada fallback siap", "No ready fallback");
  }

  const version = runtime.fallbackRuntimeVersion ? ` ${runtime.fallbackRuntimeVersion}` : "";
  const reason = runtime.fallbackRuntimeReason ? ` - ${runtime.fallbackRuntimeReason}` : "";
  return `${runtime.fallbackRuntimeLabel}${version}${reason}`;
}

function formatRuntimeWarnings(runtime: ShellState["runtime"]) {
  const warnings = Array.isArray(runtime.warnings) ? runtime.warnings : [];
  if (!warnings.length) {
    return bi("Tidak ada", "None");
  }

  return warnings.join(", ");
}

function describeHostUserDaemonTaskStatus(health?: HostUserDaemonTaskHealth | null) {
  if (!health) {
    return bi("Belum diaudit", "Not audited yet");
  }
  if (health.policyValid && health.daemonRunning) {
    return bi("Sehat", "Healthy");
  }
  if (health.policyValid) {
    return bi("Policy oke, daemon hilang", "Policy valid, daemon missing");
  }
  return bi("Policy bermasalah", "Policy invalid");
}

function describeHostUserDaemonTaskPolicy(health?: HostUserDaemonTaskHealth | null) {
  if (!health) {
    return bi("Belum ada data", "No data yet");
  }
  return health.policyValid ? bi("Valid", "Valid") : bi("Tidak valid", "Invalid");
}

function formatHostUserDaemonTaskIdentity(health?: HostUserDaemonTaskHealth | null) {
  if (!health) {
    return bi("Belum ada data", "No data yet");
  }
  const pid = health.daemonPid > 0 ? `PID ${health.daemonPid}` : bi("daemon belum ada", "daemon missing");
  return `${health.taskName || "CloudgimeHostUser-Host"} • ${pid}`;
}

function formatHostUserDaemonTaskRestart(health?: HostUserDaemonTaskHealth | null) {
  if (!health) {
    return bi("Belum ada data", "No data yet");
  }
  const count = health.taskSettings.restartCount || "?";
  const interval = health.taskSettings.restartInterval || "?";
  return `${count} / ${interval}`;
}

function formatIdleGuard(health?: HostUserDaemonTaskHealth | null) {
  if (!health) {
    return bi("Belum ada data", "No data yet");
  }
  const stopOnIdle = health.taskSettings.idleStopOnIdleEnd || bi("unknown", "unknown");
  const restartOnIdle = health.taskSettings.idleRestartOnIdle || bi("unknown", "unknown");
  return `StopOnIdleEnd=${stopOnIdle} • RestartOnIdle=${restartOnIdle}`;
}

function formatHostUserDaemonTaskIssues(health?: HostUserDaemonTaskHealth | null) {
  if (!health) {
    return bi("Belum ada data", "No data yet");
  }
  if (!health.issues.length) {
    return bi("Tidak ada issue aktif", "No active issues");
  }
  return health.issues.join(" • ");
}

function getRecentWindowsNativeDiagnosticReports(shell: ShellState) {
  const reports = shell.windowsNativeDiagnosticReports?.reports;
  if (!Array.isArray(reports)) {
    return [] as WindowsNativeDiagnosticReportEntry[];
  }

  return [...reports]
    .sort((left, right) => (right.recordedAtUnixMs || 0) - (left.recordedAtUnixMs || 0))
    .slice(0, 6);
}

function describeWindowsNativeDiagnosticFeed(shell: ShellState) {
  const reports = getRecentWindowsNativeDiagnosticReports(shell);
  if (!reports.length) {
    return bi("Belum ada report", "No reports yet");
  }

  return bi(`${reports.length} laporan terbaru`, `${reports.length} recent reports`);
}

function formatDiagnosticTimestamp(report: WindowsNativeDiagnosticReportEntry) {
  if (!report.recordedAtUnixMs) {
    return bi("Waktu belum ada", "No timestamp");
  }

  return new Date(report.recordedAtUnixMs).toLocaleString("en-GB", {
    year: "numeric",
    month: "short",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function formatDiagnosticSummary(report: WindowsNativeDiagnosticReportEntry) {
  return (
    report.detailJson?.summary ||
    report.detailText ||
    bi("Laporan gagal dari app Windows", "Windows app failure report")
  );
}

function formatDiagnosticMeta(report: WindowsNativeDiagnosticReportEntry) {
  const stage = formatDiagnosticStage(report);
  const route = formatDiagnosticRoute(report);
  return `${stage} • ${route}`;
}

function formatDiagnosticStage(report: WindowsNativeDiagnosticReportEntry) {
  const stage = (report.stage || "").trim();
  if (!stage) {
    return bi("Tahap tidak tercatat", "Stage not recorded");
  }

  switch (stage) {
    case "route_locked_failure":
      return bi("Jalur pilihan berhenti", "Chosen route stopped");
    case "player_failure":
      return bi("Player gagal dibuka", "Player failed to open");
    default:
      return stage.replace(/_/g, " ");
  }
}

function formatDiagnosticRoute(report: WindowsNativeDiagnosticReportEntry) {
  const route = (report.detailJson?.selectedRoute || "").trim().toLowerCase();
  switch (route) {
    case "direct":
      return bi("Koneksi Cepat", "Fast connection");
    case "relay":
      return bi("Koneksi Stabil", "Stable connection");
    default:
      return bi("Tidak tercatat", "Not recorded");
  }
}

function formatDiagnosticMachine(report: WindowsNativeDiagnosticReportEntry) {
  const machine = report.detailJson?.machine;
  if (!machine) {
    return bi("Tidak tercatat", "Not recorded");
  }

  const name = machine.computerName || bi("PC tidak dikenal", "Unknown PC");
  const arch = machine.processArchitecture ? ` • ${machine.processArchitecture}` : "";
  return `${name}${arch}`;
}

function formatDiagnosticHostAddress(report: WindowsNativeDiagnosticReportEntry) {
  const session = report.detailJson?.session;
  if (!session?.hostAddress) {
    return bi("Tidak tercatat", "Not recorded");
  }
  return session.httpPort ? `${session.hostAddress}:${session.httpPort}` : session.hostAddress;
}

function formatDiagnosticWebView(report: WindowsNativeDiagnosticReportEntry) {
  const webView = report.detailJson?.webView;
  if (!webView?.playerProfilePath) {
    return bi("Tidak tercatat", "Not recorded");
  }
  return webView.playerProfilePath;
}

function formatDiagnosticLogTail(report: WindowsNativeDiagnosticReportEntry) {
  const logTail = (report.detailJson?.recentLogTail || report.detailText || "").trim();
  if (!logTail) {
    return bi("Belum ada log tambahan.", "No extra log yet.");
  }
  return logTail;
}

function describeRuntimeBlockingState(shell: ShellState) {
  const blockers: string[] = [];
  if (shell.activation.phase !== "activated") {
    blockers.push(`${bi("aktivasi", "activation")}=${normalizePhase(shell.activation.phase)}`);
  }
  if (shell.runtime.lifecyclePhase !== "ready") {
    blockers.push(`${bi("runtime", "runtime")}=${normalizePhase(shell.runtime.lifecyclePhase || "unknown")}`);
  }
  if (!shell.runtime.requiredProcessesReady) {
    blockers.push(bi("proses runtime belum lengkap", "runtime processes are not ready"));
  }
  if (!shell.runtime.localHttpReady) {
    blockers.push(bi("HTTP lokal belum siap", "local HTTP is not ready"));
  }
  if (!shell.network.publicUrl) {
    blockers.push(bi("URL publik belum dibuat", "public URL is missing"));
  }

  if (!blockers.length) {
    return "";
  }

  return `${bi("Status tertahan:", "Blocked by:")} ${blockers.join(", ")}.`;
}

function describeActivationPhase(phase: string) {
  switch (phase) {
    case "prepared_local":
      return bi(
        "Langkah berikutnya: buat token di cloudgime.my.id > Settings > Host Control.",
        "Next step: issue a token in cloudgime.my.id > Settings > Host Control.",
      );
    case "locked_waiting_token":
      return bi(
        "Langkah berikutnya: paste token yang sudah dibuat di sini untuk membuka aksi runtime.",
        "Next step: paste the issued token here to unlock runtime actions.",
      );
    case "activated":
      return bi(
        "Host ini sudah aktif. Aksi runtime dan stream sudah terbuka.",
        "This host is activated. Runtime and stream actions are unlocked.",
      );
    case "suspended":
      return bi(
        "Host ini ditangguhkan sampai MASTER admin mengaktifkannya lagi.",
        "This host is suspended until MASTER admin reactivates it.",
      );
    case "revoked":
      return bi(
        "Host ini dicabut dan membutuhkan token baru.",
        "This host is revoked and needs a new token.",
      );
    default:
      return bi(
        "Jalankan setup lokal sebelum membuat token.",
        "Run local setup before you generate a token.",
      );
  }
}

function formatTimestampShort(value: string) {
  const trimmed = (value || "").trim();
  if (!trimmed) {
    return bi("Belum tercatat", "Not recorded yet");
  }

  const parsed = new Date(trimmed);
  if (Number.isNaN(parsed.getTime())) {
    return trimmed;
  }

  return parsed.toLocaleString("en-GB", {
    year: "numeric",
    month: "short",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function formatSyncStamp(value: string) {
  const trimmed = (value || "").trim();
  if (!trimmed) {
    return bi("baru saja", "just now");
  }

  const parsed = new Date(trimmed);
  if (Number.isNaN(parsed.getTime())) {
    return trimmed;
  }

  return parsed.toLocaleTimeString("en-GB", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

function extractErrorMessage(error: unknown) {
  if (typeof error === "string") {
    return error;
  }

  if (
    error &&
    typeof error === "object" &&
    "message" in error &&
    typeof error.message === "string"
  ) {
    return error.message;
  }

  return bi("Aksi host gagal.", "The host action failed.");
}

export default App;
