use image::{Rgba, RgbaImage};
use serde_json::Value;
use std::{
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kzigsaw-solver-cli"))
        .args(args)
        .output()
        .unwrap()
}

fn solve(input: &Path, output: &Path, extra: &[&str]) -> Output {
    let mut args = vec![
        "solve",
        "--assume-all-pieces-are-same-shape",
        "--assume-all-pieces-are-rectangle",
        "--assume-no-padding",
        "--piece-rectangle-width",
        "4",
        "--piece-rectangle-height",
        "4",
        "--input",
        input.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
    ];
    args.extend_from_slice(extra);
    cli(&args)
}

#[test]
fn validates_arguments_and_displays_help() {
    assert!(cli(&["help"]).status.success());
    assert!(cli(&["solve", "--help"]).status.success());
    assert_eq!(cli(&["solve"]).status.code(), Some(2));
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("input.png");
    RgbaImage::new(8, 8).save(&input).unwrap();
    let output = dir.path().join("output.png");
    assert!(
        !solve(&input, &output, &["--piece-rectangle-width", "0"])
            .status
            .success()
    );
    assert!(
        !solve(&input, &output, &["--input", input.to_str().unwrap()])
            .status
            .success()
    );
    assert!(!output.exists());
    let args = [
        "solve",
        "--piece-rectangle-width",
        "4",
        "--piece-rectangle-height",
        "4",
        "--input",
        input.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
    ];
    assert_eq!(cli(&args).status.code(), Some(2));
}

#[test]
fn multiple_inputs_mapping_json_and_text() {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("input.png");
    let original = RgbaImage::from_fn(8, 8, |x, y| {
        Rgba([(x * 12) as u8, (y * 14) as u8, 0, (x + y) as u8])
    });
    let puzzle = kzigsaw_solver_core::Puzzle::new(&original, 4, 4).unwrap();
    puzzle.render(&[3, 2, 1, 0]).unwrap().save(&input).unwrap();
    let output = dir.path().join("output.png");
    let output2 = dir.path().join("output2.png");
    let result = solve(
        &input,
        &output,
        &[
            "--input",
            input.to_str().unwrap(),
            "--output",
            output2.to_str().unwrap(),
            "--solved-information-output-format",
            "json",
        ],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let json: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(json.as_array().unwrap().len(), 2);
    assert_eq!(json[0]["pieces"][0]["piece_id"], 1);
    assert_eq!(json[0]["pieces"][0]["destination"]["row"], 2);
    assert_eq!(json[0]["pieces"][0]["destination"]["column"], 2);
    assert_eq!(image::open(&output).unwrap().to_rgba8(), original);
    assert_eq!(image::open(&output2).unwrap().to_rgba8(), original);
    let text = solve(&input, &output, &[]);
    assert!(text.status.success());
    assert!(
        String::from_utf8(text.stdout)
            .unwrap()
            .contains("piece 1: (1, 1) -> (2, 2)")
    );
}

#[test]
fn rejects_bad_images_and_validates_all_inputs_before_writing() {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("input.png");
    RgbaImage::new(8, 8).save(&input).unwrap();
    let output = dir.path().join("output.png");
    let bad = dir.path().join("bad.png");
    std::fs::write(&bad, b"broken png").unwrap();
    let output2 = dir.path().join("output2.png");
    let result = solve(
        &input,
        &output,
        &[
            "--input",
            bad.to_str().unwrap(),
            "--output",
            output2.to_str().unwrap(),
        ],
    );
    assert!(!result.status.success());
    assert!(!output.exists());
    assert!(!solve(&input, &input, &[]).status.success());
    assert!(
        !solve(
            &input,
            &output,
            &[
                "--output",
                output.to_str().unwrap(),
                "--input",
                input.to_str().unwrap()
            ]
        )
        .status
        .success()
    );
    RgbaImage::new(7, 8).save(&input).unwrap();
    assert!(!solve(&input, &output, &[]).status.success());
    RgbaImage::new(8, 8).save(&input).unwrap();
    let directory = dir.path().join("directory.png");
    std::fs::create_dir(&directory).unwrap();
    assert!(!solve(&input, &directory, &[]).status.success());
}

#[test]
fn jpeg_input_and_detection_results() {
    let dir = TempDir::new().unwrap();
    let jpeg = dir.path().join("input.jpg");
    image::RgbImage::new(4, 4).save(&jpeg).unwrap();
    assert!(
        solve(&jpeg, &dir.path().join("output.png"), &[])
            .status
            .success()
    );
    let input = dir.path().join("input.png");
    RgbaImage::from_fn(24, 24, |x, y| {
        Rgba([
            if x / 6 % 2 == 0 { 0 } else { 255 },
            if y / 8 % 2 == 0 { 0 } else { 255 },
            0,
            255,
        ])
    })
    .save(&input)
    .unwrap();
    let result = cli(&[
        "detect-edge",
        "--input",
        input.to_str().unwrap(),
        "--output-format",
        "json",
    ]);
    assert!(result.status.success());
    let json: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert!(
        json[0]["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["piece_width"] == 6 && c["piece_height"] == 8)
    );
    RgbaImage::new(24, 24).save(&input).unwrap();
    let result = cli(&[
        "detect-edge",
        "--input",
        input.to_str().unwrap(),
        "--output-format",
        "json",
    ]);
    let json: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert!(json[0]["candidates"].as_array().unwrap().is_empty());
    assert!(json[0]["reason"].is_string());
}

#[test]
fn rounding_requires_explicit_cut_and_preserves_complete_pieces() {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("input.png");
    let output = dir.path().join("output.png");
    for (width, height) in [(5, 4), (4, 7), (7, 6), (4, 4)] {
        let original = RgbaImage::from_fn(width, height, |x, y| {
            Rgba([x as u8, y as u8, 42, (x + y) as u8])
        });
        original.save(&input).unwrap();
        let no_cut = solve(&input, &output, &[]);
        if (width, height) != (4, 4) {
            assert_eq!(no_cut.status.code(), Some(1));
            assert!(
                String::from_utf8_lossy(&no_cut.stderr).contains("--piece-rounding-policy cut")
            );
        } else {
            assert!(no_cut.status.success());
        }
        let result = solve(
            &input,
            &output,
            &[
                "--piece-rounding-policy",
                "cut",
                "--solved-information-output-format",
                "json",
            ],
        );
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            image::open(&output).unwrap().to_rgba8(),
            image::imageops::crop_imm(&original, 0, 0, 4, 4).to_image()
        );
        let json: Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(json[0]["columns"], 1);
        assert_eq!(json[0]["rows"], 1);
        assert_eq!(json[0]["pieces"].as_array().unwrap().len(), 1);
    }
    for (width, height) in [(3, 8), (8, 3)] {
        RgbaImage::new(width, height).save(&input).unwrap();
        let result = solve(&input, &output, &["--piece-rounding-policy", "cut"]);
        assert_eq!(result.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&result.stderr).contains("smaller than a complete piece"));
    }
    assert_eq!(
        solve(&input, &output, &["--piece-rounding-policy", "unknown"])
            .status
            .code(),
        Some(2)
    );
}
