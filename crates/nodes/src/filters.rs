//! Filters: adjustments, blending, blurs, channels, effects, normal map tools, tiling and
//! transforms.

use std::f32::consts::TAU;

use materialcraft_image::{Image, Kind, luminance};

use crate::atomic::{height_to_normal, levels_fn, transform_sample};
use crate::catalog::{CookResult, Ctx, NodeDef, PortKind, node};
use crate::ops;

const ADJ: &str = "Filters/Adjustments";
const BLENDING: &str = "Filters/Blending";
const BLURS: &str = "Filters/Blurs";
const CHANNELS: &str = "Filters/Channels";
const EFFECTS: &str = "Filters/Effects";
const NORMAL: &str = "Filters/Normal map";
const TILING: &str = "Filters/Tiling";
const TRANSFORMS: &str = "Filters/Transforms";

pub(crate) fn defs() -> Vec<NodeDef> {
    vec![
        // ---- Adjustments
        node("auto_levels", "Auto Levels", ADJ).desc("Stretches the input to the full 0..1 range.").any_in("input", "Input").cook(auto_levels),
        node("clamp", "Clamp", ADJ)
            .desc("Clamps values between a minimum and a maximum.")
            .any_in("input", "Input")
            .float("min", "Min", 0.0, 0.0, 1.0)
            .float("max", "Max", 1.0, 0.0, 1.0)
            .cook(clamp),
        node("color_to_mask", "Color to Mask", ADJ)
            .desc("White where the input is close to a key color.")
            .color_in("input", "Input")
            .color("color", "Key color", [1.0, 0.0, 0.0, 1.0])
            .float("range", "Range", 0.2, 0.0, 1.0)
            .float("softness", "Softness", 0.1, 0.0, 1.0)
            .gray_out()
            .cook(color_to_mask),
        node("contrast_luminosity", "Contrast/Luminosity", ADJ)
            .desc("Contrast and brightness (0 = unchanged).")
            .any_in("input", "Input")
            .float("contrast", "Contrast", 0.0, -1.0, 1.0)
            .float("luminosity", "Luminosity", 0.0, -1.0, 1.0)
            .cook(contrast_luminosity),
        node("exposure", "Exposure", ADJ).desc("Exposure in stops.").any_in("input", "Input").float("stops", "Exposure (EV)", 0.0, -10.0, 10.0).cook(exposure),
        node("gamma", "Gamma", ADJ).desc("Raises values to a power.").any_in("input", "Input").float("gamma", "Gamma", 1.0, 0.01, 10.0).cook(gamma),
        node("histogram_range", "Histogram Range", ADJ)
            .desc("Compresses values into a range around a position.")
            .gray_in("input", "Input")
            .float("range", "Range", 0.5, 0.0, 1.0)
            .float("position", "Position", 0.5, 0.0, 1.0)
            .gray_out()
            .cook(histogram_range),
        node("histogram_scan", "Histogram Scan", ADJ)
            .desc("Thresholds the input at a position with adjustable contrast.")
            .gray_in("input", "Input")
            .float("position", "Position", 0.5, 0.0, 1.0)
            .float("contrast", "Contrast", 0.5, 0.0, 1.0)
            .boolean("invert", "Invert position", false)
            .gray_out()
            .cook(histogram_scan),
        node("histogram_shift", "Histogram Shift", ADJ)
            .desc("Shifts values with wrap-around.")
            .gray_in("input", "Input")
            .float("position", "Position", 0.5, 0.0, 1.0)
            .gray_out()
            .cook(histogram_shift),
        node("invert", "Invert", ADJ).desc("1 minus the input (alpha kept).").any_in("input", "Input").boolean("alpha", "Invert alpha", false).cook(invert),
        node("posterize", "Posterize", ADJ)
            .desc("Quantizes values to a number of steps.")
            .any_in("input", "Input")
            .int("steps", "Steps", 4, 2, 256)
            .cook(posterize),
        node("replace_color", "Replace Color", ADJ)
            .desc("Shifts colors near a source color towards a target color.")
            .color_in("input", "Input")
            .color("source", "Source color", [1.0, 0.0, 0.0, 1.0])
            .color("target", "Target color", [0.0, 0.0, 1.0, 1.0])
            .float("range", "Range", 0.3, 0.0, 1.0)
            .color_out()
            .cook(replace_color),
        node("threshold", "Threshold", ADJ)
            .desc("White above the threshold, black below.")
            .gray_in("input", "Input")
            .float("threshold", "Threshold", 0.5, 0.0, 1.0)
            .gray_out()
            .cook(threshold),
        node("colorize", "Colorize", ADJ)
            .desc("Tints a grayscale image between two colors.")
            .gray_in("input", "Input")
            .color("dark", "Dark color", [0.05, 0.03, 0.02, 1.0])
            .color("light", "Light color", [0.9, 0.75, 0.5, 1.0])
            .color_out()
            .cook(colorize),
        node("channel_mixer", "Channel Mixer", ADJ)
            .desc("Each output channel as a weighted sum of the input channels.")
            .color_in("input", "Input")
            .float2("red_rg", "Red from R, G", [1.0, 0.0], -2.0, 2.0)
            .float("red_b", "Red from B", 0.0, -2.0, 2.0)
            .float2("green_rg", "Green from R, G", [0.0, 1.0], -2.0, 2.0)
            .float("green_b", "Green from B", 0.0, -2.0, 2.0)
            .float2("blue_rg", "Blue from R, G", [0.0, 0.0], -2.0, 2.0)
            .float("blue_b", "Blue from B", 1.0, -2.0, 2.0)
            .color_out()
            .cook(channel_mixer),
        // ---- Blending
        node("height_blend", "Height Blend", BLENDING)
            .desc("Blends two height maps by which is higher, with a soft contrast; also outputs the mask.")
            .gray_in("top", "Height top")
            .gray_in("bottom", "Height bottom")
            .gray_in("mask", "Mask")
            .float("offset", "Height offset", 0.5, 0.0, 1.0)
            .float("contrast", "Contrast", 0.9, 0.0, 1.0)
            .output("height", "Height", PortKind::Gray)
            .output("blend_mask", "Blend mask", PortKind::Gray)
            .cook(height_blend),
        node("alpha_merge", "Alpha Merge", BLENDING)
            .desc("Puts a grayscale image in a color image's alpha.")
            .color_in("input", "Color")
            .gray_in("alpha", "Alpha")
            .color_out()
            .cook(alpha_merge),
        node("rgba_merge", "RGBA Merge", CHANNELS)
            .desc("Builds a color image from four grayscale channels.")
            .gray_in("r", "R")
            .gray_in("g", "G")
            .gray_in("b", "B")
            .gray_in("a", "A")
            .color_out()
            .cook(rgba_merge),
        node("rgba_split", "RGBA Split", CHANNELS)
            .desc("Splits a color image into four grayscale channels.")
            .color_in("input", "RGBA")
            .output("r", "R", PortKind::Gray)
            .output("g", "G", PortKind::Gray)
            .output("b", "B", PortKind::Gray)
            .output("a", "A", PortKind::Gray)
            .cook(rgba_split),
        node("switch", "Switch", BLENDING)
            .desc("Outputs input A or input B.")
            .any_in("a", "Input A")
            .any_in("b", "Input B")
            .boolean("use_b", "Use input B", false)
            .cook(switch),
        // ---- Blurs
        node("anisotropic_blur", "Anisotropic Blur", BLURS)
            .desc("A Gaussian blur stronger along one direction.")
            .any_in("input", "Input")
            .float("intensity", "Intensity", 4.0, 0.0, 32.0)
            .float("anisotropy", "Anisotropy", 0.5, 0.0, 1.0)
            .angle("angle", "Angle", 0.0)
            .cook(anisotropic_blur),
        node("non_uniform_blur", "Non Uniform Blur", BLURS)
            .desc("Blurs more where the blur map is white.")
            .any_in("input", "Input")
            .gray_in("blur_map", "Blur map")
            .float("intensity", "Intensity", 4.0, 0.0, 32.0)
            .cook(non_uniform_blur),
        node("radial_blur", "Radial Blur", BLURS)
            .desc("Zoom blur from a center.")
            .any_in("input", "Input")
            .float("intensity", "Intensity", 0.2, 0.0, 1.0)
            .float2("center", "Center", [0.5, 0.5], 0.0, 1.0)
            .cook(radial_blur),
        node("slope_blur", "Slope Blur", BLURS)
            .desc("Smears the input along the slopes of a map, like flowing material.")
            .any_in("input", "Input")
            .gray_in("slope", "Slope")
            .float("intensity", "Intensity", 8.0, -64.0, 64.0)
            .int("samples", "Samples", 8, 1, 32)
            .choice("mode", "Mode", &["Blur", "Min", "Max"], 0)
            .cook(slope_blur),
        // ---- Effects
        node("ambient_occlusion", "Ambient Occlusion", EFFECTS)
            .desc("Darkens creases of a height map.")
            .gray_in("height", "Height")
            .float("spread", "Spreading", 0.15, 0.0, 1.0)
            .float("intensity", "Height depth", 0.5, 0.0, 1.0)
            .gray_out()
            .cook(ambient_occlusion),
        node("bevel", "Bevel", EFFECTS)
            .desc("Rounds the edges of a mask into a height ramp.")
            .gray_in("input", "Mask")
            .float("distance", "Distance", 0.05, -1.0, 1.0)
            .float("smoothing", "Smoothing", 0.0, 0.0, 1.0)
            .gray_out()
            .cook(bevel),
        node("curvature", "Curvature", EFFECTS)
            .desc("Convexity (white) and concavity (black) of a normal map.")
            .color_in("normal", "Normal")
            .float("intensity", "Intensity", 1.0, 0.0, 10.0)
            .gray_out()
            .cook(curvature),
        node("dilation", "Dilation", EFFECTS)
            .desc("Grows bright areas (maximum filter).")
            .any_in("input", "Input")
            .int("radius", "Radius", 2, 0, 64)
            .cook(dilation),
        node("erosion", "Erosion", EFFECTS)
            .desc("Shrinks bright areas (minimum filter).")
            .any_in("input", "Input")
            .int("radius", "Radius", 2, 0, 64)
            .cook(erosion),
        node("edge_detect", "Edge Detect", EFFECTS)
            .desc("White along the edges of a mask.")
            .gray_in("input", "Input")
            .float("width", "Edge width", 2.0, 0.0, 32.0)
            .float("roundness", "Edge roundness", 0.5, 0.0, 1.0)
            .gray_out()
            .cook(edge_detect),
        node("glow", "Glow", EFFECTS)
            .desc("A soft glow around bright areas.")
            .any_in("input", "Input")
            .float("threshold", "Threshold", 0.5, 0.0, 1.0)
            .float("size", "Size", 8.0, 0.0, 64.0)
            .float("amount", "Amount", 0.5, 0.0, 2.0)
            .cook(glow),
        node("highpass", "Highpass", EFFECTS)
            .desc("Keeps the fine detail, centered on gray.")
            .any_in("input", "Input")
            .float("radius", "Radius", 6.0, 0.0, 64.0)
            .cook(highpass),
        node("mosaic", "Mosaic", EFFECTS).desc("Pixelates into blocks.").any_in("input", "Input").int("cells", "Cells", 16, 1, 512).cook(mosaic),
        node("swirl", "Swirl", EFFECTS)
            .desc("Twists the image around its center.")
            .any_in("input", "Input")
            .float("amount", "Amount", 0.5, -4.0, 4.0)
            .float("radius", "Radius", 0.5, 0.0, 1.0)
            .cook(swirl),
        node("vignette", "Vignette", EFFECTS)
            .desc("Darkens towards the borders.")
            .any_in("input", "Input")
            .float("amount", "Amount", 0.5, 0.0, 1.0)
            .float("softness", "Softness", 0.5, 0.0, 1.0)
            .cook(vignette),
        node("quantize", "Quantize Grayscale", EFFECTS)
            .desc("Snaps gray values to steps with soft transitions.")
            .gray_in("input", "Input")
            .int("steps", "Steps", 4, 2, 64)
            .float("smoothness", "Smoothness", 0.1, 0.0, 1.0)
            .gray_out()
            .cook(quantize),
        // ---- Normal map
        node("height_to_normal", "Height to Normal", NORMAL)
            .desc("Same as Normal: a normal map from a height map.")
            .gray_in("input", "Height")
            .float("intensity", "Intensity", 1.0, 0.0, 10.0)
            .choice("format", "Normal format", &["OpenGL", "DirectX"], 0)
            .color_out()
            .cook(height_normal),
        node("normal_blend", "Normal Blend", NORMAL)
            .desc("Combines two normal maps (reoriented normal mapping).")
            .color_in("base", "Normal base")
            .color_in("detail", "Normal detail")
            .gray_in("mask", "Mask")
            .float("opacity", "Opacity", 1.0, 0.0, 1.0)
            .color_out()
            .cook(normal_blend),
        node("normal_invert", "Normal Invert", NORMAL)
            .desc("Flips the green (and optionally red) channel: OpenGL / DirectX.")
            .color_in("input", "Normal")
            .boolean("red", "Invert red", false)
            .boolean("green", "Invert green", true)
            .color_out()
            .cook(normal_invert),
        node("normal_normalize", "Normal Normalize", NORMAL)
            .desc("Rescales normals to unit length.")
            .color_in("input", "Normal")
            .color_out()
            .cook(normal_normalize),
        node("normal_intensity", "Normal Intensity", NORMAL)
            .desc("Strengthens or flattens a normal map.")
            .color_in("input", "Normal")
            .float("intensity", "Intensity", 1.0, 0.0, 10.0)
            .color_out()
            .cook(normal_intensity),
        node("normal_to_height", "Normal to Height", NORMAL)
            .desc("Reconstructs a height map from a normal map.")
            .color_in("input", "Normal")
            .float("intensity", "Height scale", 1.0, 0.0, 10.0)
            .gray_out()
            .cook(normal_to_height),
        // ---- Tiling
        node("make_it_tile", "Make It Tile", TILING)
            .desc("Cross-fades the borders so a photo tiles.")
            .any_in("input", "Input")
            .float("blend", "Blend width", 0.25, 0.0, 0.5)
            .cook(make_it_tile),
        node("safe_transform", "Safe Transform", TILING)
            .desc("Tiling-safe transform: whole-number tiling, offsets and 90° rotations.")
            .any_in("input", "Input")
            .int("tile", "Tile", 1, 1, 64)
            .float2("offset", "Offset", [0.0, 0.0], -1.0, 1.0)
            .choice("rotation", "Rotation", &["0°", "90°", "180°", "270°"], 0)
            .boolean("flip", "Flip", false)
            .cook(safe_transform),
        // ---- Transforms
        node("mirror", "Mirror", TRANSFORMS)
            .desc("Mirrors one half of the image onto the other.")
            .any_in("input", "Input")
            .choice("axis", "Mirror type", &["Horizontal", "Vertical", "Corner"], 0)
            .boolean("flip", "Flip side", false)
            .cook(mirror),
        node("kaleidoscope", "Kaleidoscope", TRANSFORMS)
            .desc("Repeats a wedge of the image around the center.")
            .any_in("input", "Input")
            .int("segments", "Segments", 6, 2, 64)
            .angle("angle", "Angle", 0.0)
            .cook(kaleidoscope),
        node("cartesian_to_polar", "Cartesian to Polar", TRANSFORMS)
            .desc("Wraps the image around the center.")
            .any_in("input", "Input")
            .cook(cartesian_to_polar),
        node("polar_to_cartesian", "Polar to Cartesian", TRANSFORMS)
            .desc("Unwraps a circular image into bands.")
            .any_in("input", "Input")
            .cook(polar_to_cartesian),
        node("skew", "Skew", TRANSFORMS)
            .desc("Shears the image.")
            .any_in("input", "Input")
            .float("amount", "Amount", 0.25, -2.0, 2.0)
            .boolean("vertical", "Vertical", false)
            .cook(skew),
    ]
}

