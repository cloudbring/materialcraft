//! More Texture generators › Noises: fractal variants, fibers, organic cells and natural
//! materials (sand, wood, marble, stone, bark, concrete). All tile: every lattice uses whole-number
//! periods per axis.

use std::f32::consts::{PI, TAU};

use crate::catalog::{CookResult, Ctx, NodeDef, node};
use crate::generators::NOISES;
use crate::noise::{cellular, fbm, gradient2, hash3, rand1, rand2, value};

#[inline]
pub(crate) fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0).max(1e-6)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[inline]
fn wrap(i: i32, p: i32) -> i32 {
    if p <= 0 { i } else { i.rem_euclid(p) }
}

/// Gradient (Perlin) noise in about -1..1 with separate lattice periods per axis, so stretched
/// noise still tiles.
pub(crate) fn perlin_xy(x: f32, y: f32, px: i32, py: i32, seed: u32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let (ix, iy) = (x0 as i32, y0 as i32);
    let g = |i: i32, j: i32, dx: f32, dy: f32| {
        let gradient = gradient2(hash3(wrap(i, px), wrap(j, py), seed));
        gradient[0] * dx + gradient[1] * dy
    };
    let fade = |t: f32| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
    let (u, v) = (fade(fx), fade(fy));
    let a = g(ix, iy, fx, fy) + (g(ix + 1, iy, fx - 1.0, fy) - g(ix, iy, fx, fy)) * u;
    let b = g(ix, iy + 1, fx, fy - 1.0) + (g(ix + 1, iy + 1, fx - 1.0, fy - 1.0) - g(ix, iy + 1, fx, fy - 1.0)) * u;
    (a + (b - a) * v) * std::f32::consts::SQRT_2
}

/// Fractal sum of `f(level multiplier, octave)` (each 0..1): `octaves` layers, each twice as fine
/// and `gain` times weaker. Normalized to 0..1.
pub(crate) fn fractal(octaves: i32, gain: f32, f: impl Fn(f32, u32) -> f32) -> f32 {
    let (mut sum, mut amp, mut norm, mut m) = (0.0, 1.0, 0.0, 1.0);
    for o in 0..octaves.clamp(1, 10) as u32 {
        sum += f(m, o) * amp;
        norm += amp;
        amp *= gain;
        m *= 2.0;
    }
    sum / f32::max(norm, 1e-6)
}

/// Stretched fractal Perlin noise in 0..1 with `sx` × `sy` base cells.
fn fbm_xy(u: f32, v: f32, sx: f32, sy: f32, octaves: i32, gain: f32, seed: u32) -> f32 {
    fractal(octaves, gain, |m, o| {
        let (px, py) = (sx * m, sy * m);
        perlin_xy(u * px, v * py, px as i32, py as i32, seed.wrapping_add(o * 977)) * 0.5 + 0.5
    })
}

/// A thin bright line per stretched noise zero crossing (fibers, grain).
fn fiber(u: f32, v: f32, along: f32, across: f32, sharp: f32, seed: u32) -> f32 {
    (1.0 - perlin_xy(u * along, v * across, along as i32, across as i32, seed).abs()).powf(sharp)
}

fn whole(ctx: &Ctx, id: &str) -> f32 {
    ctx.f(id).max(1.0).round()
}

