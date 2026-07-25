// The only channel to the engine. Every capability the panel has is a command
// listed here, which keeps the frontend's reach auditable in one file.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import type {
  DeviceCatalog,
  EngineStatus,
  LatencyProfile,
  MirrorConfig,
  Preferences,
  Snapshot,
} from "./types";

export const api = {
  bootstrap: () => invoke<Snapshot>("bootstrap"),

  setEnabled: (enabled: boolean) => invoke<void>("set_enabled", { enabled }),

  selectSource: (id: string | null) => invoke<void>("select_source", { id }),

  addTarget: (id: string) => invoke<void>("add_target", { id }),

  removeTarget: (id: string) => invoke<void>("remove_target", { id }),

  setTargetEnabled: (id: string, enabled: boolean) =>
    invoke<void>("set_target_enabled", { id, enabled }),

  setTargetGain: (id: string, gainDb: number, muted: boolean) =>
    invoke<void>("set_target_gain", { id, gainDb, muted }),

  setLatency: (profile: LatencyProfile, customMs: number) =>
    invoke<void>("set_latency", { profile, customMs }),

  rescan: () => invoke<void>("rescan"),

  setWatching: (watching: boolean) => invoke<void>("set_watching", { watching }),

  setPreferences: (preferences: Preferences) =>
    invoke<Preferences>("set_preferences", { preferences }),

  quit: () => invoke<void>("quit"),
};

export const events = {
  status: (handler: (status: EngineStatus) => void) =>
    listen<EngineStatus>("state", (event) => handler(event.payload)),

  config: (handler: (config: MirrorConfig) => void) =>
    listen<MirrorConfig>("config", (event) => handler(event.payload)),

  catalog: (handler: (catalog: DeviceCatalog) => void) =>
    listen<DeviceCatalog>("catalog", (event) => handler(event.payload)),
};
