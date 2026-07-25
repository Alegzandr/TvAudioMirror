// Panel rendering.
//
// Two rates coexist here. Structure (which devices exist, which state they are
// in) is rebuilt only when it actually changes, compared through a signature.
// Levels move at frame rate on their own loop, reading the last value the
// engine reported. Nothing rebuilds the list twenty times a second.

import { api, events } from "./api";
import type {
  DeviceCatalog,
  EngineStatus,
  LatencyProfile,
  MirrorConfig,
  Platform,
  Preferences,
  TargetStatus,
} from "./types";

const LATENCY_LABELS: Record<LatencyProfile, string> = {
  safe: "Relaxed",
  balanced: "Balanced",
  tight: "Tight",
  custom: "Custom",
};

const INTERFACE_LABELS: Record<string, string> = {
  usb: "USB",
  bluetooth: "Bluetooth",
  hdmi: "HDMI",
  displayport: "DisplayPort",
  spdif: "S/PDIF",
  builtin: "Built-in",
  pci: "Internal",
  firewire: "FireWire",
  thunderbolt: "Thunderbolt",
  line: "Line",
  network: "Network",
  virtual: "Virtual",
  aggregate: "Aggregate",
  unknown: "",
};

/** Floor of the meter scale. Below this, silence and near-silence look alike. */
const METER_FLOOR_DB = 60;

/** Bounds of the custom buffer, mirroring what the engine clamps to. */
const MIN_CUSTOM_MS = 3;
const MAX_CUSTOM_MS = 250;
const DEFAULT_CUSTOM_MS = 18;

let catalog: DeviceCatalog = {
  sources: [],
  targets: [],
  loopbackAvailable: false,
};
let config: MirrorConfig = {
  enabled: false,
  source: null,
  targets: [],
  latency: "balanced",
  latencyMs: 18,
};
let status: EngineStatus = {
  enabled: false,
  source: null,
  targets: [],
  mirroring: false,
  requestedBufferMs: 18,
};
let preferences: Preferences = {
  startMinimized: false,
  closeToTray: true,
  autostart: false,
};
let platform: Platform = {
  os: "",
  loopbackAvailable: false,
  portable: false,
  settingsPath: "",
  maxTargets: 64,
};

let listSignature = "";
let pickerOpen = false;
let settingsOpen = false;
/** Set while a slider is being dragged, so echoes from the engine do not fight it. */
let draggingGain: string | null = null;

const app = document.querySelector<HTMLDivElement>("#app")!;

app.innerHTML = `
  <header class="titlebar">
    <div class="identity">
      <p class="state-line" id="headline" role="status"></p>
      <p class="headline" id="subline"></p>
    </div>
    <button class="switch" id="power" role="switch" aria-checked="false" aria-label="Mirror"></button>
  </header>

  <main>
    <section class="section">
      <div class="section-head">
        <h2>Source</h2>
        <button class="button quiet" id="rescan">Rescan</button>
      </div>
      <select id="source" aria-label="Output to duplicate"></select>
      <div class="readout">
        <span class="facts tabular" id="source-facts"></span>
        <span class="meter" id="source-meter"><i></i><b></b></span>
      </div>
      <p class="empty" id="source-hint" hidden></p>
    </section>

    <section class="section">
      <div class="section-head">
        <h2>Destinations</h2>
        <span class="section-note tabular" id="target-count"></span>
      </div>
      <ul class="list" id="targets"></ul>
      <p class="empty" id="targets-empty" hidden></p>
      <button class="button wide" id="add" style="margin-top:8px">Add destination</button>
      <div class="picker" id="picker" hidden></div>
    </section>
  </main>

  <footer class="statusbar">
    <label class="field">
      Latency
      <select id="latency"></select>
      <span class="section-note help tabular" id="latency-note"></span>
    </label>
    <div class="field">
      <span class="field" id="latency-custom" hidden>
        <input class="latency-ms tabular" id="latency-ms" type="number" min="${MIN_CUSTOM_MS}" max="${MAX_CUSTOM_MS}" step="1" aria-label="Buffer in milliseconds" />
        ms
      </span>
      <button class="button quiet disclosure" id="settings-toggle" aria-expanded="false" aria-controls="settings">Settings<span class="chevron" aria-hidden="true"></span></button>
    </div>
  </footer>

  <div class="settings" id="settings" hidden>
    <div class="section-head">
      <h2>Settings</h2>
      <button class="button quiet" id="settings-close">Close</button>
    </div>
    <label class="check"><input type="checkbox" id="pref-autostart" /><span>Start with the session</span></label>
    <label class="check"><input type="checkbox" id="pref-minimized" /><span>Start minimised to the tray</span></label>
    <label class="check"><input type="checkbox" id="pref-tray" /><span>Closing the window keeps it running</span></label>
    <p class="path" id="settings-path"></p>
    <button class="button" id="quit" style="margin-top:10px">Quit AudioMirror</button>
  </div>
`;

