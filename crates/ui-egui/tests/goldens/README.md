# Golden images: whole-window snapshots

Reference PNGs for `crates/ui-egui/tests/snapshot.rs` (15 scenes). Each test compares its scenes
with `<name>.png` here and fails on a difference.

- **Bless**: `MC_BLESS=1 cargo test -p materialcraft-ui-egui --test snapshot`, then review the PNGs
  in git. Each has an `.attribution` sidecar.
- **Failure**: PSNR / max / p99 in the panic message; `<name>.actual.png` and `<name>.diff.png`
  (error x8) in `target/golden-failures/`.
- **Metrics**: `craft-compare` (`max_abs_error`, `p99_abs_error`, `psnr`) over RGBA8, via
  `tests/common/golden.rs` (the same helper as TextureCraft's, with `MC_BLESS`).
- **Tolerance**: `Tolerance::RENDER` (filmcraft's): PSNR >= 45 dB, max <= 12 levels, p99 <= 2
  levels. The UI is GPU-composited (the 3D view itself is a CPU render shown as a texture); four
  runs on one machine were identical, the tolerance is for other adapters.
- **Settling**: each scene steps until the picture is unchanged for three frames (cooks, previews
  and uploads land over several frames).
- **Wall-clock text**: the 3D view's "Render time" is zeroed before each frame, and the bottom 30
  points (status bar: UI time, compute time) are blanked in the golden.
  `MC_SNAPSHOT_DIR=<dir>` still writes the full pictures.
- **GPU gate**: the tests skip without a GPU adapter (CI has none).
- **Size policy**: at most 50 KB per golden. The 1600x960 windows do not fit at full size, so each is
  stored box-downscaled by the smallest integer factor that fits (1/3 to 1/5, recorded in its
  `.attribution` note); checks apply the same factor to the render. Averaging makes the check less
  sensitive to changes of a few pixels, so these goldens catch layout, colour and missing-element
  changes, not sub-pixel text changes.