fn src(ctx: &Ctx) -> Image {
    ctx.any_or(0, Kind::Gray, [0.0, 0.0, 0.0, 1.0])
}

fn per_value(ctx: &Ctx, f: impl Fn(f32) -> f32 + Sync) -> CookResult {
    let img = src(ctx);
    Ok(vec![img.map(img.kind, |p| [f(p[0]), f(p[1]), f(p[2]), p[3]])])
}

fn auto_levels(ctx: &Ctx) -> CookResult {
    let img = src(ctx);
    let (lo, hi) = img.range();
    let span = (hi - lo).max(1e-6);
    per_value(ctx, |v| (v - lo) / span)
}

fn clamp(ctx: &Ctx) -> CookResult {
    let (a, b) = (ctx.f("min"), ctx.f("max"));
    per_value(ctx, |v| v.max(a).min(b.max(a)))
}

fn color_to_mask(ctx: &Ctx) -> CookResult {
    let img = src(ctx).to_color();
    let (k, r, s) = (ctx.v4("color"), ctx.f("range"), ctx.f("softness").max(1e-4));
    Ok(vec![img.map(Kind::Gray, |p| {
        let d = ((p[0] - k[0]).powi(2) + (p[1] - k[1]).powi(2) + (p[2] - k[2]).powi(2)).sqrt();
        [(1.0 - (d - r) / s).clamp(0.0, 1.0), 0.0, 0.0, 0.0]
    })])
}

