//! # materialcraft-formats
//!
//! File formats, L2: the native `.mcraft` package (a zip holding `package.json` and the embedded
//! `resources/`), bitmap decoding for Bitmap nodes, and texture export (PNG 8/16, JPEG, TGA,
//! TIFF 8/16, BMP, WEBP, EXR 32F, HDR) with the output naming pattern, and meshes for the 3D view
//! (OBJ, glTF/GLB), and portable materials (`.mcmat`, see [`material`]). Byte-based APIs work
//! everywhere, including the browser; path helpers are native only.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod material;
pub mod mesh;

use std::io::{Cursor, Read, Write};
use std::sync::Arc;

use materialcraft_core::{OutputFormat, Package};
use materialcraft_image::{Image, Kind, MAX_SIDE, linear_to_srgb, srgb_to_linear};

#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    #[error("not a MaterialCraft package: {0}")]
    BadPackage(String),
    #[error("image: {0}")]
    Image(String),
    #[error("I/O: {0}")]
    Io(String),
}

pub type Result<T> = std::result::Result<T, FormatError>;

/// Largest `package.json` and resource accepted when reading (allocation caps).
const MAX_JSON: u64 = 64 << 20;
const MAX_RESOURCE: u64 = 256 << 20;

/// Export file types: (extension, label).
pub const EXPORT_FORMATS: &[(&str, &str)] =
    &[("png", "PNG"), ("jpg", "JPEG"), ("tga", "TGA"), ("tif", "TIFF"), ("bmp", "BMP"), ("webp", "WEBP"), ("exr", "OpenEXR"), ("hdr", "Radiance HDR")];

/// Image file extensions [`decode`] reads.
pub const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "tga", "tif", "tiff", "bmp", "webp", "exr", "hdr"];

/// Write a package to `.mcraft` bytes.
pub fn save_package(pkg: &Package) -> Result<Vec<u8>> {
    let json = serde_json::to_vec_pretty(pkg).map_err(|e| FormatError::Io(e.to_string()))?;
    let mut buf = Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let io = |e: zip::result::ZipError| FormatError::Io(e.to_string());
        z.start_file("package.json", opts).map_err(io)?;
        z.write_all(&json).map_err(|e| FormatError::Io(e.to_string()))?;
        for r in &pkg.resources {
            // Images are already compressed.
            let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            z.start_file(format!("resources/{}", safe_file_name(&r.file)), stored).map_err(io)?;
            z.write_all(&r.bytes).map_err(|e| FormatError::Io(e.to_string()))?;
        }
        z.finish().map_err(io)?;
    }
    Ok(buf.into_inner())
}

/// Read a package from `.mcraft` bytes.
pub fn load_package(bytes: &[u8]) -> Result<Package> {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| FormatError::BadPackage(e.to_string()))?;
    let mut json = Vec::new();
    z.by_name("package.json")
        .map_err(|_| FormatError::BadPackage("package.json is missing".into()))?
        .take(MAX_JSON)
        .read_to_end(&mut json)
        .map_err(|e| FormatError::Io(e.to_string()))?;
    let mut pkg: Package = serde_json::from_slice(&json).map_err(|e| FormatError::BadPackage(e.to_string()))?;
    for r in &mut pkg.resources {
        let name = format!("resources/{}", safe_file_name(&r.file));
        let mut data = Vec::new();
        if let Ok(mut f) = z.by_name(&name) {
            f.by_ref().take(MAX_RESOURCE).read_to_end(&mut data).map_err(|e| FormatError::Io(e.to_string()))?;
        }
        r.bytes = Arc::new(data);
    }
    Ok(pkg)
}

/// A file name with no path parts (resources live flat in `resources/`).
pub fn safe_file_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let cleaned: String = base.chars().map(|c| if c.is_alphanumeric() || "._- ".contains(c) { c } else { '_' }).collect();
    let cleaned = cleaned.trim_start_matches('.').to_string();
    if cleaned.is_empty() { "resource".into() } else { cleaned }
}

/// Decode image file bytes into a linear float image (8-bit color is sRGB-decoded; grayscale
/// files stay grayscale).
pub fn decode(bytes: &[u8]) -> Result<Image> {
    // TGA has no signature: try it when the format cannot be guessed.
    let img = image::load_from_memory(bytes)
        .or_else(|e| image::load_from_memory_with_format(bytes, image::ImageFormat::Tga).map_err(|_| e))
        .map_err(|e| FormatError::Image(e.to_string()))?;
    let (w, h) = (img.width(), img.height());
    if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
        return Err(FormatError::Image(format!("{w}x{h} is outside 1..={MAX_SIDE} pixels")));
    }
    let gray = matches!(img.color(), image::ColorType::L8 | image::ColorType::L16 | image::ColorType::La8 | image::ColorType::La16);
    let float = matches!(img.color(), image::ColorType::Rgb32F | image::ColorType::Rgba32F);
    if gray {
        let l = img.into_luma16();
        let data: Vec<f32> = l.pixels().map(|p| p.0[0] as f32 / 65535.0).collect();
        return Ok(Image { w, h, kind: Kind::Gray, data: Arc::new(data) });
    }
    let rgba = img.into_rgba32f();
    let data: Vec<f32> = rgba
        .pixels()
        .flat_map(|p| {
            let [r, g, b, a] = p.0;
            if float { [r, g, b, a] } else { [srgb_to_linear(r), srgb_to_linear(g), srgb_to_linear(b), a] }
        })
        .collect();
    Ok(Image { w, h, kind: Kind::Color, data: Arc::new(data) })
}

