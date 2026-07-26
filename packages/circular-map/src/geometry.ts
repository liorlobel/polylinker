/**
 * Turning base positions into SVG paths.
 *
 * # Conventions, chosen to match what biologists already read
 *
 * Base 1 sits at 12 o'clock and coordinates increase **clockwise**. That is what
 * SnapGene, Benchling and every plasmid figure in a methods section do, so a map
 * drawn the other way is not a stylistic variation, it is wrong-looking.
 *
 * Screen coordinates have y pointing down, so `(cx + r·sin θ, cy − r·cos θ)`
 * gives 12 o'clock at `θ = 0` and moves clockwise as `θ` grows.
 */

import type { Segment } from './types.ts';

export const TAU = Math.PI * 2;

export interface Point {
  x: number;
  y: number;
}

export interface Ring {
  cx: number;
  cy: number;
}

/** Angle, in radians clockwise from 12 o'clock, for a 1-based base position. */
export function baseToAngle(base: number, length: number, originAtTop = 1): number {
  if (length <= 0) return 0;
  const shifted = base - originAtTop;
  // Positive modulo: a base before the chosen origin still lands on the circle.
  const frac = ((shifted % length) + length) % length;
  return (frac / length) * TAU;
}

export function polar(ring: Ring, radius: number, angle: number): Point {
  return {
    x: ring.cx + radius * Math.sin(angle),
    y: ring.cy - radius * Math.cos(angle),
  };
}

/** Trim to a sane number of decimals — SVG files are mostly coordinates, and
 *  17 significant figures of float noise triples the size for no benefit. */
export function n(v: number): string {
  return (Math.round(v * 100) / 100).toString();
}

function pt(p: Point): string {
  return `${n(p.x)},${n(p.y)}`;
}

/**
 * A segment on a circular molecule, split into angular ranges.
 *
 * A feature whose `end` is before its `start` runs across the origin, and is
 * two arcs on screen even though it is one thing biologically. Returning an
 * array rather than one range is what stops the origin case being a special
 * case everywhere downstream.
 */
export function segmentRanges(
  seg: Segment,
  length: number,
  circular: boolean,
): Array<{ from: number; to: number }> {
  const s = Math.max(1, Math.min(seg.start, length));
  const e = Math.max(1, Math.min(seg.end, length));
  if (s <= e) return [{ from: s - 1, to: e }];
  if (!circular) {
    // On a linear molecule a reversed interval cannot wrap; the only honest
    // reading is the span it names.
    return [{ from: Math.min(s, e) - 1, to: Math.max(s, e) }];
  }
  return [
    { from: s - 1, to: length },
    { from: 0, to: e },
  ];
}

export interface ArcSpec {
  /** 0-based half-open base range. */
  from: number;
  to: number;
  innerRadius: number;
  outerRadius: number;
  /** Draw an arrowhead at the high-coordinate end, the low end, or neither. */
  arrow: 'end' | 'start' | 'none';
  /** Arrowhead length along the arc, in px. */
  arrowLength?: number;
  /** How far the barbs stick out past the ring, in px. */
  barb?: number;
}

/**
 * The SVG path for one feature arc, with its arrowhead.
 *
 * The arrowhead is clamped to half the arc so that a short feature degrades to
 * a triangle rather than inverting itself — the classic artefact where a 30 bp
 * feature renders as a bow tie.
 */
export function arcPath(
  ring: Ring,
  spec: ArcSpec,
  length: number,
  originAtTop = 1,
): string {
  const { innerRadius: ri, outerRadius: ro } = spec;
  const a0 = baseToAngle(spec.from + 1, length, originAtTop);
  let a1 = baseToAngle(spec.to + 1, length, originAtTop);
  // A full-circle feature and a zero-length one are indistinguishable by angle
  // alone, so resolve using the base range that produced them.
  if (a1 <= a0) a1 += TAU;
  const sweep = a1 - a0;

  const arrowLen = spec.arrow === 'none' ? 0 : (spec.arrowLength ?? 8);
  const mid = (ri + ro) / 2;
  // Arc length ≈ r·θ, so an arrowhead of `arrowLen` px subtends this angle.
  let arrowAngle = arrowLen / Math.max(mid, 1e-6);
  arrowAngle = Math.min(arrowAngle, sweep * 0.5);

  const barb = spec.barb ?? Math.min(2.5, (ro - ri) * 0.35);
  const parts: string[] = [];

  if (spec.arrow === 'end') {
    const aBase = a1 - arrowAngle;
    parts.push(`M${pt(polar(ring, ro, a0))}`);
    parts.push(arcTo(ring, ro, a0, aBase, 1));
    if (arrowAngle > 0) {
      parts.push(`L${pt(polar(ring, ro + barb, aBase))}`);
      parts.push(`L${pt(polar(ring, mid, a1))}`);
      parts.push(`L${pt(polar(ring, ri - barb, aBase))}`);
    }
    parts.push(`L${pt(polar(ring, ri, aBase))}`);
    parts.push(arcTo(ring, ri, aBase, a0, 0));
  } else if (spec.arrow === 'start') {
    const aBase = a0 + arrowAngle;
    parts.push(`M${pt(polar(ring, ro, a1))}`);
    parts.push(arcTo(ring, ro, a1, aBase, 0));
    if (arrowAngle > 0) {
      parts.push(`L${pt(polar(ring, ro + barb, aBase))}`);
      parts.push(`L${pt(polar(ring, mid, a0))}`);
      parts.push(`L${pt(polar(ring, ri - barb, aBase))}`);
    }
    parts.push(`L${pt(polar(ring, ri, aBase))}`);
    parts.push(arcTo(ring, ri, aBase, a1, 1));
  } else {
    parts.push(`M${pt(polar(ring, ro, a0))}`);
    parts.push(arcTo(ring, ro, a0, a1, 1));
    parts.push(`L${pt(polar(ring, ri, a1))}`);
    parts.push(arcTo(ring, ri, a1, a0, 0));
  }
  parts.push('Z');
  return parts.join('');
}

/** An `A` command from `fromAngle` to `toAngle` at a fixed radius. */
function arcTo(
  ring: Ring,
  radius: number,
  fromAngle: number,
  toAngle: number,
  sweepFlag: 0 | 1,
): string {
  const p = polar(ring, radius, toAngle);
  const delta = Math.abs(toAngle - fromAngle);
  const large = delta > Math.PI ? 1 : 0;
  // An arc that has come all the way round would be drawn as a zero-length
  // line, because its endpoints coincide; split it so the shape survives.
  if (delta >= TAU - 1e-9) {
    const half = polar(ring, radius, fromAngle + (sweepFlag === 1 ? Math.PI : -Math.PI));
    return (
      `A${n(radius)},${n(radius)} 0 0 ${sweepFlag} ${pt(half)}` +
      `A${n(radius)},${n(radius)} 0 0 ${sweepFlag} ${pt(p)}`
    );
  }
  return `A${n(radius)},${n(radius)} 0 ${large} ${sweepFlag} ${pt(p)}`;
}

/** A straight radial line, for restriction-site ticks and ruler marks. */
export function radialLine(
  ring: Ring,
  angle: number,
  from: number,
  to: number,
): string {
  const a = polar(ring, from, angle);
  const b = polar(ring, to, angle);
  return `M${pt(a)}L${pt(b)}`;
}

/** Escape text for inclusion in XML character data or an attribute value. */
export function esc(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

/** `1234567` → `1,234,567`, without pulling in `Intl` for one call. */
export function commas(v: number): string {
  return Math.round(v).toString().replace(/\B(?=(\d{3})+(?!\d))/g, ',');
}
