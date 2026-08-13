import { test } from 'node:test';
import assert from 'node:assert/strict';

import { renderCircularMap } from '../src/render.ts';
import { TAU, baseToAngle, polar, segmentRanges } from '../src/geometry.ts';
import type { Molecule, Strand } from '../src/types.ts';

const pUC19ish: Molecule = {
  name: 'pUC19',
  length: 2686,
  topology: 'circular',
  features: [
    { name: 'AmpR', type: 'CDS', strand: 'reverse', segments: [{ start: 1626, end: 2486 }] },
    { name: 'lacZα', type: 'CDS', strand: 'reverse', segments: [{ start: 146, end: 469 }] },
    { name: 'ori', type: 'rep_origin', segments: [{ start: 867, end: 1455 }] },
    { name: 'lac promoter', type: 'promoter', strand: 'reverse', segments: [{ start: 507, end: 537 }] },
  ],
  sites: [
    { name: 'EcoRI', position: 396, unique: true },
    { name: 'HindIII', position: 447, unique: true },
  ],
};

/** Crude but sufficient: every opened tag is closed, in order. */
function tagsBalanced(svg: string): boolean {
  const stack: string[] = [];
  const re = /<(\/?)([a-zA-Z][\w-]*)([^>]*?)(\/?)>/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(svg)) !== null) {
    const [, closing, name, , selfClose] = m;
    if (selfClose === '/') continue;
    if (closing === '/') {
      if (stack.pop() !== name) return false;
    } else {
      stack.push(name);
    }
  }
  return stack.length === 0;
}

/** Default canvas is 620x620, so the ring centre is here. Every test below that
 *  measures an angle renders at that size explicitly rather than leaning on the
 *  default, so the centre is something the test states rather than inherits. */
const CX = 310;
const CY = 310;

/**
 * Every point a path command lands on, in document order.
 *
 * Not a bare scan for number pairs, because `A` is not shaped like one: its
 * first two numbers are radii and the next three are flags, so only its last
 * pair is a coordinate. `A238.2,238.2 0 1 0 354.63,76.02` read as a pair-scan
 * yields a perfectly plausible point at (238.2, 238.2) — which lands inside
 * every region the tests below check, so the wrong parse would not announce
 * itself by failing, it would just quietly stop measuring the real path.
 */
function pathPoints(d: string): Array<{ x: number; y: number }> {
  const out: Array<{ x: number; y: number }> = [];
  const re = /([MLA])([^MLAZ]*)/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(d)) !== null) {
    const nums = (m[2].match(/-?\d+(?:\.\d+)?/g) ?? []).map(Number);
    const pair = m[1] === 'A' ? nums.slice(-2) : nums.slice(0, 2);
    if (pair.length === 2) out.push({ x: pair[0], y: pair[1] });
  }
  return out;
}

/** The renderer's own angular convention, read back off a drawn point:
 *  radians clockwise from 12 o'clock, in `[0, TAU)`. The inverse of `polar`. */
function angleOf(p: { x: number; y: number }): number {
  const a = Math.atan2(p.x - CX, CY - p.y);
  return a < 0 ? a + TAU : a;
}

function radiusOf(p: { x: number; y: number }): number {
  return Math.hypot(p.x - CX, p.y - CY);
}

/** The `d` of every filled feature band, in the order the renderer emitted
 *  them — which is segment order. Leader lines and the linear backbone are
 *  `fill="none"`, ruler and site ticks name their stroke before their fill, so
 *  requiring a hex `fill` immediately after `d` selects feature bands alone. */
