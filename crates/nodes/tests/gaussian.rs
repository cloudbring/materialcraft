use std::collections::BTreeMap;
use std::hint::black_box;
use std::time::Instant;

use craft_blur::{Cancel, Edge, Gaussian, Quality, Scratch, Shape};
use materialcraft_core::Value;
use materialcraft_image::{Image, Kind};
use materialcraft_nodes::{Ctx, catalog, find, ops, resolve_params};

fn cook(id: &str, image: &Image, tile: bool, overrides: BTreeMap<String, Value>) -> Vec<Image> {
    let definition = find(id).expect("node exists");
    let params = resolve_params(definition, &overrides);
    let inputs = vec![Some(image.clone()); definition.inputs.len()];
    let vars = BTreeMap::new();
    let resource = |_: &str| None;
    let context = Ctx { w: image.w, h: image.h, seed: 7, tile, inputs: &inputs, params: &params, resource: &resource, vars: &vars };
    (definition.cook)(&context).expect("node cooks")
}

fn variance(image: &Image, horizontal: bool) -> f64 {
    let mut mass = 0.0;
    let mut first = 0.0;
    let mut second = 0.0;
    for row in 0..image.h {
        for column in 0..image.w {
            let value = f64::from(image.get_gray(i64::from(column), i64::from(row), false));
            let position = if horizontal { column } else { row } as f64;
            mass += value;
            first += value * position;
            second += value * position * position;
        }
    }
    second / mass - (first / mass).powi(2)
}

#[test]
fn clamp_matches_shared_two_dimensional_filter() {
    for kind in [Kind::Gray, Kind::Color] {
        let image = Image::generate(37, 29, kind, |column, row| {
            let value = ((column * 17 + row * 31) % 13) as f32 / 12.0;
            [value, value * 2.0 - 0.5, 1.0 - value, value * 0.25]
        });
        for sigma in [1.0, 4.0, 16.0] {
            let mut reference = image.data.as_ref().clone();
            Gaussian::new(sigma, Quality::Recursive)
                .expect("valid sigma")
                .apply(
                    &mut reference,
                    Shape::new(image.w as usize, image.h as usize, image.channels()).expect("valid shape"),
                    Edge::Clamp,
                    &mut Scratch::default(),
                    Cancel::Never,
                )
                .expect("shared filter succeeds");
            let output = ops::gaussian(&image, sigma, false);
            assert!(reference.iter().zip(output.data.iter()).all(|(expected, actual)| (expected - actual).abs() < 1e-6));
        }
    }
}

#[test]
fn gaussian_impulse_variance_is_calibrated_on_both_axes() {
    for sigma in [1.0, 4.0, 16.0] {
        for tile in [false, true] {
            for horizontal in [false, true] {
                let (width, height) = if horizontal { (1025, 1) } else { (1, 1025) };
                let impulse = Image::gray(width, height, |column, row| f32::from(column == width / 2 && row == height / 2));
                let output = ops::gaussian(&impulse, sigma, tile);
                let error = variance(&output, horizontal) / f64::from(sigma).powi(2) - 1.0;
                assert!(error.abs() <= 0.005, "sigma={sigma}, tile={tile}, horizontal={horizontal}: {error}");
                assert!((output.data.iter().map(|value| f64::from(*value)).sum::<f64>() - 1.0).abs() < 1e-6);
            }
        }
    }
}

#[test]
fn anisotropic_node_has_independent_calibrated_variances() {
    let impulse = Image::gray(513, 513, |column, row| f32::from(column == 256 && row == 256));
    for sigma in [1.0, 4.0, 16.0] {
        for tile in [false, true] {
            let output = cook(
                "anisotropic_blur",
                &impulse,
                tile,
                BTreeMap::from([("intensity".into(), Value::Float(sigma * 512.0 / 513.0)), ("anisotropy".into(), Value::Float(0.5))]),
            );
            for (horizontal, target) in [(true, sigma), (false, sigma * 0.5)] {
                let error = variance(&output[0], horizontal) / f64::from(target).powi(2) - 1.0;
                assert!(error.abs() <= 0.005, "sigma={sigma}, tile={tile}, horizontal={horizontal}: {error}");
            }
        }
    }
}

