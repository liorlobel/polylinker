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
  tickStroke: '#8a9199',
  labelFill: '#22262a',
  titleFill: '#16191c',
  subtitleFill: '#6b7280',
  leaderStroke: '#aab1b8',
  featureStroke: '#2b2f34',
  featureColors: {
    CDS: '#4f7fd0',
    gene: '#4f7fd0',
    promoter: '#4aa564',
    terminator: '#c05c5c',
    rep_origin: '#d08a3e',
    origin: '#d08a3e',
    misc_feature: '#8b7bb8',
    primer_bind: '#7e8a97',
    protein_bind: '#b87bb0',
    RBS: '#4aa564',
    polyA_signal: '#c05c5c',
    LTR: '#a08040',
    intron: '#9aa4ae',
    default: '#7f8a95',
  },
};

/** Rough advance width of a string, as a multiple of font size.
 *
 *  0.55 is a reasonable mean for the digits and mixed-case Latin that feature
 *  names consist of, in the sans-serif faces SVG viewers fall back to. It only
 *  has to be good enough to reserve space; being wrong by a few percent shifts
 *  a label, it does not break the layout. */
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
    // it gets an arc with visible free ends instead.
    const gap = 0.06 * TAU;
    const p0 = polar(ring, (ro + ri) / 2, gap / 2);
    const p1 = polar(ring, (ro + ri) / 2, TAU - gap / 2);
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
      const a = baseToAngle(base, length, originAtTop);
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
    const degrees = (span / length) * 360;
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
        const a = baseToAngle(r.from + 1, length, originAtTop);
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
          length,
          originAtTop,
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
      angle: baseToAngle(featureMidBase(f, length, circular), length, originAtTop),
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
    const a = baseToAngle(s.position, length, originAtTop);
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

  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" width="${n(width)}" height="${n(height)}" ` +
    `viewBox="0 0 ${n(width)} ${n(height)}" font-family="system-ui, -apple-system, 'Segoe UI', Helvetica, Arial, sans-serif">` +
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