function featureBands(svg: string): string[] {
  return [...svg.matchAll(/<path d="([^"]+)" fill="(#[0-9a-fA-F]{3,8})"/g)].map((m) => m[1]);
}

test('produces a well-formed, self-contained svg', () => {
  const { svg } = renderCircularMap(pUC19ish);
  assert.ok(svg.startsWith('<svg'));
  assert.ok(svg.endsWith('</svg>'));
  assert.ok(svg.includes('xmlns="http://www.w3.org/2000/svg"'));
  assert.ok(tagsBalanced(svg), 'unbalanced tags');
  // Self-contained means no external anything.
  assert.ok(!/<image|xlink:href|<script|url\(http/.test(svg));
  assert.ok(!svg.includes('NaN'), 'NaN leaked into the output');
  assert.ok(!svg.includes('undefined'));
});

/** The opening `<svg …>` tag, without the `>`. */
function root(svg: string): string {
  return svg.slice(0, svg.indexOf('>'));
}

/**
 * The root must ask for the typeface the layout arithmetic assumed.
 *
 * PROVEN TO FAIL against 7bf5aad, where the root named
 * `system-ui, -apple-system, 'Segoe UI', Helvetica, Arial, sans-serif`.
 *
 * `textWidth` reserves the margin — and so fixes the ring's radius — from a
 * 0.55 em/character estimate, and `crates/pl-draw/src/lib.rs`'s `label_width`
 * reserves with the same constant precisely so the two renderers place the ring
 * identically. What an estimate cannot do is name a face, so the root has to.
 * With `system-ui` first, a browser drew the figure in Segoe UI on Windows and
 * San Francisco on macOS while the Rust renderer drew the same scene in
 * Helvetica: two implementations agreeing to 1e-6 on every coordinate and
 * producing two different pictures, which is the one failure
 * `crates/pl-draw/tests/agreement.rs` exists to prevent and the one kind it
 * could not see.
 *
 * The three names are metric-compatible. Nimbus Sans is the free clone shipped
 * on Linux and Arial is compatible by design, which is what `pl-draw`'s
 * Helvetica width tables were cross-checked against; whichever a viewer
 * resolves, the advances are the same.
 */
test('the root asks for the typeface the layout was measured against', () => {
  const { svg } = renderCircularMap(pUC19ish);
  const family = /font-family="([^"]*)"/.exec(root(svg))?.[1];
  assert.ok(family, 'the root names no typeface, so every viewer picks its own');
  assert.ok(
    family.startsWith('Helvetica'),
    `the layout is drawn in whatever comes first here: ${family}`,
  );
  for (const compatible of ['Nimbus Sans', 'Arial']) {
    assert.ok(
      family.includes(compatible),
      `${compatible} is the fallback where Helvetica is absent, and it is missing: ${family}`,
    );
  }
  assert.ok(
    !/system-ui|-apple-system|Segoe/.test(family),
    `a face nothing here measured is offered ahead of the fallbacks: ${family}`,
  );
});

/**
 * Corners must be capped and joined the way the other renderer caps and joins.
 *
 * PROVEN TO FAIL against 7bf5aad, where the root stated neither. SVG's initial
 * values are `butt` caps and `miter` joins with a limit of 4; `pl-draw` states
 * `round` for both because its PDF back end emits `1 J 1 j`, so leaving them
 * unstated here is not "the default", it is the other value. Every leader-line
 * elbow — each one a two-segment path with a real corner at the elbow — and
 * every arrowhead point differed between the two renderings of one map.
 */
test('the root rounds its stroke caps and joins, as the rust renderer does', () => {
  const { svg } = renderCircularMap(pUC19ish);
  assert.ok(
    root(svg).includes('stroke-linecap="round"'),
    `caps left at butt while the other renderer rounds them: ${root(svg)}`,
  );
  assert.ok(
    root(svg).includes('stroke-linejoin="round"'),
    `joins left at miter while the other renderer rounds them: ${root(svg)}`,
  );
});