const el = {
  headline: byId<HTMLParagraphElement>("headline"),
  subline: byId<HTMLParagraphElement>("subline"),
  power: byId<HTMLButtonElement>("power"),
  rescan: byId<HTMLButtonElement>("rescan"),
  source: byId<HTMLSelectElement>("source"),
  sourceFacts: byId<HTMLSpanElement>("source-facts"),
  sourceMeter: byId<HTMLSpanElement>("source-meter"),
  sourceHint: byId<HTMLParagraphElement>("source-hint"),
  targets: byId<HTMLUListElement>("targets"),
  targetsEmpty: byId<HTMLParagraphElement>("targets-empty"),
  targetCount: byId<HTMLSpanElement>("target-count"),
  add: byId<HTMLButtonElement>("add"),
  picker: byId<HTMLDivElement>("picker"),
  latency: byId<HTMLSelectElement>("latency"),
  latencyNote: byId<HTMLSpanElement>("latency-note"),
  latencyMs: byId<HTMLInputElement>("latency-ms"),
  latencyCustom: byId<HTMLSpanElement>("latency-custom"),
  settingsToggle: byId<HTMLButtonElement>("settings-toggle"),
  settingsClose: byId<HTMLButtonElement>("settings-close"),
  settings: byId<HTMLDivElement>("settings"),
  settingsPath: byId<HTMLParagraphElement>("settings-path"),
  autostart: byId<HTMLInputElement>("pref-autostart"),
  minimized: byId<HTMLInputElement>("pref-minimized"),
  tray: byId<HTMLInputElement>("pref-tray"),
  quit: byId<HTMLButtonElement>("quit"),
};

function byId<T extends HTMLElement>(id: string): T {
  return document.getElementById(id) as T;
}

/* ---- Meters ------------------------------------------------------------ */

interface Meter {
  fill: HTMLElement;
  peak: HTMLElement;
  level: number;
  hold: number;
  holdUntil: number;
  target: number;
  targetPeak: number;
}

const meters = new Map<string, Meter>();

function registerMeter(key: string, container: HTMLElement) {
  meters.set(key, {
    fill: container.querySelector("i")!,
    peak: container.querySelector("b")!,
    level: 0,
    hold: 0,
    holdUntil: 0,
    target: 0,
    targetPeak: 0,
  });
}

/** Amplitude to bar position, through decibels: a linear scale would spend
 *  most of its length on levels nobody can hear. */
function scale(amplitude: number): number {
  if (!(amplitude > 0)) return 0;
  const db = 20 * Math.log10(amplitude);
  return Math.min(1, Math.max(0, (db + METER_FLOOR_DB) / METER_FLOOR_DB));
}

function feedMeter(key: string, rms: number, peak: number) {
  const meter = meters.get(key);
  if (!meter) return;
  meter.target = scale(rms);
  meter.targetPeak = scale(peak);
}