#[test]
fn tiled_gaussian_has_equal_symmetric_seam_columns_and_rows() {
    for kind in [Kind::Gray, Kind::Color] {
        let image = Image::generate(31, 23, kind, |column, row| {
            let value = f32::from(column == 0 || column == 30) + f32::from(row == 0 || row == 22);
            [value, value * 0.5 - 0.2, value * 2.0, value * 0.25]
        });
        for sigma in [1.0, 4.0, 16.0, 64.0] {
            for output in [ops::gaussian(&image, sigma, true), ops::gaussian_xy(&image, sigma, sigma * 0.5, true)] {
                for row in 0..image.h {
                    let first = output.get(0, i64::from(row), false);
                    let last = output.get(i64::from(image.w) - 1, i64::from(row), false);
                    assert!(first.iter().zip(last).all(|(first, last)| (first - last).abs() <= 1e-6), "x seam: {sigma}");
                }
                for column in 0..image.w {
                    let first = output.get(i64::from(column), 0, false);
                    let last = output.get(i64::from(column), i64::from(image.h) - 1, false);
                    assert!(first.iter().zip(last).all(|(first, last)| (first - last).abs() <= 1e-6), "y seam: {sigma}");
                }
            }
        }
    }
}

#[test]
fn tiled_gaussian_commutes_with_wrapped_shifts_and_repeated_tiles() {
    for (width, height) in [(1, 1), (1, 7), (9, 1), (3, 5), (31, 23)] {
        let image = Image::generate(width, height, Kind::Color, |column, row| {
            let value = ((column * 17 + row * 31) % 13) as f32 / 12.0;
            [value, value * 2.0 - 0.5, 1.0 - value, value * 0.25]
        });
        let shifted = Image::generate(width, height, image.kind, |column, row| image.get(i64::from(column) + 3, i64::from(row) - 2, true));
        let repeated = Image::generate(width * 3, height * 3, image.kind, |column, row| image.get(i64::from(column), i64::from(row), true));
        for sigma in [1.0, 4.0, 16.0] {
            let sigma = f32::min(sigma, width.max(height) as f32);
            let output = ops::gaussian(&image, sigma, true);
            let shifted_output = ops::gaussian(&shifted, sigma, true);
            let repeated_output = ops::gaussian(&repeated, sigma, true);
            for row in 0..height {
                for column in 0..width {
                    let expected = output.get(i64::from(column) + 3, i64::from(row) - 2, true);
                    let actual = shifted_output.get(i64::from(column), i64::from(row), false);
                    assert!(expected.iter().zip(actual).all(|(expected, actual)| (expected - actual).abs() < 1e-6));
                    let expected = output.get(i64::from(column), i64::from(row), false);
                    let actual = repeated_output.get(i64::from(column + width), i64::from(row + height), false);
                    assert!(expected.iter().zip(actual).all(|(expected, actual)| (expected - actual).abs() < 1e-6));
                }
            }
        }
    }
}

#[test]
fn gaussian_preserves_constants_channels_and_noop_inputs() {
    let image = Image::filled(19, 13, Kind::Color, [-0.25, 0.4, 2.0, 0.125]);
    for tile in [false, true] {
        for sigma in [1.0, 4.0, 16.0] {
            let output = ops::gaussian(&image, sigma, tile);
            assert!(image.data.iter().zip(output.data.iter()).all(|(before, after)| (before - after).abs() < 1e-6));
        }
        for sigma in [0.0, -1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(ops::gaussian(&image, sigma, tile), image);
        }
    }
}

#[test]
fn blur_sharpen_and_unsharp_use_the_shared_gaussian() {
    let image = Image::gray(33, 29, |column, row| ((column * 17 + row * 31) % 13) as f32 / 12.0);
    for tile in [false, true] {
        let expected = ops::gaussian(&image, 4.0, tile);
        let output = cook("blur", &image, tile, BTreeMap::from([("intensity".into(), Value::Float(4.0 * 512.0 / 33.0))]));
        assert_eq!(output[0], expected);
        for (id, sigma, overrides) in [
            ("sharpen", 1.0, BTreeMap::from([("intensity".into(), Value::Float(0.75))])),
            (
                "unsharp_mask",
                4.0,
                BTreeMap::from([
                    ("radius".into(), Value::Float(4.0 * 256.0 / 33.0)),
                    ("amount".into(), Value::Float(0.75)),
                    ("threshold".into(), Value::Float(0.0)),
                ]),
            ),
        ] {
            let blurred = ops::gaussian(&image, sigma, tile);
            let output = cook(id, &image, tile, overrides);
            for ((input, blurred), output) in image.data.iter().zip(blurred.data.iter()).zip(output[0].data.iter()) {
                assert!((output - (input + (input - blurred) * 0.75)).abs() < 1e-6, "{id}");
            }
        }
    }
}