/**
 * A run of spaces inside a name must survive the XML parser.
 *
 * PROVEN TO FAIL against 7bf5aad, where the root carried no `xml:space`. The
 * default is `xml:space="default"`, under which a parser collapses every run of
 * whitespace in character data to a single space — while `textWidth` had
 * already counted each character of the run and reserved the margin for all of
 * them. The label then draws narrower than the room kept for it: 3.34 pt per
 * collapsed space at 12 pt, Helvetica's advance for U+0020 (278/1000 em) in the
 * face the root now asks for.
 *
 * This renderer never *builds* such a run — its cut-site labels are
 * `EcoRI (396)`, one space and parentheses, where `crates/pl-draw` builds
 * `EcoRI  402` with two. The run arrives from the file instead: a GenBank
 * `/label=` or `/gene=` qualifier carries whatever the submitter typed, `esc`
 * deliberately strips only the control characters XML forbids, and nothing
 * between the file and the `<text>` normalises a double space.
 */
test('a run of spaces in a name is drawn at the width it was reserved', () => {
  const spaced: Molecule = {
    name: 'pSPACED',
    length: 2686,
    features: [{ name: 'T7  promoter', type: 'promoter', segments: [{ start: 100, end: 400 }] }],
  };
  const { svg } = renderCircularMap(spaced, { width: 700, height: 700 });
  assert.ok(
    svg.includes('T7  promoter'),
    'the run was normalised before emission, so this proves nothing',
  );
  assert.ok(
    root(svg).includes('xml:space="preserve"'),
    `the name was measured with both spaces and will be drawn with one: ${root(svg)}`,
  );
});

test('every feature and site gets a label, on a map with room', () => {
  const { labels, hiddenLabels } = renderCircularMap(pUC19ish, { width: 700, height: 700 });
  assert.deepEqual(hiddenLabels, []);
  assert.equal(labels.length, 6);
  for (const f of pUC19ish.features!) {
    assert.ok(labels.some((l) => l.text === f.name), `${f.name} unlabelled`);
  }
});

/**
 * A cut site the file puts outside the molecule is refused and reported, the
 * way an out-of-range feature already was.
 *
 * PROVEN TO FAIL at d8c218b: the site loop's only guard was
 * `!Number.isFinite(s.position)`. Every finite position went on to
 * `baseToAngle`, whose positive modulo — which exists so that a base *before*
 * the chosen origin still lands on the circle — folded it onto some in-range
 * base, while the label a few lines down was built from the **raw** number. On
 * this molecule d8c218b drew ticks at bases 1000, 1000 and 960 and captioned
 * them `EcoRI (5,000)`, `BamHI (0)` and `SalI (-40)`: a site the reader can
 * measure off the drawing and read off the label at two different coordinates,
 * neither of them a base this molecule has. `malformed` was `[]` — and in the
 * same render, two *features* carrying those same coordinates were refused and
 * reported, which is the whole of the difference. On a linear molecule there is
 * not even a modular reading to appeal to: base 1,001 of 1,000 is nowhere.
 *
 * `geometry.ts` states the rule the site path was not following: collapsing an
 * out-of-range coordinate onto a real base "is fabrication rather than loss,
 * which is worse".
 *
 * The count assertion is what closes the loophole of drawing the tick anyway
 * and merely suppressing the label: the tick and the label's anchor are pushed
 * together, so a site that reaches the drawing reaches `labels` or
 * `hiddenLabels` with it.
 *
 * TO RE-BREAK IT: in `renderCircularMap`'s site loop (src/render.ts), change
 * the line `if (s.position < 1 || s.position > length) {` to `if (false) {`.
 */
