//! Image operations shared by many nodes: Gaussian blur, Euclidean distance transform, slopes,
//! color space helpers and blend modes.

use std::sync::Arc;

use craft_blur::{Cancel, Edge, Gaussian, Quality, Scratch, Separable, Shape};
use materialcraft_image::{Image, Kind};
use rayon::prelude::*;

/// Variance-calibrated Gaussian blur with standard deviation `sigma` (pixels).
pub fn gaussian(img: &Image, sigma: f32, tile: bool) -> Image {
    gaussian_xy(img, sigma, sigma, tile)
}

/// Independent Gaussian standard deviations along x/y, with periodic extension when tiled.
pub fn gaussian_xy(img: &Image, sigma_x: f32, sigma_y: f32, tile: bool) -> Image {
    let limit = img.w.max(img.h) as f32;
    let sigma_x = if sigma_x.is_finite() && sigma_x > 0.05 { sigma_x.min(limit) } else { 0.0 };
    let sigma_y = if sigma_y.is_finite() && sigma_y > 0.05 { sigma_y.min(limit) } else { 0.0 };
    if sigma_x == 0.0 && sigma_y == 0.0 {
        return img.clone();
    }
    let (width, height, channels) = (img.w as usize, img.h as usize, img.channels());
    let Ok(shape) = Shape::new(width, height, channels) else { return img.clone() };
    if width == 0 || height == 0 || img.data.len() != shape.samples() {
        return img.clone();
    }
    let mut data = img.data.as_ref().clone();
    if gaussian_rows(&mut data, width, channels, sigma_x, tile).is_err() {
        return img.clone();
    }
    if sigma_y != 0.0 && height > 1 {
        let mut columns = transpose(&data, width, height, channels);
        if gaussian_rows(&mut columns, height, channels, sigma_y, tile).is_err() {
            return img.clone();
        }
        data = transpose(&columns, height, width, channels);
    }
    Image { w: img.w, h: img.h, kind: img.kind, data: Arc::new(data) }
}

fn gaussian_rows(pixels: &mut [f32], width: usize, channels: usize, sigma: f32, tile: bool) -> Result<(), craft_blur::Error> {
    if sigma == 0.0 || width == 1 {
        return Ok(());
    }
    let gaussian = Gaussian::new(sigma, Quality::Recursive)?;
    let padding = if tile {
        match gaussian.separable() {
            Separable::Kernel(kernel) => kernel.len() / 2,
            _ => (24.0 * sigma).ceil() as usize,
        }
    } else {
        0
    };
    let padded_width = width + padding * 2;
    let row_samples = width * channels;
    pixels.par_chunks_mut(row_samples * craft_blur::LINE_BATCH).try_for_each_init(
        || (Scratch::default(), Vec::<f32>::new()),
        |(scratch, padded), rows| {
            let height = rows.len() / row_samples;
            if padding == 0 {
                return gaussian.filter_rows(rows, Shape::new(width, height, channels)?, Edge::Clamp, scratch, Cancel::Never);
            }
            let shape = Shape::new(padded_width, height, channels)?;
            padded.clear();
            padded.try_reserve(shape.samples()).map_err(|_| craft_blur::Error::Allocation)?;
            for row in rows.chunks_exact(row_samples) {
                let mut remaining = padded_width * channels;
                let mut source = (width - padding % width) % width * channels;
                while remaining != 0 {
                    let count = remaining.min(row_samples - source);
                    padded.extend_from_slice(&row[source..source + count]);
                    remaining -= count;
                    source = 0;
                }
            }
            gaussian.filter_rows(padded, shape, Edge::Clamp, scratch, Cancel::Never)?;
            for (output, input) in rows.chunks_exact_mut(row_samples).zip(padded.chunks_exact(padded_width * channels)) {
                output.copy_from_slice(&input[padding * channels..padding * channels + row_samples]);
            }
            Ok(())
        },
    )
}

fn transpose(pixels: &[f32], width: usize, height: usize, channels: usize) -> Vec<f32> {
    let mut output = vec![0.0; pixels.len()];
    output.par_chunks_mut(32 * height * channels).enumerate().for_each(|(block, columns)| {
        let column_count = columns.len() / (height * channels);
        for row in 0..height {
            for column in 0..column_count {
                let source = (row * width + block * 32 + column) * channels;
                let target = (column * height + row) * channels;
                columns[target..target + channels].copy_from_slice(&pixels[source..source + channels]);
            }
        }
    });
    output
}

