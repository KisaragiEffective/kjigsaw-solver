//! Reproducible synthetic workload, not a natural-image accuracy benchmark.
use image::{Rgba, RgbaImage};
use kzigsaw_solver_core::{Puzzle, solve};
use kzigsaw_solver_parallelization::compute_costs;
use std::time::Instant;

fn main() {
    benchmark(
        "linear gradient",
        RgbaImage::from_fn(160, 160, |x, y| {
            Rgba([x as u8, y as u8, ((x + y) / 2) as u8, 255])
        }),
    );
    let original = RgbaImage::from_fn(160, 160, |x, y| {
        let (x, y) = (f64::from(x), f64::from(y));
        Rgba([
            (128.0 + 50.0 * (x / 25.0).sin() + 40.0 * (y / 21.0).cos()) as u8,
            (128.0 + 60.0 * ((x + y) / 33.0).sin()) as u8,
            (128.0 + 55.0 * ((x - y) / 29.0).cos()) as u8,
            255,
        ])
    });
    benchmark("periodic smooth texture", original);
}

fn benchmark(label: &str, original: RgbaImage) {
    let puzzle = Puzzle::new(&original, 8, 8).unwrap();
    // 137 is coprime to 400, producing a deterministic permutation.
    let permutation: Vec<_> = (0..400).map(|i| (i * 137 + 53) % 400).collect();
    let shuffled = puzzle.render(&permutation).unwrap();
    let puzzle = Puzzle::new(&shuffled, 8, 8).unwrap();
    let start = Instant::now();
    let costs = compute_costs(&puzzle).unwrap();
    let cost_time = start.elapsed();
    let order = solve(&puzzle, &costs).unwrap();
    let correct = order
        .iter()
        .enumerate()
        .filter(|&(destination, &source)| permutation[source] == destination)
        .count();
    let mut neighbors = 0;
    for i in 0..400 {
        let source = permutation[order[i]];
        if i % 20 < 19 && source % 20 < 19 && permutation[order[i + 1]] == source + 1 {
            neighbors += 1;
        }
        if i < 380 && source < 380 && permutation[order[i + 20]] == source + 20 {
            neighbors += 1;
        }
    }
    println!(
        "{label}: 400 pieces (20x20), 8x8 pixels each; adjacency={cost_time:?}, total={:?}, correct={correct}/400 ({:.2}%)",
        start.elapsed(),
        correct as f64 / 4.0
    );
    println!("correct neighbor pairs={neighbors}/760");
}