test('a restriction site outside the molecule is reported, not folded onto a real base', () => {
  const { svg, labels, hiddenLabels, malformed } = renderCircularMap(
    {
      name: 'p1000',
      length: 1000,
      topology: 'circular',
      sites: [
        { name: 'EcoRI', position: 5000 },
        { name: 'BamHI', position: 0 },
        { name: 'SalI', position: -40 },
        { name: 'PstI', position: 400, unique: true },
      ],
    },
    { width: 700, height: 700 },
  );

  assert.deepEqual(labels.map((l) => l.text), ['PstI (400)']);
  assert.deepEqual(malformed.map((m) => m.name).sort(), ['BamHI', 'EcoRI', 'SalI']);
  assert.equal(labels.length + hiddenLabels.length + malformed.length, 4);
  for (const refused of ['EcoRI', 'BamHI', 'SalI']) {
    assert.ok(!svg.includes(refused), `${refused} was drawn on a base it does not cut`);
  }
  assert.ok(!svg.includes('5,000'), 'the figure prints a coordinate the molecule has not got');
  // ...and a site that really is in range is still drawn, because refusing
  // everything would be a safe renderer that draws the wrong picture.
  assert.ok(svg.includes('PstI (400)'), 'the real site must survive');

  // A linear molecule has no modular reading at all, so one past the end is as
  // unplaceable as five times the length.
  const past = renderCircularMap({
    name: 'insert',
    length: 1000,
    topology: 'linear',
    sites: [{ name: 'XhoI', position: 1001 }, { name: 'NotI', position: 1000 }],
  });
  assert.deepEqual(past.malformed.map((m) => m.name), ['XhoI']);
  assert.deepEqual(past.labels.map((l) => l.text), ['NotI (1,000)']);
});

test('no two placed labels overlap', () => {
  const { labels } = renderCircularMap(pUC19ish, { width: 700, height: 700, fontSize: 12 });
  const bySide = { start: [] as number[], end: [] as number[] };
  for (const l of labels) bySide[l.anchor].push(l.y);
  for (const side of ['start', 'end'] as const) {
    const ys = bySide[side].sort((a, b) => a - b);
    for (let i = 1; i < ys.length; i++) {
      assert.ok(ys[i] - ys[i - 1] >= 12 - 1e-6, `${side} labels overlap`);
    }
  }
});

test('a crowded map reports what it could not fit rather than dropping it silently', () => {
  const crowded: Molecule = {
    name: 'crowded',
    length: 5000,
    features: Array.from({ length: 120 }, (_, i) => ({
      name: `feature-${i}`,
      segments: [{ start: i * 40 + 1, end: i * 40 + 30 }],
    })),
  };
  const { labels, hiddenLabels } = renderCircularMap(crowded, { width: 400, height: 400 });
  assert.ok(hiddenLabels.length > 0, 'a 120-label map in 400px must drop some');
  assert.equal(labels.length + hiddenLabels.length, 120);
});

test('base 1 is at twelve o’clock and coordinates increase clockwise', () => {
  const ring = { cx: 0, cy: 0 };
  const top = polar(ring, 10, baseToAngle(1, 1000));
  assert.ok(Math.abs(top.x) < 1e-9);
  assert.ok(Math.abs(top.y + 10) < 1e-9, 'base 1 is not at the top');

  // A quarter of the way round should be at 3 o’clock (x positive).
  const quarter = polar(ring, 10, baseToAngle(251, 1000));
  assert.ok(quarter.x > 9.9, 'coordinates do not increase clockwise');
  assert.ok(Math.abs(quarter.y) < 0.1);
});

test('a feature that crosses the origin is drawn as two arcs, not one wrong one', () => {
  const ranges = segmentRanges({ start: 4900, end: 100 }, 5000, true);
  assert.equal(ranges.length, 2);
  assert.deepEqual(ranges[0], { from: 4899, to: 5000 });
  assert.deepEqual(ranges[1], { from: 0, to: 100 });

  const { svg } = renderCircularMap({
    name: 'wrap',
    length: 5000,
    topology: 'circular',
    features: [{ name: 'spans-origin', strand: 'forward', segments: [{ start: 4900, end: 100 }] }],
  });
  assert.ok(tagsBalanced(svg));
  assert.ok(!svg.includes('NaN'));
});

test('a reversed interval on a linear molecule does not silently wrap', () => {
  const ranges = segmentRanges({ start: 400, end: 100 }, 5000, false);
  assert.equal(ranges.length, 1, 'a linear molecule has no origin to cross');
});

