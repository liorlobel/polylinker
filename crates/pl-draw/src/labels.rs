//! Optimal label placement, in Rust.
//!
//! The same problem and the same solution as `@polylinker/circular-map`'s
//! `labels.ts`, ported so the desktop app and the CLI place labels identically
//! to the browser tool.
//!
//! Given desired heights `dᵢ` and label heights `hᵢ`, minimise
//! `Σ wᵢ(yᵢ − dᵢ)²` subject to `y₍ᵢ₊₁₎ − yᵢ ≥ (hᵢ + h₍ᵢ₊₁₎)/2`. Substituting
//! `zᵢ = yᵢ − cᵢ` for the cumulative minimum offset turns that into plain
//! monotonicity — **isotonic regression** — solved exactly in O(n) by
//! pool-adjacent-violators.
//!
//! The obvious alternative, nudging colliding labels apart until nothing moves,
//! can fail to converge and depends on iteration order, so the same plasmid
//! renders differently twice. This is exact, one pass, identical every time.
//!
//! The clamp goes on the regression's **output**, not its input. Clamping the
//! targets first looks equivalent and is not: it changes the weighted block
//! means, so the answer drifts even when the unclamped optimum was feasible.
//! That bug shipped in the TypeScript version and moved 27 of 500 random maps.

/// A label wanting to sit somewhere.
#[derive(Debug, Clone, Copy)]
pub struct LabelBox {
    pub ideal: f64,
    pub height: f64,
    /// Resistance to displacement. Larger features hold their position.
    pub weight: f64,
}

#[derive(Debug, Clone, Default)]
pub struct Placement {
    /// Final y per input, in input order. `None` for a dropped label.
    pub positions: Vec<Option<f64>>,
    /// Indices dropped because the column could not hold them all, lightest
    /// first — removal order, so the reader sees what yielded and why.
    pub dropped: Vec<usize>,
}

/// The least-squares non-decreasing fit: pool adjacent violators.
///
/// Merges adjacent blocks whenever the running means go the wrong way. That is
/// what makes it exact — the optimum is piecewise-constant on precisely these
/// blocks.
pub fn isotonic(targets: &[f64], weights: &[f64]) -> Vec<f64> {
    struct Block {
        n: usize,
        w: f64,
        wy: f64,
    }
    let mut blocks: Vec<Block> = Vec::with_capacity(targets.len());
    for (i, &t) in targets.iter().enumerate() {
        let mut b = Block {
            n: 1,
            w: weights[i],
            wy: weights[i] * t,
        };
        while let Some(prev) = blocks.last() {
            if prev.wy / prev.w <= b.wy / b.w {
                break;
            }
            let p = blocks.pop().expect("checked by last()");
            b = Block {
                n: p.n + b.n,
                w: p.w + b.w,
                wy: p.wy + b.wy,
            };
        }
        blocks.push(b);
    }
    let mut out = Vec::with_capacity(targets.len());
    for b in &blocks {
        let mean = b.wy / b.w;
        for _ in 0..b.n {
            out.push(mean);
        }
    }
    out
}