function animate(now: number) {
  for (const meter of meters.values()) {
    // Instant attack, gentle release: the engine already reports the maximum
    // since the last read, so rising is a fact and falling is a choice.
    meter.level =
      meter.target > meter.level
        ? meter.target
        : meter.level + (meter.target - meter.level) * 0.14;

    if (meter.targetPeak >= meter.hold) {
      meter.hold = meter.targetPeak;
      meter.holdUntil = now + 900;
    } else if (now > meter.holdUntil) {
      meter.hold = Math.max(meter.level, meter.hold - 0.008);
    }

    meter.fill.style.transform = `scaleX(${meter.level.toFixed(4)})`;
    meter.peak.style.transform = `translateX(${(meter.hold * 100).toFixed(2)}cqw)`;
    meter.peak.style.opacity = meter.hold > 0.01 ? "1" : "0";
  }

  requestAnimationFrame(animate);
}

requestAnimationFrame(animate);

/* ---- Wording ----------------------------------------------------------- */

function connectionLabel(id: string): string {
  const entry =
    catalog.targets.find((device) => device.id === id) ??
    catalog.sources.find((device) => device.id === id);
  return entry ? (INTERFACE_LABELS[entry.interface] ?? "") : "";
}

function seconds(ms: number | null): string {
  if (ms === null || ms <= 0) return "";
  return `retrying in ${Math.ceil(ms / 1000)} s`;
}

function targetStatusText(target: TargetStatus): string {
  switch (target.state) {
    case "idle":
      return "Off";

    case "missing":
      return "Not connected";

    case "connecting":
      return status.source && status.source.state === "live"
        ? [target.detail, seconds(target.retryInMs)].filter(Boolean).join(", ") ||
            "Connecting"
        : "Waiting for the source";

    case "priming":
      return "Starting";

    case "live": {
      const parts = [`${target.latencyMs.toFixed(1)} ms`];
      if (target.sampleRate) {
        parts.push(`${(target.sampleRate / 1000).toFixed(target.sampleRate % 1000 ? 1 : 0)} kHz`);
      }
      if (target.underruns > 0) {
        parts.push(`${target.underruns} dropout${target.underruns === 1 ? "" : "s"}`);
      }
      return parts.join(" · ");
    }

    case "failed":
      return [target.detail ?? "Could not open", seconds(target.retryInMs)]
        .filter(Boolean)
        .join(", ");
  }
}

/** Where the measured latency actually comes from. The requested setting only
 *  governs the middle term; the two blocks belong to the audio system. */
function latencyBreakdown(target: TargetStatus): string {
  return (
    `Capture block ${target.captureMs.toFixed(1)} ms, ` +
    `buffer ${target.bufferMs.toFixed(1)} ms, ` +
    `render block ${target.renderMs.toFixed(1)} ms. ` +
    `Buffer target ${target.bufferTargetMs.toFixed(1)} ms. ` +
    `Only the buffer follows the latency setting; the blocks are set by the audio system.` +
    (target.correctionPpm !== 0
      ? ` Clock correction ${target.correctionPpm > 0 ? "+" : ""}${target.correctionPpm} ppm.`
      : "")
  );
}

function headlineText(): { text: string; tone: string } {
  if (!status.enabled) return { text: "Paused", tone: "" };
  if (!config.source) return { text: "No source selected", tone: "" };

  const source = status.source;
  if (source && source.state === "missing") {
    return { text: "Source not connected", tone: "" };
  }
  if (source && source.state === "failed") {
    return { text: source.detail ?? "Source unavailable", tone: "failed" };
  }

  const live = status.targets.filter((target) => target.state === "live").length;
  if (live > 0) {
    return {
      text: `Mirroring to ${live} device${live === 1 ? "" : "s"}`,
      tone: "",
    };
  }

  const enabled = config.targets.filter((target) => target.enabled).length;
  if (enabled === 0) return { text: "No destinations", tone: "" };
  return { text: "Connecting", tone: "" };
}

/* ---- Rendering --------------------------------------------------------- */

