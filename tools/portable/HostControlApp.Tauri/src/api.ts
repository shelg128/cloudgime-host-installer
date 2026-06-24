import { invoke } from "@tauri-apps/api/core";
import type { ActionOutcome, ShellState } from "./types";

export type HostAction =
  | "setup_host"
  | "verify_startup"
  | "start_host"
  | "restart_runtime"
  | "stop_host"
  | "install_service"
  | "start_service"
  | "stop_service"
  | "remove_service"
  | "configure_firewall"
  | "collect_support";

export async function getShellState(): Promise<ShellState> {
  return invoke<ShellState>("get_shell_state");
}

export async function setAdminPassword(password: string): Promise<ShellState> {
  return invoke<ShellState>("set_admin_password", { password });
}

export async function unlockApp(password: string): Promise<ShellState> {
  return invoke<ShellState>("unlock_app", { password });
}

export async function lockApp(): Promise<ShellState> {
  return invoke<ShellState>("lock_app");
}

export async function changeAdminPassword(password: string): Promise<ShellState> {
  return invoke<ShellState>("change_admin_password", { password });
}

export async function runHostAction(action: HostAction): Promise<ActionOutcome> {
  return invoke<ActionOutcome>("run_host_action", { action });
}

export async function runPreflightHost(args: { fix: boolean }): Promise<ActionOutcome> {
  return invoke<ActionOutcome>("preflight_host", args);
}

export async function savePreferences(args: {
  displayName: string;
  controlPlaneUrl: string;
}): Promise<ShellState> {
  return invoke<ShellState>("save_preferences", {
    displayName: args.displayName,
    controlPlaneUrl: args.controlPlaneUrl,
    display_name: args.displayName,
    control_plane_url: args.controlPlaneUrl,
  });
}

export async function saveAudioPreferences(args: {
  mode: string;
  selectedAudioSinkName: string;
  selectedVirtualSinkName: string;
  selectedMicrophoneName: string;
}): Promise<ActionOutcome> {
  return invoke<ActionOutcome>("save_audio_preferences", {
    mode: args.mode,
    selectedAudioSinkName: args.selectedAudioSinkName,
    selectedVirtualSinkName: args.selectedVirtualSinkName,
    selectedMicrophoneName: args.selectedMicrophoneName,
    selected_audio_sink_name: args.selectedAudioSinkName,
    selected_virtual_sink_name: args.selectedVirtualSinkName,
    selected_microphone_name: args.selectedMicrophoneName,
  });
}

export async function saveDisplayPreferences(args: {
  mode: string;
  customDeviceName: string;
  customDeviceId: string;
  customLabel: string;
}): Promise<ActionOutcome> {
  return invoke<ActionOutcome>("save_display_preferences", {
    mode: args.mode,
    customDeviceName: args.customDeviceName,
    customDeviceId: args.customDeviceId,
    customLabel: args.customLabel,
    custom_device_name: args.customDeviceName,
    custom_device_id: args.customDeviceId,
    custom_label: args.customLabel,
  });
}

export async function syncHostBinding(): Promise<ShellState> {
  return invoke<ShellState>("sync_host_binding");
}

export async function claimSetupToken(args: {
  setupToken: string;
  expectedTokenKind: "instance_pair" | "always_on_host" | "";
}): Promise<ActionOutcome> {
  return invoke<ActionOutcome>("claim_activation_license", {
    activationLicense: args.setupToken,
    expectedTokenKind: args.expectedTokenKind,
    activation_license: args.setupToken,
    expected_token_kind: args.expectedTokenKind,
  });
}

export async function activateWithLicense(args: {
  activationLicense: string;
  expectedActivationLicenseKind: "instance_pair" | "always_on_host" | "";
}): Promise<ActionOutcome> {
  return invoke<ActionOutcome>("claim_setup_token", {
    setupToken: args.activationLicense,
    expectedTokenKind: args.expectedActivationLicenseKind,
  });
}

export async function resetLocalHostIdentity(): Promise<ActionOutcome> {
  return invoke<ActionOutcome>("reset_local_host_identity");
}

export async function redeemActivationToken(args: {
  activationToken: string;
}): Promise<ActionOutcome> {
  return invoke<ActionOutcome>("redeem_activation_token", {
    activationToken: args.activationToken,
    activation_token: args.activationToken,
  });
}

export async function recoverHostActivation(): Promise<ActionOutcome> {
  return invoke<ActionOutcome>("recover_host_activation");
}

export async function sendHeartbeat(): Promise<ActionOutcome> {
  return invoke<ActionOutcome>("send_heartbeat");
}

export async function uploadHostDiagnostic(args: { reason: string }): Promise<ActionOutcome> {
  return invoke<ActionOutcome>("upload_host_diagnostic", args);
}

export async function uninstallInstalledHost(args: { password: string }): Promise<string> {
  return invoke<string>("uninstall_installed_host", args);
}

export async function launchEmergencyUninstaller(): Promise<string> {
  return invoke<string>("launch_emergency_uninstaller");
}

export async function toggleDualStream(enabled: boolean): Promise<ActionOutcome> {
  return invoke<ActionOutcome>("toggle_dual_stream", { enabled });
}