/// Place one column of labels between `lo` and `hi`.
///
/// Inputs need not be sorted; results come back in input order. When the labels
/// cannot all fit, the lightest are dropped and reported — dropping is a real
/// decision with a visible consequence and is never silent.
pub fn place_column(boxes: &[LabelBox], lo: f64, hi: f64) -> Placement {
    let mut out = Placement {
        positions: vec![None; boxes.len()],
        dropped: Vec::new(),
    };
    if boxes.is_empty() {
        return out;
    }

    let mut order: Vec<usize> = (0..boxes.len()).collect();
    order.sort_by(|&a, &b| {
        boxes[a]
            .ideal
            .partial_cmp(&boxes[b].ideal)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.cmp(&b))
    });

    let total = |o: &[usize]| -> f64 { o.iter().map(|&i| boxes[i].height).sum() };
    while !order.is_empty() && total(&order) > hi - lo {
        let worst = order
            .iter()
            .enumerate()
            .min_by(|a, b| {
                boxes[*a.1]
                    .weight
                    .partial_cmp(&boxes[*b.1].weight)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(a.0.cmp(&b.0))
            })
            .map(|(k, _)| k)
            .expect("non-empty");
        out.dropped.push(order.remove(worst));
    }
    if order.is_empty() {
        return out;
    }

    let m = order.len();
    let mut c = vec![0.0; m];
    for i in 1..m {
        c[i] = c[i - 1] + (boxes[order[i - 1]].height + boxes[order[i]].height) / 2.0;
    }

    // The box constraints in z-space, where every zᵢ shares one interval
    // because the per-height terms cancel against `cᵢ`.
    let sum: f64 = total(&order);
    let (mut z_lo, mut z_hi) = (f64::NEG_INFINITY, f64::INFINITY);
    let mut before = 0.0;
    for i in 0..m {
        let h = boxes[order[i]].height;
        let after = sum - before - h;
        z_lo = z_lo.max(lo + before + h / 2.0 - c[i]);
        z_hi = z_hi.min(hi - after - h / 2.0 - c[i]);
        before += h;
    }

    let targets: Vec<f64> = order
        .iter()
        .enumerate()
        .map(|(i, &j)| boxes[j].ideal - c[i])
        .collect();
    let weights: Vec<f64> = order.iter().map(|&j| boxes[j].weight.max(1e-6)).collect();
    let z = isotonic(&targets, &weights);
    for i in 0..m {
        out.positions[order[i]] = Some(z[i].clamp(z_lo, z_hi) + c[i]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boxes(ideals: &[f64], h: f64) -> Vec<LabelBox> {
        ideals
            .iter()
            .map(|&ideal| LabelBox {
                ideal,
                height: h,
                weight: 1.0,
            })
            .collect()
    }

    #[test]
    fn labels_that_fit_are_not_moved() {
        let p = place_column(&boxes(&[20.0, 60.0, 100.0], 10.0), 0.0, 200.0);
        assert!(p.dropped.is_empty());
        assert_eq!(p.positions, vec![Some(20.0), Some(60.0), Some(100.0)]);
    }

    #[test]
    fn identical_labels_stack_symmetrically_about_their_ideal() {
        // The assertion that catches a clamp applied to the regression's
        // input: n identical boxes wanting one y must come back centred on it.
        for n in [1usize, 2, 3, 4, 5, 6, 9, 15] {
            let b = boxes(&vec![200.0; n], 12.0);
            let p = place_column(&b, 0.0, 400.0);
            assert!(p.dropped.is_empty());
            let ys: Vec<f64> = p.positions.iter().map(|x| x.unwrap()).collect();
            let mean = ys.iter().sum::<f64>() / n as f64;
            assert!((mean - 200.0).abs() < 1e-9, "n={n}: centred on {mean}");
            let mut sorted = ys.clone();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
            for i in 1..n {
                assert!((sorted[i] - sorted[i - 1] - 12.0).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn colliding_labels_are_separated_by_exactly_their_height() {
        let p = place_column(&boxes(&[50.0, 52.0, 54.0], 10.0), 0.0, 200.0);
        let ys: Vec<f64> = p.positions.iter().map(|x| x.unwrap()).collect();
        assert!(ys[1] - ys[0] >= 10.0 - 1e-9);
        assert!(ys[2] - ys[1] >= 10.0 - 1e-9);
        // Symmetric input, symmetric answer.
        assert!((ys[1] - 52.0).abs() < 1e-9);
    }

    #[test]
    fn what_cannot_fit_is_dropped_by_weight_and_reported() {
        let b: Vec<LabelBox> = (0..10)
            .map(|i| LabelBox {
                ideal: 30.0,
                height: 12.0,
                weight: (i + 1) as f64,
            })
            .collect();
        let p = place_column(&b, 0.0, 60.0);
        assert_eq!(p.dropped, vec![0, 1, 2, 3, 4], "the five lightest go");
        assert_eq!(p.positions.iter().filter(|x| x.is_some()).count(), 5);
    }

    #[test]
    fn everything_placed_stays_inside_the_band() {
        let b: Vec<LabelBox> = (0..12)
            .map(|i| LabelBox {
                ideal: -300.0 + i as f64 * 70.0,
                height: 12.0,
                weight: 1.0,
            })
            .collect();
        let p = place_column(&b, 0.0, 200.0);
        for (i, y) in p.positions.iter().enumerate() {
            if let Some(y) = y {
                assert!(y - 6.0 >= -1e-6, "{i}: {y} above the band");
                assert!(y + 6.0 <= 200.0 + 1e-6, "{i}: {y} below the band");
            }
        }
    }

    #[test]
    fn placement_is_deterministic() {
        let b: Vec<LabelBox> = (0..40)
            .map(|i| LabelBox {
                ideal: ((i * 7919) % 300) as f64,
                height: 12.0,
                weight: 1.0 + (i % 5) as f64,
            })
            .collect();
        let first = place_column(&b, 0.0, 400.0);
        for _ in 0..10 {
            let again = place_column(&b, 0.0, 400.0);
            assert_eq!(again.positions, first.positions);
            assert_eq!(again.dropped, first.dropped);
        }
    }
}
