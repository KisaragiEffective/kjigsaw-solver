use crate::{Costs, Puzzle, arrangement_cost};

/// Keep an existing placement unless gradient reconstruction improves its boundaries.
/// This also preserves the fast placement when gradient evidence is ambiguous.
pub fn refine_with_gradients(
    puzzle: &Puzzle,
    costs: &Costs,
    initial: Vec<usize>,
) -> Result<Vec<usize>, String> {
    let mut used = vec![false; puzzle.piece_count()];
    if initial.len() != used.len() {
        return Err("placement has the wrong length".into());
    }
    for &id in &initial {
        if id >= used.len() || used[id] {
            return Err("placement must be a permutation".into());
        }
        used[id] = true;
    }
    let proposed = solve_slower(puzzle, costs)?;
    if arrangement_cost(&proposed, puzzle.columns, costs) + 1e-9
        < arrangement_cost(&initial, puzzle.columns, costs)
    {
        Ok(proposed)
    } else {
        Ok(initial)
    }
}

struct Edge {
    pixels: Vec<[f64; 3]>,
    gradients: Vec<[f64; 3]>,
    mean: [f64; 3],
    inverse: [[f64; 3]; 3],
}

impl Edge {
    fn new(puzzle: &Puzzle, id: usize, side: usize) -> Self {
        let image = &puzzle.pieces[id];
        let vertical = side >= 2;
        let end = side.is_multiple_of(2);
        let (depth, length) = if vertical {
            (image.height(), image.width())
        } else {
            (image.width(), image.height())
        };
        let outer = if end { depth - 1 } else { 0 };
        let inner = if end {
            outer.saturating_sub(1)
        } else {
            1.min(depth - 1)
        };
        let pixel = |p, d| {
            let value = if vertical {
                image.get_pixel(p, d)
            } else {
                image.get_pixel(d, p)
            };
            [
                f64::from(value[0]),
                f64::from(value[1]),
                f64::from(value[2]),
            ]
        };
        let mut pixels = Vec::new();
        let mut gradients = Vec::new();
        let mut mean = [0.0; 3];
        for p in 0..length {
            let a = pixel(p, outer);
            let b = pixel(p, inner);
            let gradient = std::array::from_fn(|c| if end { a[c] - b[c] } else { b[c] - a[c] });
            for c in 0..3 {
                mean[c] += gradient[c] / f64::from(length);
            }
            pixels.push(a);
            gradients.push(gradient);
        }
        let mut covariance = [[0.0; 3]; 3];
        for g in &gradients {
            for i in 0..3 {
                for j in 0..3 {
                    covariance[i][j] += (g[i] - mean[i]) * (g[j] - mean[j]) / f64::from(length);
                }
            }
        }
        for (i, row) in covariance.iter_mut().enumerate() {
            row[i] += 9.0;
        }
        // Invert the positive definite, regularized 3x3 covariance with elimination.
        let mut inverse = [[0.0; 3]; 3];
        for (i, row) in inverse.iter_mut().enumerate() {
            row[i] = 1.0;
        }
        for k in 0..3 {
            let divisor = covariance[k][k];
            for j in 0..3 {
                covariance[k][j] /= divisor;
                inverse[k][j] /= divisor;
            }
            for i in 0..3 {
                if i == k {
                    continue;
                }
                let factor = covariance[i][k];
                for j in 0..3 {
                    covariance[i][j] -= factor * covariance[k][j];
                    inverse[i][j] -= factor * inverse[k][j];
                }
            }
        }
        Self {
            pixels,
            gradients,
            mean,
            inverse,
        }
    }
}

/// Per-pixel gradient checks and edge-local color covariance.
/// The reference/original image is never used by this model.
pub struct GradientModel {
    edges: Vec<[Edge; 4]>,
}

impl GradientModel {
    pub fn new(puzzle: &Puzzle) -> Self {
        Self {
            edges: (0..puzzle.piece_count())
                .map(|id| std::array::from_fn(|side| Edge::new(puzzle, id, side)))
                .collect(),
        }
    }

    /// Enumerate an admissible boundary only if every pixel/channel passes.
    /// Tolerance: |gap - mean(local gradients)| <= 4*(16+|g1|+|g2|).
    pub fn adjacency(&self, a: usize, b: usize, vertical: bool) -> f64 {
        if a == b {
            return f64::INFINITY;
        }
        let side = if vertical { 2 } else { 0 };
        let (a, b) = (&self.edges[a][side], &self.edges[b][side + 1]);
        let mut cost = 0.0;
        let mut rejected = 0;
        for p in 0..a.pixels.len() {
            let gap: [f64; 3] = std::array::from_fn(|c| b.pixels[p][c] - a.pixels[p][c]);
            for (c, &channel_gap) in gap.iter().enumerate() {
                let predicted = (a.gradients[p][c] + b.gradients[p][c]) / 2.0;
                let tolerance = 4.0 * (16.0 + a.gradients[p][c].abs() + b.gradients[p][c].abs());
                if (channel_gap - predicted).abs() > tolerance {
                    rejected += 1;
                }
            }
            for edge in [a, b] {
                let residual: [f64; 3] = std::array::from_fn(|c| gap[c] - edge.mean[c]);
                for i in 0..3 {
                    for j in 0..3 {
                        cost += residual[i] * edge.inverse[i][j] * residual[j];
                    }
                }
            }
        }
        // Rejected pairs remain available only as last-resort completions.
        cost / (a.pixels.len() as f64 * 6.0) + rejected as f64 * 1000.0
    }
}

