//! CPU-parallel adjacency evaluation; the core itself has no Rayon dependency.
use kzigsaw_solver_core::{Costs, GradientModel, Puzzle};
use rayon::prelude::*;

pub fn compute_costs(puzzle: &Puzzle) -> Result<Costs, String> {
    Costs::from_rows(
        (0..puzzle.piece_count())
            .into_par_iter()
            .map(|a| {
                (
                    (0..puzzle.piece_count())
                        .map(|b| puzzle.adjacency(a, b, false))
                        .collect(),
                    (0..puzzle.piece_count())
                        .map(|b| puzzle.adjacency(a, b, true))
                        .collect(),
                )
            })
            .collect(),
    )
}

pub fn compute_gradient_costs(puzzle: &Puzzle) -> Result<Costs, String> {
    let model = GradientModel::new(puzzle);
    Costs::from_rows(
        (0..puzzle.piece_count())
            .into_par_iter()
            .map(|a| {
                (
                    (0..puzzle.piece_count())
                        .map(|b| model.adjacency(a, b, false))
                        .collect(),
                    (0..puzzle.piece_count())
                        .map(|b| model.adjacency(a, b, true))
                        .collect(),
                )
            })
            .collect(),
    )
}
