# Hardwave LoudLab — Changelog

## v0.6.7 — HTTP /ipc fallback for webview → host param edits (2026-05-21)

Even after v0.6.6 fixed the normalization, knobs and sliders still didn't
respond to user input. Root cause: the wry IPC bridge
(`window.ipc.postMessage`) is unreliable in the WebView2 STA context FL
Studio hosts us in — the same class of bug the analyser hit and worked
around in vst-webviews commit d61ff33 by switching to HTTP POST.

This release applies the same fix in LoudLab. The packet server (which was
already running on a loopback port for outbound `MasterPacket` streaming)
now also accepts `POST /ipc` with a JSON body, routes through `handle_ipc`
with its proper `preview_normalized` conversion. The init script exposes
the port as `window.__loudlab_packet_port` so the webview's `sendParam`
helper can POST to it. The legacy IPC path is kept as a belt-and-braces
fallback for old webviews / browser context.

### Changed

- `src/editor.rs` server thread: distinguishes GET (return latest packet),
  POST /ipc (apply param/genre/auto message), and OPTIONS (CORS preflight).
  Buffer size grows from 1 KB → 4 KB so the body of a `set_param` POST
  isn't truncated.
- Poll script injects `window.__loudlab_packet_port = <port>` so the
  frontend can find the loopback port.

The webview side (`apps/loudlab/src/hooks/useHwPacket.ts`) ships in the
matching `vst-webviews` deploy.

## v0.6.6 — Fix knob/slider interactions clamping to extremes (2026-05-21)

Every knob and slider in the LoudLab webview behaved as if it only had two
positions — minimum and maximum — because the IPC handler in `editor.rs`
was passing the webview's plain-unit value (dB, Hz, percent) directly into
`raw_set_parameter_normalized`, which expects a [0.0, 1.0] normalized
value. Any plain value ≥ 1 was silently clamped to 1.0; anything ≤ 0 was
clamped to 0. Result: dragging an EQ gain knob to +1.4 dB sent `1.4` →
clamped to 1.0 → param jumped to its max. The webview showed the snap-back
on the next packet, so users perceived "knobs that don't work."

`handle_ipc` now routes plain values through `ParamPtr::preview_normalized`
before calling the GUI context's set-normalized, which is the nih-plug
contract. Knobs, sliders, and any future webview-driven param edits will
land at the correct position the first time.

## v0.6.5 — Webview-visible metering (2026-05-21)

The right-rail meters in the LoudLab webview (LUFS-S, LUFS-I, DR,
Correlation, M/S Ratio) and the header sample-rate stat were rendering as
`—` because the engine never emitted those fields. This release wires them
all through the `MasterPacket` so the UI is live end-to-end.

### Added — DSP

- **Short-term LUFS** (3 s K-weighted window) on top of the existing 400 ms
  momentary measurement, sharing the same biquad K-weighting cascade.
- **Integrated LUFS** following ITU-R BS.1770-4 — 100 ms block emission,
  absolute gate at −70 LUFS, relative gate at −10 LU below the ungated
  mean. Block-energy log is bounded at 18 000 entries (~30 min) so the
  audio-thread allocation stays predictable.
- **`StereoMeter`** in `dsp::metering` — rolling Pearson correlation on
  L/R and a mid/side energy fraction, both over a 3 s sliding window with
  O(1) per-sample update cost (running sums of L, R, L², R², L·R, M², S²).
- **Dynamic range** as Peak-to-Loudness Ratio (`true_peak_db − LUFS-S`,
  clamped to ≥ 0).

### Added — Protocol

`MasterPacket` gains six new fields, all populated from the existing
`output_meter` and the new `output_stereo_meter`:

- `lufs_short_term: f32` — 3 s K-weighted output loudness.
- `lufs_integrated: f32` — gated integrated output loudness.
- `dynamic_range: f32` — PLR in dB.
- `correlation: f32` — L/R Pearson in [−1, +1].
- `ms_ratio: f32` — mid-energy fraction in [0, 1].
- `sample_rate: f32` — engine sample rate in Hz.

The defaults in `editor::snapshot_params` are set so the webview sees sane
values (correlation 1.0, ms_ratio 0.5, sample_rate 44 100) before the
first audio block, instead of NaN-like nulls.

### Not yet wired

- `track_duration` is still emitted as null in the webview (it requires
  reading the host transport's loop range; deferred so this release can
  land the metering values without a Transport-API dependency).

## v0.6.4 — Crash-reporter telemetry

Earlier history available via `git log`.
