//! Optional integration acceptance test using local vectors and ImageMagick.
use std::{path::PathBuf, process::Command};

#[test]
#[ignore = "requires commons/1 shuffled vectors and ~/.local/bin/magick; run in release mode"]
fn commons_slower_ssim() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixture = root.join("test_vectors/commons/1");
    let magick = PathBuf::from(std::env::var_os("HOME").unwrap()).join(".local/bin/magick");
    assert!(magick.is_file(), "missing {}", magick.display());
    let temporary = tempfile::TempDir::new().unwrap();
    let output_dir = std::env::var_os("KZIGSAW_TEST_OUTPUT_DIR")
        .map(|path| root.join(path))
        .unwrap_or_else(|| temporary.path().to_owned());
    std::fs::create_dir_all(&output_dir).unwrap();
    let mut inputs: Vec<_> = std::fs::read_dir(fixture.join("shuffled"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "png"))
        .collect();
    inputs.sort();
    assert_eq!(
        inputs.len(),
        8,
        "expected all eight commons/1 shuffled images"
    );
    let mut scores = String::new();
    for input in inputs {
        let output = output_dir.join(input.file_name().unwrap());
        let result = Command::new(env!("CARGO_BIN_EXE_kzigsaw-solver-cli"))
            .args([
                "solve",
                "--strategy",
                "slower",
                "--assume-all-pieces-are-same-shape",
                "--assume-all-pieces-are-rectangle",
                "--assume-no-padding",
                "--piece-rectangle-width",
                "64",
                "--piece-rectangle-height",
                "64",
                "--piece-rounding-policy",
                "cut",
                "--solved-information-output-format",
                "json",
                "--input",
            ])
            .arg(&input)
            .arg("--output")
            .arg(&output)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        std::fs::write(output.with_extension("json"), &result.stdout).unwrap();
        // Compare directly against the original JPEG, including its extra bottom 13 rows.
        let comparison = Command::new(&magick)
            .args(["compare", "-metric", "SSIM"])
            .arg(fixture.join("original.jpg"))
            .arg(&output)
            .arg("null:")
            .output()
            .unwrap();
        assert!(
            matches!(comparison.status.code(), Some(0 | 1)),
            "{}",
            String::from_utf8_lossy(&comparison.stderr)
        );
        let metric = String::from_utf8(comparison.stderr).unwrap();
        let score: f64 = metric.trim().parse().unwrap();
        let line = format!(
            "{} SSIM={score:.6}\n",
            input.file_name().unwrap().to_string_lossy()
        );
        print!("{line}");
        scores.push_str(&line);
        assert!(score >= 0.9, "{}: SSIM {score} < 0.9000", input.display());
    }
    std::fs::write(output_dir.join("ssim.txt"), scores).unwrap();
}
