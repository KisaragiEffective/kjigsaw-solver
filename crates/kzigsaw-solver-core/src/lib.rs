//! Rectangular jigsaw reconstruction without rotation or reflection.
use image::{RgbaImage, imageops};
use serde::Serialize;
mod gradient;
pub use gradient::{GradientModel, refine_with_gradients, solve_slower};

#[derive(Clone)]
pub struct Puzzle {
    pub columns: usize,
    pub rows: usize,
    pub piece_width: u32,
    pub piece_height: u32,
    pieces: Vec<RgbaImage>,
}

impl Puzzle {
    pub fn new(image: &RgbaImage, width: u32, height: u32) -> Result<Self, String> {
        if width == 0 || height == 0 || image.width() == 0 || image.height() == 0 {
            return Err("image and piece dimensions must be positive".into());
        }
        if !image.width().is_multiple_of(width) || !image.height().is_multiple_of(height) {
            return Err("image dimensions must be divisible by piece dimensions".into());
        }
        let columns = (image.width() / width) as usize;
        let rows = (image.height() / height) as usize;
        let pieces = (0..rows)
            .flat_map(|y| {
                (0..columns).map(move |x| {
                    imageops::crop_imm(image, x as u32 * width, y as u32 * height, width, height)
                        .to_image()
                })
            })
            .collect();
        Ok(Self {
            columns,
            rows,
            piece_width: width,
            piece_height: height,
            pieces,
        })
    }

    pub fn piece_count(&self) -> usize {
        self.pieces.len()
    }

    /// Cost of placing `b` to the right of (or below) `a`. RGB only.
    pub fn adjacency(&self, a: usize, b: usize, vertical: bool) -> f64 {
        if a == b {
            return f64::INFINITY;
        }
        let (along, depth) = if vertical {
            (self.piece_width, self.piece_height)
        } else {
            (self.piece_height, self.piece_width)
        };
        let pixel = |piece: usize, d: u32, p: u32| {
            if vertical {
                self.pieces[piece].get_pixel(p, d)
            } else {
                self.pieces[piece].get_pixel(d, p)
            }
        };
        let mut cost = 0.0;
        for p in 0..along {
            for c in 0..3 {
                let left = f64::from(pixel(a, depth - 1, p)[c]);
                let right = f64::from(pixel(b, 0, p)[c]);
                let gap = right - left;
                cost += gap * gap;
                if depth > 1 {
                    let before = left - f64::from(pixel(a, depth - 2, p)[c]);
                    let after = f64::from(pixel(b, 1, p)[c]) - right;
                    cost += 0.25 * ((gap - before).powi(2) + (gap - after).powi(2));
                }
            }
        }
        cost / (f64::from(along) * 3.0)
    }

    /// `order[destination] = source`, both zero based.
    pub fn render(&self, order: &[usize]) -> Result<RgbaImage, String> {
        let mut seen = vec![false; self.piece_count()];
        if order.len() != seen.len() {
            return Err("placement has the wrong length".into());
        }
        for &id in order {
            if id >= seen.len() || seen[id] {
                return Err("placement must be a permutation".into());
            }
            seen[id] = true;
        }
        let mut output = RgbaImage::new(
            self.columns as u32 * self.piece_width,
            self.rows as u32 * self.piece_height,
        );
        for (destination, &source) in order.iter().enumerate() {
            imageops::replace(
                &mut output,
                &self.pieces[source],
                ((destination % self.columns) as u32 * self.piece_width).into(),
                ((destination / self.columns) as u32 * self.piece_height).into(),
            );
        }
        Ok(output)
    }
}

/// Dense adjacency tables. Row a, column b means b follows a.
pub struct Costs {
    count: usize,
    horizontal: Vec<f64>,
    vertical: Vec<f64>,
}

impl Costs {
    pub fn from_rows(rows: Vec<(Vec<f64>, Vec<f64>)>) -> Result<Self, String> {
        let count = rows.len();
        if count == 0
            || rows.iter().any(|(h, v)| {
                h.len() != count
                    || v.len() != count
                    || h.iter().chain(v).any(|x| x.is_nan() || *x < 0.0)
            })
        {
            return Err("invalid adjacency matrix".into());
        }
        let (horizontal, vertical): (Vec<_>, Vec<_>) = rows.into_iter().unzip();
        Ok(Self {
            count,
            horizontal: horizontal.into_iter().flatten().collect(),
            vertical: vertical.into_iter().flatten().collect(),
        })
    }
    fn get(&self, a: usize, b: usize, vertical: bool) -> f64 {
        (if vertical {
            &self.vertical
        } else {
            &self.horizontal
        })[a * self.count + b]
    }
}