function renderHeader() {
  const { text, tone } = headlineText();
  el.headline.textContent = text;
  el.headline.dataset.tone = tone;

  const source = status.source;
  el.subline.textContent = source
    ? `From ${source.name}`
    : catalog.sources.length
      ? "Choose the output you want to duplicate"
      : "No devices found";

  el.power.setAttribute("aria-checked", String(status.enabled));
}

function renderSourceChoices() {
  const selected = config.source?.id ?? "";
  const options: string[] = [`<option value="">Select an output</option>`];

  const outputs = catalog.sources.filter((device) => device.loopback);
  const inputs = catalog.sources.filter((device) => !device.loopback);

  if (outputs.length) {
    options.push(`<optgroup label="Outputs">`);
    for (const device of outputs) options.push(option(device.id, device.name, device.isDefault));
    options.push(`</optgroup>`);
  }
  if (inputs.length) {
    options.push(`<optgroup label="Inputs">`);
    for (const device of inputs) options.push(option(device.id, device.name, device.isDefault));
    options.push(`</optgroup>`);
  }

  // A device that is configured but currently absent still belongs in the list,
  // otherwise selecting it back would look like the setting was lost.
  if (selected && !catalog.sources.some((device) => device.id === selected)) {
    options.push(
      `<optgroup label="Not connected">${option(selected, config.source!.name, false)}</optgroup>`,
    );
  }

  el.source.innerHTML = options.join("");
  el.source.value = selected;

  const hint = !catalog.loopbackAvailable
    ? "This system cannot capture an output directly. Pick a monitor or loopback input exposed by your sound server."
    : "";
  el.sourceHint.textContent = hint;
  el.sourceHint.hidden = hint === "";
}

function option(value: string, label: string, isDefault: boolean): string {
  const suffix = isDefault ? " (system default)" : "";
  return `<option value="${escape(value)}">${escape(label)}${suffix}</option>`;
}

function renderSourceFacts() {
  const source = status.source;
  if (!source || source.state === "idle") {
    el.sourceFacts.textContent = "";
    return;
  }

  const facts: string[] = [];
  if (source.loopback) facts.push("Loopback");
  if (source.sampleRate) facts.push(`${(source.sampleRate / 1000).toFixed(source.sampleRate % 1000 ? 1 : 0)} kHz`);
  if (source.channels) facts.push(`${source.channels} ch`);
  if (source.state === "missing") facts.push("not connected");

  el.sourceFacts.innerHTML = facts.map((fact) => `<span>${escape(fact)}</span>`).join("");
}

function renderTargets() {
  // Structure only. Connection state changes constantly and is applied in
  // place: rebuilding the list for it would pull the DOM out from under a
  // slider the user is currently dragging.
  const signature = config.targets
    .map((target) => `${target.id}:${target.enabled}:${target.name}`)
    .join("|");

  if (signature === listSignature) {
    updateTargets();
    return;
  }
  listSignature = signature;

  meters.forEach((_, key) => {
    if (key !== "source") meters.delete(key);
  });

  el.targets.innerHTML = config.targets
    .map((target) => {
      const live = status.targets.find((entry) => entry.id === target.id);
      const connection = connectionLabel(target.id);
      return `
        <li class="row" data-id="${escape(target.id)}" data-state="${live?.state ?? "idle"}">
          <div class="row-name">
            <strong>${escape(target.name)}</strong>
            ${connection ? `<span class="link">${connection}</span>` : ""}
          </div>
          <button class="remove" data-act="remove" aria-label="Remove ${escape(target.name)}">&times;</button>
          <div class="row-state">
            <span class="status tabular"></span>
            <span class="meter"><i></i><b></b></span>
          </div>
          <div class="row-controls">
            <input class="gain" type="range" min="-40" max="6" step="0.5"
                   value="${target.gainDb}" aria-label="Gain for ${escape(target.name)}" />
            <output class="gain-value tabular"></output>
            <button class="mute" data-act="mute" aria-pressed="${target.muted}">Mute</button>
          </div>
        </li>`;
    })
    .join("");

  for (const row of el.targets.querySelectorAll<HTMLLIElement>(".row")) {
    registerMeter(row.dataset.id!, row.querySelector(".meter")!);
  }

  updateTargets();
}

