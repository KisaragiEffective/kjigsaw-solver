use clap::{Args, Parser, Subcommand, ValueEnum};
use image::{ImageFormat, ImageReader, RgbaImage};
use kzigsaw_solver_core::{DimensionCandidate, Puzzle, detect_edges, solve};
use kzigsaw_solver_parallelization::compute_costs;
use serde::Serialize;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    process::ExitCode,
};

#[derive(Parser)]
#[command(
    version,
    about = "Restore rectangular jigsaw images without rotation or padding"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Reorder pieces using boundary color and gradient continuity (approximate).
    Solve(SolveArgs),
    /// Suggest piece dimensions from periodic discontinuities.
    DetectEdge(DetectArgs),
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Text,
    Json,
}

#[derive(Clone, Copy, ValueEnum)]
enum PieceRoundingPolicy {
    Cut,
}

#[derive(Args)]
struct SolveArgs {
    #[arg(long, required = true)]
    assume_all_pieces_are_same_shape: bool,
    #[arg(long, required = true)]
    assume_all_pieces_are_rectangle: bool,
    #[arg(long, required = true)]
    assume_no_padding: bool,
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    piece_rectangle_width: u32,
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    piece_rectangle_height: u32,
    /// Discard incomplete pieces at the right and bottom edges.
    #[arg(long, value_enum)]
    piece_rounding_policy: Option<PieceRoundingPolicy>,
    #[arg(long, required = true, action = clap::ArgAction::Append)]
    input: Vec<PathBuf>,
    #[arg(long, required = true, action = clap::ArgAction::Append)]
    output: Vec<PathBuf>,
    #[arg(long, value_enum, default_value = "text")]
    solved_information_output_format: Format,
}

#[derive(Args)]
struct DetectArgs {
    #[arg(long, required = true, action = clap::ArgAction::Append)]
    input: Vec<PathBuf>,
    #[arg(long, value_enum, default_value = "text")]
    output_format: Format,
}

#[derive(Serialize)]
struct Position {
    row: usize,
    column: usize,
}
#[derive(Serialize)]
struct PieceMapping {
    piece_id: usize,
    source: Position,
    destination: Position,
}
#[derive(Serialize)]
struct Solution {
    input: PathBuf,
    output: PathBuf,
    columns: usize,
    rows: usize,
    piece_width: u32,
    piece_height: u32,
    pieces: Vec<PieceMapping>,
}
#[derive(Serialize)]
struct Detection {
    input: PathBuf,
    candidates: Vec<DimensionCandidate>,
    reason: Option<String>,
}