/// Bit depth for export.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Depth {
    U8,
    U16,
    F32,
}

impl Depth {
    pub fn from_format(f: OutputFormat) -> Depth {
        match f {
            OutputFormat::U8 => Depth::U8,
            OutputFormat::F16 | OutputFormat::F32 => Depth::F32,
            _ => Depth::U16,
        }
    }
}

/// Encode `img` as `ext` (see [`EXPORT_FORMATS`]). `srgb`: color values are written through the
/// sRGB curve (base color); data maps (normal, roughness…) are written linear.
pub fn encode(img: &Image, ext: &str, depth: Depth, srgb: bool) -> Result<Vec<u8>> {
    let ext = ext.trim_start_matches('.').to_ascii_lowercase();
    let enc = |v: f32| if srgb && img.kind == Kind::Color { linear_to_srgb(v) } else { v };
    let (w, h) = (img.w, img.h);
    let color = img.kind == Kind::Color;
    let px = |i: usize| -> [f32; 4] {
        if color {
            let s = img.data.get(i * 4..i * 4 + 4).unwrap_or(&[0.0, 0.0, 0.0, 1.0]);
            [enc(s[0]), enc(s[1]), enc(s[2]), s[3]]
        } else {
            let v = img.data.get(i).copied().unwrap_or(0.0);
            [v, v, v, 1.0]
        }
    };
    let n = (w * h) as usize;
    let fmt = match ext.as_str() {
        "png" => image::ImageFormat::Png,
        "jpg" | "jpeg" => image::ImageFormat::Jpeg,
        "tga" => image::ImageFormat::Tga,
        "tif" | "tiff" => image::ImageFormat::Tiff,
        "bmp" => image::ImageFormat::Bmp,
        "webp" => image::ImageFormat::WebP,
        "exr" => image::ImageFormat::OpenExr,
        "hdr" => image::ImageFormat::Hdr,
        other => return Err(FormatError::Image(format!("unsupported export format `{other}`"))),
    };
    let float_only = matches!(fmt, image::ImageFormat::OpenExr | image::ImageFormat::Hdr);
    let sixteen_ok = matches!(fmt, image::ImageFormat::Png | image::ImageFormat::Tiff);
    let q8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    let q16 = |v: f32| (v.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16;
    let dynimg: image::DynamicImage = if float_only {
        let data: Vec<f32> = (0..n)
            .flat_map(|i| {
                let p = px(i);
                [p[0], p[1], p[2]]
            })
            .collect();
        let buf = image::Rgb32FImage::from_raw(w, h, data).ok_or_else(|| FormatError::Image("bad buffer".into()))?;
        buf.into()
    } else if depth != Depth::U8 && sixteen_ok {
        if color {
            let data: Vec<u16> = (0..n).flat_map(|i| px(i).map(q16)).collect();
            image::ImageBuffer::<image::Rgba<u16>, _>::from_raw(w, h, data).ok_or_else(|| FormatError::Image("bad buffer".into()))?.into()
        } else {
            let data: Vec<u16> = (0..n).map(|i| q16(px(i)[0])).collect();
            image::ImageBuffer::<image::Luma<u16>, _>::from_raw(w, h, data).ok_or_else(|| FormatError::Image("bad buffer".into()))?.into()
        }
    } else if color && !matches!(fmt, image::ImageFormat::Jpeg) {
        let data: Vec<u8> = (0..n).flat_map(|i| px(i).map(q8)).collect();
        image::RgbaImage::from_raw(w, h, data).ok_or_else(|| FormatError::Image("bad buffer".into()))?.into()
    } else if color {
        let data: Vec<u8> = (0..n)
            .flat_map(|i| {
                let p = px(i);
                [q8(p[0]), q8(p[1]), q8(p[2])]
            })
            .collect();
        image::RgbImage::from_raw(w, h, data).ok_or_else(|| FormatError::Image("bad buffer".into()))?.into()
    } else {
        let data: Vec<u8> = (0..n).map(|i| q8(px(i)[0])).collect();
        image::GrayImage::from_raw(w, h, data).ok_or_else(|| FormatError::Image("bad buffer".into()))?.into()
    };
    let mut out = Cursor::new(Vec::new());
    dynimg.write_to(&mut out, fmt).map_err(|e| FormatError::Image(e.to_string()))?;
    Ok(out.into_inner())
}

/// The file stem of an exported output: `pattern` with `$(graph)`, `$(identifier)` and
/// `$(usage)` replaced, sanitized for file systems.
pub fn output_name(pattern: &str, graph: &str, identifier: &str, usage: &str) -> String {
    let s = pattern.replace("$(graph)", graph).replace("$(identifier)", identifier).replace("$(usage)", usage);
    safe_file_name(&s)
}

/// Write `bytes` atomically using a durable staging file.
#[cfg(not(target_arch = "wasm32"))]
pub fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    craft_atomic::atomic_write(path, bytes).map_err(|e| FormatError::Io(format!("{}: {e}", path.display())))
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod atomic_tests {
    use std::fs::{File, Permissions};
    use std::io;
    use std::path::Path;
    use std::sync::{Arc, Barrier};

    use craft_atomic::{AtomicFs, NewFilePermissions, RealFs, RenameRetry};

    struct FailSync;

    impl AtomicFs for FailSync {
        fn create_new(&self, path: &Path, permissions: NewFilePermissions) -> io::Result<File> {
            RealFs.create_new(path, permissions)
        }
        fn sync(&self, _file: &File) -> io::Result<()> {
            Err(io::Error::other("injected sync failure"))
        }
        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            RealFs.rename(from, to)
        }
        fn hard_link(&self, from: &Path, to: &Path) -> io::Result<()> {
            RealFs.hard_link(from, to)
        }
        fn remove(&self, path: &Path) -> io::Result<()> {
            RealFs.remove(path)
        }
        fn sync_dir(&self, dir: &Path) -> io::Result<()> {
            RealFs.sync_dir(dir)
        }
        fn set_permissions(&self, file: &File, permissions: Permissions) -> io::Result<()> {
            RealFs.set_permissions(file, permissions)
        }
    }

    fn directory(name: &str) -> io::Result<std::path::PathBuf> {
        let dir = std::env::temp_dir().join(format!("materialcraft-atomic-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    #[test]
    fn sync_failure_preserves_old_file() {
        let dir = directory("failure").unwrap();
        let path = dir.join("save");
        let old = vec![1; 256 * 1024];
        let new = vec![2; 256 * 1024];
        super::write_atomic(&path, &old).unwrap();
        assert!(craft_atomic::atomic_write_custom(&FailSync, RenameRetry::platform(), &path, &new).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), old);
        super::write_atomic(&path, &new).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), new);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn concurrent_saves_are_complete() {
        let dir = directory("concurrent").unwrap();
        let path = dir.join("save");
        let barrier = Arc::new(Barrier::new(2));
        let writers: Vec<_> = [1, 2]
            .into_iter()
            .map(|byte| {
                let path = path.clone();
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    let data = vec![byte; 256 * 1024];
                    barrier.wait();
                    for _ in 0..10 {
                        super::write_atomic(&path, &data).unwrap();
                    }
                })
            })
            .collect();
        for writer in writers {
            writer.join().unwrap();
        }
        let result = std::fs::read(&path).unwrap();
        assert!(result == vec![1; 256 * 1024] || result == vec![2; 256 * 1024]);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use materialcraft_core::{Graph, Resource};

    #[test]
    fn package_round_trip() {
        let mut p = Package::new("Test");
        let mut g = Graph::new("Bricks");
        let a = g.add_node("brick_generator", [0.0, 0.0]);
        let o = g.add_node("output", [200.0, 0.0]);
        g.connect(a, "output", o, "input").unwrap();
        p.graphs.push(g);
        let png = encode(&Image::filled(4, 4, Kind::Color, [1.0, 0.5, 0.0, 1.0]), "png", Depth::U8, true).unwrap();
        p.resources.push(Resource { name: "tex".into(), file: "../../evil/tex.png".into(), bytes: Arc::new(png) });
        let bytes = save_package(&p).unwrap();
        let q = load_package(&bytes).unwrap();
        assert_eq!(q.graphs, p.graphs);
        assert_eq!(q.resources[0].bytes, p.resources[0].bytes);
        let img = decode(&q.resources[0].bytes).unwrap();
        assert!((img.get(0, 0, false)[1] - 0.5).abs() < 0.01);
        assert!(load_package(b"not a zip").is_err());
    }

    #[test]
    fn export_every_format() {
        let img = Image::generate(8, 8, Kind::Color, |x, y| [x as f32 / 7.0, y as f32 / 7.0, 0.5, 1.0]);
        let gray = img.to_gray();
        for (ext, _) in EXPORT_FORMATS {
            for depth in [Depth::U8, Depth::U16, Depth::F32] {
                for i in [&img, &gray] {
                    let b = encode(i, ext, depth, true).unwrap_or_else(|e| panic!("{ext}: {e}"));
                    let back = decode(&b).unwrap_or_else(|e| panic!("{ext} decode: {e}"));
                    assert_eq!((back.w, back.h), (8, 8));
                }
            }
        }
        let png16 = encode(&gray, "png", Depth::U16, false).unwrap();
        assert!((decode(&png16).unwrap().get(7, 0, false)[0] - gray.get(7, 0, false)[0]).abs() < 1e-3);
    }

    #[test]
    fn names_are_safe() {
        assert_eq!(output_name("$(graph)_$(usage)", "Bricks", "out", "baseColor"), "Bricks_baseColor");
        assert_eq!(safe_file_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_file_name(".."), "resource");
    }
}