/**
 * A join is one gene, so it gets one arrowhead — on the segment the gene ends
 * on, pointing the way the gene is read.
 *
 * PROVEN TO FAIL at d8c218b: not as shipped, but against three mutations of
 * `renderCircularMap`'s feature loop that d8c218b's version of this test passes,
 * which is the same defect. That version's only assertion was
 * `svg.match(/<path[^>]*fill="#[^"]*"/g).length === 3` — a count of how many
 * bands carry a hex fill, which is three whether each band is a plain arc or a
 * complete arrowhead, because `arcPath` returns one `d` per range either way.
 * The property in the title, and the one this package's README sells two
 * paragraphs after promising the suite "checks that claim directly rather than
 * taking it on trust", was never looked at. All three of the following left the
 * whole 44-test suite green: `i === arrowOn` widened to `arrowOn >= 0`, which
 * puts an arrowhead on **every exon**; `arrowOn` forced to `-1`, which removes
 * the arrowhead from **every feature on every map**; and the
 * `strand === 'reverse' ? 'start' : 'end'` conditional swapped, which points
 * every arrowhead the **wrong way** down the molecule. Nothing else in the
 * package covers it either — `hostile.test.ts`'s `arcPath` case asserts only
 * `startsWith('M') && includes('A') && !NaN`, satisfied by all three, and
 * `agreement.json` records four root attributes and not one arc.
 *
 * Two things are read off the geometry here rather than counted:
 *
 *   - An arrowhead is the only thing that puts **interior `L` commands** into
 *     `arcPath`'s output. A plain band is `M · A · L · A · Z`: exactly one `L`,
 *     the radial step from the outer arc to the inner one. An arrowhead adds
 *     the two barbs and the point, for four. So "more than one `L`" is
 *     "this band is the arrowed one", exactly.
 *   - The point of the arrowhead is the only vertex at the ring's **mid**
 *     radius — every other vertex sits at `ri`, `ro`, or one barb outside
 *     either — so the direction the arrow points is `angleOf` that vertex, and
 *     it must be the far end of the terminal exon on the forward strand and the
 *     near end of the first exon on the reverse. Without this second half the
 *     `'start'`/`'end'` swap survives: it moves the arrowhead within its
 *     segment without changing which segment carries it, so the `L`-counts are
 *     4,1,1 either way.
 *
 * TO RE-BREAK IT: in `renderCircularMap` (src/render.ts), change
 * `arrow: i === arrowOn ? …` to `arrow: arrowOn >= 0 ? …`.
 */
test('a multi-segment feature draws one arrow, not one per exon', () => {
  const exons = [
    { start: 100, end: 400 },
    { start: 700, end: 1000 },
    { start: 1400, end: 1800 },
  ];
  const bands = (strand: Strand): string[] => {
    const { svg } = renderCircularMap(
      {
        name: 'joined',
        length: 3000,
        features: [{ name: 'spliced', type: 'CDS', strand, segments: exons }],
      },
      { width: 620, height: 620 },
    );
    assert.ok(tagsBalanced(svg));
    const ds = featureBands(svg);
    assert.equal(ds.length, 3, `expected one path per segment, got ${ds.length}`);
    return ds;
  };
  /** Which of the three bands carry an arrowhead. */
  const arrowed = (ds: string[]): boolean[] =>
    ds.map((d) => (d.match(/L/g) ?? []).length > 1);
  /** Where the arrowhead on this band points. */
  const tipAngle = (d: string): number => {
    const pts = pathPoints(d);
    const rs = pts.map(radiusOf);
    // `ro + barb` and `ri - barb` are the extremes and the barb is symmetric,
    // so their mean is the mid radius the point sits on.
    const mid = (Math.max(...rs) + Math.min(...rs)) / 2;
    let best = 0;
    for (let i = 1; i < pts.length; i++) {
      if (Math.abs(rs[i] - mid) < Math.abs(rs[best] - mid)) best = i;
    }
    return angleOf(pts[best]);
  };

  const forward = bands('forward');
  assert.deepEqual(arrowed(forward), [false, false, true], 'the arrow is on the terminal exon');
  // The end of exon 3 is base 1800, whose far edge is where base 1801 begins.
  assert.ok(
    Math.abs(tipAngle(forward[2]) - baseToAngle(1801, 3000)) < 1e-3,
    'a forward arrow must point at the high-coordinate end of its exon',
  );

  const reverse = bands('reverse');
  assert.deepEqual(arrowed(reverse), [true, false, false], 'the arrow is on the first exon');
  assert.ok(
    Math.abs(tipAngle(reverse[0]) - baseToAngle(100, 3000)) < 1e-3,
    'a reverse arrow must point at the low-coordinate end of its exon',
  );

  // And a feature with no strand claims no direction at all.
  assert.deepEqual(arrowed(bands('none')), [false, false, false]);
});

