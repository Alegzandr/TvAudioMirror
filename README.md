# AudioMirror

Duplicates one audio output to as many devices as you want. Picks formats
itself, corrects clock drift continuously, and reconnects devices on its own
when they come back on another port.

Rust and Tauri. No driver to install on Windows or macOS.

## What it does

- Captures any output (loopback) or any input, and feeds it to any number of
  destinations at once.
- Resamples per destination when sample rates differ, and keeps resampling by a
  fraction of a percent afterwards to hold each buffer on target.
- Survives unplugging: a device that disappears keeps its settings and its
  place in the list, and comes back on its own.
- Lives in the tray, can start with the session, and can start minimised.
- Runs portable: settings travel next to the executable.

## Why the clock correction matters

Two audio devices never share a clock. A few parts per million of difference,
invisible over a second, drains or floods a buffer within minutes. That is what
produces the periodic clicks you get from naive duplication, which by then can
only drop or repeat frames.

AudioMirror regulates ring occupancy instead. Occupancy integrates the rate
difference between capture and rendering, so the plant is a pure integrator and
a proportional-integral loop closed around it gives a second-order system whose
bandwidth and damping are chosen directly, not tuned blind. The correction is
capped at 0.4 %, two orders of magnitude above real-world drift and below the
threshold where a pitch change becomes noticeable.

Each destination reports the correction it is applying, in parts per million.
On matched hardware it sits near zero; on a cheap USB interface it may hold at a
few hundred, indefinitely, without a single dropout.

## Latency

The panel reports measured latency, not the setting you asked for: capture
block, plus mean buffer occupancy, plus render block.

The buffer target is a floor, not a promise. Capture delivers whole blocks at a
time, so occupancy swings by one block every cycle; a target smaller than that
block would touch zero every round and stutter constantly, whatever the drift
corrector did. The engine therefore never plans below one and a half capture
blocks.

On Windows, shared-mode WASAPI delivers roughly 10 ms blocks on both ends, so
end-to-end latency lands around 25 to 40 ms depending on the profile. Going
below that needs exclusive mode, which would lock the device away from every
other application and cannot be done for loopback at all.

## Platform notes

| | Capture an output | Capture an input |
|---|---|---|
| Windows | Native, WASAPI loopback | Yes |
| macOS 14.6+ | Native, aggregate device with a tap | Yes |
| macOS below 14.6 | Needs a virtual device (BlackHole, Loopback) | Yes |
| Linux | Through a monitor source | Yes |

On Linux, capturing an output means selecting the monitor source your sound
server exposes. Those appear as ordinary inputs, and the ALSA backend used by
default does not list them. Build with one of these to get them:

```sh
cargo build --release --features linux-pipewire
cargo build --release --features linux-pulseaudio
```

The interface says so on its own when the platform cannot capture an output
directly, rather than offering a choice that would fail.

## Building

Needs Rust 1.82 or newer and Node 20 or newer.

```sh
npm install
npm run app          # development, with hot reload on the frontend
npm run app:build    # bundled release build
```

Windows additionally needs the WebView2 runtime, which ships with Windows 11
and current Windows 10.

If the build stops on `Are you sure you have RC.EXE in your $PATH`, the Windows
SDK resource compiler is installed but not on the path. Point at it once per
shell, adjusting the SDK version to the one you have:

```powershell
$env:PATH = "C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64;$env:PATH"
```

Regenerating the icon from its source drawing:

```sh
node tools/make-icon.mjs
npx tauri icon src-tauri/icons/source.png
```

## Portable mode

Create an empty file named `AudioMirror.portable` next to the executable.
Settings then live in `AudioMirror.config.json` in the same folder, and nothing
is written anywhere else. Without it, settings go to the usual per-user
location.

## Layout

```
src/                  panel: vanilla TypeScript, no framework
src-tauri/src/
  audio/
    ring.rs           lock-free single-producer ring, in frames
    drift.rs          clock drift control loop
    channels.rs       channel count adaptation
    convert.rs        hardware sample formats to and from float
    meter.rs          lock-free level metering
    device.rs         enumeration and identity across replugs
    source.rs         capture and fan-out
    sink.rs           rendering, resampling, gain
    engine.rs         supervision and reconciliation
    model.rs          shared types
  settings.rs         persistence, portable or per-user
  ipc.rs              commands exposed to the panel
  tray.rs             tray icon and menu
docs/                 product and design context
```

The audio path allocates nothing in steady state. Every buffer is sized when a
stream opens; taps are attached and detached through bounded pre-allocated
queues, and retired ones travel back to the supervision thread to be freed
there rather than inside a callback.

## Tests

```sh
cd src-tauri && cargo test
```

The drift corrector is tested against simulated clocks at plus and minus 200
ppm over sixty seconds, and against a mis-primed buffer, checking that
occupancy converges without ever running dry.