pub(crate) fn defs() -> Vec<NodeDef> {
    let n = |id, label| node(id, label, NOISES).generator();
    vec![
        n("value_noise", "Value Noise")
            .desc("Fractal value noise: smooth blocks of random values.")
            .float("scale", "Scale", 8.0, 1.0, 256.0)
            .int("octaves", "Octaves", 1, 1, 10)
            .float("roughness", "Roughness", 0.5, 0.0, 1.0)
            .gray_out()
            .cook(value_noise),
        n("ridged_fractal", "Ridged Fractal")
            .desc("Ridged multi-octave noise: sharp crests like mountain ridges.")
            .float("scale", "Scale", 4.0, 1.0, 64.0)
            .int("octaves", "Octaves", 6, 1, 10)
            .float("roughness", "Roughness", 0.5, 0.0, 1.0)
            .float("sharpness", "Sharpness", 2.0, 0.5, 8.0)
            .gray_out()
            .cook(ridged_fractal),
        n("voronoi_fractal", "Voronoi Fractal")
            .desc("Several octaves of cell bumps.")
            .float("scale", "Scale", 4.0, 1.0, 64.0)
            .int("octaves", "Octaves", 4, 1, 8)
            .float("roughness", "Roughness", 0.5, 0.0, 1.0)
            .float("disorder", "Disorder", 1.0, 0.0, 1.0)
            .gray_out()
            .cook(voronoi_fractal),
        n("cells_fractal", "Cells Fractal")
            .desc("Several octaves of cell borders: cracked, crystalline networks.")
            .float("scale", "Scale", 4.0, 1.0, 64.0)
            .int("octaves", "Octaves", 3, 1, 8)
            .float("roughness", "Roughness", 0.5, 0.0, 1.0)
            .float("disorder", "Disorder", 1.0, 0.0, 1.0)
            .gray_out()
            .cook(cells_fractal),
        n("directional_noise", "Directional Noise")
            .desc("Fractal noise stretched along one axis: streaks and brushed patterns.")
            .float("scale", "Scale along", 2.0, 1.0, 64.0)
            .float("scale_across", "Scale across", 24.0, 1.0, 256.0)
            .int("octaves", "Octaves", 5, 1, 10)
            .float("roughness", "Roughness", 0.5, 0.0, 1.0)
            .boolean("vertical", "Vertical", false)
            .gray_out()
            .cook(directional_noise),
        n("fibers", "Fibers")
            .desc("Thin, nearly parallel fibers.")
            .float("density", "Density", 96.0, 4.0, 512.0)
            .float("length", "Length", 2.0, 1.0, 32.0)
            .float("sharpness", "Sharpness", 12.0, 1.0, 64.0)
            .boolean("vertical", "Vertical", false)
            .gray_out()
            .cook(fibers),
        n("messy_fibers", "Messy Fibers")
            .desc("Tangled fibers in both directions, warped by clouds.")
            .float("density", "Density", 64.0, 4.0, 512.0)
            .float("warp", "Warp", 0.5, 0.0, 2.0)
            .float("sharpness", "Sharpness", 10.0, 1.0, 64.0)
            .gray_out()
            .cook(messy_fibers),
        n("leather", "Leather")
            .desc("Leather grain: small bumpy cells split by creases, over soft clouds.")
            .float("scale", "Scale", 24.0, 2.0, 128.0)
            .float("crease", "Crease width", 0.25, 0.0, 1.0)
            .float("warp", "Warp", 0.4, 0.0, 1.0)
            .float("clouds", "Clouds", 0.3, 0.0, 1.0)
            .gray_out()
            .cook(leather),
        n("microscope", "Microscope View")
            .desc("Strongly warped cell walls, like tissue under a microscope.")
            .float("scale", "Scale", 6.0, 1.0, 64.0)
            .float("warp", "Warp", 0.6, 0.0, 2.0)
            .float("width", "Wall width", 0.15, 0.01, 1.0)
            .gray_out()
            .cook(microscope),
        n("gaussian_spots", "Gaussian Spots")
            .desc("Scattered soft round spots of random size and intensity.")
            .float("scale", "Scale", 8.0, 1.0, 64.0)
            .float("size", "Size", 0.3, 0.01, 1.0)
            .float("size_random", "Size random", 0.5, 0.0, 1.0)
            .float("density", "Density", 0.8, 0.0, 1.0)
            .gray_out()
            .cook(gaussian_spots),
        n("crackle", "Crackle")
            .desc("Wiggly cracks along cell borders, with finer secondary cracks.")
            .float("scale", "Scale", 5.0, 1.0, 64.0)
            .float("width", "Width", 0.08, 0.005, 0.5)
            .float("disorder", "Disorder", 0.5, 0.0, 2.0)
            .float("amount", "Amount", 1.0, 0.0, 1.0)
            .gray_out()
            .cook(crackle),
        n("sand_dunes", "Sand Dunes")
            .desc("Wind-shaped dunes: gentle windward slopes, steep lee sides and fine ripples.")
            .int("count", "Dunes", 6, 1, 64)
            .float("warp", "Warp", 0.6, 0.0, 2.0)
            .float("steepness", "Lee side", 0.3, 0.05, 0.95)
            .float("ripples", "Ripples", 0.15, 0.0, 1.0)
            .gray_out()
            .cook(sand_dunes),
        n("wood_grain", "Wood Grain")
            .desc("Distorted growth rings and fibers of sawn wood (grain along X).")
            .float("rings", "Rings", 16.0, 1.0, 128.0)
            .float("stretch", "Stretch", 6.0, 1.0, 32.0)
            .float("sharpness", "Ring sharpness", 3.0, 0.5, 16.0)
            .float("fibers", "Fibers", 0.3, 0.0, 1.0)
            .gray_out()
            .cook(wood_grain),
        n("marble", "Marble")
            .desc("Thin veins from sine bands warped by fractal noise.")
            .float("scale", "Bands", 2.0, 1.0, 32.0)
            .float("turbulence", "Turbulence", 3.0, 0.0, 10.0)
            .float("sharpness", "Vein sharpness", 4.0, 1.0, 64.0)
            .choice("direction", "Direction", &["Horizontal", "Vertical", "Diagonal"], 2)
            .gray_out()
            .cook(marble),
        n("stones", "Stones")
            .desc("Packed rounded stones with gaps (height).")
            .float("scale", "Scale", 6.0, 1.0, 64.0)
            .float("gap", "Gap", 0.06, 0.0, 0.5)
            .float("bevel", "Bevel", 0.25, 0.01, 1.0)
            .float("height_random", "Height random", 0.4, 0.0, 1.0)
            .float("detail", "Detail", 0.2, 0.0, 1.0)
            .float("disorder", "Disorder", 1.0, 0.0, 1.0)
            .gray_out()
            .cook(stones),
        n("ripples", "Ripples")
            .desc("Interfering waves, like a water surface.")
            .float("scale", "Scale", 8.0, 1.0, 32.0)
            .int("waves", "Waves", 10, 1, 32)
            .float("warp", "Warp", 0.3, 0.0, 2.0)
            .gray_out()
            .cook(ripples),
        n("concrete", "Concrete")
            .desc("Cement clouds with small air pits and fine grain.")
            .float("scale", "Scale", 4.0, 1.0, 32.0)
            .float("pits", "Pit density", 0.3, 0.0, 1.0)
            .float("pit_size", "Pit size", 0.5, 0.0, 1.0)
            .float("roughness", "Roughness", 0.6, 0.0, 1.0)
            .gray_out()
            .cook(concrete),
        n("bark", "Bark")
            .desc("Deep vertical bark furrows.")
            .float("scale", "Furrows", 8.0, 1.0, 64.0)
            .float("stretch", "Length", 1.0, 1.0, 16.0)
            .float("warp", "Warp", 0.4, 0.0, 2.0)
            .float("roughness", "Roughness", 0.5, 0.0, 1.0)
            .gray_out()
            .cook(bark),
    ]
}

