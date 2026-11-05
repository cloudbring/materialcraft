//! Golden-image checks for the snapshot tests (filmcraft's `testkit::golden`, same shape).
//!
//! `assert_golden` compares an RGBA8 render with a committed PNG in `tests/goldens/`. Metrics come
//! from `craft-compare` (PSNR, max and p99 absolute error). With `MC_BLESS=1` it writes the golden
//! instead (and its `.attribution` sidecar). On a mismatch it writes `<name>.actual.png` and
//! `<name>.diff.png` (error x8) to `target/golden-failures/` and panics.
//!
//! Size policy: a golden is at most [`MAX_GOLDEN_BYTES`]. If the full-size PNG is larger, blessing
//! box-downscales it by the smallest integer factor that fits; checks apply the same factor
//! (found from the golden's size) to the render before comparing. See `tests/goldens/README.md`.
//!
//! Included by `mod common;` from `crates/ui-egui/tests/snapshot.rs`. (Same helper as
//! TextureCraft's `crates/render/tests/common/golden.rs`, with `MC_BLESS`.)
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use craft_compare::image::{Image, max_abs_error, p99_abs_error, psnr, write_diff_png};

pub const MAX_GOLDEN_BYTES: usize = 50 * 1024;

/// Limits on a render's difference from its golden, in 8-bit levels (PSNR over RGBA, peak 255).
#[derive(Clone, Copy, Debug)]
pub struct Tolerance {
    pub min_psnr_db: f64,
    pub max_abs: f64,
    pub p99: f64,
}

impl Tolerance {
    /// Deterministic CPU output: bit-exact.
    pub const EXACT: Tolerance = Tolerance { min_psnr_db: f64::INFINITY, max_abs: 0.0, p99: 0.0 };
    /// GPU output across adapters (hardware vs software rounding of shader paths): filmcraft's
    /// `Tolerance::RENDER`, PSNR >= 45 dB, max <= 12 levels, p99 <= 2 levels.
    pub const RENDER: Tolerance = Tolerance { min_psnr_db: 45.0, max_abs: 12.0, p99: 2.0 };
}

fn blessing() -> bool {
    std::env::var("MC_BLESS").is_ok_and(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
}

fn failures_dir() -> PathBuf {
    let target = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"));
    target.join("golden-failures")
}

fn goldens_dir(manifest_dir: &str) -> PathBuf {
    Path::new(manifest_dir).join("tests").join("goldens")
}

fn encode(w: u32, h: u32, px: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let enc = image::codecs::png::PngEncoder::new_with_quality(&mut out, image::codecs::png::CompressionType::Best, image::codecs::png::FilterType::Adaptive);
    image::ImageEncoder::write_image(enc, px, w, h, image::ExtendedColorType::Rgba8).expect("encode png");
    out
}

fn decode(path: &Path) -> Result<(u32, u32, Vec<u8>), String> {
    let img = image::open(path).map_err(|e| format!("{}: {e}", path.display()))?.to_rgba8();
    Ok((img.width(), img.height(), img.into_raw()))
}

/// Box-average `px` by an integer factor (edge boxes use the pixels they have), round to nearest.
fn downscale(w: u32, h: u32, px: &[u8], f: u32) -> (u32, u32, Vec<u8>) {
    if f == 1 {
        return (w, h, px.to_vec());
    }
    let (ow, oh) = (w.div_ceil(f), h.div_ceil(f));
    let mut out = Vec::with_capacity((ow * oh * 4) as usize);
    for oy in 0..oh {
        for ox in 0..ow {
            let mut sum = [0u32; 4];
            let mut n = 0u32;
            for y in (oy * f)..((oy + 1) * f).min(h) {
                for x in (ox * f)..((ox + 1) * f).min(w) {
                    let i = ((y * w + x) * 4) as usize;
                    for c in 0..4 {
                        sum[c] += u32::from(px[i + c]);
                    }
                    n += 1;
                }
            }
            out.extend(sum.iter().map(|s| ((s + n / 2) / n) as u8));
        }
    }
    (ow, oh, out)
}

fn to_image(w: u32, h: u32, px: &[u8]) -> Image {
    Image::from_rgba8(w as usize, h as usize, px).expect("rgba8 image")
}

fn write_failure(name: &str, suffix: &str, w: u32, h: u32, px: &[u8]) {
    let dir = failures_dir();
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join(format!("{name}.{suffix}.png")), encode(w, h, px));
}