fn load(path: &Path) -> Result<RgbaImage, String> {
    let reader = ImageReader::open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .with_guessed_format()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if !matches!(reader.format(), Some(ImageFormat::Png | ImageFormat::Jpeg)) {
        return Err(format!(
            "{}: only PNG and JPEG inputs are supported",
            path.display()
        ));
    }
    reader
        .decode()
        .map(|i| i.to_rgba8())
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn json(value: &impl Serialize) -> Result<(), String> {
    serde_json::to_writer_pretty(std::io::stdout().lock(), value).map_err(|e| e.to_string())?;
    println!();
    Ok(())
}

fn solve_command(args: SolveArgs) -> Result<(), String> {
    if args.input.len() != args.output.len() {
        return Err("--input and --output must have exactly the same number of paths".into());
    }
    let input_paths: HashSet<_> = args
        .input
        .iter()
        .map(|p| {
            p.canonicalize()
                .map_err(|e| format!("{}: {e}", p.display()))
        })
        .collect::<Result<_, _>>()?;
    let mut output_paths = HashSet::new();
    for output in &args.output {
        if !output
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("png"))
        {
            return Err(format!(
                "{}: output must have a .png extension",
                output.display()
            ));
        }
        let resolved = if output.exists() {
            output
                .canonicalize()
                .map_err(|e| format!("{}: {e}", output.display()))?
        } else {
            let parent = output
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            parent
                .canonicalize()
                .map_err(|e| format!("{}: {e}", parent.display()))?
                .join(output.file_name().ok_or("invalid output filename")?)
        };
        if input_paths.contains(&resolved) {
            return Err(format!(
                "{}: output must not overwrite an input image",
                output.display()
            ));
        }
        if !output_paths.insert(resolved) {
            return Err("output paths must be distinct".into());
        }
    }
    // Validate every input before creating any output.
    let puzzles: Vec<_> = args
        .input
        .iter()
        .map(|path| {
            let mut image = load(path)?;
            let width = image.width() / args.piece_rectangle_width * args.piece_rectangle_width;
            let height = image.height() / args.piece_rectangle_height * args.piece_rectangle_height;
            if width == 0 || height == 0 {
                return Err(format!("{}: image {}x{} is smaller than a complete piece ({}x{})", path.display(), image.width(), image.height(), args.piece_rectangle_width, args.piece_rectangle_height));
            }
            if width != image.width() || height != image.height() {
                match args.piece_rounding_policy {
                    None => return Err(format!("{}: image dimensions {}x{} are not divisible by piece dimensions {}x{}; specify --piece-rounding-policy cut to discard the remainder at the right and bottom edges (output size: {}x{})", path.display(), image.width(), image.height(), args.piece_rectangle_width, args.piece_rectangle_height, width, height)),
                    Some(PieceRoundingPolicy::Cut) => image = image::imageops::crop_imm(&image, 0, 0, width, height).to_image(),
                }
            }
            Puzzle::new(
                &image,
                args.piece_rectangle_width,
                args.piece_rectangle_height,
            )
            .map_err(|e| format!("{}: {e}", path.display()))
        })
        .collect::<Result<_, _>>()?;
    let mut results = Vec::new();
    for ((input, output), puzzle) in args.input.iter().zip(&args.output).zip(&puzzles) {
        eprintln!(
            "Solving {} ({} pieces)...",
            input.display(),
            puzzle.piece_count()
        );
        let costs = compute_costs(puzzle)?;
        let order = solve(puzzle, &costs)?;
        puzzle
            .render(&order)?
            .save_with_format(output, ImageFormat::Png)
            .map_err(|e| format!("{}: {e}", output.display()))?;
        let position = |index: usize| Position {
            row: index / puzzle.columns + 1,
            column: index % puzzle.columns + 1,
        };
        let mut pieces: Vec<_> = order
            .iter()
            .enumerate()
            .map(|(destination, &source)| PieceMapping {
                piece_id: source + 1,
                source: position(source),
                destination: position(destination),
            })
            .collect();
        pieces.sort_by_key(|p| p.piece_id);
        results.push(Solution {
            input: input.clone(),
            output: output.clone(),
            columns: puzzle.columns,
            rows: puzzle.rows,
            piece_width: puzzle.piece_width,
            piece_height: puzzle.piece_height,
            pieces,
        });
    }
    match args.solved_information_output_format {
        Format::Json => json(&results)?,
        Format::Text => {
            for result in results {
                println!(
                    "{} -> {} ({} columns, {} rows)",
                    result.input.display(),
                    result.output.display(),
                    result.columns,
                    result.rows
                );
                for p in result.pieces {
                    println!(
                        "piece {}: ({}, {}) -> ({}, {})",
                        p.piece_id,
                        p.source.row,
                        p.source.column,
                        p.destination.row,
                        p.destination.column
                    );
                }
            }
        }
    }
    Ok(())
}

fn run(cli: Cli) -> Result<(), String> {
    match cli.command {
        Command::Solve(args) => solve_command(args),
        Command::DetectEdge(args) => {
            let results: Vec<_> = args.input.iter().map(|input| {
                let candidates = detect_edges(&load(input)?);
                let reason = candidates.is_empty().then(|| "Insufficient periodic boundary evidence on both axes; specify piece dimensions manually (single-row/column and one-pixel pieces cannot be inferred).".into());
                Ok(Detection { input: input.clone(), candidates, reason })
            }).collect::<Result<_,String>>()?;
            match args.output_format {
                Format::Json => json(&results)?,
                Format::Text => {
                    for result in results {
                        println!("{}:", result.input.display());
                        if let Some(reason) = result.reason {
                            println!("  {reason}");
                        }
                        for c in result.candidates {
                            println!(
                                "  {}x{} pixels ({} columns, {} rows), score {:.6}",
                                c.piece_width, c.piece_height, c.columns, c.rows, c.score
                            );
                        }
                    }
                }
            }
            Ok(())
        }
    }
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