fn value_noise(ctx: &Ctx) -> CookResult {
    let (sc, o, r, s) = (whole(ctx, "scale"), ctx.i("octaves"), ctx.f("roughness"), ctx.seed);
    Ok(vec![ctx.gen_gray(|u, v| fractal(o, r, |m, k| value(u * sc * m, v * sc * m, (sc * m) as i32, s.wrapping_add(k * 131))))])
}

fn ridged_fractal(ctx: &Ctx) -> CookResult {
    let (sc, o, r, sh, s) = (whole(ctx, "scale"), ctx.i("octaves"), ctx.f("roughness"), ctx.f("sharpness").max(0.1), ctx.seed);
    Ok(vec![ctx.gen_gray(|u, v| {
        fractal(o, r, |m, k| (1.0 - perlin_xy(u * sc * m, v * sc * m, (sc * m) as i32, (sc * m) as i32, s.wrapping_add(k * 7919)).abs()).powf(sh))
    })])
}

fn cell_fractal(ctx: &Ctx, f: impl Fn(crate::noise::Cell) -> f32 + Sync) -> CookResult {
    let (sc, o, r, d, s) = (whole(ctx, "scale"), ctx.i("octaves"), ctx.f("roughness"), ctx.f("disorder"), ctx.seed);
    Ok(vec![ctx.gen_gray(|u, v| fractal(o, r, |m, k| f(cellular(u * sc * m, v * sc * m, (sc * m) as i32, d, 0, s.wrapping_add(k * 311)))))])
}

fn voronoi_fractal(ctx: &Ctx) -> CookResult {
    cell_fractal(ctx, |c| (1.0 - c.f1).clamp(0.0, 1.0))
}

fn cells_fractal(ctx: &Ctx) -> CookResult {
    cell_fractal(ctx, |c| ((c.f2 - c.f1) * 2.0).min(1.0))
}