/// Check `rgba` (w x h) against `<manifest_dir>/tests/goldens/<name>.png`, or bless it.
#[track_caller]
pub fn assert_golden(manifest_dir: &str, name: &str, w: u32, h: u32, rgba: &[u8], tol: Tolerance, title: &str) {
    assert_eq!(rgba.len(), (w * h * 4) as usize, "{name}: pixel buffer size");
    let path = goldens_dir(manifest_dir).join(format!("{name}.png"));
    if blessing() {
        let (mut f, mut bytes) = (1, encode(w, h, rgba));
        let mut dims = (w, h);
        while bytes.len() > MAX_GOLDEN_BYTES && f < 32 {
            f += 1;
            let (dw, dh, dp) = downscale(w, h, rgba, f);
            bytes = encode(dw, dh, &dp);
            dims = (dw, dh);
        }
        assert!(bytes.len() <= MAX_GOLDEN_BYTES, "{name}: {} bytes at 1/{f} still over the {MAX_GOLDEN_BYTES}-byte limit", bytes.len());
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir goldens");
        std::fs::write(&path, &bytes).expect("write golden");
        let side = PathBuf::from(format!("{}.attribution", path.display()));
        if !side.exists() {
            let text = format!(
                "asset:        {name}.png\ntitle:        {title}\nauthor:       MaterialCraft contributors\nsource:       original work (rendered by MaterialCraft's own renderer from procedural content; no external media)\nlicense:      MIT OR Apache-2.0\nnotes:        Golden reference image (1/{f} scale, {}x{}). Regenerate with MC_BLESS=1.\n",
                dims.0, dims.1
            );
            std::fs::write(&side, text).expect("write sidecar");
        }
        eprintln!("blessed {} ({} bytes, 1/{f}, {}x{})", path.display(), bytes.len(), dims.0, dims.1);
        return;
    }
    let (gw, gh, gpx) = decode(&path).unwrap_or_else(|e| panic!("{e}\n(missing or unreadable golden; generate it with MC_BLESS=1)"));
    let f = (1..=32).find(|f| w.div_ceil(*f) == gw && h.div_ceil(*f) == gh).unwrap_or_else(|| panic!("{name}: render is {w}x{h} but the golden is {gw}x{gh}"));
    let (_, _, apx) = downscale(w, h, rgba, f);
    let (a, b) = (to_image(gw, gh, &gpx), to_image(gw, gh, &apx));
    let (max, p99, db) = (max_abs_error(&a, &b).expect("max") * 255.0, p99_abs_error(&a, &b).expect("p99") * 255.0, psnr(&a, &b).expect("psnr"));
    if db >= tol.min_psnr_db && max <= tol.max_abs + 1e-6 && p99 <= tol.p99 + 1e-6 {
        return;
    }
    write_failure(name, "actual", gw, gh, &apx);
    let mut diff = Vec::new();
    write_diff_png(&a, &b, 8.0, &mut diff).expect("diff png");
    let _ = std::fs::create_dir_all(failures_dir());
    let _ = std::fs::write(failures_dir().join(format!("{name}.diff.png")), diff);
    panic!(
        "golden mismatch {}: PSNR {db:.2} dB, max {max:.0}, p99 {p99:.0} (tolerance: PSNR >= {} dB, max <= {}, p99 <= {}); actual and x8 diff in {}",
        path.display(),
        tol.min_psnr_db,
        tol.max_abs,
        tol.p99,
        failures_dir().display()
    );
}