test('a fragment is drawn as an outline, a whole feature is filled', () => {
  const base = { name: 'x', length: 3000, features: [{ name: 'AmpR', type: 'CDS', segments: [{ start: 100, end: 900 }] }] };
  const whole = renderCircularMap(base as Molecule).svg;
  const frag = renderCircularMap({
    ...base,
    features: [{ ...base.features[0], fragment: true }],
  } as Molecule).svg;

  assert.ok(whole.includes('fill="#4f7fd0"'), 'a whole CDS should be filled');
  assert.ok(frag.includes('fill="none"'), 'a fragment should be unfilled');
  assert.ok(frag.includes('stroke-dasharray'), 'a fragment should be dashed');
});

test('a very small feature becomes a tick rather than a sub-pixel arrowhead', () => {
  const { svg } = renderCircularMap({
    name: 'tiny',
    length: 100000,
    features: [{ name: 'lox', strand: 'forward', segments: [{ start: 500, end: 534 }] }],
  });
  // A tick is a straight two-point path; an arrow contains arc commands.
  const featurePaths = svg.match(/<path d="M[^"]*" stroke="#7f8a95"[^>]*>/g) ?? [];
  assert.ok(featurePaths.length >= 1);
  assert.ok(!featurePaths[0].includes('A'), 'should not be an arc');
});

test('linear and circular molecules are drawn differently', () => {
  const circ = renderCircularMap({ name: 'a', length: 1000, topology: 'circular' }).svg;
  const lin = renderCircularMap({ name: 'a', length: 1000, topology: 'linear' }).svg;
  assert.ok(circ.includes('<circle'), 'a circular molecule gets a closed backbone');
  assert.ok(!lin.includes('<circle'), 'a linear molecule must not be drawn as a closed ring');
});

/**
 * A linear molecule's coordinate system must have the same free ends its
 * backbone does.
 *
 * PROVEN TO FAIL at d8c218b, where the 6% break was a local inside the backbone
 * branch and nothing else knew about it. Features, ruler ticks and site ticks
 * all went through `baseToAngle` over the full 360 degrees, so the backbone
 * claimed 338.40 degrees while the coordinate system spread the molecule over
 * 359.64 and the 6% of bases nearest each end were drawn in the region the
 * backbone had been removed from. Terminal features bridged the break and the
 * map read as closed — the one thing the break is there to deny.
 *
 * At its worst it was not a near miss but an exact collision: this 2,000 bp
 * molecule with its one whole-length feature emitted a feature path
 * **byte-identical** to the same molecule drawn circular, a complete annulus
 * painted over the mid-radius backbone. Rasterised and diffed, the linear and
 * circular figures differed by 2 pixels out of 384,400, max channel delta
 * 26/255. A gBlock or a PCR product annotated end to end rendered as a plasmid,
 * and `malformed` was empty, because nothing was malformed: the renderer drew
 * what it was asked to draw, on the wrong circle.
 *
 * The old test for this could not fail either — it asserted only
 * `!lin.includes('<circle')`, so emitting the backbone as two half-arcs with
 * `gap = 0`, a fully closed ring for a linear molecule, left the suite green.
 * Here the break is measured off the drawn arc and every other coordinate is
 * required to fall inside it, so the two cannot drift apart again in either
 * direction: close the gap and the containment assertions have nothing left to
 * contain; widen the mapping and the coordinates leave the arc.
 *
 * TO RE-BREAK IT: in `renderCircularMap` (src/render.ts), change
 * `const mapLength = circular ? length : length * (TAU / (TAU - LINEAR_GAP));`
 * to `const mapLength = length;`.
 */