fn directional_noise(ctx: &Ctx) -> CookResult {
    let (along, across, o, r, s) = (whole(ctx, "scale"), whole(ctx, "scale_across"), ctx.i("octaves"), ctx.f("roughness"), ctx.seed);
    let vert = ctx.b("vertical");
    Ok(vec![ctx.gen_gray(|u, v| if vert { fbm_xy(u, v, across, along, o, r, s) } else { fbm_xy(u, v, along, across, o, r, s) })])
}

fn fibers(ctx: &Ctx) -> CookResult {
    let (d, l, sh, s) = (whole(ctx, "density"), whole(ctx, "length"), ctx.f("sharpness").max(1.0), ctx.seed);
    let vert = ctx.b("vertical");
    Ok(vec![ctx.gen_gray(|u, v| {
        let (u, v) = if vert { (v, u) } else { (u, v) };
        let a = fiber(u, v, l, d, sh, s);
        let b = fiber(u, v, l * 2.0, d * 2.0, sh, s ^ 0x51);
        (a + b * 0.6).min(1.0)
    })])
}

fn messy_fibers(ctx: &Ctx) -> CookResult {
    let (d, w, sh, s) = (whole(ctx, "density"), ctx.f("warp") * 0.15, ctx.f("sharpness").max(1.0), ctx.seed);
    Ok(vec![ctx.gen_gray(|u, v| {
        let wu = u + (fbm(u, v, 3.0, 4, 0.5, s ^ 1) - 0.5) * w;
        let wv = v + (fbm(u, v, 3.0, 4, 0.5, s ^ 2) - 0.5) * w;
        let h = fiber(wu, wv, 3.0, d, sh, s);
        let vt = fiber(wv, wu, 3.0, d, sh, s ^ 0x77);
        let fine = fiber(wv, wu, 6.0, d * 2.0, sh, s ^ 0x99);
        (h + vt * 0.8 + fine * 0.4).min(1.0)
    })])
}

fn leather(ctx: &Ctx) -> CookResult {
    let (sc, cw, w, cl, s) = (whole(ctx, "scale"), ctx.f("crease").max(1e-3), ctx.f("warp"), ctx.f("clouds"), ctx.seed);
    let lo = (sc / 6.0).round().max(1.0);
    Ok(vec![ctx.gen_gray(|u, v| {
        let qx = (fbm(u, v, lo, 4, 0.5, s ^ 3) - 0.5) * w / sc * 2.0;
        let qy = (fbm(u, v, lo, 4, 0.5, s ^ 4) - 0.5) * w / sc * 2.0;
        let c = cellular((u + qx) * sc, (v + qy) * sc, sc as i32, 1.0, 0, s);
        let crease = smoothstep(0.0, cw, c.f2 - c.f1);
        let bump = (1.0 - c.f1 * 0.9).clamp(0.0, 1.0);
        let clouds = fbm(u, v, lo, 5, 0.5, s ^ 7);
        (crease * (0.55 + 0.45 * bump) * (1.0 - cl + cl * clouds)).clamp(0.0, 1.0)
    })])
}

fn microscope(ctx: &Ctx) -> CookResult {
    let (sc, w, wd, s) = (whole(ctx, "scale"), ctx.f("warp"), ctx.f("width").max(1e-3), ctx.seed);
    Ok(vec![ctx.gen_gray(|u, v| {
        let qx = (fbm(u, v, sc, 4, 0.5, s ^ 5) - 0.5) * w / sc * 3.0;
        let qy = (fbm(u, v, sc, 4, 0.5, s ^ 6) - 0.5) * w / sc * 3.0;
        let c = cellular((u + qx) * sc, (v + qy) * sc, sc as i32, 1.0, 0, s);
        let wall = 1.0 - smoothstep(0.0, wd, c.f2 - c.f1);
        let inner = fbm(u, v, sc * 2.0, 4, 0.5, s ^ 8) * 0.35 * (1.0 - c.f1).max(0.0);
        (wall + inner).clamp(0.0, 1.0)
    })])
}