/// Squared Euclidean distance (pixels) from every pixel to the nearest pixel where `inside` is
/// true (Felzenszwalb–Huttenlocher, exact, linear time). With `tile`, distances wrap.
pub fn distance_sq(w: u32, h: u32, inside: impl Fn(usize) -> bool + Sync, tile: bool) -> Vec<f32> {
    let (w, h) = (w as usize, h as usize);
    const INF: f32 = 1e20;
    // Tiling: run on a 3x tiled line so distances cross the edges.
    let rep = if tile { 3 } else { 1 };
    let edt_1d = |f: &[f32]| -> Vec<f32> {
        let n = f.len();
        let f: Vec<f64> = f.iter().map(|v| *v as f64).collect();
        let mut d = vec![0.0f32; n];
        let mut v = vec![0usize; n];
        let mut z = vec![0.0f64; n + 1];
        let mut k = 0usize;
        z[0] = -(INF as f64);
        z[1] = INF as f64;
        let cross = |q: usize, p: usize| ((f[q] + (q * q) as f64) - (f[p] + (p * p) as f64)) / (2.0 * q as f64 - 2.0 * p as f64);
        for q in 1..n {
            let mut s = cross(q, v[k]);
            while k > 0 && s <= z[k] {
                k -= 1;
                s = cross(q, v[k]);
            }
            k += 1;
            v[k] = q;
            z[k] = s;
            z[k + 1] = INF as f64;
        }
        k = 0;
        for (q, dq) in d.iter_mut().enumerate() {
            while k + 1 < n && z[k + 1] < q as f64 {
                k += 1;
            }
            let p = v[k];
            *dq = ((q as f64 - p as f64).powi(2) + f[p]) as f32;
        }
        d
    };
    // Columns.
    let cols: Vec<Vec<f32>> = (0..w)
        .into_par_iter()
        .map(|x| {
            let f: Vec<f32> = (0..h * rep).map(|y| if inside((y % h) * w + x) { 0.0 } else { INF }).collect();
            let d = edt_1d(&f);
            let off = if tile { h } else { 0 };
            d[off..off + h].to_vec()
        })
        .collect();
    // Rows.
    let rows: Vec<Vec<f32>> = (0..h)
        .into_par_iter()
        .map(|y| {
            let f: Vec<f32> = (0..w * rep).map(|x| cols[x % w][y]).collect();
            let d = edt_1d(&f);
            let off = if tile { w } else { 0 };
            d[off..off + w].to_vec()
        })
        .collect();
    rows.concat()
}

/// Slope (dx, dy) of a gray image by central differences, in value units per pixel.
#[inline]
pub fn slope(img: &Image, x: i64, y: i64, tile: bool) -> (f32, f32) {
    let dx = (img.get_gray(x + 1, y, tile) - img.get_gray(x - 1, y, tile)) * 0.5;
    let dy = (img.get_gray(x, y + 1, tile) - img.get_gray(x, y - 1, tile)) * 0.5;
    (dx, dy)
}

pub fn rgb_to_hsl(c: [f32; 4]) -> [f32; 3] {
    let (r, g, b) = (c[0], c[1], c[2]);
    let mx = r.max(g).max(b);
    let mn = r.min(g).min(b);
    let l = (mx + mn) * 0.5;
    if (mx - mn).abs() < 1e-7 {
        return [0.0, 0.0, l];
    }
    let d = mx - mn;
    let s = if l > 0.5 { d / (2.0 - mx - mn) } else { d / (mx + mn) };
    let h = if mx == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if mx == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    [h / 6.0, s, l]
}

