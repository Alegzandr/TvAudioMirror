// Mirrors the shapes the engine serialises. Kept by hand and deliberately
// narrow: only what the panel actually reads.

export type LinkState =
  | "idle"
  | "missing"
  | "connecting"
  | "priming"
  | "live"
  | "failed";

export type LatencyProfile = "safe" | "balanced" | "tight" | "custom";

export interface DeviceEntry {
  id: string;
  name: string;
  interface: string;
  isDefault: boolean;
  sampleRate: number;
  channels: number;
  loopback: boolean;
}

export interface DeviceCatalog {
  sources: DeviceEntry[];
  targets: DeviceEntry[];
  loopbackAvailable: boolean;
}

export interface SourceStatus {
  id: string;
  name: string;
  state: LinkState;
  detail: string | null;
  retryInMs: number | null;
  sampleRate: number;
  channels: number;
  loopback: boolean;
  peak: number;
  rms: number;
}

export interface TargetStatus {
  id: string;
  name: string;
  state: LinkState;
  detail: string | null;
  retryInMs: number | null;
  sampleRate: number;
  channels: number;
  gainDb: number;
  muted: boolean;
  latencyMs: number;
  captureMs: number;
  bufferMs: number;
  renderMs: number;
  correctionPpm: number;
  underruns: number;
  overruns: number;
  peak: number;
  rms: number;
}

export interface EngineStatus {
  enabled: boolean;
  source: SourceStatus | null;
  targets: TargetStatus[];
  mirroring: boolean;
}

export interface TargetConfig {
  id: string;
  name: string;
  enabled: boolean;
  gainDb: number;
  muted: boolean;
}

export interface MirrorConfig {
  enabled: boolean;
  source: { id: string; name: string } | null;
  targets: TargetConfig[];
  latency: LatencyProfile;
  latencyMs: number;
}

export interface Preferences {
  startMinimized: boolean;
  closeToTray: boolean;
  autostart: boolean;
}

export interface Platform {
  os: string;
  loopbackAvailable: boolean;
  portable: boolean;
  settingsPath: string;
  maxTargets: number;
}

export interface Snapshot {
  catalog: DeviceCatalog;
  status: EngineStatus;
  config: MirrorConfig;
  preferences: Preferences;
  platform: Platform;
}