#[test]
#[ignore = "numerical audit and 2048-square release benchmark"]
fn gaussian_measurements() {
    for sigma in [1.0, 4.0, 16.0] {
        let impulse = Image::gray(1025, 1, |column, _| f32::from(column == 512));
        for tile in [false, true] {
            let isotropic = ops::gaussian(&impulse, sigma, tile);
            let anisotropic = cook(
                "anisotropic_blur",
                &impulse,
                tile,
                BTreeMap::from([("intensity".into(), Value::Float(sigma * 512.0 / 1025.0)), ("anisotropy".into(), Value::Float(1.0))]),
            );
            for (name, output) in [("gaussian", &isotropic), ("anisotropic", &anisotropic[0])] {
                let measured = variance(output, true);
                println!("VARIANCE {name} tile={tile} sigma={sigma} variance={measured:.9} error={:.6}%", (measured / f64::from(sigma).powi(2) - 1.0) * 100.0);
            }
        }
    }
    let image = Image::gray(2048, 2048, |column, row| ((column * 17 + row * 31) % 257) as f32 / 256.0);
    for tile in [false, true] {
        for sigma in [2.0, 16.0, 64.0] {
            black_box(ops::gaussian(&image, sigma, tile));
            let mut times: Vec<f64> = (0..5)
                .map(|_| {
                    let started = Instant::now();
                    black_box(ops::gaussian(black_box(&image), sigma, tile));
                    started.elapsed().as_secs_f64() * 1000.0
                })
                .collect();
            times.sort_by(f64::total_cmp);
            println!("BENCH gray 2048x2048 tile={tile} sigma={sigma} median_ms={:.3} samples={times:?}", times[2]);
        }
    }
}

#[test]
#[ignore = "compare all node outputs against a pre-migration capture"]
fn gaussian_fixture_audit() {
    let directory = std::env::var("MC_BLUR_FIXTURES").expect("MC_BLUR_FIXTURES");
    std::fs::create_dir_all(&directory).expect("create audit directory");
    let mut changed = BTreeMap::<String, f32>::new();
    let mut cases = 0;
    for kind in [Kind::Gray, Kind::Color] {
        let image = Image::generate(32, 32, kind, |column, row| {
            let first = ((column * 17 + row * 31) % 257) as f32 / 256.0;
            [first, column as f32 / 31.0, row as f32 / 31.0, 0.25 + first * 0.75]
        });
        for tile in [false, true] {
            for definition in catalog().iter().filter(|definition| definition.id != "graph_instance") {
                for (index, output) in cook(definition.id, &image, tile, BTreeMap::new()).iter().enumerate() {
                    let path = format!("{directory}/{}-{kind:?}-{tile}-{index}.f32", definition.id);
                    if let Ok(before) = std::fs::read(&path) {
                        assert_eq!(before.len(), output.data.len() * 4);
                        let delta = before
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .zip(output.data.iter())
                            .fold(0.0f32, |delta, (bytes, after)| delta.max((f32::from_le_bytes(*bytes) - after).abs()));
                        if delta > 0.0 {
                            changed.entry(definition.id.into()).and_modify(|maximum| *maximum = maximum.max(delta)).or_insert(delta);
                        }
                    } else {
                        let bytes: Vec<u8> = output.data.iter().flat_map(|sample| sample.to_le_bytes()).collect();
                        std::fs::write(path, bytes).expect("capture baseline");
                    }
                    cases += 1;
                }
            }
        }
    }
    println!("AUDIT {cases} outputs; {} changed nodes", changed.len());
    for (id, delta) in changed {
        println!("FIXTURE {id} max_delta={delta:.9}");
    }
}
