/**
 * The data this renderer draws, and nothing more.
 *
 * Deliberately *not* Polylinker's own molecule type. This package is meant to
 * be adopted by seqviz, plascad, OpenCloning and pLannotate, none of which
 * should have to take on our model to use our renderer — so the input is the
 * smallest structure that can describe a plasmid map, with every field a caller
 * would have anyway.
 */

/** 1-based inclusive, matching GenBank and SnapGene. See `docs/PLAN.md` §5.3.1. */
export interface Segment {
  /** First base, 1-based inclusive. */
  start: number;
  /** Last base, 1-based inclusive. May be < start on a circular molecule, which
   *  means the segment runs across the origin. */
  end: number;
}

export type Strand = 'forward' | 'reverse' | 'none';

export interface Feature {
  name: string;
  /**
   * One or more segments. Multiple segments are how a join — exons, a
   * split ORF, an origin-spanning element — is represented. A renderer that
   * assumes one interval per feature silently draws the wrong thing, so this
   * is an array at the lowest level rather than a special case bolted on.
   */
  segments: Segment[];
  strand?: Strand;
  /** GenBank-ish key: `CDS`, `promoter`, `rep_origin`, … Used for default colour. */
  type?: string;
  /** `#rrggbb`. Overrides the type-derived default. */
  color?: string;
  /**
   * Drawn as an unfilled outline rather than a solid arrow — the annotator's
   * signal that only part of a database feature was found. `docs/PLAN.md` §7.7
   * step 8 notes this is something SnapGene does not do well.
   */
  fragment?: boolean;
  /** Free text for a `<title>` tooltip. */
  note?: string;
  /** Opaque passthrough so callers can map a rendered element back to their own object. */
  id?: string;
}

export interface Molecule {
  name: string;
  /** Total bases. Required: a map cannot be drawn without knowing the whole. */
  length: number;
  topology?: 'circular' | 'linear';
  features?: Feature[];
  /** Restriction sites, drawn as ticks with labels outside the ring. */
  sites?: Site[];
  /** Shown under the name in the centre, e.g. "5,386 bp". Defaults to the length. */
  subtitle?: string;
}

export interface Site {
  name: string;
  /** 1-based position of the cut. */
  position: number;
  /** Enzymes cutting once are conventionally emphasised. */
  unique?: boolean;
}

export interface Theme {
  background: string;
  backboneStroke: string;
  tickStroke: string;
  labelFill: string;
  titleFill: string;
  subtitleFill: string;
  leaderStroke: string;
  featureStroke: string;
  /** Fallbacks by GenBank key; `default` is required. */
  featureColors: Record<string, string>;
}

export interface RenderOptions {
  width?: number;
  height?: number;
  /** Outer radius of the feature ring as a fraction of the smaller half-dimension. */
  radiusFraction?: number;
  /** Thickness of the feature ring in px. */
  ringWidth?: number;
  /** Font size for feature labels in px. */
  fontSize?: number;
  /** Minimum vertical space between two stacked labels, in px. */
  labelSpacing?: number;
  /** Draw a scale ruler with base-position ticks. */
  ruler?: boolean;
  /** Approximate number of ruler ticks. */
  tickCount?: number;
  theme?: Partial<Theme>;
  /**
   * Features whose arc is thinner than this many degrees are drawn as a tick
   * rather than an arrow, because an arrowhead below a few pixels reads as
   * noise. They still get a label.
   */
  minFeatureDegrees?: number;
  /** Rotate the map so this base is at 12 o'clock. */
  originAtTop?: number;
}

/** Where a label ended up, so callers can implement hit-testing or tooltips. */
export interface PlacedLabel {
  text: string;
  x: number;
  y: number;
  anchor: 'start' | 'end';
  /** The point on the ring this label describes. */
  target: { x: number; y: number };
  featureIndex: number;
}

export interface RenderResult {
  svg: string;
  labels: PlacedLabel[];
  /**
   * Features whose label could not be placed without overlapping another.
   *
   * Returned rather than quietly dropped: a map that silently omits three
   * labels looks exactly like a map of a plasmid with three fewer features,
   * and the caller is the only one who can decide whether to enlarge the
   * canvas, shrink the font, or show a list beside the figure.
   */
  hiddenLabels: string[];
  /**
   * Features that could not be drawn at all, with the reason.
   *
   * Deliberately **not** folded into `hiddenLabels`, whose contract is "would
   * have overlapped, try a bigger canvas" — advice that cannot help a feature
   * whose coordinates are `NaN`.
   *
   * Before this existed, such a feature was either silently skipped or, worse,
   * *fabricated*: a segment starting past the end of the molecule had both its
   * endpoints clamped independently and collapsed onto a 1 bp arc at the last
   * base, drawn and labelled with the real feature's name. Inventing a feature
   * is worse than losing one, and neither should be silent.
   */
  malformed: Array<{ name: string; reason: string }>;
}
