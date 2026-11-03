//! Tileable noise primitives: integer hashing, value and gradient (Perlin) noise on a periodic
//! lattice, fractal sums and cellular (Worley) noise. Every function takes normalized coordinates
//! (0..1 across the image) and a period in cells, so whole-number scales tile seamlessly.
//! The 2D gradient lookup retains the existing 24-bit hash-to-angle mapping: 256 precomputed
//! coarse unit vectors plus a small residual rotation, not a new discrete gradient set. This
//! preserves images within 1e-6 rather than bit-for-bit; the 3D path already uses 12 fixed
//! gradients and is unchanged.

use std::sync::OnceLock;

// The hash maps to 24-bit continuous angles, not one of 256 discrete directions. The table
// stores the 256 coarse angles of that same mapping; a short rotation by the remaining bits
// retains the original gradient field within floating-point rounding (not bit-for-bit).
static GRADIENTS_2D: OnceLock<[[f32; 2]; 256]> = OnceLock::new();

#[inline]
pub(crate) fn gradient2(hash: u32) -> [f32; 2] {
    const ANGLE_STEP: f32 = std::f32::consts::TAU / (1u32 << 24) as f32;
    let index = hash >> 8;
    let coarse = (index >> 16) as usize;
    let base = GRADIENTS_2D.get_or_init(|| {
        std::array::from_fn(|index| {
            let angle = (index << 16) as f32 * ANGLE_STEP;
            [angle.cos(), angle.sin()]
        })
    })[coarse];
    let delta = index as f32 * ANGLE_STEP - (coarse << 16) as f32 * ANGLE_STEP;
    let delta2 = delta * delta;
    let sine = delta * (1.0 + delta2 * (-1.0 / 6.0 + delta2 / 120.0));
    let cosine = 1.0 + delta2 * (-0.5 + delta2 / 24.0);
    [base[0] * cosine - base[1] * sine, base[1] * cosine + base[0] * sine]
}

/// A 32-bit integer hash (lowbias32 by Chris Wellons, public domain).
#[inline]
pub fn hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x
}

#[inline]
pub fn hash3(x: i32, y: i32, seed: u32) -> u32 {
    hash((x as u32).wrapping_mul(0x8da6_b343) ^ hash((y as u32).wrapping_mul(0xd816_3841) ^ seed.wrapping_mul(0xcb1a_b31f)))
}

/// Uniform 0..1 from a lattice point.
#[inline]
pub fn rand2(x: i32, y: i32, seed: u32) -> f32 {
    (hash3(x, y, seed) >> 8) as f32 / (1u32 << 24) as f32
}

/// Uniform 0..1 from an index.
#[inline]
pub fn rand1(i: u32, seed: u32) -> f32 {
    (hash(i ^ hash(seed.wrapping_add(0x9e37_79b9))) >> 8) as f32 / (1u32 << 24) as f32
}