fn contrast_luminosity(ctx: &Ctx) -> CookResult {
    let (c, l) = (ctx.f("contrast"), ctx.f("luminosity"));
    let k = if c >= 0.0 { 1.0 / (1.0 - c * 0.99) } else { 1.0 + c };
    per_value(ctx, |v| ((v - 0.5) * k + 0.5 + l).clamp(0.0, 1.0))
}

fn exposure(ctx: &Ctx) -> CookResult {
    let k = 2f32.powf(ctx.f("stops"));
    per_value(ctx, |v| v * k)
}

fn gamma(ctx: &Ctx) -> CookResult {
    let g = ctx.f("gamma").max(0.01);
    per_value(ctx, |v| v.max(0.0).powf(1.0 / g))
}

fn histogram_range(ctx: &Ctx) -> CookResult {
    let (r, p) = (ctx.f("range"), ctx.f("position"));
    let img = ctx.gray_or(0, 0.0);
    Ok(vec![img.map(Kind::Gray, |c| [(p + (c[0] - 0.5) * r).clamp(0.0, 1.0), 0.0, 0.0, 0.0])])
}

/// The Histogram Scan mapping.
pub fn histogram_scan_fn(v: f32, position: f32, contrast: f32) -> f32 {
    let width = (1.0 - contrast).max(0.001);
    let lo = position * (1.0 + width) - width;
    levels_fn(v, lo, 0.5, lo + width, 0.0, 1.0)
}