#[derive(Clone)]
struct State {
    order: Vec<usize>,
    used: Vec<bool>,
    cost: f64,
}

fn arrangement_cost(order: &[usize], columns: usize, costs: &Costs) -> f64 {
    let mut total = 0.0;
    for p in 0..order.len() {
        if p % columns > 0 {
            total += costs.get(order[p - 1], order[p], false);
        }
        if p >= columns {
            total += costs.get(order[p - columns], order[p], true);
        }
    }
    total
}

/// Merge the most distinctive neighbor pairs into translation-only components.
/// This supplies a starting arrangement independent of the first piece's ID.
fn assemble(puzzle: &Puzzle, costs: &Costs) -> Option<Vec<usize>> {
    let n = puzzle.piece_count();
    if n == 1 {
        return Some(vec![0]);
    }
    let mut edges = Vec::new();
    for vertical in [false, true] {
        if (vertical && puzzle.rows == 1) || (!vertical && puzzle.columns == 1) {
            continue;
        }
        let second = |values: Vec<f64>| {
            let mut best = [f64::INFINITY; 2];
            for value in values {
                if value < best[0] {
                    best[1] = best[0];
                    best[0] = value;
                } else if value < best[1] {
                    best[1] = value;
                }
            }
            if best[1].is_finite() {
                best[1]
            } else {
                best[0]
            }
        };
        let outgoing: Vec<_> = (0..n)
            .map(|a| {
                second(
                    (0..n)
                        .filter(|&b| a != b)
                        .map(|b| costs.get(a, b, vertical))
                        .collect(),
                )
            })
            .collect();
        let incoming: Vec<_> = (0..n)
            .map(|b| {
                second(
                    (0..n)
                        .filter(|&a| a != b)
                        .map(|a| costs.get(a, b, vertical))
                        .collect(),
                )
            })
            .collect();
        for (a, &out) in outgoing.iter().enumerate() {
            for (b, &inc) in incoming.iter().enumerate() {
                if a != b {
                    edges.push((
                        costs.get(a, b, vertical) - (out + inc) / 2.0,
                        a,
                        b,
                        vertical,
                    ));
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
    let mut groups: Vec<usize> = (0..n).collect();
    let mut positions = vec![(0_i64, 0_i64); n];
    let mut remaining = n;
    for (_, a, b, vertical) in edges {
        let (ga, gb) = (groups[a], groups[b]);
        if ga == gb {
            continue;
        }
        let offset = (
            positions[a].0 + i64::from(!vertical) - positions[b].0,
            positions[a].1 + i64::from(vertical) - positions[b].1,
        );
        let mut occupied = std::collections::HashSet::new();
        let (mut min_x, mut max_x, mut min_y, mut max_y) = (i64::MAX, i64::MIN, i64::MAX, i64::MIN);
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
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }
        if !valid || max_x - min_x >= puzzle.columns as i64 || max_y - min_y >= puzzle.rows as i64 {
            continue;
        }
        for i in 0..n {
            if groups[i] == gb {
                groups[i] = ga;
                positions[i].0 += offset.0;
                positions[i].1 += offset.1;
            }
        }
        remaining -= 1;
        if remaining == 1 {
            let mut order = vec![0; n];
            for (id, &(x, y)) in positions.iter().enumerate() {
                order[(y - min_y) as usize * puzzle.columns + (x - min_x) as usize] = id;
            }
            return Some(order);
        }
    }
    None
}

/// Beam search (64 states) and component assembly, then at most 20 swap sweeps.
pub fn solve(puzzle: &Puzzle, costs: &Costs) -> Result<Vec<usize>, String> {
    let n = puzzle.piece_count();
    if n != costs.count {
        return Err("adjacency matrix size differs from puzzle".into());
    }
    let mut beam = vec![State {
        order: Vec::new(),
        used: vec![false; n],
        cost: 0.0,
    }];
    for position in 0..n {
        // Keep only lightweight candidate descriptors, cloning the best states afterward.
        let mut candidates = Vec::new();
        for (parent, state) in beam.iter().enumerate() {
            for id in 0..n {
                if state.used[id] {
                    continue;
                }
                let mut cost = state.cost;
                if position % puzzle.columns > 0 {
                    cost += costs.get(state.order[position - 1], id, false);
                }
                if position >= puzzle.columns {
                    cost += costs.get(state.order[position - puzzle.columns], id, true);
                }
                candidates.push((cost, parent, id));
            }
        }
        let compare = |a: &(f64, usize, usize), b: &(f64, usize, usize)| {
            a.0.total_cmp(&b.0)
                .then_with(|| beam[a.1].order.cmp(&beam[b.1].order))
                .then(a.2.cmp(&b.2))
        };
        if candidates.len() > 64 {
            candidates.select_nth_unstable_by(64, compare);
            candidates.truncate(64);
        }
        candidates.sort_unstable_by(compare);
        beam = candidates
            .into_iter()
            .map(|(cost, parent, id)| {
                let mut next = beam[parent].clone();
                next.order.push(id);
                next.used[id] = true;
                next.cost = cost;
                next
            })
            .collect();
    }
    let mut order = beam.remove(0).order;
    if let Some(assembled) = assemble(puzzle, costs)
        && arrangement_cost(&assembled, puzzle.columns, costs)
            < arrangement_cost(&order, puzzle.columns, costs)
    {
        order = assembled;
    }
    for _ in 0..20 {
        let mut improved = false;
        for a in 0..n {
            for b in a + 1..n {
                let mut edges = Vec::with_capacity(8);
                for p in [a, b] {
                    if p % puzzle.columns > 0 {
                        edges.push((p - 1, p, false));
                    }
                    if p % puzzle.columns + 1 < puzzle.columns {
                        edges.push((p, p + 1, false));
                    }
                    if p >= puzzle.columns {
                        edges.push((p - puzzle.columns, p, true));
                    }
                    if p + puzzle.columns < n {
                        edges.push((p, p + puzzle.columns, true));
                    }
                }
                edges.sort_unstable();
                edges.dedup();
                let score = |order: &[usize]| {
                    edges
                        .iter()
                        .map(|&(x, y, v)| costs.get(order[x], order[y], v))
                        .sum::<f64>()
                };
                let before = score(&order);
                order.swap(a, b);
                if score(&order) + 1e-9 < before {
                    improved = true;
                } else {
                    order.swap(a, b);
                }
            }
        }
        if !improved {
            break;
        }
    }
    Ok(order)
}

#[derive(Debug, Serialize)]
pub struct DimensionCandidate {
    pub piece_width: u32,
    pub piece_height: u32,
    pub columns: u32,
    pub rows: u32,
    pub score: f64,
}

/// Scores periodic discontinuities; scores are relative contrasts, not probabilities.
pub fn detect_edges(image: &RgbaImage) -> Vec<DimensionCandidate> {
    fn axis(image: &RgbaImage, vertical: bool) -> Vec<(u32, f64)> {
        let (length, cross) = if vertical {
            (image.height(), image.width())
        } else {
            (image.width(), image.height())
        };
        if length < 3 || cross == 0 {
            return Vec::new();
        }
        let mut differences = vec![0.0; length as usize];
        for p in 1..length {
            let mut sum = 0.0;
            for q in 0..cross {
                let (a, b) = if vertical {
                    (image.get_pixel(q, p - 1), image.get_pixel(q, p))
                } else {
                    (image.get_pixel(p - 1, q), image.get_pixel(p, q))
                };
                for c in 0..3 {
                    sum += (f64::from(a[c]) - f64::from(b[c])).powi(2);
                }
            }
            differences[p as usize] = sum / (f64::from(cross) * 3.0);
        }
        let mut candidates = Vec::new();
        for size in 2..length {
            if !length.is_multiple_of(size) {
                continue;
            }
            let (mut boundary, mut inside, mut nb, mut ni) = (0.0, 0.0, 0_u32, 0_u32);
            for p in 1..length {
                if p % size == 0 {
                    boundary += differences[p as usize];
                    nb += 1;
                } else {
                    inside += differences[p as usize];
                    ni += 1;
                }
            }
            let (boundary, inside) = (boundary / f64::from(nb), inside / f64::from(ni));
            let score = (boundary - inside) / (boundary + inside + 1.0);
            if score > 0.0 {
                candidates.push((size, score));
            }
        }
        candidates.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        candidates.truncate(5);
        candidates
    }
    let widths = axis(image, false);
    let heights = axis(image, true);
    let mut result = Vec::new();
    for &(width, x) in &widths {
        for &(height, y) in &heights {
            result.push(DimensionCandidate {
                piece_width: width,
                piece_height: height,
                columns: image.width() / width,
                rows: image.height() / height,
                score: (x + y) / 2.0,
            });
        }
    }
    result.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then(a.piece_width.cmp(&b.piece_width))
            .then(a.piece_height.cmp(&b.piece_height))
    });
    result.truncate(10);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn costs(p: &Puzzle) -> Costs {
        Costs::from_rows(
            (0..p.piece_count())
                .map(|a| {
                    (
                        (0..p.piece_count())
                            .map(|b| p.adjacency(a, b, false))
                            .collect(),
                        (0..p.piece_count())
                            .map(|b| p.adjacency(a, b, true))
                            .collect(),
                    )
                })
                .collect(),
        )
        .unwrap()
    }

    #[test]
    fn restores_known_image_including_alpha_and_lines() {
        for (columns, rows) in [(3, 2), (4, 1), (1, 4), (1, 1)] {
            let original = RgbaImage::from_fn(columns * 5, rows * 5, |x, y| {
                Rgba([
                    (x * 7) as u8,
                    (y * 9) as u8,
                    (x * 3 + y * 4) as u8,
                    (x + y) as u8,
                ])
            });
            let p = Puzzle::new(&original, 5, 5).unwrap();
            let reversed: Vec<_> = (0..p.piece_count()).rev().collect();
            let shuffled = p.render(&reversed).unwrap();
            let p = Puzzle::new(&shuffled, 5, 5).unwrap();
            let order = solve(&p, &costs(&p)).unwrap();
            assert_eq!(p.render(&order).unwrap(), original);
        }
    }

    #[test]
    fn validates_dimensions_and_permutations() {
        let image = RgbaImage::new(6, 4);
        assert!(Puzzle::new(&image, 0, 2).is_err());
        assert!(Puzzle::new(&image, 4, 2).is_err());
        let p = Puzzle::new(&image, 3, 2).unwrap();
        assert!(p.render(&[0, 0, 2, 3]).is_err());
        assert!(p.render(&[0]).is_err());
        let p = Puzzle::new(&image, 1, 1).unwrap();
        assert!(p.adjacency(0, 1, false).is_finite());
    }

    #[test]
    fn component_initialization_handles_more_than_64_pieces() {
        let original = RgbaImage::from_fn(36, 32, |x, y| {
            Rgba([(x * 3) as u8, (y * 4) as u8, (x + y) as u8, 255])
        });
        let puzzle = Puzzle::new(&original, 4, 4).unwrap();
        let expected: Vec<_> = (0..72).rev().collect();
        let shuffled = puzzle.render(&expected).unwrap();
        let puzzle = Puzzle::new(&shuffled, 4, 4).unwrap();
        let costs = costs(&puzzle);
        assert_eq!(assemble(&puzzle, &costs).unwrap(), expected);
        let order = solve(&puzzle, &costs).unwrap();
        assert_eq!(puzzle.render(&order).unwrap(), original);
        assert_eq!(solve(&puzzle, &costs).unwrap(), order);
    }

    #[test]
    fn detects_periodic_edges_and_reports_no_evidence() {
        let image = RgbaImage::from_fn(24, 24, |x, y| {
            Rgba([
                if x / 6 % 2 == 0 { 0 } else { 255 },
                if y / 8 % 2 == 0 { 0 } else { 255 },
                0,
                255,
            ])
        });
        assert!(
            detect_edges(&image)
                .iter()
                .any(|c| c.piece_width == 6 && c.piece_height == 8)
        );
        assert!(detect_edges(&RgbaImage::new(24, 24)).is_empty());
        assert!(detect_edges(&RgbaImage::new(1, 1)).is_empty());
    }
}
