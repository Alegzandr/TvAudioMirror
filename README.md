# TV Audio Mirror

Small Windows utility that mirrors the audio from the **current default playback device** to the **first output device whose name contains `TV`**.

Typical use case:

-   you work on a PC with speakers/headset ✅
-   you have a TV connected over HDMI ✅
-   you **don't** want Windows to extend the desktop / move the mouse there ✅
-   but you **sometimes** want the TV to receive the same audio as the PC ✅

This tool does exactly that, automatically, and lives in the tray.

---

## Features

-   ✅ Mirrors audio from current _default_ render device to the first device whose name contains **`TV`** (case-insensitive)
-   ✅ Does **nothing** if the default device is already a TV
-   ✅ Auto-refresh when the default device changes (plug/unplug USB headset, BT, etc.)
-   ✅ Mute / volume slider for the TV device
-   ✅ “Open sound settings” button to quickly rename devices
-   ✅ Minimizes to the tray
-   ✅ Can start **directly in the tray** with `--tray`
-   ✅ Lightweight, no virtual drivers
-   ✅ Localized (en, fr, es, de, it, pt-BR, zh-Hans, ru, ja, ko) via `.resx`
-   ✅ Tray icon menu offers quick Open / Reload / Exit actions
-   ✅ Open-source friendly: no designer hell, very few comments, clean code

---

## How it works

-   Uses **NAudio** to:
    -   capture loopback audio from the default render device (`WasapiLoopbackCapture`)
    -   push audio to the TV device (`WasapiOut`, shared mode)
-   A small timer (1.5s) polls the default device. When it changes, the pipeline is rebuilt.
-   A device is considered a “TV” if its **friendly name** contains `"tv"` (case-insensitive).
    -   Example: rename your HDMI device to `Samsung TV (HDMI)` in Windows sound settings → it will be picked.

---

## Requirements

-   Windows 10/11
-   .NET 6/7/8 Desktop Runtime (if you publish _framework-dependent_)
-   An audio output device whose name contains **TV**
-   Visual Studio 2022+ (for building)

---

## Build

### Visual Studio

1. Open the solution
2. Build → Publish → Folder
3. Target: `win-x64`
4. Publish
5. Your `TvAudioMirror.exe` will be in the publish folder

### CLI

```bash
dotnet restore
```

```bash
dotnet publish -c Release -r win-x64 ^
  -p:PublishSingleFile=true ^
  -p:IncludeNativeLibrariesForSelfExtract=true ^
  -p:EnableCompressionInSingleFile=true ^
  --self-contained true ^
  -o .\publish\win-x64
```


