# AudioMirror design

## Color strategy

Restrained. Tinted neutrals plus one accent, held under 10 % of the surface.

Every neutral is tinted toward the accent hue at chroma 0.004 to 0.008, so the
greys never read as digital grey. Both themes are authored, neither derived
from the other by inversion.

**Accent: desaturated brass**, `oklch(0.52 0.11 65)` light / `oklch(0.76 0.11 72)`
dark. It appears only on the master switch when engaged, the focus ring, the
current selection, and the peak marker on a meter. It never decorates.

Amber is conventionally a warning colour, so failure is kept clearly apart:
`oklch(0.52 0.16 25)` light / `oklch(0.68 0.15 25)` dark, a distinctly redder
hue at higher chroma. Waiting and idle states carry no colour at all, only
muted text. Three registers, no overlap.

## Meters

Level is shown by occupation, not by colour. A 3 px track, filled in a strong
neutral to the smoothed root-mean-square value, with a one-pixel accent line at
the recent peak. No green-to-red ramp, no segments.

Amplitude maps through decibels, not linearly: `-60 dB` to `0 dB` across the
track. A linear scale spends most of its length on values nobody can hear.

Decay is animated in the frontend at frame rate rather than pushed from the
engine: the engine reports the maximum since the last read, the interface owns
how that falls away.

## Typography

One family, the platform's own UI stack. Fixed rem scale, ratio near 1.2:
`0.6875` / `0.75` / `0.8125` / `0.9375` / `1.25` rem. Weights 400, 500, 600.

Every measured number uses `font-variant-numeric: tabular-nums`, so a latency
readout updating twenty times a second does not jitter.

## Layout

A single column at a fixed rhythm. Sections separated by hairlines, never by
cards. Destinations are list rows with a hairline between them; nested
containers are refused outright.

Spacing scale: 2, 4, 6, 8, 12, 16, 20, 28 px. Section padding is 14 px
horizontal, varying vertically by density.

## Motion

150 ms, `cubic-bezier(0.22, 1, 0.36, 1)`, on state and feedback only. Meters
are exempt: they run on their own animation loop. Disclosure regions fade their
content in without animating height. Everything is disabled under
`prefers-reduced-motion`.

## Components

Native `select`, `input[type=range]` and `input[type=checkbox]` are styled, not
replaced. Every interactive element carries default, hover, focus-visible,
active and disabled states.