fn histogram_scan(ctx: &Ctx) -> CookResult {
    let p = if ctx.b("invert") { 1.0 - ctx.f("position") } else { ctx.f("position") };
    let c = ctx.f("contrast");
    let img = ctx.gray_or(0, 0.0);
    Ok(vec![img.map(Kind::Gray, |v| [histogram_scan_fn(v[0], 1.0 - p, c), 0.0, 0.0, 0.0])])
}

fn histogram_shift(ctx: &Ctx) -> CookResult {
    let p = ctx.f("position");
    let img = ctx.gray_or(0, 0.0);
    Ok(vec![img.map(Kind::Gray, |v| [(v[0] + p).rem_euclid(1.0), 0.0, 0.0, 0.0])])
}

fn invert(ctx: &Ctx) -> CookResult {
    let img = src(ctx);
    let a = ctx.b("alpha");
    Ok(vec![img.map(img.kind, |p| [1.0 - p[0], 1.0 - p[1], 1.0 - p[2], if a { 1.0 - p[3] } else { p[3] }])])
}

fn posterize(ctx: &Ctx) -> CookResult {
    let n = (ctx.i("steps").max(2) - 1) as f32;
    per_value(ctx, |v| (v * n).round() / n)
}

fn replace_color(ctx: &Ctx) -> CookResult {
    let img = src(ctx).to_color();
    let (s, t, r) = (ctx.v4("source"), ctx.v4("target"), ctx.f("range").max(1e-4));
    Ok(vec![img.map(Kind::Color, |p| {
        let d = ((p[0] - s[0]).powi(2) + (p[1] - s[1]).powi(2) + (p[2] - s[2]).powi(2)).sqrt();
        let w = (1.0 - d / r).clamp(0.0, 1.0);
        [p[0] + (t[0] - s[0]) * w, p[1] + (t[1] - s[1]) * w, p[2] + (t[2] - s[2]) * w, p[3]]
    })])
}

