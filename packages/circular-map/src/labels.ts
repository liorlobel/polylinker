/**
 * Label placement.
 *
 * `docs/PLAN.md` §3 calls automatic non-overlapping label placement with leader
 * lines "the hardest layout problem in the product", and corrects the source
 * research for having called it cheap. It is the thing that separates a map you
 * would put in a paper from a map you would not.
 *
 * # The problem, stated exactly
 *
 * Every feature wants its label at the height of the point it describes. Labels
 * have height and may not overlap. So: given desired heights `d₁…dₙ` and label
 * heights `h₁…hₙ`, find actual heights `y₁…yₙ` minimising the weighted squared
 * displacement `Σ wᵢ(yᵢ − dᵢ)²` subject to `y₍ᵢ₊₁₎ − yᵢ ≥ (hᵢ + h₍ᵢ₊₁₎)/2`.
 *
 * # Why this is solved rather than approximated
 *
 * The usual approach is iterative relaxation — nudge colliding labels apart,
 * repeat until nothing moves. It is easy to write, usually looks fine, and has
 * two failure modes that show up exactly when a map is crowded: it can fail to
 * converge, and its answer depends on iteration order, so the same plasmid
 * renders differently in two sessions.
 *
 * This has a closed form instead. Substituting `zᵢ = yᵢ − cᵢ`, where `cᵢ` is the
 * cumulative minimum offset, turns the ordered-with-gaps constraint into plain
 * monotonicity `z₁ ≤ z₂ ≤ … ≤ zₙ` — which is **isotonic regression**, solved
 * exactly in O(n) by pool-adjacent-violators. The result is the provably optimal
 * placement, computed in one pass, identical every time.
 */

export interface LabelBox {
  /** The y this label would sit at if nothing were in the way. */
  ideal: number;
  /** Vertical space it occupies, in px. */
  height: number;
  /** Resistance to displacement. Larger features earn larger weights, so a
   *  20 kb backbone label stays put and a 12 bp site label yields. */
  weight?: number;
}

export interface ColumnResult {
  /** Final y for each input, in input order. `NaN` for dropped labels. */
  positions: number[];
  /** Indices that could not be placed, lowest weight first. */
  dropped: number[];
}

/**
 * Pool-adjacent-violators: the least-squares non-decreasing fit to `targets`.
 *
 * Merges adjacent blocks whenever the running means go the wrong way, which is
 * what makes it exact: the optimal solution is piecewise-constant on precisely
 * these blocks.
 */
export function isotonic(targets: number[], weights: number[]): number[] {
  const blocks: { n: number; w: number; wy: number }[] = [];
  for (let i = 0; i < targets.length; i++) {
    let b = { n: 1, w: weights[i], wy: weights[i] * targets[i] };
    while (blocks.length > 0) {
      const prev = blocks[blocks.length - 1];
      if (prev.wy / prev.w <= b.wy / b.w) break;
      blocks.pop();
      b = { n: prev.n + b.n, w: prev.w + b.w, wy: prev.wy + b.wy };
    }
    blocks.push(b);
  }
  const out: number[] = [];
  for (const b of blocks) {
    const mean = b.wy / b.w;
    for (let i = 0; i < b.n; i++) out.push(mean);
  }
  return out;
}

/**
 * Place one column of labels between `lo` and `hi`.
 *
 * Inputs need not be sorted; the result is returned in input order.
 *
 * When the labels cannot all fit, the lightest are dropped until they do, and
 * their indices are reported. Dropping is a real decision with a visible
 * consequence, so it is never silent — see {@link ColumnResult.dropped} and
 * `RenderResult.hiddenLabels`.
 */
export function placeColumn(boxes: LabelBox[], lo: number, hi: number): ColumnResult {
  if (boxes.length === 0) return { positions: [], dropped: [] };

  // Work on indices sorted by ideal position; the constraint chain is only
  // meaningful in that order.
  let order = boxes.map((_, i) => i).sort((a, b) => boxes[a].ideal - boxes[b].ideal);
  const dropped: number[] = [];

  // Drop the lightest until the column physically fits.
  const totalHeight = () => order.reduce((s, i) => s + boxes[i].height, 0);
  while (order.length > 0 && totalHeight() > hi - lo) {
    let worst = 0;
    for (let k = 1; k < order.length; k++) {
      const w = (i: number) => boxes[i].weight ?? 1;
      if (w(order[k]) < w(order[worst])) worst = k;
    }
    dropped.push(order[worst]);
    order = order.filter((_, k) => k !== worst);
  }

  const n = order.length;
  const positions = new Array<number>(boxes.length).fill(NaN);
  if (n === 0) return { positions, dropped };

  // cᵢ — the minimum cumulative offset that separation demands.
  const c = new Array<number>(n).fill(0);
  for (let i = 1; i < n; i++) {
    c[i] = c[i - 1] + (boxes[order[i - 1]].height + boxes[order[i]].height) / 2;
  }

  // The box constraints, mapped into z-space. Because every zᵢ shares the same
  // feasible interval there, clamping the targets into it is enough: isotonic
  // regression returns weighted means of its targets, so the answer cannot
  // leave their range.
  let zLo = -Infinity;
  let zHi = Infinity;
  let before = 0;
  const total = totalHeight();
  for (let i = 0; i < n; i++) {
    const h = boxes[order[i]].height;
    const after = total - before - h;
    zLo = Math.max(zLo, lo + before + h / 2 - c[i]);
    zHi = Math.min(zHi, hi - after - h / 2 - c[i]);
    before += h;
  }

  const targets = order.map((idx, i) =>
    Math.min(Math.max(boxes[idx].ideal - c[i], zLo), zHi),
  );
  const weights = order.map((idx) => Math.max(boxes[idx].weight ?? 1, 1e-6));

  const z = isotonic(targets, weights);
  for (let i = 0; i < n; i++) positions[order[i]] = z[i] + c[i];
  return { positions, dropped };
}
