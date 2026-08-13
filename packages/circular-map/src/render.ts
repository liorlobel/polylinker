/**
 * The renderer: molecule in, SVG string out.
 *
 * Pure. No DOM, no measurement, no side effects — so the same call produces the
 * same bytes in a browser, in Node, in a test, and in a CI job that rasterises
 * figures for a paper. That property is why text width is estimated rather than
 * measured: asking the DOM would make the output depend on which fonts happen
 * to be installed, and a map that reflows between machines cannot be a figure.
 */

import {
  TAU,
  arcPath,
  baseToAngle,
  commas,
  esc,
  n,
  polar,
  radialLine,
  safeColor,
  segmentRanges,
  type Ring,
} from './geometry.ts';
import { placeColumn, type LabelBox } from './labels.ts';
import type {
  Feature,
  Molecule,
  PlacedLabel,
  RenderOptions,
  RenderResult,
  Theme,
} from './types.ts';

const DEFAULT_THEME: Theme = {
  background: 'transparent',
  backboneStroke: '#33383d',
  tickStroke: '#6b7280',
  labelFill: '#22262a',
  titleFill: '#16191c',
  subtitleFill: '#6b7280',
  leaderStroke: '#868d95',
  featureStroke: '#2b2f34',
  featureColors: {
    CDS: '#4f7fd0',
    gene: '#4f7fd0',
    promoter: '#4aa564',
    terminator: '#c05c5c',
    rep_origin: '#c07e2e',
    origin: '#c07e2e',
    misc_feature: '#8b7bb8',
    primer_bind: '#7e8a97',
    protein_bind: '#b87bb0',
    RBS: '#4aa564',
    polyA_signal: '#c05c5c',
    LTR: '#a08040',
    intron: '#87919b',
    default: '#7f8a95',
  },
};

/**
 * How much of the turn a linear molecule's two free ends take out of it.
 *
 * 6% — 21.6 degrees, 10.8 at each end — is the visible break that says "this
 * molecule does not close". It is a module constant rather than a local inside
 * the backbone branch because it is not only the backbone's business: on a
 * linear molecule every base is mapped into the *remaining* 94% (see
 * `mapLength` and `mapOrigin` in `renderCircularMap`), so the drawn break and
 * the coordinate system make the same statement about topology.
 *
 * Until 2026-08-13 only the backbone knew this number. Features, ruler ticks
 * and site ticks all went through `baseToAngle` over the full 360 degrees, so
 * the 6% of bases nearest each end were drawn in the region the backbone had
 * been removed from, and terminal features bridged the break. Worse: a 2,000 bp
 * linear molecule carrying one 1..2000 feature emitted a feature path
 * byte-identical to the same molecule drawn circular — a complete annulus
 * painted over the mid-radius backbone, 2 differing pixels out of 384,400 when
 * the two were rasterised and diffed. A gBlock or a PCR product annotated end
 * to end rendered as a plasmid, which is precisely the reading the gap exists
 * to deny, and `malformed` was empty because nothing was malformed: the
 * renderer drew exactly what it had been asked to draw, on the wrong circle.
 */
const LINEAR_GAP = 0.06 * TAU;

/** Rough advance width of a string, as a multiple of font size.
 *
 *  0.55 is a reasonable mean for the digits and mixed-case Latin that feature
 *  names consist of, in the sans-serif faces SVG viewers fall back to. It only
 *  has to be good enough to reserve space; being wrong by a few percent shifts
 *  a label, it does not break the layout.
 *
 *  **Still an estimate, and still 0.55, on purpose.** The obvious alternative is
 *  Helvetica's own advance tables — `crates/pl-draw/src/pdf.rs` carries them,
 *  and they really would measure the face the root now asks for. Adopting them
 *  here would be a mistake, because this number is spent on exactly one thing:
 *  `margin`, which fixes the ring's radius. `pl-draw` reserves that same margin
 *  with the same 0.55 em/character — see its `label_width`, whose doc names
 *  *this file* as the reason it does not use its own tables there either — so
 *  measuring differently on one side would put the ring at a different radius in
 *  the two renderers. That is precisely the drift
 *  `crates/pl-draw/tests/agreement.rs` exists to catch, and it would buy a
 *  precision this renderer has nothing to spend on: it never fits, crops or
 *  shortens a label, so unlike `pl-draw`'s `fit_label` there is no decision here
 *  that a width off by a few percent can get wrong, and the margin is capped at
 *  30% of the canvas in any case.
 *
 *  What an estimate cannot do is name a face — 0.55 em/character is no
 *  typeface's metric — which is why the `font-family` on the root does. */