fn threshold(ctx: &Ctx) -> CookResult {
    let t = ctx.f("threshold");
    let img = ctx.gray_or(0, 0.0);
    Ok(vec![img.map(Kind::Gray, |v| [if v[0] >= t { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0])])
}

fn colorize(ctx: &Ctx) -> CookResult {
    let (a, b) = (ctx.v4("dark"), ctx.v4("light"));
    let img = ctx.gray_or(0, 0.0);
    Ok(vec![img.map(Kind::Color, |v| {
        let t = v[0];
        [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t, 1.0]
    })])
}

fn channel_mixer(ctx: &Ctx) -> CookResult {
    let img = src(ctx).to_color();
    let (r, g, b) = (ctx.v2("red_rg"), ctx.v2("green_rg"), ctx.v2("blue_rg"));
    let (rb, gb, bb) = (ctx.f("red_b"), ctx.f("green_b"), ctx.f("blue_b"));
    Ok(vec![
        img.map(Kind::Color, |p| [r[0] * p[0] + r[1] * p[1] + rb * p[2], g[0] * p[0] + g[1] * p[1] + gb * p[2], b[0] * p[0] + b[1] * p[1] + bb * p[2], p[3]]),
    ])
}

fn height_blend(ctx: &Ctx) -> CookResult {
    let (top, bot) = (ctx.gray_or(0, 0.0), ctx.gray_or(1, 0.0));
    let mask = ctx.sized(2).map(|m| m.to_gray());
    let (off, c) = (ctx.f("offset") - 0.5, ctx.f("contrast").clamp(0.0, 0.999));
    let soft = (1.0 - c) * 0.5 + 1e-4;
    let n = (ctx.w * ctx.h) as usize;
    let mut h = vec![0.0; n];
    let mut m = vec![0.0; n];
    for i in 0..n {
        let (t, b) = (top.data[i] + off, bot.data[i]);
        let mut k = ((t - b) / soft * 0.5 + 0.5).clamp(0.0, 1.0);
        if let Some(mm) = &mask {
            k *= mm.data[i];
        }
        h[i] = b + (t - b) * k;
        m[i] = k;
    }
    Ok(vec![ops::gray_from(ctx.w, ctx.h, h), ops::gray_from(ctx.w, ctx.h, m)])
}

fn alpha_merge(ctx: &Ctx) -> CookResult {
    let c = ctx.any_or(0, Kind::Color, [0.0, 0.0, 0.0, 1.0]).to_color();
    let a = ctx.gray_or(1, 1.0);
    Ok(vec![Image::generate(ctx.w, ctx.h, Kind::Color, |x, y| {
        let p = c.get(x as i64, y as i64, false);
        [p[0], p[1], p[2], a.get_gray(x as i64, y as i64, false)]
    })])
}

fn rgba_merge(ctx: &Ctx) -> CookResult {
    let ch: Vec<Image> = (0..4).map(|i| ctx.gray_or(i, if i == 3 { 1.0 } else { 0.0 })).collect();
    Ok(vec![Image::generate(ctx.w, ctx.h, Kind::Color, |x, y| {
        let i = (y * ctx.w + x) as usize;
        [ch[0].data[i], ch[1].data[i], ch[2].data[i], ch[3].data[i]]
    })])
}

fn rgba_split(ctx: &Ctx) -> CookResult {
    let img = ctx.any_or(0, Kind::Color, [0.0, 0.0, 0.0, 1.0]).to_color();
    Ok((0..4).map(|k| img.map(Kind::Gray, |p| [p[k], 0.0, 0.0, 0.0])).collect())
}

fn switch(ctx: &Ctx) -> CookResult {
    let i = usize::from(ctx.b("use_b"));
    Ok(vec![ctx.sized(i).unwrap_or_else(|| Image::filled(ctx.w, ctx.h, Kind::Gray, [0.0; 4]))])
}

fn anisotropic_blur(ctx: &Ctx) -> CookResult {
    let img = src(ctx);
    let s = ctx.f("intensity") * ctx.w as f32 / 512.0;
    let an = ctx.f("anisotropy");
    let a = ctx.f("angle") * TAU;
    // Blur in a rotated frame: rotate, blur x/y separately, rotate back.
    let (sx, sy) = (s, s * (1.0 - an));
    if a.abs() < 1e-4 {
        return Ok(vec![ops::gaussian_xy(&img, sx, sy, ctx.tile)]);
    }
    let rotated =
        Image::generate_uv(ctx.w, ctx.h, img.kind, |u, v| transform_sample(&img, u, v, [0.0, 0.0], [1.0, 1.0], a / TAU, false, true).unwrap_or([0.0; 4]));
    let b = ops::gaussian_xy(&rotated, sx, sy, true);
    Ok(vec![Image::generate_uv(ctx.w, ctx.h, img.kind, |u, v| transform_sample(&b, u, v, [0.0, 0.0], [1.0, 1.0], -a / TAU, false, true).unwrap_or([0.0; 4]))])
}

fn non_uniform_blur(ctx: &Ctx) -> CookResult {
    let img = src(ctx);
    let map = ctx.gray_or(1, 1.0);
    let s = ctx.f("intensity") * ctx.w as f32 / 512.0;
    // Four blur levels, blended by the map.
    let levels: Vec<Image> = (0..4).map(|i| ops::gaussian(&img, s * i as f32 / 3.0, ctx.tile)).collect();
    Ok(vec![Image::generate(ctx.w, ctx.h, img.kind, |x, y| {
        let t = map.get_gray(x as i64, y as i64, false).clamp(0.0, 1.0) * 3.0;
        let i = (t.floor() as usize).min(2);
        let f = t - i as f32;
        let (a, b) = (levels[i].get(x as i64, y as i64, false), levels[i + 1].get(x as i64, y as i64, false));
        [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f, a[2] + (b[2] - a[2]) * f, a[3] + (b[3] - a[3]) * f]
    })])
}

fn radial_blur(ctx: &Ctx) -> CookResult {
    let img = src(ctx);
    let (k, c) = (ctx.f("intensity"), ctx.v2("center"));
    let n = 24;
    Ok(vec![Image::generate_uv(ctx.w, ctx.h, img.kind, |u, v| {
        let mut acc = [0.0; 4];
        for i in 0..n {
            let t = 1.0 - k * i as f32 / n as f32;
            let p = img.sample(c[0] + (u - c[0]) * t, c[1] + (v - c[1]) * t, ctx.tile);
            for j in 0..4 {
                acc[j] += p[j] / n as f32;
            }
        }
        acc
    })])
}

fn slope_blur(ctx: &Ctx) -> CookResult {
    let img = src(ctx);
    let slope = ops::gaussian(&ctx.gray_or(1, 0.5), 1.0, ctx.tile);
    let k = ctx.f("intensity") / 256.0;
    let n = ctx.i("samples").clamp(1, 64);
    let mode = ctx.i("mode");
    Ok(vec![Image::generate_uv(ctx.w, ctx.h, img.kind, |u, v| {
        let (mut pu, mut pv) = (u, v);
        let mut acc = img.sample(u, v, ctx.tile);
        let mut cnt = 1.0;
        for _ in 0..n {
            let (x, y) = ((pu * ctx.w as f32) as i64, (pv * ctx.h as f32) as i64);
            let (dx, dy) = ops::slope(&slope, x, y, ctx.tile);
            let l = (dx * dx + dy * dy).sqrt().max(1e-6);
            pu -= dx / l * k / n as f32 * 4.0;
            pv -= dy / l * k / n as f32 * 4.0;
            let p = img.sample(pu, pv, ctx.tile);
            for j in 0..4 {
                acc[j] = match mode {
                    1 => acc[j].min(p[j]),
                    2 => acc[j].max(p[j]),
                    _ => acc[j] + p[j],
                };
            }
            cnt += 1.0;
        }
        if mode == 0 { acc.map(|a| a / cnt) } else { acc }
    })])
}

fn ambient_occlusion(ctx: &Ctx) -> CookResult {
    let h = ctx.gray_or(0, 0.0);
    let spread = ctx.f("spread").max(0.001) * ctx.w as f32 * 0.25;
    let depth = ctx.f("intensity");
    let wide = ops::gaussian(&h, spread, ctx.tile);
    let mid = ops::gaussian(&h, spread * 0.35, ctx.tile);
    Ok(vec![Image::generate(ctx.w, ctx.h, Kind::Gray, |x, y| {
        let i = (y * ctx.w + x) as usize;
        let occ = ((wide.data[i] - h.data[i]).max(0.0) * 0.6 + (mid.data[i] - h.data[i]).max(0.0) * 0.4) * depth * 6.0;
        [(1.0 - occ).clamp(0.0, 1.0), 0.0, 0.0, 0.0]
    })])
}

fn bevel(ctx: &Ctx) -> CookResult {
    let m = ctx.gray_or(0, 0.0);
    let d = ctx.f("distance");
    let dist_px = (d.abs() * ctx.w as f32).max(1e-3);
    let data = m.data.clone();
    let outside = ops::distance_sq(ctx.w, ctx.h, |i| data[i] > 0.5, ctx.tile);
    let inside = ops::distance_sq(ctx.w, ctx.h, |i| data[i] <= 0.5, ctx.tile);
    let v: Vec<f32> = (0..outside.len())
        .map(|i| {
            if d >= 0.0 {
                // Inner bevel: ramp up from the edge inward.
                if data[i] > 0.5 { (inside[i].sqrt() / dist_px).min(1.0) } else { 0.0 }
            } else if data[i] > 0.5 {
                1.0
            } else {
                (1.0 - outside[i].sqrt() / dist_px).max(0.0)
            }
        })
        .collect();
    let img = ops::gray_from(ctx.w, ctx.h, v);
    let s = ctx.f("smoothing") * ctx.w as f32 / 128.0;
    Ok(vec![ops::gaussian(&img.map(Kind::Gray, |p| [(p[0] * std::f32::consts::FRAC_PI_2).sin(), 0.0, 0.0, 0.0]), s, ctx.tile)])
}

fn curvature(ctx: &Ctx) -> CookResult {
    let n = ctx.any_or(0, Kind::Color, [0.5, 0.5, 1.0, 1.0]).to_color();
    let k = ctx.f("intensity") * 2.0;
    Ok(vec![Image::generate(ctx.w, ctx.h, Kind::Gray, |x, y| {
        let (x, y) = (x as i64, y as i64);
        let dx = n.get(x + 1, y, ctx.tile)[0] - n.get(x - 1, y, ctx.tile)[0];
        let dy = n.get(x, y + 1, ctx.tile)[1] - n.get(x, y - 1, ctx.tile)[1];
        [(0.5 + (dx - dy) * k).clamp(0.0, 1.0), 0.0, 0.0, 0.0]
    })])
}

fn morph(ctx: &Ctx, max: bool) -> CookResult {
    let img = src(ctx);
    let r = ctx.i("radius").clamp(0, 64) as i64;
    if r == 0 {
        return Ok(vec![img]);
    }
    // Separable square window: rows then columns.
    let pass = |src: &Image, horizontal: bool| {
        Image::generate(src.w, src.h, src.kind, |x, y| {
            let mut acc = src.get(x as i64, y as i64, ctx.tile);
            for o in -r..=r {
                let p = if horizontal { src.get(x as i64 + o, y as i64, ctx.tile) } else { src.get(x as i64, y as i64 + o, ctx.tile) };
                for k in 0..4 {
                    acc[k] = if max { acc[k].max(p[k]) } else { acc[k].min(p[k]) };
                }
            }
            acc
        })
    };
    Ok(vec![pass(&pass(&img, true), false)])
}

fn dilation(ctx: &Ctx) -> CookResult {
    morph(ctx, true)
}

fn erosion(ctx: &Ctx) -> CookResult {
    morph(ctx, false)
}

fn edge_detect(ctx: &Ctx) -> CookResult {
    let m = ctx.gray_or(0, 0.0);
    let w = (ctx.f("width") * ctx.w as f32 / 256.0).max(0.5);
    let data = m.data.clone();
    let a = ops::distance_sq(ctx.w, ctx.h, |i| data[i] > 0.5, ctx.tile);
    let b = ops::distance_sq(ctx.w, ctx.h, |i| data[i] <= 0.5, ctx.tile);
    let round = ctx.f("roundness");
    let v: Vec<f32> = a
        .iter()
        .zip(&b)
        .map(|(a, b)| {
            let d = a.sqrt().max(b.sqrt());
            let t = (1.0 - d / w).max(0.0);
            t.powf(1.0 - round * 0.8)
        })
        .collect();
    Ok(vec![ops::gray_from(ctx.w, ctx.h, v)])
}

fn glow(ctx: &Ctx) -> CookResult {
    let img = src(ctx);
    let (t, amt) = (ctx.f("threshold"), ctx.f("amount"));
    let bright = img.map(img.kind, |p| {
        let l = luminance(p);
        let k = if l > t { 1.0 } else { 0.0 };
        [p[0] * k, p[1] * k, p[2] * k, p[3]]
    });
    let b = ops::gaussian(&bright, ctx.f("size") * ctx.w as f32 / 256.0, ctx.tile);
    Ok(vec![Image::generate(ctx.w, ctx.h, img.kind, |x, y| {
        let (p, g) = (img.get(x as i64, y as i64, false), b.get(x as i64, y as i64, false));
        [p[0] + g[0] * amt, p[1] + g[1] * amt, p[2] + g[2] * amt, p[3]]
    })])
}

fn highpass(ctx: &Ctx) -> CookResult {
    let img = src(ctx);
    let b = ops::gaussian(&img, ctx.f("radius") * ctx.w as f32 / 256.0, ctx.tile);
    Ok(vec![Image::generate(ctx.w, ctx.h, img.kind, |x, y| {
        let (p, q) = (img.get(x as i64, y as i64, false), b.get(x as i64, y as i64, false));
        [p[0] - q[0] + 0.5, p[1] - q[1] + 0.5, p[2] - q[2] + 0.5, p[3]]
    })])
}

fn mosaic(ctx: &Ctx) -> CookResult {
    let img = src(ctx);
    let n = ctx.i("cells").max(1) as f32;
    Ok(vec![Image::generate_uv(ctx.w, ctx.h, img.kind, |u, v| img.sample(((u * n).floor() + 0.5) / n, ((v * n).floor() + 0.5) / n, ctx.tile))])
}

fn swirl(ctx: &Ctx) -> CookResult {
    let img = src(ctx);
    let (amt, rad) = (ctx.f("amount") * TAU, ctx.f("radius").max(1e-3));
    Ok(vec![Image::generate_uv(ctx.w, ctx.h, img.kind, |u, v| {
        let (x, y) = (u - 0.5, v - 0.5);
        let r = (x * x + y * y).sqrt();
        let t = (1.0 - r / rad).max(0.0);
        let a = amt * t * t;
        let (s, c) = a.sin_cos();
        img.sample(x * c - y * s + 0.5, x * s + y * c + 0.5, ctx.tile)
    })])
}

fn vignette(ctx: &Ctx) -> CookResult {
    let img = src(ctx);
    let (amt, soft) = (ctx.f("amount"), ctx.f("softness").max(0.01));
    Ok(vec![Image::generate_uv(ctx.w, ctx.h, img.kind, |u, v| {
        let p = img.sample_nearest(u, v, false);
        let r = ((u - 0.5).powi(2) + (v - 0.5).powi(2)).sqrt() * std::f32::consts::SQRT_2;
        let k = 1.0 - amt * ((r - (1.0 - soft)) / soft).clamp(0.0, 1.0);
        [p[0] * k, p[1] * k, p[2] * k, p[3]]
    })])
}

fn quantize(ctx: &Ctx) -> CookResult {
    let n = (ctx.i("steps").max(2) - 1) as f32;
    let s = ctx.f("smoothness").clamp(0.0, 1.0) * 0.5;
    let img = ctx.gray_or(0, 0.0);
    Ok(vec![img.map(Kind::Gray, |p| {
        let x = p[0] * n;
        let f = x.fract();
        let t = if s > 0.0 { ((f - 0.5 + s) / (2.0 * s)).clamp(0.0, 1.0) } else { (f >= 0.5) as i32 as f32 };
        [(x.floor() + t * t * (3.0 - 2.0 * t)) / n, 0.0, 0.0, 0.0]
    })])
}

fn height_normal(ctx: &Ctx) -> CookResult {
    let h = ctx.gray_or(0, 0.0);
    Ok(vec![height_to_normal(&h, ctx.f("intensity") * ctx.w as f32 / 32.0, ctx.i("format") == 1, ctx.tile, false)])
}

fn unpack(p: [f32; 4]) -> [f32; 3] {
    [p[0] * 2.0 - 1.0, p[1] * 2.0 - 1.0, p[2] * 2.0 - 1.0]
}

fn pack(n: [f32; 3], a: f32) -> [f32; 4] {
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-6);
    [n[0] / l * 0.5 + 0.5, n[1] / l * 0.5 + 0.5, n[2] / l * 0.5 + 0.5, a]
}

