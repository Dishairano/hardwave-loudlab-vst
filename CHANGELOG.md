# Hardwave LoudLab — Changelog

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