function textWidth(s: string, fontSize: number): number {
  return s.length * fontSize * 0.55;
}

function featureColor(f: Feature, theme: Theme): string {
  // `Object.hasOwn`, because `f.type` is a GenBank feature key straight out of
  // a file. A key of `constructor`, `toString` or `__proto__` otherwise reaches
  // through to `Object.prototype` and yields an inherited function.
  const byType =
    (f.type && Object.hasOwn(theme.featureColors, f.type)
      ? theme.featureColors[f.type]
      : undefined) ?? theme.featureColors.default;
  // Never interpolate a caller-supplied colour raw — see `safeColor`.
  return safeColor(f.color, safeColor(byType, DEFAULT_THEME.featureColors.default));
}

function featureSpan(f: Feature, length: number, circular: boolean): number {
  let total = 0;
  for (const seg of f.segments) {
    for (const r of segmentRanges(seg, length, circular)) total += r.to - r.from;
  }
  return total;
}

/** The base a feature's label should point at: the midpoint of its extent. */
function featureMidBase(f: Feature, length: number, circular: boolean): number {
  const ranges = f.segments.flatMap((s) => segmentRanges(s, length, circular));
  if (ranges.length === 0) return 1;
  const first = ranges[0].from;
  let acc = 0;
  const total = ranges.reduce((s, r) => s + (r.to - r.from), 0);
  const half = total / 2;
  for (const r of ranges) {
    const w = r.to - r.from;
    if (acc + w >= half) return r.from + (half - acc) + 1;
    acc += w;
  }
  return first + 1;
}

