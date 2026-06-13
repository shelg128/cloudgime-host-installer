export type RailKey =
  | "operator"
  | "access"
  | "audio"
  | "display"
  | "health"
  | "maintenance"
  | "support"
  | "admin";

export interface ShellState {
  auth: AuthState;
  bundleRoot: string;
  installerAvailable: boolean;
  install: InstallView;
  activation: ActivationView;
  network: NetworkView;
  audio: AudioView;
  display: DisplayView;
  runtime: RuntimeView;
  support: SupportView;
  paths: PathView;
  hostUserDaemonTaskHealth?: HostUserDaemonTaskHealth | null;
  windowsNativeDiagnosticReports?: WindowsNativeDiagnosticReportsFile | null;
}

export interface AuthState {
  passwordExists: boolean;
  needsPasswordSetup: boolean;
  unlocked: boolean;
}

export interface InstallView {
  installedMode: boolean;
  installRoot: string;
  dataRoot: string;
  uninstallRegistered: boolean;
  launchIntent: string;
}

export interface ActivationView {
  hostId: string;
  displayName: string;
  sentinelPcId: string;
  sentinelDeviceId: string;
  keeperEntryId: string;
  tokenKind: string;
  instanceType: string;
  phase: string;
  controlPlaneUrl: string;
  activatedAtUtc: string;
  redeemedAtUtc: string;
  lastHeartbeatAtUtc: string;
  readyForStream: boolean;
  runtimeTokenPresent: boolean;
  activationRecordIdPresent: boolean;
}

export interface NetworkView {
  publicUrl: string;
  localUrl: string;
}

export interface RuntimeView {
  lifecyclePhase: string;
  healthGrade: string;
  audioStatus: string;
  serviceState: string;
  runtimeLabel: string;
  runtimeKey: string;
  runtimeProfileKey: string;
  runtimeVersion: string;
  encoder: string;
  capture: string;
  captureReason: string;
  selectionReason: string;
  ffmpegSource: string;
  fallbackRuntimeLabel: string;
  fallbackRuntimeVersion: string;
  fallbackRuntimeReason: string;
  warnings: string[];
  localHttpReady: boolean;
  requiredProcessesReady: boolean;
}

export interface AudioView {
  mode: string;
  selectedAudioSinkName: string;
  selectedVirtualSinkName: string;
  selectedMicrophoneName: string;
  selectionReason: string;
  routingStatus: string;
  routingReason: string;
  availableOutputs: string[];
  availableInputs: string[];
}

export interface DisplayView {
  mode: string;
  customDeviceName: string;
  customDeviceId: string;
  customLabel: string;
  effectiveLabel: string;
  updatedAt: string;
  dualStreamEnabled: boolean;
}

export interface SupportView {
  supportBundleCount: number;
  lastSupportBundleId: string;
  lastSupportBundlePath: string;
  rawStatusJson: string;
}

export interface PathView {
  bundleRoot: string;
  serverFolderPath: string;
  supportFolderPath: string;
  runtimeFilePath: string;
  releaseInfoPath: string;
  capabilityProfilePath: string;
  audioPackagePath: string;
  audioInfPath: string;
  displayStatePath: string;
}

export interface HostUserDaemonTaskHealth {
  schemaVersion: number;
  taskName: string;
  bundleRoot: string;
  daemonPath: string;
  checkedAtUtc: string;
  policyValid: boolean;
  taskState: string;
  lastTaskResult: number;
  lastRunTimeUtc: string;
  daemonRunning: boolean;
  daemonPid: number;
  taskSettings: HostUserDaemonTaskSettings;
  issues: string[];
}

export interface HostUserDaemonTaskSettings {
  multipleInstancesPolicy: string;
  restartCount: string;
  restartInterval: string;
  executionTimeLimit: string;
  startWhenAvailable: string;
  hidden: string;
  disallowStartIfOnBatteries: string;
  stopIfGoingOnBatteries: string;
  useUnifiedSchedulingEngine: string;
  idleStopOnIdleEnd: string;
  idleRestartOnIdle: string;
}

export interface WindowsNativeDiagnosticReportsFile {
  schemaVersion: number;
  reports: WindowsNativeDiagnosticReportEntry[];
}

export interface WindowsNativeDiagnosticReportEntry {
  tokenId: string;
  sessionId: string;
  userId: number;
  sequence: number;
  eventName: string;
  stage: string;
  recordedAtUnixMs: number;
  clientTimeUnixMs?: number | null;
  detailJson?: WindowsNativeDiagnosticDetail | null;
  detailText?: string | null;
}

export interface WindowsNativeDiagnosticDetail {
  schemaVersion?: number;
  reportKind?: string;
  createdAtUtc?: string;
  summary?: string;
  detail?: string;
  selectedRoute?: string;
  selectedRelayRegion?: string | null;
  machine?: {
    computerName?: string;
    userName?: string;
    osVersion?: string;
    is64BitOperatingSystem?: boolean;
    processArchitecture?: string;
  };
  app?: {
    version?: string;
    logPath?: string;
    sourceUri?: string;
    webBaseUrl?: string;
    keeperTunnelSessionPresent?: boolean;
  };
  session?: {
    tokenId?: string;
    sessionId?: string;
    hostId?: number;
    appId?: number;
    connectionMode?: string;
    relayAllowed?: boolean;
    hostAddress?: string;
    httpPort?: number;
  };
  webView?: {
    lobbyProfilePath?: string;
    playerProfilePath?: string;
    additionalBrowserArguments?: string;
  };
  exception?: string | null;
  recentLogTail?: string;
}

export interface ActionOutcome {
  message: string;
  state: ShellState;
}
