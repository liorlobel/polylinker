# @polylinker/circular-map

Plasmid maps as SVG, from a plain object. No DOM, no framework, **no runtime
dependencies**.

```ts
import { renderCircularMap } from '@polylinker/circular-map';

const { svg, labels, hiddenLabels } = renderCircularMap({
  name: 'pUC19',
  length: 2686,
  topology: 'circular',
  features: [
    { name: 'AmpR',  type: 'CDS',        strand: 'reverse', segments: [{ start: 1626, end: 2486 }] },
    { name: 'lacZα', type: 'CDS',        strand: 'reverse', segments: [{ start: 146,  end: 469  }] },
    { name: 'ori',   type: 'rep_origin',                    segments: [{ start: 867,  end: 1455 }] },
  ],
  sites: [{ name: 'EcoRI', position: 396, unique: true }],
});
```

It is a pure function: same input, same bytes. That is why it works unchanged in
a browser, in Node, in a test, and in a CI job that rasterises figures for a
paper — and why text width is *estimated* rather than measured. Asking the DOM
for metrics would make the output depend on which fonts happen to be installed,
and a figure that reflows between machines is not a figure.

## Why it is a separate package

`docs/PLAN.md` §3: the map is the visible quality signal in this product
category, and no open component does it at the standard biologists expect. It
has to be built either way, so it is built standalone — usable by seqviz,
plascad, OpenCloning and pLannotate rather than only by Polylinker.

## Label placement

The plan calls automatic non-overlapping label placement with leader lines *"the
hardest layout problem in the product"*, and corrects the source research for
having called it cheap.

The usual approach is iterative relaxation: nudge colliding labels apart, repeat
until nothing moves. It is easy to write, usually looks fine, and has two failure
modes that appear exactly when a map is crowded — it can fail to converge, and
its result depends on iteration order, so the same plasmid renders differently in
two sessions.

This solves it exactly instead. Stated properly, the problem is: given desired
heights `dᵢ` and label heights `hᵢ`, minimise `Σ wᵢ(yᵢ − dᵢ)²` subject to
`y₍ᵢ₊₁₎ − yᵢ ≥ (hᵢ + h₍ᵢ₊₁₎)/2`. Substituting `zᵢ = yᵢ − cᵢ` for the cumulative
minimum offset `cᵢ` turns that constraint into plain monotonicity
`z₁ ≤ z₂ ≤ … ≤ zₙ` — which is **isotonic regression**, solved exactly in O(n) by
pool-adjacent-violators.

The result is the provably optimal placement, in one pass, identical every time.
The test suite checks that claim directly rather than taking it on trust: 200
random layouts, each compared against 400 random feasible alternatives, none of
which may score better.

Larger features get larger weights, so a backbone label holds its position and a
12 bp site label yields.

## Things it gets right that are easy to get wrong

- **Origin-spanning features.** A feature whose `end` precedes its `start` is one
  thing biologically and two arcs on screen. Returned as two ranges from the
  bottom layer, so the origin case is not a special case everywhere downstream.
- **Multi-segment features.** A join gets one arrowhead on its terminal segment,
  not one per exon.
- **Tiny features.** Below about a degree an arrowhead is smaller than a pixel and
  reads as dirt on the figure, so it degrades to a tick — which is honest at any
  size. The arrowhead is also clamped to half the arc, so a 30 bp feature cannot
  render as the classic inverted bow tie.
- **Linear molecules** get an arc with visible free ends. Drawing them as a closed
  ring would be a lie about topology.
- **Labels that do not fit** are returned in `hiddenLabels`, never dropped
  silently. A map that quietly omits three labels looks exactly like a map of a
  plasmid with three fewer features, and only the caller can decide whether to
  enlarge the canvas, shrink the font, or list them beside the figure.
- **Base 1 at twelve o'clock, coordinates clockwise** — what SnapGene, Benchling
  and every plasmid figure in a methods section do. The other way round is not a
  stylistic variation, it is wrong-looking.

## API

```ts
renderCircularMap(molecule: Molecule, options?: RenderOptions): RenderResult
```

`RenderResult` carries `svg`, the `labels` that were placed (with their anchor
points, so callers can add their own hit-testing or tooltips), and
`hiddenLabels`. Features may carry an `id`, emitted as `data-feature-id`, so a
rendered element maps back to the caller's own object.

Notable options: `width`, `height`, `ringWidth`, `fontSize`, `ruler`,
`originAtTop` (rotate so a given base sits at the top), `minFeatureDegrees`, and
a partial `theme`.

Coordinates are **1-based inclusive**, matching GenBank and SnapGene.

## Development

```bash
npm test        # node:test, no framework
npm run build   # tsc to dist/
```

```bash
node --experimental-strip-types examples/from-genbank.ts plasmid.gb > map.svg
```

## Licence

MIT OR Apache-2.0.