fn gaussian_spots(ctx: &Ctx) -> CookResult {
    let (sc, size, sr, dens, s) = (whole(ctx, "scale"), ctx.f("size").max(0.01), ctx.f("size_random"), ctx.f("density"), ctx.seed);
    Ok(vec![ctx.gen_gray(|u, v| {
        let mut sum = 0.0;
        for layer in 0..2u32 {
            let n = sc * (layer + 1) as f32;
            let (x, y) = (u * n, v * n);
            let (ix, iy) = (x.floor() as i32, y.floor() as i32);
            let ls = s.wrapping_add(layer * 0x3c6e);
            for j in -1..=1 {
                for i in -1..=1 {
                    let (cx, cy) = (wrap(ix + i, n as i32), wrap(iy + j, n as i32));
                    if rand2(cx, cy, ls ^ 9) > dens {
                        continue;
                    }
                    let px = (ix + i) as f32 + rand2(cx, cy, ls);
                    let py = (iy + j) as f32 + rand2(cx, cy, ls ^ 1);
                    let r = size * (1.0 - sr * rand2(cx, cy, ls ^ 2)) * 0.5;
                    let d2 = (px - x).powi(2) + (py - y).powi(2);
                    sum += (-d2 / (2.0 * r * r)).exp() * (0.4 + 0.6 * rand2(cx, cy, ls ^ 3)) / (layer + 1) as f32;
                }
            }
        }
        sum.min(1.0)
    })])
}

fn crackle(ctx: &Ctx) -> CookResult {
    let (sc, wd, dis, amt, s) = (whole(ctx, "scale"), ctx.f("width").max(1e-3), ctx.f("disorder"), ctx.f("amount"), ctx.seed);
    Ok(vec![ctx.gen_gray(|u, v| {
        let qx = (fbm(u, v, sc, 4, 0.5, s ^ 1) - 0.5) * dis / sc;
        let qy = (fbm(u, v, sc, 4, 0.5, s ^ 2) - 0.5) * dis / sc;
        let (wu, wv) = (u + qx, v + qy);
        let c = cellular(wu * sc, wv * sc, sc as i32, 1.0, 0, s);
        let main = 1.0 - smoothstep(0.0, wd, c.f2 - c.f1);
        let n2 = sc * 3.0;
        let c2 = cellular(wu * n2, wv * n2, n2 as i32, 1.0, 0, s ^ 5);
        // Finer cracks only in some areas, so they break off instead of closing into cells.
        let fine = (1.0 - smoothstep(0.0, wd * 1.5, c2.f2 - c2.f1)) * smoothstep(0.55, 0.65, fbm(u, v, sc * 2.0, 3, 0.5, s ^ 11)) * 0.6;
        let vis = smoothstep(0.9 - amt, 1.1 - amt, fbm(u, v, 2.0, 3, 0.5, s ^ 3) * 0.999);
        (main.max(fine) * if amt >= 1.0 { 1.0 } else { vis }).clamp(0.0, 1.0)
    })])
}

fn sand_dunes(ctx: &Ctx) -> CookResult {
    let (n, w, k, rp, s) = (ctx.i("count").max(1) as f32, ctx.f("warp"), ctx.f("steepness").clamp(0.05, 0.95), ctx.f("ripples"), ctx.seed);
    Ok(vec![ctx.gen_gray(|u, v| {
        let q = fbm(u, v, 3.0, 5, 0.5, s) - 0.5;
        let t = (v * n + q * w * 2.0).rem_euclid(1.0);
        let p = if t < 1.0 - k { t / (1.0 - k) } else { (1.0 - t) / k };
        let p = p * p * (3.0 - 2.0 * p);
        let rip = (TAU * (v * n * 8.0 + q * 6.0 + u * 2.0)).sin() * 0.5 + 0.5;
        p * (1.0 - rp) + rip * rp * (0.5 + 0.5 * p)
    })])
}

fn wood_grain(ctx: &Ctx) -> CookResult {
    let (rings, st, sh, fb, s) = (ctx.f("rings").max(1.0), whole(ctx, "stretch"), ctx.f("sharpness").max(0.1), ctx.f("fibers"), ctx.seed);
    Ok(vec![ctx.gen_gray(|u, v| {
        let n = fbm_xy(u, v, 1.0, st, 4, 0.45, s);
        let t = (n * rings).rem_euclid(1.0);
        let ring = t.powf(sh);
        let f = fiber(u, v, 2.0, 160.0, 8.0, s ^ 0x3b);
        ((1.0 - ring * 0.65) * (1.0 - f * fb * 0.35)).clamp(0.0, 1.0)
    })])
}