function updateTargets() {
  el.targetCount.textContent = config.targets.length
    ? `${config.targets.length} of ${platform.maxTargets}`
    : "";

  // Kept out of the rebuild branch on purpose: an empty list produces an empty
  // signature, which matches the initial one, so the branch never runs on a
  // first launch, which is exactly when this text is needed.
  el.targetsEmpty.hidden = config.targets.length > 0;
  el.targetsEmpty.textContent =
    "Nothing is being duplicated yet. Add an output below and it starts as soon as the source plays.";

  for (const row of el.targets.querySelectorAll<HTMLLIElement>(".row")) {
    const id = row.dataset.id!;
    const target = config.targets.find((entry) => entry.id === id);
    const live = status.targets.find((entry) => entry.id === id);
    if (!target) continue;

    row.dataset.state = live?.state ?? "idle";
    row.dataset.muted = String(target.muted);
    const statusText = row.querySelector<HTMLElement>(".status")!;
    statusText.textContent = live ? targetStatusText(live) : "Off";
    statusText.title = live && live.state === "live" ? latencyBreakdown(live) : "";

    const gain = row.querySelector<HTMLInputElement>(".gain")!;
    const value = row.querySelector<HTMLOutputElement>(".gain-value")!;

    // Never overwrite a control the user is holding, by pointer or by keyboard.
    if (draggingGain !== id && document.activeElement !== gain) {
      gain.value = String(target.gainDb);
    }
    // The slider stays usable while muted, so a level can be set before
    // unmuting. The mute button alone carries that state.
    value.textContent = `${target.gainDb > 0 ? "+" : ""}${target.gainDb.toFixed(1)} dB`;

    row.querySelector(".mute")!.setAttribute("aria-pressed", String(target.muted));

    if (live) feedMeter(id, live.rms, live.peak);
  }
}

function renderPicker() {
  el.picker.hidden = !pickerOpen;
  el.add.textContent = pickerOpen ? "Cancel" : "Add destination";
  if (!pickerOpen) return;

  const taken = new Set(config.targets.map((target) => target.id));
  const available = catalog.targets.filter((device) => !taken.has(device.id));

  el.picker.innerHTML = available.length
    ? available
        .map((device) => {
          const connection = INTERFACE_LABELS[device.interface] ?? "";
          return `<button data-add="${escape(device.id)}">${escape(device.name)}${
            connection ? `<span class="link">${connection}</span>` : ""
          }</button>`;
        })
        .join("")
    : `<p class="empty" style="padding:10px">Every available output is already a destination.</p>`;
}

function renderLatency() {
  const profiles: LatencyProfile[] = ["safe", "balanced", "tight", "custom"];
  if (el.latency.options.length === 0) {
    el.latency.innerHTML = profiles
      .map((profile) => `<option value="${profile}">${LATENCY_LABELS[profile]}</option>`)
      .join("");
  }
  el.latency.value = config.latency;

  const custom = config.latency === "custom";
  // Hiding the box while it holds the focus would strand it on an element that
  // just went away, and the blur that follows would report a value the profile
  // no longer uses.
  if (!custom && document.activeElement === el.latencyMs) el.latency.focus();
  el.latencyCustom.hidden = !custom;
  if (custom && document.activeElement !== el.latencyMs) {
    el.latencyMs.value = String(config.latencyMs || DEFAULT_CUSTOM_MS);
  }
}

/** Reads the millisecond box within the bounds the engine enforces, and writes
 *  the result back, so what is displayed and what is sent are one number.
 *  Rewriting it also clears the field's pending edit, which is what would
 *  otherwise reach the engine as a late `change` once the field is gone. */