#[inline]
fn fade(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

#[inline]
fn wrapi(i: i32, p: i32) -> i32 {
    if p <= 0 { i } else { i.rem_euclid(p) }
}

/// Value noise in 0..1, lattice period `p` cells (coordinates already scaled to cells).
pub fn value(x: f32, y: f32, p: i32, seed: u32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (tx, ty) = (fade(x - x0), fade(y - y0));
    let (ix, iy) = (x0 as i32, y0 as i32);
    let r = |i: i32, j: i32| rand2(wrapi(i, p), wrapi(j, p), seed);
    let a = r(ix, iy) + (r(ix + 1, iy) - r(ix, iy)) * tx;
    let b = r(ix, iy + 1) + (r(ix + 1, iy + 1) - r(ix, iy + 1)) * tx;
    a + (b - a) * ty
}

/// Gradient (Perlin) noise in about -1..1, lattice period `p` cells.
pub fn perlin(x: f32, y: f32, p: i32, seed: u32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let (ix, iy) = (x0 as i32, y0 as i32);
    let g = |i: i32, j: i32, dx: f32, dy: f32| {
        let gradient = gradient2(hash3(wrapi(i, p), wrapi(j, p), seed));
        gradient[0] * dx + gradient[1] * dy
    };
    let (u, v) = (fade(fx), fade(fy));
    let n00 = g(ix, iy, fx, fy);
    let n10 = g(ix + 1, iy, fx - 1.0, fy);
    let n01 = g(ix, iy + 1, fx, fy - 1.0);
    let n11 = g(ix + 1, iy + 1, fx - 1.0, fy - 1.0);
    let a = n00 + (n10 - n00) * u;
    let b = n01 + (n11 - n01) * u;
    (a + (b - a) * v) * std::f32::consts::SQRT_2
}

/// Fractal sum of Perlin noise in 0..1: `octaves` layers from `scale` cells, each `lacunarity`
/// (2) times finer and `gain` times weaker. Tileable when `scale` is whole.
pub fn fbm(u: f32, v: f32, scale: f32, octaves: u32, gain: f32, seed: u32) -> f32 {
    let mut sum = 0.0;
    let mut amp = 1.0;
    let mut norm = 0.0;
    let mut s = scale.max(1.0).round();
    for o in 0..octaves.clamp(1, 12) {
        sum += perlin(u * s, v * s, s as i32, seed.wrapping_add(o * 1013)) * amp;
        norm += amp;
        amp *= gain;
        s *= 2.0;
    }
    (sum / norm.max(1e-6)) * 0.5 + 0.5
}

/// Ridged (creased) fractal: `1 - |perlin|` layers, 0..1.
pub fn ridged(u: f32, v: f32, scale: f32, octaves: u32, gain: f32, seed: u32) -> f32 {
    let mut sum = 0.0;
    let mut amp = 1.0;
    let mut norm = 0.0;
    let mut s = scale.max(1.0).round();
    for o in 0..octaves.clamp(1, 12) {
        let n = 1.0 - perlin(u * s, v * s, s as i32, seed.wrapping_add(o * 7919)).abs();
        sum += n * n * amp;
        norm += amp;
        amp *= gain;
        s *= 2.0;
    }
    sum / norm.max(1e-6)
}

/// Result of a cellular lookup: the nearest and second-nearest feature distances (in cells),
/// the nearest feature's random id (0..1) and its position (cells).
#[derive(Clone, Copy, Debug)]
pub struct Cell {
    pub f1: f32,
    pub f2: f32,
    pub id: f32,
    pub center: [f32; 2],
}

/// Worley noise with one jittered feature point per cell (`jitter` 0..1), period `p` cells.
/// `metric`: 0 Euclidean, 1 Manhattan, 2 Chebyshev.
pub fn cellular(x: f32, y: f32, p: i32, jitter: f32, metric: u32, seed: u32) -> Cell {
    let (ix, iy) = (x.floor() as i32, y.floor() as i32);
    let mut f1 = f32::MAX;
    let mut f2 = f32::MAX;
    let mut id = 0.0;
    let mut center = [0.0, 0.0];
    for j in -2..=2 {
        for i in -2..=2 {
            let (cx, cy) = (ix + i, iy + j);
            let (wx, wy) = (wrapi(cx, p), wrapi(cy, p));
            let px = cx as f32 + 0.5 + (rand2(wx, wy, seed) - 0.5) * jitter;
            let py = cy as f32 + 0.5 + (rand2(wx, wy, seed ^ 0x5bd1_e995) - 0.5) * jitter;
            let (dx, dy) = (px - x, py - y);
            let d = match metric {
                1 => dx.abs() + dy.abs(),
                2 => dx.abs().max(dy.abs()),
                _ => (dx * dx + dy * dy).sqrt(),
            };
            if d < f1 {
                f2 = f1;
                f1 = d;
                id = rand2(wx, wy, seed ^ 0x2545_f491);
                center = [px, py];
            } else if d < f2 {
                f2 = d;
            }
        }
    }
    Cell { f1, f2, id, center }
}

/// Gaussian-distributed sample (Box–Muller) from two uniforms.
pub fn gauss(u1: f32, u2: f32) -> f32 {
    (-2.0 * u1.max(1e-7).ln()).sqrt() * (std::f32::consts::TAU * u2).cos()
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::noises_more::perlin_xy;

    fn old_perlin_xy(x: f32, y: f32, px: i32, py: i32, seed: u32) -> f32 {
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let (ix, iy) = (x0 as i32, y0 as i32);
        let gradient = |i: i32, j: i32, dx: f32, dy: f32| {
            let angle = (hash3(wrapi(i, px), wrapi(j, py), seed) >> 8) as f32 * (std::f32::consts::TAU / (1u32 << 24) as f32);
            angle.cos() * dx + angle.sin() * dy
        };
        let (u, v) = (fade(fx), fade(fy));
        let a = gradient(ix, iy, fx, fy) + (gradient(ix + 1, iy, fx - 1.0, fy) - gradient(ix, iy, fx, fy)) * u;
        let b = gradient(ix, iy + 1, fx, fy - 1.0) + (gradient(ix + 1, iy + 1, fx - 1.0, fy - 1.0) - gradient(ix, iy + 1, fx, fy - 1.0)) * u;
        (a + (b - a) * v) * std::f32::consts::SQRT_2
    }

    fn old_fbm(u: f32, v: f32, scale: f32, octaves: u32, gain: f32, seed: u32) -> f32 {
        let (mut sum, mut amp, mut norm, mut period) = (0.0, 1.0, 0.0, scale.max(1.0).round());
        for octave in 0..octaves.clamp(1, 12) {
            let x = u * period;
            let y = v * period;
            let (x0, y0) = (x.floor(), y.floor());
            let (fx, fy) = (x - x0, y - y0);
            let (ix, iy) = (x0 as i32, y0 as i32);
            let gradient = |i: i32, j: i32, dx: f32, dy: f32| {
                let angle = (hash3(wrapi(i, period as i32), wrapi(j, period as i32), seed.wrapping_add(octave * 1013)) >> 8) as f32
                    * (std::f32::consts::TAU / (1u32 << 24) as f32);
                angle.cos() * dx + angle.sin() * dy
            };
            let (fade_x, fade_y) = (fade(fx), fade(fy));
            let n00 = gradient(ix, iy, fx, fy);
            let n10 = gradient(ix + 1, iy, fx - 1.0, fy);
            let n01 = gradient(ix, iy + 1, fx, fy - 1.0);
            let n11 = gradient(ix + 1, iy + 1, fx - 1.0, fy - 1.0);
            let a = n00 + (n10 - n00) * fade_x;
            let b = n01 + (n11 - n01) * fade_x;
            sum += (a + (b - a) * fade_y) * std::f32::consts::SQRT_2 * amp;
            norm += amp;
            amp *= gain;
            period *= 2.0;
        }
        (sum / norm.max(1e-6)) * 0.5 + 0.5
    }

    #[test]
    fn gradient_lookup_matches_continuous_angle_and_tiles() {
        let mut state = 0xacf0_1234_u32;
        let mut max_difference = 0.0f32;
        for coarse in 0..=255u32 {
            for index in [(coarse << 16).saturating_sub(1), coarse << 16, ((coarse << 16) + 1).min((1 << 24) - 1)] {
                let hash = index << 8;
                let angle = index as f32 * (std::f32::consts::TAU / (1u32 << 24) as f32);
                let gradient = gradient2(hash);
                max_difference = max_difference.max((gradient[0] - angle.cos()).abs()).max((gradient[1] - angle.sin()).abs());
            }
        }
        for _ in 0..4096 {
            state = hash(state.wrapping_add(0x9e37_79b9));
            let angle = (state >> 8) as f32 * (std::f32::consts::TAU / (1u32 << 24) as f32);
            let gradient = gradient2(state);
            max_difference = max_difference.max((gradient[0] - angle.cos()).abs()).max((gradient[1] - angle.sin()).abs());
            let seed = state;
            let px = 1 + (state % 37) as i32;
            let py = 1 + ((state >> 8) % 29) as i32;
            let x = (state & 0xffff) as f32 / 4096.0;
            let y = (state >> 16) as f32 / 4096.0;
            let expected = old_perlin_xy(x, y, px, py, seed);
            let actual = perlin_xy(x, y, px, py, seed);
            max_difference = max_difference.max((expected - actual).abs());
            max_difference = max_difference.max((old_perlin_xy(x, y, px, px, seed) - perlin(x, y, px, seed)).abs());
            assert!((perlin(x, y, px, seed) - perlin_xy(x, y, px, px, seed)).abs() < 1e-6);
            assert!((actual - perlin_xy(x + px as f32, y, px, py, seed)).abs() < 1e-5);
            assert!((actual - perlin_xy(x, y + py as f32, px, py, seed)).abs() < 1e-5);
            let u = (state & 1023) as f32 / 1024.0;
            let v = ((state >> 10) & 1023) as f32 / 1024.0;
            max_difference = max_difference.max((fbm(u, v, px as f32, 8, 0.5, seed) - old_fbm(u, v, px as f32, 8, 0.5, seed)).abs());
            assert!((fbm(0.0, v, px as f32, 8, 0.5, seed) - fbm(1.0, v, px as f32, 8, 0.5, seed)).abs() < 1e-6);
        }
        assert!(max_difference <= 1e-6, "max difference: {max_difference}");
    }

    #[test]
    #[ignore = "release-only performance benchmark"]
    fn bench_fbm_2048_eight_octaves() {
        std::hint::black_box(fbm(0.0, 0.0, 8.0, 8, 0.5, 7));
        let start = std::time::Instant::now();
        let mut checksum = 0.0f32;
        for y in 0..2048 {
            for x in 0..2048 {
                checksum += std::hint::black_box(fbm(x as f32 / 2048.0, y as f32 / 2048.0, 8.0, 8, 0.5, 7));
            }
        }
        eprintln!("fbm 2048² 8 octaves: {:.2} ms (checksum {checksum})", start.elapsed().as_secs_f64() * 1000.0);
        std::hint::black_box(checksum);
    }

    #[test]
    fn perlin_tiles_at_its_period() {
        for k in 0..20 {
            let y = k as f32 * 0.37;
            let a = perlin(0.0, y, 4, 7);
            let b = perlin(4.0, y, 4, 7);
            assert!((a - b).abs() < 1e-5, "{a} {b}");
        }
        let a = fbm(0.0, 0.3, 4.0, 5, 0.5, 1);
        let b = fbm(1.0, 0.3, 4.0, 5, 0.5, 1);
        assert!((a - b).abs() < 1e-4);
    }

    #[test]
    fn cellular_tiles_and_ranges() {
        let a = cellular(0.1, 0.2, 5, 1.0, 0, 3).f1;
        let b = cellular(5.1, 0.2, 5, 1.0, 0, 3).f1;
        assert!((a - b).abs() < 1e-4);
        assert!((0.0..1.5).contains(&a));
    }

    #[test]
    fn rand_is_uniformish() {
        let m: f32 = (0..10000).map(|i| rand1(i, 9)).sum::<f32>() / 10000.0;
        assert!((m - 0.5).abs() < 0.02);
    }
}
