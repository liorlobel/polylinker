import { test } from 'node:test';
import assert from 'node:assert/strict';

import { renderCircularMap } from '../src/render.ts';
import { baseToAngle, polar, segmentRanges } from '../src/geometry.ts';
import type { Molecule } from '../src/types.ts';

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

test('a multi-segment feature draws one arrow, not one per exon', () => {
  const { svg } = renderCircularMap({
    name: 'joined',
    length: 3000,
    features: [
      {
        name: 'spliced',
        type: 'CDS',
        strand: 'forward',
        segments: [
          { start: 100, end: 400 },
          { start: 700, end: 1000 },
          { start: 1400, end: 1800 },
        ],
      },
    ],
  });
  const paths = svg.match(/<path[^>]*fill="#[^"]*"/g) ?? [];
  assert.equal(paths.length, 3, 'expected one path per segment');
  assert.ok(tagsBalanced(svg));
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
