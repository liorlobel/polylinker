/**
 * `@polylinker/circular-map` — plasmid maps as SVG, from a plain object.
 *
 * ```ts
 * import { renderCircularMap } from '@polylinker/circular-map';
 *
 * const { svg } = renderCircularMap({
 *   name: 'pUC19',
 *   length: 2686,
 *   topology: 'circular',
 *   features: [
 *     { name: 'AmpR', type: 'CDS', strand: 'reverse',
 *       segments: [{ start: 1626, end: 2486 }] },
 *     { name: 'lacZα', type: 'CDS', strand: 'reverse',
 *       segments: [{ start: 146, end: 469 }] },
 *   ],
 * });
 * ```
 *
 * No DOM and no runtime dependencies, so it works in a browser, in Node, and in
 * a CI job that rasterises figures. `docs/PLAN.md` §3 makes the case for
 * publishing it standalone: the renderer is the visible quality signal in this
 * product category, no open component does it well, and building it separately
 * makes it usable by seqviz, plascad, OpenCloning and pLannotate rather than
 * only by us.
 */

export { renderCircularMap, niceStep } from './render.ts';
export { isotonic, placeColumn } from './labels.ts';
export type { LabelBox, ColumnResult } from './labels.ts';
export {
  TAU,
  arcPath,
  baseToAngle,
  commas,
  esc,
  n,
  polar,
  safeColor,
  segmentRanges,
  type Point,
  type Ring,
  type ArcSpec,
} from './geometry.ts';
export type {
  Feature,
  Molecule,
  PlacedLabel,
  RenderOptions,
  RenderResult,
  Segment,
  Site,
  Strand,
  Theme,
} from './types.ts';