/// Construct a globally consistent grid from gradient-compatible candidate pairs.
pub fn solve_slower(puzzle: &Puzzle, costs: &Costs) -> Result<Vec<usize>, String> {
    let n = puzzle.piece_count();
    if costs.count != n {
        return Err("adjacency matrix size differs from puzzle".into());
    }
    if n == 1 {
        return Ok(vec![0]);
    }
    let mut edges = Vec::new();
    for vertical in [false, true] {
        if (vertical && puzzle.rows == 1) || (!vertical && puzzle.columns == 1) {
            continue;
        }
        let mut outgoing = vec![[f64::INFINITY; 2]; n];
        let mut incoming = outgoing.clone();
        fn update(best: &mut [f64; 2], value: f64) {
            if value < best[0] {
                best[1] = best[0];
                best[0] = value;
            } else if value < best[1] {
                best[1] = value;
            }
        }
        for (a, out) in outgoing.iter_mut().enumerate() {
            for (b, inc) in incoming.iter_mut().enumerate() {
                if a != b {
                    let c = costs.get(a, b, vertical);
                    update(out, c);
                    update(inc, c);
                }
            }
        }
        for (a, out) in outgoing.iter().enumerate() {
            for (b, inc) in incoming.iter().enumerate() {
                if a == b {
                    continue;
                }
                let c = costs.get(a, b, vertical);
                let ratio = c / out[1].min(inc[1]).max(1e-9);
                if ratio < 0.9 && c < 1000.0 {
                    edges.push((ratio, a, b, vertical));
                }
            }
        }
    }
    edges.sort_unstable_by(|a, b| {
        a.0.total_cmp(&b.0)
            .then(a.1.cmp(&b.1))
            .then(a.2.cmp(&b.2))
            .then(a.3.cmp(&b.3))
    });
    let mut groups: Vec<_> = (0..n).collect();
    let mut positions = vec![(0_i32, 0_i32); n];
    for (_, a, b, vertical) in edges {
        let (ga, gb) = (groups[a], groups[b]);
        if ga == gb {
            continue;
        }
        let offset = (
            positions[a].0 + i32::from(!vertical) - positions[b].0,
            positions[a].1 + i32::from(vertical) - positions[b].1,
        );
        let mut occupied = std::collections::HashSet::new();
        let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        let mut valid = true;
        for i in 0..n {
            if groups[i] != ga && groups[i] != gb {
                continue;
            }
            let (mut x, mut y) = positions[i];
            if groups[i] == gb {
                x += offset.0;
                y += offset.1;
            }
            if !occupied.insert((x, y)) {
                valid = false;
                break;
            }
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
        if !valid || x1 - x0 >= puzzle.columns as i32 || y1 - y0 >= puzzle.rows as i32 {
            continue;
        }
        for i in 0..n {
            if groups[i] == gb {
                groups[i] = ga;
                positions[i].0 += offset.0;
                positions[i].1 += offset.1;
            }
        }
    }
    let mut sizes = vec![0; n];
    for &group in &groups {
        sizes[group] += 1;
    }
    let anchor = (0..n)
        .max_by_key(|&id| (sizes[id], std::cmp::Reverse(id)))
        .unwrap();
    let x0 = (0..n)
        .filter(|&i| groups[i] == anchor)
        .map(|i| positions[i].0)
        .min()
        .unwrap();
    let y0 = (0..n)
        .filter(|&i| groups[i] == anchor)
        .map(|i| positions[i].1)
        .min()
        .unwrap();
    let mut order = vec![usize::MAX; n];
    let mut used = vec![false; n];
    for i in 0..n {
        if groups[i] == anchor {
            let (x, y) = positions[i];
            order[(y - y0) as usize * puzzle.columns + (x - x0) as usize] = i;
            used[i] = true;
        }
    }
    while used.iter().any(|&u| !u) {
        let mut best = (0, f64::INFINITY, usize::MAX, usize::MAX);
        for position in 0..n {
            if order[position] != usize::MAX {
                continue;
            }
            let neighbors: Vec<_> = [
                (
                    position % puzzle.columns > 0,
                    position.wrapping_sub(1),
                    false,
                    true,
                ),
                (
                    position % puzzle.columns + 1 < puzzle.columns,
                    position + 1,
                    false,
                    false,
                ),
                (
                    position >= puzzle.columns,
                    position.wrapping_sub(puzzle.columns),
                    true,
                    true,
                ),
                (
                    position + puzzle.columns < n,
                    position + puzzle.columns,
                    true,
                    false,
                ),
            ]
            .into_iter()
            .filter(|&(valid, p, _, _)| valid && order[p] != usize::MAX)
            .collect();
            let count = neighbors.len();
            if count < best.0 {
                continue;
            }
            for (id, &is_used) in used.iter().enumerate() {
                if is_used {
                    continue;
                }
                let score: f64 = neighbors
                    .iter()
                    .map(|&(_, p, v, before)| {
                        if before {
                            costs.get(order[p], id, v)
                        } else {
                            costs.get(id, order[p], v)
                        }
                    })
                    .sum();
                if count > best.0 || score < best.1 {
                    best = (count, score, position, id);
                }
            }
        }
        if best.2 == usize::MAX {
            return Err("could not complete gradient placement".into());
        }
        order[best.2] = best.3;
        used[best.3] = true;
    }
    Ok(order)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn costs(puzzle: &Puzzle) -> Costs {
        let model = GradientModel::new(puzzle);
        Costs::from_rows(
            (0..puzzle.piece_count())
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
        .unwrap()
    }

    #[test]
    fn restores_rectangles_lines_and_single_pixel_dimensions() {
        for (columns, rows, width, height) in [
            (3, 2, 5, 4),
            (4, 1, 5, 4),
            (1, 4, 5, 4),
            (1, 1, 1, 1),
            (3, 2, 1, 1),
        ] {
            let original = RgbaImage::from_fn(columns * width, rows * height, |x, y| {
                Rgba([
                    (x * 7) as u8,
                    (y * 9) as u8,
                    (x * 3 + y * 4) as u8,
                    (x + y) as u8,
                ])
            });
            let puzzle = Puzzle::new(&original, width, height).unwrap();
            let reversed: Vec<_> = (0..puzzle.piece_count()).rev().collect();
            let shuffled = puzzle.render(&reversed).unwrap();
            let puzzle = Puzzle::new(&shuffled, width, height).unwrap();
            let costs = costs(&puzzle);
            let order = solve_slower(&puzzle, &costs).unwrap();
            let restored = puzzle.render(&order).unwrap();
            if width > 1 || height > 1 || puzzle.piece_count() == 1 {
                assert_eq!(restored, original);
            } else {
                // One-pixel pieces have no directional gradient evidence.
                assert_eq!(restored.dimensions(), original.dimensions());
            }
            assert_eq!(solve_slower(&puzzle, &costs).unwrap(), order);
        }
    }

    #[test]
    fn checks_every_boundary_pixel_in_both_directions() {
        for vertical in [false, true] {
            let (w, h) = if vertical { (8, 16) } else { (16, 8) };
            let blank = RgbaImage::new(w, h);
            let puzzle = Puzzle::new(&blank, 8, 8).unwrap();
            assert_eq!(GradientModel::new(&puzzle).adjacency(0, 1, vertical), 0.0);
            let mut image = blank;
            // A single outlier on the incoming edge and its interior continuation.
            let (x, y) = if vertical { (7, 8) } else { (8, 7) };
            image.put_pixel(x, y, Rgba([255, 0, 0, 0]));
            image.put_pixel(
                x + u32::from(!vertical),
                y + u32::from(vertical),
                Rgba([255, 0, 0, 0]),
            );
            let puzzle = Puzzle::new(&image, 8, 8).unwrap();
            assert!(GradientModel::new(&puzzle).adjacency(0, 1, vertical) >= 1000.0);
        }
    }

    #[test]
    fn refinement_corrects_errors_and_preserves_ambiguous_solutions() {
        let image = RgbaImage::from_fn(12, 8, |x, y| Rgba([(x * 7) as u8, (y * 9) as u8, 0, 255]));
        let puzzle = Puzzle::new(&image, 4, 4).unwrap();
        let gradient_costs = costs(&puzzle);
        assert_eq!(
            refine_with_gradients(&puzzle, &gradient_costs, vec![5, 4, 3, 2, 1, 0]).unwrap(),
            vec![0, 1, 2, 3, 4, 5]
        );
        assert!(refine_with_gradients(&puzzle, &gradient_costs, vec![0, 0, 2, 3, 4, 5]).is_err());
        let puzzle = Puzzle::new(&RgbaImage::new(8, 8), 4, 4).unwrap();
        let initial = vec![3, 2, 1, 0];
        assert_eq!(
            refine_with_gradients(&puzzle, &costs(&puzzle), initial.clone()).unwrap(),
            initial
        );
    }
}