fn flat_normal(ctx: &Ctx, i: usize) -> Image {
    ctx.any_or(i, Kind::Color, [0.5, 0.5, 1.0, 1.0]).to_color()
}

fn normal_blend(ctx: &Ctx) -> CookResult {
    let (base, det) = (flat_normal(ctx, 0), flat_normal(ctx, 1));
    let mask = ctx.sized(2).map(|m| m.to_gray());
    let op = ctx.f("opacity");
    Ok(vec![Image::generate(ctx.w, ctx.h, Kind::Color, |x, y| {
        let (x, y) = (x as i64, y as i64);
        let k = op * mask.as_ref().map_or(1.0, |m| m.get_gray(x, y, false));
        let b = unpack(base.get(x, y, false));
        let mut d = unpack(det.get(x, y, false));
        d = [d[0] * k, d[1] * k, 1.0 + (d[2] - 1.0) * k];
        // Reoriented normal mapping (Barré-Brisebois & Hill).
        let t = [b[0], b[1], b[2] + 1.0];
        let u = [-d[0], -d[1], d[2]];
        let dot = t[0] * u[0] + t[1] * u[1] + t[2] * u[2];
        let r = [t[0] * dot / t[2] - u[0], t[1] * dot / t[2] - u[1], t[2] * dot / t[2] - u[2]];
        pack(r, 1.0)
    })])
}