function commitCustomMs(): number {
  const ms = Math.min(
    MAX_CUSTOM_MS,
    Math.max(MIN_CUSTOM_MS, Math.round(Number(el.latencyMs.value)) || DEFAULT_CUSTOM_MS),
  );
  el.latencyMs.value = String(ms);
  return ms;
}

/** Says so when the engine could not honour the requested buffer, rather than
 *  leaving the user to wonder why 3 ms reads as 36 ms. */
function renderLatencyNote() {
  const planned = status.targets
    .filter((target) => target.bufferTargetMs > 0)
    .map((target) => target.bufferTargetMs);

  if (planned.length === 0) {
    el.latencyNote.textContent = "";
    el.latencyNote.title = "";
    return;
  }

  const held = Math.max(...planned);
  const raised = held > status.requestedBufferMs + 1;

  el.latencyNote.textContent = raised ? `held at ${held.toFixed(0)} ms` : "";
  el.latencyNote.title = raised
    ? `The audio system hands over capture in whole blocks. A buffer shorter than one of those would run dry every cycle, whatever the drift correction did, so the engine holds it at ${held.toFixed(0)} ms.`
    : "";
}

function renderPreferences() {
  el.autostart.checked = preferences.autostart;
  el.minimized.checked = preferences.startMinimized;
  el.tray.checked = preferences.closeToTray;
  el.settingsPath.textContent = platform.portable
    ? `Portable. Settings in ${platform.settingsPath}`
    : platform.settingsPath;
}

function render() {
  renderHeader();
  renderSourceFacts();
  renderTargets();

  if (status.source) feedMeter("source", status.source.rms, status.source.peak);
  else feedMeter("source", 0, 0);
}

