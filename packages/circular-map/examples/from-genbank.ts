/**
 * Render a GenBank file to SVG.
 *
 *   node --experimental-strip-types examples/from-genbank.ts pUC19.gb > map.svg
 *
 * The GenBank parsing here is deliberately minimal — enough to demonstrate the
 * renderer on real records, not a parser anyone should depend on. Polylinker's
 * own reader (`pl convert`) is the real one; this exists so the example has no
 * dependencies.
 */

import { readFileSync } from 'node:fs';
import { renderCircularMap } from '../src/render.ts';
import type { Feature, Molecule, Segment, Strand } from '../src/types.ts';

/** Feature keys not worth drawing: they cover the whole molecule and say nothing. */
const SKIP = new Set(['source', 'gene']);

export function parseGenBank(text: string): Molecule {
  const locus = /^LOCUS\s+(\S+)\s+(\d+)\s+bp.*?\b(circular|linear)?\b/im.exec(text);
  const name = locus?.[1] ?? 'unnamed';
  const length = locus ? Number(locus[2]) : 0;
  const topology = locus?.[3] === 'linear' ? 'linear' : 'circular';

  const features: Feature[] = [];
  const featuresBlock = /^FEATURES\s+Location\/Qualifiers\s*$([\s\S]*?)^(?:ORIGIN|CONTIG|\/\/)/m.exec(text);
  if (!featuresBlock) return { name, length, topology, features };

  // Each feature starts at column 5; qualifiers are indented further.
  const entries = featuresBlock[1].split(/\n(?=\s{5}\S)/);
  for (const entry of entries) {
    const head = /^\s{5}(\S+)\s+(.+?)(?=\n\s{21}\/|\n*$)/s.exec(entry);
    if (!head) continue;
    const key = head[1];
    if (SKIP.has(key)) continue;
    const location = head[2].replace(/\s+/g, '');

    const strand: Strand = location.includes('complement')
      ? 'reverse'
      : key === 'CDS' || key === 'promoter' || key === 'terminator'
        ? 'forward'
        : 'none';

    const segments: Segment[] = [];
    for (const m of location.matchAll(/(\d+)\.\.[><]?(\d+)|(?<!\.)\b(\d+)\b(?!\.)/g)) {
      if (m[1] && m[2]) segments.push({ start: Number(m[1]), end: Number(m[2]) });
      else if (m[3]) segments.push({ start: Number(m[3]), end: Number(m[3]) });
    }
    if (segments.length === 0) continue;

    const label =
      /\/(?:label|gene|product|note|standard_name)="?([^"\n]+)"?/.exec(entry)?.[1]?.trim() ?? key;

    features.push({ name: label.slice(0, 28), type: key, strand, segments });
  }
  return { name, length, topology, features };
}

const path = process.argv[2];
if (!path) {
  console.error('usage: from-genbank.ts <file.gb>');
  process.exit(2);
}
const mol = parseGenBank(readFileSync(path, 'utf8'));
const { svg, hiddenLabels } = renderCircularMap(mol, { width: 760, height: 760 });
if (hiddenLabels.length) {
  console.error(`note: ${hiddenLabels.length} label(s) did not fit: ${hiddenLabels.join(', ')}`);
}
console.error(`${mol.name}: ${mol.length} bp, ${mol.features?.length ?? 0} features`);
process.stdout.write(svg);