fn normal_invert(ctx: &Ctx) -> CookResult {
    let n = flat_normal(ctx, 0);
    let (r, g) = (ctx.b("red"), ctx.b("green"));
    Ok(vec![n.map(Kind::Color, |p| [if r { 1.0 - p[0] } else { p[0] }, if g { 1.0 - p[1] } else { p[1] }, p[2], p[3]])])
}

fn normal_normalize(ctx: &Ctx) -> CookResult {
    let n = flat_normal(ctx, 0);
    Ok(vec![n.map(Kind::Color, |p| pack(unpack(p), p[3]))])
}

fn normal_intensity(ctx: &Ctx) -> CookResult {
    let n = flat_normal(ctx, 0);
    let k = ctx.f("intensity");
    Ok(vec![n.map(Kind::Color, |p| {
        let v = unpack(p);
        pack([v[0] * k, v[1] * k, v[2].max(1e-3)], p[3])
    })])
}

fn normal_to_height(ctx: &Ctx) -> CookResult {
    // Integrate the slopes at several scales (a multigrid-style Poisson solve: coarse levels set
    // the broad shape, finer ones the detail).
    let n = flat_normal(ctx, 0);
    let (w, h) = (ctx.w, ctx.h);
    let gx = n.map(Kind::Gray, |p| {
        let v = unpack(p);
        [-v[0] / v[2].max(0.1), 0.0, 0.0, 0.0]
    });
    let gy = n.map(Kind::Gray, |p| {
        let v = unpack(p);
        [v[1] / v[2].max(0.1), 0.0, 0.0, 0.0]
    });
    let mut height = Image::filled(8.min(w), 8.min(h), Kind::Gray, [0.0; 4]);
    let mut size = 8u32;
    loop {
        let (sw, sh) = (size.min(w), size.min(h));
        let (gxs, gys) = (gx.resize(sw, sh, true), gy.resize(sw, sh, true));
        let scale = w as f32 / sw as f32;
        let mut cur = height.resize(sw, sh, true);
        for _ in 0..40 {
            let prev = cur.clone();
            cur = Image::generate(sw, sh, Kind::Gray, |x, y| {
                let (x, y) = (x as i64, y as i64);
                let nb = prev.get(x - 1, y, true)[0] + prev.get(x + 1, y, true)[0] + prev.get(x, y - 1, true)[0] + prev.get(x, y + 1, true)[0];
                // Divergence of the gradient field (pixels of this level).
                let div = (gxs.get(x + 1, y, true)[0] - gxs.get(x - 1, y, true)[0] + gys.get(x, y + 1, true)[0] - gys.get(x, y - 1, true)[0]) * 0.5 / scale;
                [(nb - div) * 0.25, 0.0, 0.0, 0.0]
            });
        }
        height = cur;
        if sw >= w && sh >= h {
            break;
        }
        size *= 2;
    }
    let (lo, hi) = height.range();
    let k = ctx.f("intensity");
    let span = (hi - lo).max(1e-6);
    Ok(vec![height.map(Kind::Gray, |p| [(((p[0] - lo) / span - 0.5) * k + 0.5).clamp(0.0, 1.0), 0.0, 0.0, 0.0])])
}