export function renderCircularMap(
  molecule: Molecule,
  options: RenderOptions = {},
): RenderResult {
  const width = options.width ?? 620;
  const height = options.height ?? 620;
  const fontSize = options.fontSize ?? 12;
  const ringWidth = options.ringWidth ?? 18;
  const labelSpacing = options.labelSpacing ?? 3;
  const originAtTop = options.originAtTop ?? 1;
  const minDeg = options.minFeatureDegrees ?? 1.2;
  // A caller-supplied theme is as untrusted as a feature colour, so every
  // colour in it is laundered once, here, rather than at each use site.
  const merged: Theme = {
    ...DEFAULT_THEME,
    ...options.theme,
    featureColors: {
      ...DEFAULT_THEME.featureColors,
      ...(options.theme?.featureColors ?? {}),
    },
  };
  const theme: Theme = {
    background: safeColor(merged.background, DEFAULT_THEME.background),
    backboneStroke: safeColor(merged.backboneStroke, DEFAULT_THEME.backboneStroke),
    tickStroke: safeColor(merged.tickStroke, DEFAULT_THEME.tickStroke),
    labelFill: safeColor(merged.labelFill, DEFAULT_THEME.labelFill),
    titleFill: safeColor(merged.titleFill, DEFAULT_THEME.titleFill),
    subtitleFill: safeColor(merged.subtitleFill, DEFAULT_THEME.subtitleFill),
    leaderStroke: safeColor(merged.leaderStroke, DEFAULT_THEME.leaderStroke),
    featureStroke: safeColor(merged.featureStroke, DEFAULT_THEME.featureStroke),
    featureColors: merged.featureColors,
  };

  // Non-finite input is rejected outright, not clamped. `Math.max(1, ...)` let
  // Infinity through, `niceStep(Infinity / 12)` returned 1, and the ruler loop
  // became `for (base = 1; base <= Infinity; base += 1)` — 4 GB and a crash in
  // 1.2 s, with every tick reading `MNaN,NaN` anyway. A GenBank record with an
  // absurd LOCUS length reaches this through the package's own example reader.
  const malformed: Array<{ name: string; reason: string }> = [];
  const rawLength = molecule.length;
  const lengthOk = Number.isFinite(rawLength) && rawLength >= 1;
  if (!lengthOk) {
    malformed.push({
      name: molecule.name,
      reason: `molecule length ${rawLength} is not a usable number`,
    });
  }
  const length = lengthOk ? Math.max(1, Math.floor(rawLength)) : 1;
  const circular = (molecule.topology ?? 'circular') === 'circular';
  const features = molecule.features ?? [];
  const sites = molecule.sites ?? [];

  // The base-to-angle mapping for *this* molecule, expressed as the
  // `(length, originAtTop)` pair that every call site below hands to
  // `baseToAngle` — including `arcPath`, which maps its own two endpoints
  // internally and therefore cannot be steered any other way.
  //
  // A circular molecule maps as it always did: the whole turn, rotated so
  // `originAtTop` sits at 12 o'clock.
  //
  // A linear one has to land base 1 on one free end of the backbone arc and the
  // far end of base `length` on the other, i.e. base `b` at
  // `LINEAR_GAP / 2 + (b - 1) / length * (TAU - LINEAR_GAP)`. Rather than write
  // that expression out at five call sites — and be unable to write it at all
  // inside `arcPath`, which lives in `geometry.ts` and takes only these two
  // parameters — it is expressed as the circle it is: the linear molecule plus
  // a phantom stretch of `length * LINEAR_GAP / (TAU - LINEAR_GAP)` bases
  // filling the gap, with the origin rotated back over half of that stretch.
  //
  //   mapLength = length * TAU / (TAU - LINEAR_GAP)
  //   mapOrigin = 1 - LINEAR_GAP * length / (2 * (TAU - LINEAR_GAP))
  //
  // Substituted into `baseToAngle`, `(b - mapOrigin) / mapLength * TAU` is
  // exactly `LINEAR_GAP / 2 + (b - 1) * (TAU - LINEAR_GAP) / length` — an
  // identity, not an approximation. Every base a caller can reach, 1 through
  // `length + 1` (the exclusive end `arcPath` asks about), lands strictly
  // inside `[0, mapLength)`, so the positive modulo in `baseToAngle` never
  // wraps. That is the half that matters most: it was the wrap that turned a
  // whole-length feature into a closed ring, because base `length + 1` came
  // back to base 1's angle and `arcPath` completed the arc the long way round.
  //
  // `originAtTop` is deliberately dropped for a linear molecule. There is no
  // origin to rotate — base 1 is the 5' end, a physical fact rather than a
  // numbering convention — and honouring it would mean cutting the molecule
  // somewhere in its middle and drawing the two halves as though they joined,
  // which is the same lie about topology in a different place.
  const mapLength = circular ? length : length * (TAU / (TAU - LINEAR_GAP));
  const mapOrigin = circular
    ? originAtTop
    : 1 - (LINEAR_GAP * length) / (2 * (TAU - LINEAR_GAP));

  const ring: Ring = { cx: width / 2, cy: height / 2 };

  // Reserve the widest label on each side, plus the leader, so the ring is as
  // large as it can be without labels running off the canvas.
  const longest = Math.max(
    0,
    ...features.map((f) => textWidth(f.name, fontSize)),
    ...sites.map((s) => textWidth(s.name, fontSize)),
  );
  const margin = Math.min(longest + 34, Math.min(width, height) * 0.3);
  const outer =
    options.radiusFraction != null
      ? (Math.min(width, height) / 2) * options.radiusFraction
      : Math.min(width, height) / 2 - margin;
  const ro = Math.max(30, outer);
  const ri = ro - ringWidth;

  const body: string[] = [];
  const overlay: string[] = [];

  // ---- backbone -----------------------------------------------------------
  if (circular) {
    body.push(
      `<circle cx="${n(ring.cx)}" cy="${n(ring.cy)}" r="${n((ro + ri) / 2)}" ` +
        `fill="none" stroke="${theme.backboneStroke}" stroke-width="1.25"/>`,
    );
  } else {
    // A linear molecule drawn on the same ring would be a lie about topology;
    // it gets an arc with visible free ends instead. `mapLength`/`mapOrigin`
    // above put base 1 on the first of those ends and the far end of base
    // `length` on the second, so the break is empty of content by construction
    // rather than by luck — nothing is drawn across it, and a feature that runs
    // to either end of the molecule stops where the backbone stops.
    const p0 = polar(ring, (ro + ri) / 2, LINEAR_GAP / 2);
    const p1 = polar(ring, (ro + ri) / 2, TAU - LINEAR_GAP / 2);
    body.push(
      `<path d="M${n(p0.x)},${n(p0.y)} A${n((ro + ri) / 2)},${n((ro + ri) / 2)} 0 1 1 ${n(p1.x)},${n(p1.y)}" ` +
        `fill="none" stroke="${theme.backboneStroke}" stroke-width="1.25"/>`,
    );
  }

  // ---- ruler --------------------------------------------------------------
  if (options.ruler !== false) {
    const target = options.tickCount ?? 12;
    const step = niceStep(length / target);
    for (let base = step; base <= length; base += step) {
      const a = baseToAngle(base, mapLength, mapOrigin);
      body.push(
        `<path d="${radialLine(ring, a, ri - 4, ri - 9)}" stroke="${theme.tickStroke}" stroke-width="1" fill="none"/>`,
      );
      const p = polar(ring, ri - 18, a);
      body.push(
        `<text x="${n(p.x)}" y="${n(p.y)}" font-size="${n(fontSize * 0.72)}" ` +
          `fill="${theme.tickStroke}" text-anchor="middle" dominant-baseline="middle">${commas(base)}</text>`,
      );
    }
  }

  // ---- features -----------------------------------------------------------
  interface Anchor {
    index: number;
    text: string;
    angle: number;
    weight: number;
  }
  const anchors: Anchor[] = [];

  features.forEach((f, index) => {
    const colour = featureColor(f, theme);
    const span = featureSpan(f, length, circular);
    // Degrees of arc this feature will actually occupy, so over `mapLength`
    // rather than `length`: on a linear molecule the two differ by 6%, and the
    // only consumer is the tiny-feature threshold below, which should be
    // measuring the arc that gets drawn and not one 6% wider than it.
    const degrees = (span / mapLength) * 360;
    const strand = f.strand ?? 'none';
    const ranges = f.segments.flatMap((s) => segmentRanges(s, length, circular));
    if (ranges.length === 0) {
      // No segments at all, coordinates outside the molecule, or NaN. Each of
      // these used to vanish with no signal anywhere in the result.
      malformed.push({
        name: f.name,
        reason:
          f.segments.length === 0
            ? 'no segments'
            : `segments do not fall within 1..${length}`,
      });
      return;
    }

    // The arrow belongs on the terminal piece only, or the feature appears to
    // be several features each pointing somewhere.
    const arrowOn =
      strand === 'forward' ? ranges.length - 1 : strand === 'reverse' ? 0 : -1;

    ranges.forEach((r, i) => {
      const tiny = degrees < minDeg;
      const title = f.note ? `<title>${esc(f.name)} — ${esc(f.note)}</title>` : '';
      const idAttr = f.id ? ` data-feature-id="${esc(f.id)}"` : '';
      if (tiny) {
        // Below a degree or so an arrowhead is smaller than a pixel and reads
        // as dirt on the figure. A tick is honest at any size.
        const a = baseToAngle(r.from + 1, mapLength, mapOrigin);
        body.push(
          `<path d="${radialLine(ring, a, ri, ro)}" stroke="${colour}" ` +
            `stroke-width="1.75" fill="none"${idAttr}>${title}</path>`,
        );
      } else {
        const d = arcPath(
          ring,
          {
            from: r.from,
            to: r.to,
            innerRadius: ri,
            outerRadius: ro,
            arrow: i === arrowOn ? (strand === 'reverse' ? 'start' : 'end') : 'none',
          },
          mapLength,
          mapOrigin,
        );
        const fill = f.fragment ? 'none' : colour;
        const stroke = f.fragment ? colour : theme.featureStroke;
        const dash = f.fragment ? ' stroke-dasharray="4 2"' : '';
        body.push(
          `<path d="${d}" fill="${fill}" stroke="${stroke}" ` +
            `stroke-width="${f.fragment ? 1.5 : 0.6}"${dash}${idAttr}>${title}</path>`,
        );
      }
    });

    anchors.push({
      index,
      text: f.name,
      angle: baseToAngle(featureMidBase(f, length, circular), mapLength, mapOrigin),
      // Bigger features hold their position; small ones give way.
      weight: 1 + Math.log10(1 + span),
    });
  });

  // ---- restriction sites --------------------------------------------------
  sites.forEach((s, i) => {
    if (!Number.isFinite(s.position)) {
      malformed.push({ name: s.name, reason: `position ${s.position} is not a number` });
      return;
    }
    // The same refuse-and-report the feature loop above does, and for the same
    // reason — this branch used to check only that the position was a *number*.
    //
    // Anything finite went straight to `baseToAngle`, whose positive modulo
    // exists to let a base before the chosen origin still land on the circle
    // and will just as happily fold a cut at base 5,000 of a 1,000 bp molecule
    // onto base 1,000. The caption is built from the raw value a few lines
    // down, so the figure then carried the tick at one base and the words
    // `EcoRI (5,000)` beside it: a site the reader can measure off the drawing
    // *and* read off the label, at two different coordinates, neither of them
    // real. On a linear molecule there is not even a modular reading to appeal
    // to — 1,001 of 1,000 is nowhere at all — and `malformed` stayed empty
    // through every one of these, which is the one signal a caller has that a
    // map is incomplete.
    //
    // `geometry.ts` states the principle for the feature path: fabrication is
    // worse than loss. A site the file could not place honestly is dropped from
    // the drawing and named here instead, so a caller can say so beside the
    // figure rather than publish a coordinate that cannot exist.
    if (s.position < 1 || s.position > length) {
      malformed.push({
        name: s.name,
        reason: `position ${s.position} does not fall within 1..${length}`,
      });
      return;
    }
    const a = baseToAngle(s.position, mapLength, mapOrigin);
    body.push(
      `<path d="${radialLine(ring, a, ro, ro + 6)}" stroke="${theme.tickStroke}" stroke-width="1" fill="none"/>`,
    );
    anchors.push({
      index: features.length + i,
      text: `${s.name} (${commas(s.position)})`,
      angle: a,
      weight: s.unique ? 1.5 : 0.6,
    });
  });

  // ---- labels -------------------------------------------------------------
  const lineHeight = fontSize + labelSpacing;
  const right: number[] = [];
  const left: number[] = [];
  anchors.forEach((a, i) => (Math.sin(a.angle) >= 0 ? right : left).push(i));

  const placedLabels: PlacedLabel[] = [];
  const hiddenLabels: string[] = [];
  const pad = 8;

  for (const [side, idxs] of [
    ['right', right],
    ['left', left],
  ] as const) {
    const boxes: LabelBox[] = idxs.map((i) => {
      const a = anchors[i];
      return {
        ideal: polar(ring, ro + 14, a.angle).y,
        height: lineHeight,
        weight: a.weight,
      };
    });
    const { positions, dropped } = placeColumn(boxes, pad + fontSize, height - pad);
    for (const d of dropped) hiddenLabels.push(anchors[idxs[d]].text);
    // `placeColumn` writes NaN for the entries it dropped, and those are
    // already accounted for in `hiddenLabels` — only a NaN that is *not* a
    // deliberate drop indicates something malformed.
    const droppedSet = new Set(dropped);

    idxs.forEach((anchorIdx, k) => {
      const y = positions[k];
      const a = anchors[anchorIdx];
      if (!Number.isFinite(y)) {
        if (!droppedSet.has(k)) {
          // Neither drawn, nor in `labels`, nor in `hiddenLabels` — the one
          // documented signal that a map is incomplete said all was well.
          malformed.push({ name: a.text, reason: 'label position could not be computed' });
        }
        return;
      }
      const dir = side === 'right' ? 1 : -1;
      const labelX = ring.cx + dir * (ro + 26);
      const target = polar(ring, ro + 2, a.angle);
      const elbow = polar(ring, ro + 12, a.angle);

      overlay.push(
        `<path d="M${n(target.x)},${n(target.y)}L${n(elbow.x)},${n(elbow.y)}` +
          `L${n(labelX - dir * 4)},${n(y)}" fill="none" ` +
          `stroke="${theme.leaderStroke}" stroke-width="0.9"/>`,
      );
      overlay.push(
        `<text x="${n(labelX)}" y="${n(y)}" font-size="${n(fontSize)}" ` +
          `fill="${theme.labelFill}" text-anchor="${side === 'right' ? 'start' : 'end'}" ` +
          `dominant-baseline="middle">${esc(a.text)}</text>`,
      );
      placedLabels.push({
        text: a.text,
        x: labelX,
        y,
        anchor: side === 'right' ? 'start' : 'end',
        target,
        featureIndex: a.index,
      });
    });
  }

  // ---- centre -------------------------------------------------------------
  const subtitle = molecule.subtitle ?? `${commas(length)} bp`;
  overlay.push(
    `<text x="${n(ring.cx)}" y="${n(ring.cy - 4)}" font-size="${n(fontSize * 1.25)}" ` +
      `font-weight="600" fill="${theme.titleFill}" text-anchor="middle" ` +
      `dominant-baseline="middle">${esc(molecule.name)}</text>`,
  );
  overlay.push(
    `<text x="${n(ring.cx)}" y="${n(ring.cy + fontSize + 2)}" font-size="${n(fontSize * 0.9)}" ` +
      `fill="${theme.subtitleFill}" text-anchor="middle" ` +
      `dominant-baseline="middle">${esc(subtitle)}</text>`,
  );

  const bg =
    theme.background === 'transparent'
      ? ''
      : `<rect width="${n(width)}" height="${n(height)}" fill="${theme.background}"/>`;

  // Three presentation attributes, each of which decides something the geometry
  // above already assumed. They are checked against the Rust renderer's root in
  // `crates/pl-draw/tests/agreement.rs`, because a root element belongs to no
  // function and a function-by-function harness cannot see it drift.
  //
  //   font-family   The chain is metric-compatible: Nimbus Sans is the free
  //                 clone on Linux, Arial is compatible by design, and whichever
  //                 a viewer resolves the advances are Helvetica's — which is
  //                 what `pl-draw`'s width tables measure and what its PDF and
  //                 EPS exports of the same molecule are typeset in. This used
  //                 to lead with `system-ui, -apple-system, 'Segoe UI'`, so the
  //                 same scene drew in Segoe UI on Windows and San Francisco on
  //                 macOS while the other renderer drew it in Helvetica.
  //   stroke-*      SVG's initial values are `butt` and `miter`. Leaving them
  //                 unstated is not "the default look" but the other value:
  //                 `pl-draw` states `round` for both because its PDF back end
  //                 emits `1 J 1 j`, so every leader elbow and arrowhead point
  //                 differed between the two renderings of one map.
  //   xml:space     Without it the parser collapses a run of whitespace in a
  //                 `<text>` to one space, while `textWidth` above has already
  //                 reserved room for every character of the run. Feature names
  //                 arrive from files — a GenBank `/label=` carries whatever the
  //                 submitter typed — and `esc` strips only the control
  //                 characters XML forbids.
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" width="${n(width)}" height="${n(height)}" ` +
    `viewBox="0 0 ${n(width)} ${n(height)}" font-family="Helvetica, 'Nimbus Sans', Arial, sans-serif" ` +
    `stroke-linecap="round" stroke-linejoin="round" xml:space="preserve">` +
    `<title>${esc(molecule.name)}</title>` +
    bg +
    body.join('') +
    overlay.join('') +
    `</svg>`;

  return { svg, labels: placedLabels, hiddenLabels, malformed };
}

/** Round a tick interval up to something a human would have chosen. */
export function niceStep(raw: number): number {
  if (!Number.isFinite(raw) || raw <= 0) return 1;
  const mag = Math.pow(10, Math.floor(Math.log10(raw)));
  const norm = raw / mag;
  const step = norm <= 1 ? 1 : norm <= 2 ? 2 : norm <= 5 ? 5 : 10;
  return Math.max(1, step * mag);
}
