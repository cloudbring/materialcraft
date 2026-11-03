# Variance-calibrated Gaussian nodes (#2)

MaterialCraft uses `craft-blur::Gaussian` with `Quality::Recursive` for Gaussian filtering.
The shared crate selects a variance-calibrated sampled kernel below sigma 8 and a recursive
filter for larger sigmas. `Cargo.lock` pins `algo/integration` to
`91e0328f63f027086214472d48cfb4909d95a2a6`, which includes `Edge::Wrap` from `fc163dc`;
the dependency retains its repoint TODO for release.

## Scope and compatibility

The filtering implementations were `ops::gaussian` (sampled kernel / three running-sum boxes)
and `filters::blur_xy` (isotropic blur plus repeatedly truncated horizontal Gaussians).
Both now use the shared implementation; anisotropic blur applies its x/y sigmas independently.
All existing callers inherit the change, including blur, sharpen, unsharp mask, high-pass,
ambient occlusion, height curvature, shadows, glow, noise smoothing and weathering.
`gaussian_noise` and `gaussian_spots` are generators, not Gaussian image filters, and are
unchanged. The exact small-window median (#147) is untouched.

Existing graph parameters and pixel/preview scaling are preserved, as are independent scalar
channel filtering (including alpha), the invalid/tiny-sigma identity and the image-size sigma
cap. This intentionally changes the appearance of existing graphs when recooked. There is no
documented graph/evaluation/algorithm version or persisted cook cache to bump. Package format
version 1 describes serialization, not evaluation, and remains unchanged. No legacy-look
compatibility switch is introduced.

## Periodic edges

Tiled filtering passes `Edge::Wrap` directly to craft-blur; untiled filtering still uses
`Edge::Clamp`. MaterialCraft no longer constructs, filters or crops periodic halos, and no
longer owns a padded-row buffer. Scratch storage is reused per Rayon worker in batches of
eight lines; blocked transposes allow the same path to filter the other axis without unsafe
code. Independent channel filtering, sigma validation, the image-size sigma cap and no-op
handling are unchanged.

Regression tests cover matching first/last columns and rows within `1e-6` for input symmetric
about each wrap boundary, plus circular-shift equivariance and agreement with a 3x3 repeated
tile for arbitrary asymmetric RGBA input, including singleton axes and sigmas larger than a
tile side. Arbitrary periodic images need not have identical adjacent first/last pixels;
the symmetry fixture is what makes that seam equality a meaningful test.

## Measured variance

Measurements made before changing production code, November 3, 2026, using a centered unit
impulse in a 1025x1 gray image and full-output f64 moment accumulation. The anisotropic node
uses anisotropy 1 and intensity scaled to request the same horizontal sigma. Clamp and tiled
results agree to the precision below. Errors are relative to requested sigma squared.

| Implementation | sigma 1 old error | sigma 4 old error | sigma 16 old error | Maximum new absolute error |
| --- | ---: | ---: | ---: | ---: |
| Shared isotropic Gaussian | -0.408800% | -7.636971% | -2.084501% | 0.000002% |
| Anisotropic horizontal Gaussian | -7.568782% | -13.194417% | -15.731452% | 0.000002% |

The old f32 running sums leave small nonzero tails, so measured full-line variance differs
slightly from ideal integer box-width variance. Sharpen/unsharp and AO reuse the isotropic
implementation, rather than owning a third Gaussian kernel. Automated tests require <=0.5%
variance error on both image axes, both edge modes, and anisotropic x/y sigmas separately.

## Fixture audit

There are **no tracked node goldens or fixtures to update**. A pre-migration numerical capture
covers 972 output images: every catalog node except graph instances, default parameters,
seed 7, deterministic 32x32 gray/RGBA input, and clamp/tiled modes. Fifteen nodes change;
maximum absolute linear-f32 sample deltas across their outputs are listed here. All other
captured outputs, including median and standalone curvature, are unchanged.

| Node | Maximum sample delta |
| --- | ---: |
| ambient_occlusion | 0.073308289 |
| anisotropic_blur | 0.065900609 |
| anisotropic_noise | 0.000514925 |
| blur | 0.022368431 |
| dirt_mask | 0.021150649 |
| drop_shadow | 0.024512410 |
| glow | 0.000535488 |
| height_curvature | 0.053632736 |
| height_normal_ao | 0.000870824 |
| highpass | 0.000118852 |
| non_uniform_blur | 0.081237674 |
| rust | 0.002220988 |
| sharpen | 0.000653863 |
| slope_blur | 0.020391077 |
| unsharp_mask | 0.085562885 |

The ignored `gaussian_fixture_audit` test captures to `MC_BLUR_FIXTURES` when files do not yet
exist and compares against them on subsequent runs. These transient raw captures live in
ignored `plan/blur-fixtures`, not in the shipped repository.

## Release benchmark

2048x2048 grayscale, deterministic nonconstant input, four Rayon workers, one warm-up and
median of five timed invocations, including allocation, padding and transposes. Same machine,
same release profile and harness before/after; not a universal speed guarantee.

| Sigma | Clamp before ms | Clamp after ms | Tiled before ms | Tiled after ms |
| --- | ---: | ---: | ---: | ---: |
| 2 | 40.466 | 17.604 | 43.728 | 19.369 |
| 16 | 114.294 | 19.157 | 116.729 | 20.885 |
| 64 | 127.690 | 19.222 | 120.636 | 24.897 |

Reproduce with the rustup toolchain:

```sh
export CARGO_TARGET_DIR="$PWD/target-2" RAYON_NUM_THREADS=4
export PATH="$(dirname "$(rustup which cargo)"):$PATH"
export CARGO="$(rustup which cargo)" RUSTC="$(rustup which rustc)"
cargo test -p materialcraft-nodes --release --test gaussian gaussian_measurements -- --ignored --nocapture
MC_BLUR_FIXTURES="$PWD/plan/blur-fixtures" cargo test -p materialcraft-nodes --release --test gaussian gaussian_fixture_audit -- --ignored --nocapture
```

## Validation

- Node tests: 34 passed, zero failed, three ignored (one existing performance test and the two
  opt-in audit/benchmark tests). Both opt-in migration tests also pass when explicitly run.
- Workspace clippy passes with only the existing `clippy::chunks_exact_to_as_chunks` warnings
  in `nodes/src/filters_extra.rs` and `nodes/src/text.rs`, and deprecated `fetch_update` in
  `automation/src/security.rs` allowed at invocation; no source lint suppressions added.
- Formatting and whitespace checks pass; layering checks all 14 workspace crates.
- All ten `cargo xtask wasm` packages pass with the rustup toolchain. Explicitly set its
  executable directory first in PATH: the machine's default Homebrew compiler has no wasm
  sysroot, even when launched through `rustup run stable cargo`.
- Removed 102 legacy production lines, replaced by 79 adapter lines (net -23). No median,
  unrelated-warning, package-format or evaluation-cache changes.

## Edge::Wrap follow-up (#2)

November 3, 2026: a temporary test module retained the previous padded implementation until
its outputs were compared by `f32::to_bits()` with the Wrap adapter. All 1,056 combinations
passed: the existing deterministic 32x32 gray/RGBA node inputs, requested sigmas 1 through
128 plus 0.051, 0.1, 0.5 and 7.999, and isotropic, half-strength y, x-only and y-only passes.
The existing image-size cap still applies, so effective sigma on these fixtures is at most
32. Separately, byte-for-byte comparisons of all 972 catalog outputs (both edge modes)
found zero changed outputs. The temporary padded implementation was then deleted. A retained
regression test compares fixture outputs bit-for-bit against the shared periodic filter;
the existing symmetric-seam test is unchanged.

This is fixture-scoped identity, not a universal bitwise promise. Additional asymmetric
stress fixtures exposed one-ULP differences from the former finite halo: gray 257x129 with
sigma x/y 0/19 at sample 4201, and color 31x23 with requested sigma x/y 52/26 (effective
31/26) at sample 557, using channels `[v, 2v - 0.5, 1 - v, 0.25 + 0.75v]`, where
`v = ((17x + 31y) % 257) / 256`. Neither case changes the required node fixtures or seam test.

The initial separate before/after benchmark encountered substantial host-load variation.
For the comparison below, the temporary test timed both adapters in the same release process,
alternating execution order, with four Rayon workers, one warm-up per adapter and the median
of five invocations. Input and allocation/transpose costs match `gaussian_measurements`.

| Sigma | Padded before ms | Wrap after ms |
| --- | ---: | ---: |
| 2 | 18.330 | 18.102 |
| 16 | 20.602 | 20.705 |
| 64 | 24.794 | 23.699 |

These are whole-node 2048x2048 grayscale timings, not the shared crate's direct-filter
benchmark; no general 2.67x speedup is claimed. The production adapter removes 37 lines and
adds six (net -31), retaining no app-owned padding code.

Follow-up validation: `cargo test -p materialcraft-nodes` and the same command with
`--release` each pass 35 tests, with zero failures and three opt-in tests ignored. Workspace
clippy passes with `-D warnings -A clippy::chunks_exact_to_as_chunks -A deprecated`, allowing
only the previously known warning classes. Formatting and whitespace checks pass, layering
passes for all 14 crates, and all ten `cargo xtask wasm` packages pass (existing engine wasm
warnings remain). All Cargo commands use the rustup toolchain and the dedicated `target-2`
directory, which is deleted after validation.