fn make_it_tile(ctx: &Ctx) -> CookResult {
    let img = src(ctx);
    let b = ctx.f("blend").clamp(0.01, 0.5);
    // Blend the image with a half-offset copy near the borders.
    Ok(vec![Image::generate_uv(ctx.w, ctx.h, img.kind, |u, v| {
        let p = img.sample(u, v, true);
        let q = img.sample(u + 0.5, v + 0.5, true);
        let edge = (u.min(1.0 - u)).min(v.min(1.0 - v));
        let t = (edge / b).clamp(0.0, 1.0);
        let t = t * t * (3.0 - 2.0 * t);
        [q[0] + (p[0] - q[0]) * t, q[1] + (p[1] - q[1]) * t, q[2] + (p[2] - q[2]) * t, q[3] + (p[3] - q[3]) * t]
    })])
}

fn safe_transform(ctx: &Ctx) -> CookResult {
    let img = src(ctx);
    let n = ctx.i("tile").max(1) as f32;
    let off = ctx.v2("offset");
    let (rot, flip) = (ctx.i("rotation"), ctx.b("flip"));
    Ok(vec![Image::generate_uv(ctx.w, ctx.h, img.kind, |u, v| {
        let (mut x, mut y) = ((u * n - off[0]).rem_euclid(1.0), (v * n - off[1]).rem_euclid(1.0));
        for _ in 0..rot.rem_euclid(4) {
            (x, y) = (y, 1.0 - x);
        }
        if flip {
            x = 1.0 - x;
        }
        img.sample(x, y, true)
    })])
}

fn mirror(ctx: &Ctx) -> CookResult {
    let img = src(ctx);
    let (axis, flip) = (ctx.i("axis"), ctx.b("flip"));
    let m = |t: f32| {
        if flip {
            if t < 0.5 { 1.0 - t } else { t }
        } else if t > 0.5 {
            1.0 - t
        } else {
            t
        }
    };
    Ok(vec![Image::generate_uv(ctx.w, ctx.h, img.kind, |u, v| match axis {
        1 => img.sample(u, m(v), ctx.tile),
        2 => img.sample(m(u), m(v), ctx.tile),
        _ => img.sample(m(u), v, ctx.tile),
    })])
}

fn kaleidoscope(ctx: &Ctx) -> CookResult {
    let img = src(ctx);
    let seg = TAU / ctx.i("segments").clamp(2, 64) as f32;
    let a0 = ctx.f("angle") * TAU;
    Ok(vec![Image::generate_uv(ctx.w, ctx.h, img.kind, |u, v| {
        let (x, y) = (u - 0.5, v - 0.5);
        let r = (x * x + y * y).sqrt();
        let mut a = (y.atan2(x) - a0).rem_euclid(seg);
        if a > seg * 0.5 {
            a = seg - a;
        }
        let a = a + a0;
        img.sample(0.5 + r * a.cos(), 0.5 + r * a.sin(), ctx.tile)
    })])
}

fn cartesian_to_polar(ctx: &Ctx) -> CookResult {
    let img = src(ctx);
    Ok(vec![Image::generate_uv(ctx.w, ctx.h, img.kind, |u, v| {
        let (x, y) = (u - 0.5, v - 0.5);
        let r = (x * x + y * y).sqrt() * 2.0;
        let a = (y.atan2(x) / TAU).rem_euclid(1.0);
        img.sample(a, r, true)
    })])
}

fn polar_to_cartesian(ctx: &Ctx) -> CookResult {
    let img = src(ctx);
    Ok(vec![Image::generate_uv(ctx.w, ctx.h, img.kind, |u, v| {
        let (a, r) = (u * TAU, v * 0.5);
        img.sample(0.5 + r * a.cos(), 0.5 + r * a.sin(), ctx.tile)
    })])
}

fn skew(ctx: &Ctx) -> CookResult {
    let img = src(ctx);
    let (k, vert) = (ctx.f("amount"), ctx.b("vertical"));
    Ok(vec![Image::generate_uv(ctx.w, ctx.h, img.kind, |u, v| {
        if vert { img.sample(u, v + (u - 0.5) * k, ctx.tile) } else { img.sample(u + (v - 0.5) * k, v, ctx.tile) }
    })])
}
