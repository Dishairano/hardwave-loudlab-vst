# Hardwave LoudLab — Changelog

## v0.7.0 — Auto loudness + your ceiling (2026-07-18)

- Auto mode now targets loudness, not just tone. Pick a preset and it masters toward the right competitive level for the genre — no more manually riding Output to get loud. Slow and predictable by design, so it won't pump your quiet sections.
- You choose your ceiling. The limiter ceiling now defaults to a codec-safe -1 dBTP and is fully yours to set — push it to 0 if you want to run right to the edge.
- A dedicated **Sub** control. A broad ~45 Hz push/cut you ride like your own hand on the low end — works in Auto mode too, on top of the genre balance, so it never fights the preset.


## v0.6.15 — Fairer free trials (2026-07-11)

- Free trials are now one per machine. Spinning up a new account no longer resets the 14-day trial. If you've bought LoudLab or you're on a Hardwave plan, nothing changes — full access continues exactly as before.


## v0.6.14 — Auto-mode bass fix & stability (2026-07-08)

- Fixed Auto mode silently mono-ing your bass: the mono-bass switch now works in Auto mode too, and defaults to off — low end passes through in stereo unless you turn it on. (This finishes the fix that started in v0.6.11.)
- The spectrum analyzer no longer goes blank while Auto mode is on.
- The plugin now reports its processing delay to the DAW, so tracks stay perfectly time-aligned (proper delay compensation).
- Crash reporting no longer stalls audio: reports upload in the background and repeated identical crashes are sent at most once a minute.
- License metadata aligned with the GPL-3.0 relicense.


## v0.6.13 — No more 8 kHz cut + cleaner saturation (2026-05-31)

- **Multiband compressor is now off by default.** With the high band's crossover at 8 kHz and a default threshold/ratio that bit into hot material, an unconfigured chain was reading as "an 8 kHz cut" on bright tracks. It's still there as an opt-in — the default chain now leaves the spectrum untouched.
- **Saturation also runs alias-free** (when you opt it in). Same 2× anti-aliasing wrapper as the limiter soft-clip got in v0.6.12, so pushing drive on hot/bright material stays clean instead of developing the gritty top end.

## v0.6.12 — Cleaner highs at loud levels (2026-05-31)

- **Anti-aliasing on the limiter's soft-clip.** Hot, bright material was developing a gritty/digital edge on the top — the soft-clip's harmonics were folding back into the audible range. The clip now runs internally at 2× the sample rate and is band-limited on the way out, so a hot screech or a slammed master stays clean instead of getting that "low-bitrate" feel.
- No new controls, no preset changes — the chain sounds the same on quiet material and noticeably cleaner the harder you push it. Adds a tiny fixed latency (a handful of samples).

## v0.6.11 — Bypass until configured + mono-bass default off (2026-05-25)

- LoudLab now starts **bypassed** on a fresh instance — the engine is a clean passthrough until you choose what you're mastering (Master / Drum / Instrument). No more processing your track with default settings before you've set it up.
- **Mono Bass is now off by default.** Summing the sub to mono by default was thinning out the low end; it's still there as an opt-in toggle, but the default chain now leaves your sub untouched.

## v0.6.10 — Saturation + per-band makeup gain (2026-05-24)

- Added a tanh saturation stage between the multiband compressor and the stereo width module. Three new params: `sat_enabled`, `sat_drive` (0–24 dB), `sat_mix` (0–1). Off by default — existing projects sound identical. DC-blocked, drive-normalised so unity drive = unity gain.
- Per-band makeup gain (`comp_sub_makeup`, `comp_lm_makeup`, `comp_hm_makeup`, `comp_hi_makeup`) is now exposed as a Rust param and surfaces in the packet. The DSP-side `makeup_db` field has been there since v0.6.0; it just wasn't reachable.
- Unlocks the webview's Advanced mode: every control there now maps to a real Rust param. Saturation, per-band makeup, plus the existing thresh/ratio/atk/rel/freq/Q stack. No more decorative knobs.
- No breaking changes — every new param defaults to "off" or "0 dB" so v0.6.9 saved presets load identically.

## v0.6.9 — Step 1 Diagnose wired end-to-end (2026-05-21)

The Learn-mode "Diagnose" step previously rendered against fake numbers —
hardcoded capture progress, hardcoded LUFS-M peak, no real "is playing"
indicator, and a dead "Reset capture" button. This release exposes the
engine signals the step needs.

### Added — MasterPacket fields

- `is_playing: bool` — last reported transport.playing state. Drives the
  Step 1 "Listening / Paused" pill.
- `lufs_max_momentary: f32` — running peak-hold of the output bus's
  momentary LUFS since the last `reset_capture`. Drives the "LUFS-M
  (drop peak)" stat card.

### Added — IPC

- `reset_capture` message. The editor's IPC handler flips a shared
  `AtomicBool`; `process()` drains it at the top of the next block and
  resets the input/output LUFS meters, the stereo meter, the
  `max_pos_samples` track-duration estimate, and the peak-momentary hold.
  Used by the Step 1 "Reset capture" button.

### Threading

- `MasterEditor` now owns an `Arc<AtomicBool>` shared with the audio
  thread. Both the wry IPC handler and the HTTP /ipc POST handler use it.
  Release/Acquire ordering pairs the editor write with the audio-thread
  read so the reset is visible across cores.

## v0.6.8 — master bypass + track duration (2026-05-21)

The header On/Off toggle in the webview previously sent `master_enabled`,
but that param did not exist on the Rust side — so the toggle was a no-op.
And the "Track loaded · MM:SS" readout always showed `00:00` because the
plugin never read the host transport.

### Added

- `master_enabled: BoolParam` (default true). When false, the per-sample
  loop in `process()` does nothing but feed the input/output/stereo meters
  with the dry signal and `continue` — the buffer is iterated in place, so
  leaving the samples unwritten passes them through unchanged. No gain, no
  EQ, no comp, no stereo, no limiter, no mix. Now the header On/Off
  actually bypasses the plugin.
- `track_duration: f32` field on `MasterPacket`. Each block reads
  `context.transport().pos_samples()` and tracks the maximum observed
  position; the packet emits `max_pos_samples / sample_rate`. Pragmatic
  stand-in for project length, since most DAWs don't expose total length
  to a plugin.

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
clamped to 0.

### Fixed

- Knob and slider drags now land at the correct value the first time —
  dragging EQ low gain to +1.4 dB used to snap to the +24 dB ceiling.
- `handle_ipc` routes plain values through `ParamPtr::preview_normalized`
  before calling `raw_set_parameter_normalized`, the nih-plug contract
  for plain → normalized conversion using each param's declared range.
- Applies to every webview-driven param: EQ gains, compressor thresholds,
  filter frequencies, limiter ceiling, mix, and any future additions.

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