fn marble(ctx: &Ctx) -> CookResult {
    let (n, turb, sh, dir, s) = (whole(ctx, "scale"), ctx.f("turbulence"), ctx.f("sharpness").max(1.0), ctx.i("direction"), ctx.seed);
    Ok(vec![ctx.gen_gray(|u, v| {
        let base = match dir {
            0 => v,
            1 => u,
            _ => u + v,
        };
        let f = fbm(u, v, 3.0, 6, 0.55, s);
        let vein = |k: f32, f: f32| (1.0 - (PI * (base * n * k + turb * f)).sin().abs()).powf(sh * k);
        // Main veins plus a finer, fainter network.
        let v2 = vein(2.0, fbm(u, v, 4.0, 6, 0.55, s ^ 9));
        (0.25 + 0.2 * fbm(u, v, 6.0, 5, 0.5, s ^ 4) + 0.6 * vein(1.0, f).max(v2 * 0.45)).clamp(0.0, 1.0)
    })])
}

fn stones(ctx: &Ctx) -> CookResult {
    let (sc, gap, bev, hr, det, dis, s) =
        (whole(ctx, "scale"), ctx.f("gap"), ctx.f("bevel").max(1e-3), ctx.f("height_random"), ctx.f("detail"), ctx.f("disorder"), ctx.seed);
    Ok(vec![ctx.gen_gray(|u, v| {
        let c = cellular(u * sc, v * sc, sc as i32, dis, 0, s);
        let e = (c.f2 - c.f1) * 0.5 - gap * 0.5;
        if e <= 0.0 {
            return 0.0;
        }
        let dome = smoothstep(0.0, bev, e).sqrt();
        let d = fbm(u, v, sc * 2.0, 4, 0.5, s ^ 3);
        (dome * (1.0 - hr * c.id) * (1.0 - det + det * d)).clamp(0.0, 1.0)
    })])
}

fn ripples(ctx: &Ctx) -> CookResult {
    let (sc, n, w, s) = (whole(ctx, "scale"), ctx.i("waves").clamp(1, 32) as u32, ctx.f("warp"), ctx.seed);
    // Whole-number wave vectors keep every wave periodic over the image.
    let waves: Vec<(f32, f32, f32, f32)> = (0..n)
        .map(|k| {
            let (dy, dx) = (rand1(k * 4, s) * TAU).sin_cos();
            let mag = sc * (0.5 + 0.5 * rand1(k * 4 + 1, s));
            let (mut kx, ky) = ((dx * mag).round(), (dy * mag).round());
            if kx == 0.0 && ky == 0.0 {
                kx = 1.0;
            }
            (kx, ky, rand1(k * 4 + 2, s), 1.0 / (1.0 + k as f32 * 0.3))
        })
        .collect();
    let norm: f32 = waves.iter().map(|w| w.3).sum();
    Ok(vec![ctx.gen_gray(|u, v| {
        let q = (fbm(u, v, 2.0, 3, 0.5, s ^ 7) - 0.5) * w;
        let sum: f32 = waves.iter().map(|(kx, ky, ph, a)| (TAU * (kx * u + ky * v + ph + q)).sin() * a).sum();
        (sum / norm.max(1e-6) * 0.5 + 0.5).clamp(0.0, 1.0)
    })])
}

fn concrete(ctx: &Ctx) -> CookResult {
    let (sc, pits, ps, r, s) = (whole(ctx, "scale"), ctx.f("pits"), ctx.f("pit_size"), ctx.f("roughness"), ctx.seed);
    let pn = (sc * 12.0).round();
    Ok(vec![ctx.gen_gray(|u, v| {
        let base = fbm(u, v, sc, 8, r, s);
        let c = cellular(u * pn, v * pn, pn as i32, 1.0, 0, s ^ 5);
        let rad = 0.08 + ps * 0.3;
        let pit = if c.id < pits { 1.0 - smoothstep(rad * 0.6, rad, c.f1) } else { 0.0 };
        let grain = value(u * 192.0, v * 192.0, 192, s ^ 9);
        (base * 0.75 + grain * 0.25 - pit * 0.45).clamp(0.0, 1.0)
    })])
}

fn bark(ctx: &Ctx) -> CookResult {
    let (sx, sy, w, r, s) = (whole(ctx, "scale"), whole(ctx, "stretch"), ctx.f("warp"), ctx.f("roughness"), ctx.seed);
    Ok(vec![ctx.gen_gray(|u, v| {
        let q = (fbm(u, v, 4.0, 4, 0.5, s ^ 3) - 0.5) * w / sx;
        fractal(5, r, |m, k| {
            let (px, py) = (sx * m, sy * m);
            (1.0 - perlin_xy((u + q) * px, v * py, px as i32, py as i32, s.wrapping_add(k * 613)).abs()).powi(2)
        })
    })])
}
