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
 *  17 significant figures of float noise triples the size for no benefit.
 *
 *  Ties round **away from zero**, not the `Math.round` rule of toward +∞. The
 *  two differ only on negative halves — `Math.round(-0.5)` is `-0` where Rust's
 *  `f64::round` gives `-1` — but a directional tie-break tilts a whole figure
 *  by 0.01 px in +y, and it made the two renderers disagree. Symmetric is both
 *  the less surprising rule and the portable one. */
export function n(v: number): string {
  return (Math.sign(v) * (Math.round(Math.abs(v) * 100) / 100)).toString();
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
  // A segment lying wholly outside the molecule describes nothing here.
  //
  // Clamping both endpoints independently collapsed such a segment onto a 1 bp
  // range at the last base — drawn, and labelled with the real feature's name,
  // at 359.6 degrees. That is fabrication rather than loss, which is worse;
  // `renderCircularMap` detects the case and reports it via
  // `RenderResult.malformed` rather than inventing a feature.
  if (!Number.isFinite(seg.start) || !Number.isFinite(seg.end)) return [];
  if (!Number.isFinite(length) || length <= 0) return [];
  if (seg.start > length && seg.end > length) return [];
  // The mirror image of the line above, and it was missing. A segment wholly
  // *below* base 1 — `0-0`, or a negative pair from an importer that wrote
  // 0-based coordinates — names no base either, but `Math.max(1, ...)` on both
  // endpoints collapsed it onto a 1 bp range at base 1 and drew it there under
  // the real feature's name. The same fabrication as the past-the-end case, at
  // the other end of the molecule.
  if (seg.start < 1 && seg.end < 1) return [];
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
  if (!Number.isFinite(length) || length <= 0) return '';
  const a0 = baseToAngle(spec.from + 1, length, originAtTop);
  let a1 = baseToAngle(spec.to + 1, length, originAtTop);
  // A full-circle feature and a zero-length one are indistinguishable by angle
  // alone, so resolve using the base range that produced them.
  //
  // The test is `!==`, not `> 0`: a wrapping span such as {from: 4900, to: 100}
  // on a 5000 bp circle has a *negative* raw difference and is a legitimate
  // +14.4 degree arc once TAU is added. Rejecting on `> 0` would draw it
  // backwards through -345.6 degrees with an inverted arrowhead. Only an
  // exactly-zero span is empty, and that used to render as a complete annulus —
  // an 18 px ring where the caller asked for nothing.
  if (spec.to === spec.from) return '';
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

/**
 * Escape text for inclusion in XML character data or an attribute value.
 *
 * Control characters are dropped rather than escaped. XML 1.0 forbids most of
 * them outright — `&#1;` is just as illegal as a raw U+0001 — so escaping would
 * have produced a document that still fails to parse. Feature names come out of
 * binary `.dna` payloads where a stray control byte is entirely possible, and
 * one such byte made the whole rendered map unparseable.
 *
 * The dropped range is exactly the illegal one and no wider. U+007F (DEL) used
 * to be stripped here as well, which was a silent disagreement with
 * `pl_draw::esc`: DEL is a *legal* XML 1.0 character — the Char production is
 * `#x9 | #xA | #xD | [#x20-#xD7FF] | ...` and #x7F falls inside [#x20-#xD7FF],
 * confirmed by parsing it literally in both a text node and an attribute value
 * with expat 2.8.1 — so the Rust renderer kept it while this one deleted it, and
 * one feature name rendered two ways depending on which renderer drew the
 * figure. Aligned on keeping it: this function's job is to produce a parseable
 * document, not to censor characters the specification allows. Dropping a legal
 * character is also the direction that loses data silently.
 *
 * `agreement.rs::xml_escaping_agrees` pins the two implementations together
 * over the whole 0x00-0x1f range plus DEL; the corpus previously reached three
 * control codepoints, all of which happened to agree.
 *
 * Written with \u escapes rather than literal control bytes on purpose. The
 * literal form renders as invisible or replacement glyphs in a diff, which is
 * how a stray DEL in a character class went unnoticed here.
 */
export function esc(s: string): string {
  return s
    .replace(/[\u0000-\u0008\u000B\u000C\u000E-\u001F]/g, '')
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

/**
 * A colour that is safe to interpolate into an SVG attribute, or `fallback`.
 *
 * Colours are **not** author-controlled. They arrive in `.dna` and GenBank files
 * downloaded from Addgene and emailed between labs, and this renderer's whole
 * purpose is to produce SVG that a page embeds inline. A feature whose colour is
 * `#fff" onload="…` therefore closes the `fill` attribute and adds an event
 * handler to the caller's page:
 *
 * ```
 * <path d="…" fill="#fff" onload="alert(1)" x="" stroke="#2b2f34">
 * ```
 *
 * Escaping alone would be safe but produces a nonsense colour; a colour field
 * has a small, checkable grammar, so this validates against it and falls back
 * rather than rendering something the caller did not mean. Anything unrecognised
 * is refused, which is the correct direction to fail for a value that is only
 * ever cosmetic.
 *
 * There is a second job here, and it went unnoticed until 2026-08-13. What this
 * function returns is interpolated into `fill="..."` and `stroke="..."` without
 * passing through `esc`, so a colour is the one string in a figure that reaches
 * an attribute value unescaped, and this is therefore the last place that can
 * keep a character XML 1.0 forbids out of the document. The functional-notation
 * class below used to admit VT and FF. One of those bytes in a colour out of a
 * downloaded file does not produce a wrong fill: it produces a file no
 * conformant parser will open at all, which is the whole figure lost rather
 * than one feature miscoloured. See the note on that line, and
 * `pl_draw::safe_color`, corrected in the same change.
 */
export function safeColor(value: string | undefined, fallback: string): string {
  // Type-checked, not just truthiness-checked. This is the laundering boundary
  // for values that came out of a file, and a `type` of `constructor` or
  // `__proto__` made a theme lookup return an inherited *function*, whose
  // `.trim()` threw and took the whole render down.
  if (typeof value !== 'string' || value === '') return fallback;
  const v = value.trim();
  // #rgb, #rgba, #rrggbb, #rrggbbaa
  if (/^#(?:[0-9a-fA-F]{3,4}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})$/.test(v)) return v;
  // rgb()/rgba()/hsl()/hsla() with only numbers, %, commas, slashes and spaces.
  //
  // ASCII whitespace spelled out rather than `\s`, which in JavaScript also
  // matches NBSP, U+2028 and the rest of the Unicode space category. The Rust
  // port checks bytes and would refuse those; rather than carry a divergence
  // that only shows up on input neither implementation should accept, both
  // refuse it. See `crates/pl-draw/tests/agreement.rs`.
  //
  // Tab, LF and CR only. VT (`\v`, U+000B) and FF (`\f`, U+000C) were in this
  // class until 2026-08-13, and neither is an XML 1.0 `Char` — the production
  // is `#x9 | #xA | #xD | [#x20-#xD7FF] | ...`. A colour such as
  // `rgb(79,127,208\v)` out of a `.dna` or GenBank file was therefore
  // returned unchanged and written straight into a `fill` attribute, and the
  // whole rendered document then failed to parse — not one wrong colour, no
  // figure at all. `esc` above has dropped exactly those two codepoints, in
  // exactly this notation, since it was written; this line had simply never
  // been read next to it. `.trim()` hides the easy cases, both bytes being
  // Unicode whitespace, but it cannot reach an interior one.
  //
  // Removed here and from `pl_draw::safe_color` in one change, with
  // `crates/pl-draw/tests/agreement.json` regenerated in it, so the two
  // implementations stay pinned to each other rather than drifting apart on a
  // case the fixture used to record as agreed.
  if (/^(?:rgba?|hsla?)\([0-9eE+\-.,%/ \t\n\r]*\)$/.test(v)) return v;
  // A bare CSS colour keyword, plus the two SVG paint keywords.
  if (/^[a-zA-Z]{1,32}$/.test(v)) return v;
  return fallback;
}

/** `1234567` → `1,234,567`, without pulling in `Intl` for one call. */
export function commas(v: number): string {
  return Math.round(v).toString().replace(/\B(?=(\d{3})+(?!\d))/g, ',');
}