test('a linear molecule draws its bases between its own free ends', () => {
  const length = 2000;
  const whole = { name: 'insert', type: 'CDS', segments: [{ start: 1, end: length }] };
  const size = { width: 620, height: 620 };
  const lin = renderCircularMap({ name: 'gBlock', length, topology: 'linear', features: [whole] }, size);
  const circ = renderCircularMap({ name: 'gBlock', length, topology: 'circular', features: [whole] }, size);

  // The two free ends, read back off the arc the renderer actually drew rather
  // than recomputed from the constant. `#33383d` is the default backboneStroke.
  const arc = /<path d="([^"]+)" fill="none" stroke="#33383d" stroke-width="1\.25"\/>/.exec(lin.svg)?.[1];
  assert.ok(arc, 'a linear molecule must be drawn as an open arc, not a circle');
  const ends = pathPoints(arc!);
  assert.equal(ends.length, 2, 'an open arc has exactly two endpoints');
  const g0 = angleOf(ends[0]);
  const g1 = angleOf(ends[1]);
  const gap = TAU - (g1 - g0);
  assert.ok(
    gap > 0.01 * TAU && gap < 0.2 * TAU,
    `the break must be visible and must not eat the map: ${(gap / TAU) * 360} degrees`,
  );

  const band = featureBands(lin.svg);
  assert.equal(band.length, 1);
  const angles = pathPoints(band[0]).map(angleOf);
  for (const a of angles) {
    assert.ok(
      a >= g0 - 1e-3 && a <= g1 + 1e-3,
      `a feature corner at ${(a / TAU) * 360} degrees is outside the backbone`,
    );
  }
  // Not merely inside: base 1 lands on one free end and base `length` on the
  // other, so a molecule annotated end to end fills the arc and stops there.
  assert.ok(Math.abs(Math.min(...angles) - g0) < 1e-3, 'base 1 is not on the first free end');
  assert.ok(Math.abs(Math.max(...angles) - g1) < 1e-3, 'base 2000 is not on the second');

  // Ruler ticks share the mapping or the numbers beside the arc name the wrong
  // bases. The last tick is the one that used to fall in the break.
  const ticks = [
    ...lin.svg.matchAll(/<path d="(M[^"]+)" stroke="#6b7280" stroke-width="1" fill="none"\/>/g),
  ].map((m) => m[1]);
  assert.ok(ticks.length > 0, 'no ruler was drawn, so this proves nothing');
  for (const t of ticks) {
    for (const a of pathPoints(t).map(angleOf)) {
      assert.ok(
        a >= g0 - 1e-3 && a <= g1 + 1e-3,
        `a ruler tick at ${(a / TAU) * 360} degrees is outside the backbone`,
      );
    }
  }

  assert.notEqual(
    band[0],
    featureBands(circ.svg)[0],
    'the linear molecule drew the same closed annulus as the plasmid',
  );
});

test('feature names containing xml metacharacters are escaped', () => {
  const { svg } = renderCircularMap({
    name: 'P<&>"Q',
    length: 1000,
    features: [{ name: 'tet<R> & "friends"', segments: [{ start: 10, end: 500 }] }],
  });
  assert.ok(!/<text[^>]*>[^<]*<[^/]/.test(svg), 'raw < leaked into text content');
  assert.ok(svg.includes('&lt;') && svg.includes('&amp;'));
  assert.ok(tagsBalanced(svg));
});