function escape(value: string): string {
  return value.replace(
    /[&<>"']/g,
    (character) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[
        character
      ]!,
  );
}

/* ---- Interaction ------------------------------------------------------- */

el.power.addEventListener("click", () => {
  const next = !status.enabled;
  status = { ...status, enabled: next };
  renderHeader();
  void api.setEnabled(next);
});

el.rescan.addEventListener("click", () => void api.rescan());

el.source.addEventListener("change", () => {
  void api.selectSource(el.source.value || null);
});

el.add.addEventListener("click", () => {
  pickerOpen = !pickerOpen;
  renderPicker();
});

el.picker.addEventListener("click", (event) => {
  const button = (event.target as HTMLElement).closest<HTMLButtonElement>("[data-add]");
  if (!button) return;
  pickerOpen = false;
  renderPicker();
  void api.addTarget(button.dataset.add!);
});

el.targets.addEventListener("click", (event) => {
  const button = (event.target as HTMLElement).closest<HTMLButtonElement>("[data-act]");
  const row = (event.target as HTMLElement).closest<HTMLLIElement>(".row");
  if (!button || !row) return;

  const id = row.dataset.id!;
  const target = config.targets.find((entry) => entry.id === id);
  if (!target) return;

  if (button.dataset.act === "remove") {
    void api.removeTarget(id);
  } else {
    void api.setTargetGain(id, target.gainDb, !target.muted);
  }
});

el.targets.addEventListener("pointerdown", (event) => {
  const gain = (event.target as HTMLElement).closest<HTMLInputElement>(".gain");
  const row = (event.target as HTMLElement).closest<HTMLLIElement>(".row");
  if (gain && row) draggingGain = row.dataset.id!;
});

window.addEventListener("pointerup", () => {
  draggingGain = null;
});

el.targets.addEventListener("input", (event) => {
  const gain = (event.target as HTMLInputElement).closest<HTMLInputElement>(".gain");
  const row = (event.target as HTMLElement).closest<HTMLLIElement>(".row");
  if (!gain || !row) return;

  const id = row.dataset.id!;
  const target = config.targets.find((entry) => entry.id === id);
  if (!target) return;

  const value = Number(gain.value);
  target.gainDb = value;
  row.querySelector<HTMLOutputElement>(".gain-value")!.textContent =
    `${value > 0 ? "+" : ""}${value.toFixed(1)} dB`;
  void api.setTargetGain(id, value, target.muted);
});

el.latency.addEventListener("change", () => {
  const profile = el.latency.value as LatencyProfile;
  // Leaving custom carries the box's content along, rather than leaving the box
  // to report it separately: two settings in flight at once can reach the
  // engine in either order, and the one saying "custom" winning would put the
  // profile straight back where it was. Entering custom takes the stored value,
  // which is the one the box is about to display.
  const ms = config.latency === "custom" ? commitCustomMs() : config.latencyMs || DEFAULT_CUSTOM_MS;
  config = { ...config, latency: profile, latencyMs: ms };
  renderLatency();
  void api.setLatency(profile, ms);
});

el.latencyMs.addEventListener("change", () => {
  // A blur delivers this after the fact, by which time the profile may have
  // moved on. Sending it then would drag it back to custom.
  if (config.latency !== "custom") return;
  const ms = commitCustomMs();
  config = { ...config, latencyMs: ms };
  void api.setLatency("custom", ms);
});

function setSettingsOpen(open: boolean) {
  settingsOpen = open;
  el.settings.hidden = !open;
  el.settingsToggle.setAttribute("aria-expanded", String(open));
  el.settingsToggle.classList.toggle("is-open", open);
  // Closing from inside the panel would otherwise strand the keyboard focus on
  // an element that just went away.
  if (!open && el.settings.contains(document.activeElement)) el.settingsToggle.focus();
}

el.settingsToggle.addEventListener("click", () => setSettingsOpen(!settingsOpen));

el.settingsClose.addEventListener("click", () => setSettingsOpen(false));

document.addEventListener("keydown", (event) => {
  if (event.key !== "Escape") return;
  if (settingsOpen) {
    setSettingsOpen(false);
  } else if (pickerOpen) {
    pickerOpen = false;
    renderPicker();
  }
});

for (const [input, key] of [
  [el.autostart, "autostart"],
  [el.minimized, "startMinimized"],
  [el.tray, "closeToTray"],
] as const) {
  input.addEventListener("change", async () => {
    const wanted = { ...preferences, [key]: input.checked };
    try {
      preferences = await api.setPreferences(wanted);
    } catch {
      // The system refused, most likely the autostart registration. Show what
      // is actually in place rather than what was asked for.
    }
    renderPreferences();
  });
}

el.quit.addEventListener("click", () => void api.quit());

// Metering costs nothing while nobody is looking at it.
document.addEventListener("visibilitychange", () => {
  void api.setWatching(document.visibilityState === "visible");
});

/* ---- Wiring ------------------------------------------------------------ */

registerMeter("source", el.sourceMeter);

void (async () => {
  // Listeners first. The engine publishes its device list once, on its first
  // scan, and that can happen before the web view has finished loading:
  // subscribing after the snapshot would lose it and leave the panel convinced
  // the machine has no audio devices.
  await events.status((next) => {
    status = next;
    renderHeader();
    renderSourceFacts();
    renderTargets();
    renderLatencyNote();
    if (next.source) feedMeter("source", next.source.rms, next.source.peak);
  });

  await events.config((next) => {
    config = next;
    renderSourceChoices();
    renderLatency();
    renderTargets();
    if (pickerOpen) renderPicker();
  });

  await events.catalog((next) => {
    catalog = next;
    renderSourceChoices();
    listSignature = "";
    renderTargets();
    if (pickerOpen) renderPicker();
  });

  const snapshot = await api.bootstrap();
  catalog = snapshot.catalog;
  config = snapshot.config;
  status = snapshot.status;
  preferences = snapshot.preferences;
  platform = snapshot.platform;

  renderSourceChoices();
  renderLatency();
  renderPreferences();
  renderLatencyNote();
  render();

  // The snapshot may predate an event that arrived while it was in flight.
  // Asking for a forced republish settles the two into one consistent state.
  await api.rescan();
})();
