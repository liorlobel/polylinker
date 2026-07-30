# Trademarks

Polylinker is an independent project. It is **not affiliated with, endorsed by,
or sponsored by** any company named on this page.

## Marks referenced

| Mark | Owner (as understood) |
|---|---|
| SnapGene | GSL Biotech LLC (Dotmatics / Siemens group) |
| Benchling | Benchling, Inc. |
| Geneious | Biomatters Ltd. |
| Vector NTI | Thermo Fisher Scientific |
| Gibson Assembly, NEBuilder HiFi | New England Biolabs, Inc. |
| In-Fusion | Takara Bio Inc. |
| Gateway, TOPO | Thermo Fisher Scientific |
| Golden Gate | used descriptively for Type IIS assembly |
| IBM Plex | IBM Corp. |
| Bitstream, Vera | Bitstream, Inc. |
| Noto | Google Inc. |
| Ubuntu, Canonical | Canonical Ltd. |

All other trademarks are the property of their respective owners.

## Marks carried inside the shipped binary

The last four entries above are different in kind from the rest of this page.
They are not referenced in documentation — they are **embedded in
`polylinker.exe`**, in the `name` tables of font files it carries. That is an
obligation rather than a courtesy for at least one of them, and it is separate
from the copyright notice recorded in NOTICE.

The list is the four faces whose `name` ID 7 is non-empty, read from the shipped
`.ttf` files rather than from anyone's documentation. Hack's ID 7 is empty and
emoji-icon-font's is empty, which is why neither appears; Hack's mark obligation
comes through the Bitstream Vera licence text instead.

**IBM Plex**, verbatim from `name` ID 7 of both IBM Plex Mono 2.005 and IBM Plex
Sans 3.005, added when those faces were vendored on 2026-07-30:

> IBM Plex® is a trademark of IBM Corp, registered in many jurisdictions
> worldwide.

**Bitstream Vera**, reached through Hack, which `epaint_default_fonts` embeds.
The Bitstream Vera licence requires the copyright *and trademark* notices to
appear in all copies of the Font Software typefaces, and becomes null and void
for modified fonts distributed under the Vera names. Polylinker embeds it
unmodified. Its text is at `bins/pl-gui/fonts/Hack-MIT-and-BitstreamVera.txt`
and ships with every release.

**Noto**, verbatim from `name` ID 7 of Noto Emoji 1.05, which
`epaint_default_fonts` embeds and which supplies U+26A0, the hidden-cut-sites
warning marker:

> Noto is a trademark of Google Inc.

**Ubuntu and Canonical**, verbatim from `name` ID 7 of Ubuntu Light 0.83, which
`epaint_default_fonts` embeds and which remains in both font chains behind the
Plex faces:

> Ubuntu and Canonical are registered trademarks of Canonical Ltd.

Both Plex faces ship byte-for-byte as their upstream releases, under the names
their licence permits them to carry. Neither mark appears in this project's own
name, in any package or repository name, in any user-facing claim of endorsement,
or written into any file Polylinker generates — the last being the restriction
most of the rest of this page is about.

## How this project uses them

Only **nominatively** — to state accurately what Polylinker does, in the
smallest amount necessary, in a way that does not imply endorsement. This is the
*New Kids on the Block v. News America* (9th Cir. 1992) three-part test.

**Acceptable, and used here:**

- "Reads and writes SnapGene `.dna` files."
- "Imports files created by SnapGene."
- An accurate, non-disparaging feature-comparison table.
- `"snapgene"` as a format identifier string in an API — as Biopython already
  ships in `SeqIO.parse(f, "snapgene")`.

**Never used, and rejected in review:**

- Any project, package, repository or domain name containing another party's
  mark: *OpenSnapGene*, *SnapGene Lite*, *FreeSnapGene*, *SnapGene-NG*,
  `snapgene-*.org`.
- "SnapGene-compatible" as a *product name*, rather than as a descriptive
  sentence.
- Another product's logo, icon, colour scheme, trade dress or screenshots.
- Unverifiable disparagement.
- **Writing another party's mark into files Polylinker generates**, beyond the
  bare magic bytes the format itself requires. This is precisely the trigger in
  the Autodesk / Open Design Alliance dispute: the format cloning survived, and
  it was the *branding strings written into generated files* that drew the
  trademark claim.

## Preferred generic terms

Simulating a laboratory method in software is not practising the method, but the
method names are registered marks. Use the generic term:

| Instead of | Write |
|---|---|
| Gibson Assembly®, NEBuilder HiFi® | homology-overlap assembly / isothermal overlap assembly |
| Golden Gate (as a brand) | Type IIS assembly |
| Gateway® | BP / LR recombination |
| In-Fusion® | homology-overlap assembly |
| TOPO® | topoisomerase-mediated cloning |