test('a hostile colour cannot inject an svg attribute', () => {
  // Colours are not author-controlled: they arrive in .dna and GenBank files
  // downloaded from Addgene and emailed between labs, and this renderer exists
  // to produce SVG that a page embeds inline. Before this was fixed, a feature
  // coloured `#fff" onload="alert(1)" x="` closed the fill attribute and added
  // an event handler to the host page.
  const hostile = [
    '#fff" onload="alert(1)" x="',
    '" onmouseover="alert(1)',
    'url(javascript:alert(1))',
    "#fff' onload='alert(1)",
    '#fff"><script>alert(1)</script><path d="',
  ];
  for (const color of hostile) {
    const { svg } = renderCircularMap({
      name: 'x',
      length: 1000,
      features: [{ name: 'f', color, segments: [{ start: 10, end: 500 }] }],
    });
    assert.ok(!/onload|onmouseover|<script|javascript:/i.test(svg), `injected via ${color}`);
    assert.ok(tagsBalanced(svg), `unbalanced after ${color}`);
  }

  // ...and an ordinary colour still works, because refusing everything would be
  // a safe renderer that draws the wrong picture.
  for (const good of ['#4f7fd0', '#abc', 'rebeccapurple', 'rgb(10, 20, 30)', 'rgba(1,2,3,0.5)']) {
    const { svg } = renderCircularMap({
      name: 'x',
      length: 1000,
      features: [{ name: 'f', color: good, segments: [{ start: 10, end: 500 }] }],
    });
    assert.ok(svg.includes(`fill="${good}"`), `${good} should survive`);
  }
});

test('a hostile theme cannot inject either', () => {
  const { svg } = renderCircularMap(
    { name: 'x', length: 1000, features: [{ name: 'f', segments: [{ start: 1, end: 500 }] }] },
    { theme: { backboneStroke: '#000" onload="alert(1)', labelFill: '"><script>x</script>' } },
  );
  assert.ok(!/onload|<script/i.test(svg));
  assert.ok(tagsBalanced(svg));
});

test('rendering is deterministic', () => {
  const first = renderCircularMap(pUC19ish).svg;
  for (let i = 0; i < 10; i++) {
    assert.equal(renderCircularMap(pUC19ish).svg, first);
  }
});

test('the origin can be rotated without breaking coordinates', () => {
  const { svg } = renderCircularMap(pUC19ish, { originAtTop: 1500 });
  assert.ok(tagsBalanced(svg));
  assert.ok(!svg.includes('NaN'));
  // Base 1500 should now sit at the top.
  assert.ok(Math.abs(baseToAngle(1500, 2686, 1500)) < 1e-9);
});

test('degenerate molecules do not throw', () => {
  for (const m of [
    { name: 'empty', length: 0 },
    { name: 'one base', length: 1, features: [{ name: 'f', segments: [{ start: 1, end: 1 }] }] },
    { name: 'no features', length: 5000, features: [] },
    {
      name: 'whole-molecule feature',
      length: 1000,
      features: [{ name: 'all', strand: 'forward' as const, segments: [{ start: 1, end: 1000 }] }],
    },
    {
      name: 'out of range',
      length: 100,
      features: [{ name: 'past the end', segments: [{ start: 90, end: 5000 }] }],
    },
  ]) {
    const { svg } = renderCircularMap(m as Molecule);
    assert.ok(tagsBalanced(svg), `${m.name}: unbalanced`);
    assert.ok(!svg.includes('NaN'), `${m.name}: NaN`);
    assert.ok(!svg.includes('Infinity'), `${m.name}: Infinity`);
  }
});

test('labels point at the feature they name', () => {
  const { labels } = renderCircularMap(pUC19ish, { width: 700, height: 700 });
  const amp = labels.find((l) => l.text === 'AmpR')!;
  assert.ok(amp);
  // AmpR spans 1626..2486 of 2686, whose midpoint is ~2056 — past the halfway
  // point, so it is on the left of the circle.
  assert.ok(amp.target.x < 350, `AmpR target should be left of centre, got ${amp.target.x}`);
  assert.equal(amp.anchor, 'end');
});