pub fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [f32; 3] {
    if s <= 0.0 {
        return [l, l, l];
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let f = |mut t: f32| {
        t = t.rem_euclid(1.0);
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    [f(h + 1.0 / 3.0), f(h), f(h - 1.0 / 3.0)]
}

pub const BLEND_MODES: &[&str] = &[
    "Copy",
    "Add (Linear dodge)",
    "Subtract",
    "Multiply",
    "Add Sub",
    "Max (Lighten)",
    "Min (Darken)",
    "Switch",
    "Divide",
    "Overlay",
    "Screen",
    "Soft light",
    "Difference",
    "Color dodge",
    "Color burn",
    "Hard light",
];

/// Blend one channel: `b` background, `f` foreground.
#[inline]
pub fn blend_channel(mode: i32, b: f32, f: f32) -> f32 {
    match mode {
        1 => b + f,
        2 => b - f,
        3 => b * f,
        4 => b + 2.0 * f - 1.0,
        5 => b.max(f),
        6 => b.min(f),
        8 => {
            if f.abs() < 1e-6 {
                if b > 0.0 { 1.0 } else { 0.0 }
            } else {
                b / f
            }
        }
        9 => {
            if b < 0.5 {
                2.0 * b * f
            } else {
                1.0 - 2.0 * (1.0 - b) * (1.0 - f)
            }
        }
        10 => 1.0 - (1.0 - b) * (1.0 - f),
        11 => {
            // W3C soft light.
            if f <= 0.5 {
                b - (1.0 - 2.0 * f) * b * (1.0 - b)
            } else {
                let d = if b <= 0.25 { ((16.0 * b - 12.0) * b + 4.0) * b } else { b.max(0.0).sqrt() };
                b + (2.0 * f - 1.0) * (d - b)
            }
        }
        12 => (b - f).abs(),
        13 => {
            if f >= 1.0 {
                1.0
            } else {
                (b / (1.0 - f)).min(1.0)
            }
        }
        14 => {
            if f <= 0.0 {
                0.0
            } else {
                1.0 - ((1.0 - b) / f).min(1.0)
            }
        }
        15 => {
            if f < 0.5 {
                2.0 * b * f
            } else {
                1.0 - 2.0 * (1.0 - b) * (1.0 - f)
            }
        }
        _ => f,
    }
}

/// Blend `fg` over `bg` with `mode`, weighted by `opacity` (0..1, already including any mask).
/// `use_alpha`: the foreground's alpha also weights it.
#[inline]
pub fn blend_px(mode: i32, bg: [f32; 4], fg: [f32; 4], opacity: f32, use_alpha: bool, clamp: bool) -> [f32; 4] {
    let t = opacity * if use_alpha { fg[3] } else { 1.0 };
    let mut o = [0.0; 4];
    for k in 0..3 {
        let mut v = blend_channel(mode, bg[k], fg[k]);
        if clamp {
            v = v.clamp(0.0, 1.0);
        }
        o[k] = bg[k] + (v - bg[k]) * t;
    }
    o[3] = if mode == 0 || mode == 7 { bg[3] + (fg[3] - bg[3]) * opacity } else { bg[3] };
    o
}

/// A gray image of the output size from per-pixel values.
pub fn gray_from(w: u32, h: u32, v: Vec<f32>) -> Image {
    Image { w, h, kind: Kind::Gray, data: Arc::new(v) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blur_keeps_mean_and_tiles() {
        let img = Image::gray(32, 32, |x, y| if (x / 4 + y / 4) % 2 == 0 { 1.0 } else { 0.0 });
        for s in [1.0, 6.0] {
            let b = gaussian(&img, s, true);
            assert!((b.mean()[0] - img.mean()[0]).abs() < 1e-3, "sigma {s}");
        }
        let flat = gaussian(&Image::filled(8, 8, Kind::Color, [0.2, 0.4, 0.6, 1.0]), 4.0, false);
        assert!((flat.get(3, 3, false)[1] - 0.4).abs() < 1e-5);
    }

    #[test]
    fn edt_exact() {
        let d = distance_sq(9, 1, |i| i == 4, false);
        assert_eq!(d, vec![16.0, 9.0, 4.0, 1.0, 0.0, 1.0, 4.0, 9.0, 16.0]);
        let t = distance_sq(9, 1, |i| i == 0, true);
        assert_eq!(t[8], 1.0);
    }

    #[test]
    fn hsl_round_trip() {
        let c = [0.8, 0.3, 0.1, 1.0];
        let h = rgb_to_hsl(c);
        let r = hsl_to_rgb(h[0], h[1], h[2]);
        for k in 0..3 {
            assert!((r[k] - c[k]).abs() < 1e-5);
        }
    }
}
