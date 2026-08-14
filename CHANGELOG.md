# Changelog

This file exists because of the updater. Since v0.1.2 a copy of Polylinker can
tell you that a newer version exists, and until now there was nowhere to find
out what is in it before saying yes. A version number on its own is not
information, and "there is an update" without "here is what changed" is a
request to trust rather than to check.

The format is [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Versions
are three numbers compared as numbers, not as text: `pl update` refuses anything
that is not numerically newer than the copy running it, so 0.1.10 is an upgrade
from 0.1.2 and never the reverse. That rule and the attack it exists to stop —
a signed rollback onto an older release with public vulnerabilities — are in
[`crates/pl-update/src/version.rs`](crates/pl-update/src/version.rs). This is
0.x, and no compatibility promise has been made yet.

**Nothing in any of these releases is code-signed**, and nothing above them is
going to be: code signing came off the roadmap on 2026-08-06, so this is a
settled property of the project rather than a run of versions waiting for a
certificate. Windows SmartScreen and macOS Gatekeeper do not recognise the
publisher and say so, on every version below. The signing described under 0.1.2
covers the release *manifest*, which is a different thing;
[`docs/RELEASING.md`](docs/RELEASING.md) is precise about which guarantee is
which.

## [Unreleased]

### Added

- **A fourth platform, `windows-arm64`, shipping both a zip and an MSI:**
  `polylinker-<version>-windows-arm64.zip` and
  `polylinker-<version>-windows-arm64.msi`, built and tested natively on
  GitHub's `windows-11-arm` runners, which are free for public repositories as
  this one is. Both files and not the zip alone — the `.msi` is the file a
  Windows reader is told to take first, and an architecture that gets half of
  what the other one gets reads to a user as an architecture where the software
  does not really work.
  It is also the file `pl update` hands over on Windows, so a zip-only ARM64
  release would have had to keep declining to update.

  **What this entry is careful not to say.** It does not say that Windows ARM64
  is supported, verified or tested beyond one thing: a CI leg. **No person has
  ever run a Polylinker binary on Windows ARM64** — not the editor's window, not
  `pl`, not the installer — and nobody here can. `aarch64-pc-windows-msvc` is
  installed on the maintainer's machine and genuinely compiles this workspace's
  library crates; every *binary* crate then stops at
  `linker 'link.exe' not found`, because a rustup target is a standard library
  and not a toolchain, and the ARM64 linker is a separate Visual Studio
  component. Measured on that machine rather than assumed: its MSVC install
  offers `x64` and `x86` target linkers under `VC\Tools\MSVC\<ver>\bin\Hostx64\`
  and no `arm64` one. That is also why cross-compiling was refused as the
  shipping path — a cross-built artifact no machine of that architecture has
  executed is a guess with a checksum on it — and why there is no local
  rehearsal of an ARM64 release and no local reproduction of an ARM64 bug
  report.

  Consequently every timing, every corpus figure and every "built and verified
  natively" sentence in [`README.md`](README.md) is an x86-64 measurement.
  Those sentences now name the architecture, which they did not before this
  release, because the moment a fourth platform appeared they stopped being
  about all of them.

  **What the ARM64 leg checks and what it leaves unchecked is a table in
  [`docs/RELEASING.md`](docs/RELEASING.md)**, under *Windows on ARM64*, and not
  a sentence here. It is a real difference from what `windows-x64` gets on every
  commit, and a changelog bullet is exactly the place such a difference gets
  rounded off to "it works now". The short form: the ARM64 leg lives in
  `ci.yml`'s `test` job and runs `fmt`, `clippy` and the whole Rust test suite
  natively on every commit; it does **not** run `tools/ci.ps1`, so the gate this
  project counts the steps of has three legs and none of them is this
  architecture, and the archive and installer are built, verified, installed and
  uninstalled only when a release is cut.

  **Two differences a user can hit, stated here rather than left in the
  table.** The ARM64 archive is permitted to ship **without the Python
  extension module** — `crates/pl-py` links against CPython, Windows resolves
  those symbols at link time, and whether the ARM64 runner carries an ARM64
  CPython was never established from a machine that cannot link ARM64 at all.
  The permission is not a silent `if`: `tools/release.ps1` takes the omission
  only for `windows-arm64`, only with a reason naming that label, and only if no
  `.pyd` was in fact built; it writes an `omitted:` line into `SHA256SUMS.txt`,
  and `tools/check-archive.ps1` holds the archive to that line and prints the
  waiver whether the archive passes or fails. And the ARM64 binaries **do not
  carry the static C runtime the x86-64 ones carry**: `.cargo/config.toml`
  scopes `-C target-feature=+crt-static` to `x86_64-pc-windows-msvc` and
  declares nothing for `aarch64-pc-windows-msvc`, so they import
  `VCRUNTIME140.dll` — not part of Windows, redistributable needs administrator
  rights, and the exact dependency that config file exists to remove for the
  user `docs/PLAN.md:120` describes. The step that asserts its absence lives in
  the gate, and this leg does not run the gate.

  One behaviour change follows from the artifact existing. `pl update` on ARM64
  used to **decline**, and correctly:
  [`crates/pl-update/src/flow.rs`](crates/pl-update/src/flow.rs) maps a platform
  to an artifact through a `#[cfg]` cascade with an explicit `None` fallback, so
  a platform the release workflow does not build gets a refusal rather than an
  x86-64 `.msi`. It failed closed. Adding the ARM64 arm and publishing the ARM64
  file are therefore one change and not two: an arm naming a file no release
  carries converts that clean refusal into a 404.

## [0.10.3] - 2026-08-14

**The browser prototype told users that no enzyme cut their plasmid, when every
one did.** That is the whole reason this release exists, and it is the only
user-facing false scientific statement three audit rounds have found. Twelve
findings from `docs/AUDIT-2026-08-14-r3.md` are fixed here.

### Fixed

- **`prototype/dna-reader.html` reported a scan it never ran as a result.** The
  page skips its restriction digest above 400,000 bases to keep the main thread
  responsive; the Enzymes tab then computed "enzymes that do not cut" as every
  enzyme absent from an empty result and rendered **"58 of the 58 enzymes in the
  set do not cut this molecule at all"**, naming all 58. Measured against the
  shipped binary on a 500,000 bp circle: `pl digest` reports **zero**
  non-cutters, 47 enzymes cutting a hundred times or more, AarI cutting 56.
  Exactly inverted, and presented as a measurement.

  The map hint one function away already said "enzyme scan skipped". The tab
  contradicted it in the same page load.

  **"Not scanned" is now a value that cannot be read as an answer.** `null`
  rather than `[]`, chosen deliberately: `null.filter` throws, `[].filter`
  quietly yields zero, and it was the quiet zero that turned a skip into a
  claim. All five consumers were carrying the same conflation. Two further
  instances fell out of the fix — `coreDigest` also returned `[]` when the wasm
  call declines, and a file that declares 3,000 bp with no bases at all was
  being digested, coming back empty, and reported as a molecule 58 enzymes had
  been tried against.

  **The reason it survived four audits is that `prototype/` appeared zero times
  in `tools/ci.ps1`.** There is a gate step now, and it carries floors with
  their reasons attached: the template must be at least 900 lines, and
  `check_page.js` must make at least fifteen assertions, "because a harness with
  most of them deleted still ends by printing ALL CHECKS PASSED".

- **A feature colour from an untrusted file went verbatim into a live CSS
  `style` attribute**, so a crafted `.dna` could make a page that promises
  "nothing is uploaded, no network" fetch a remote URL. It now passes through an
  accept-list transcribed arm-for-arm from `pl-draw`'s `safe_color`, verified
  against the Rust by running both over 26 vectors: 26/26 identical verdicts.

- **A file with no bases reported "GC 0.0%"** where the core deliberately
  returns null, and the page's footer claimed a fifty-enzyme set while the same
  page computed and displayed 58.

- **An exported PDF map had no background while `--check-contrast` certified it
  against `#ffffff`.** `pdf::pdf_at` now paints its ground, the audit measures
  against the crate constant rather than a hard-coded string, and the passing
  line names the colour it used. **v0.10.2's release note claimed this was
  already done**; only the SVG half had shipped, and `pl-draw`'s own doc said so
  under a heading reading "What still does not paint it" while the changelog
  said otherwise. Rather than narrow the claim, the claim was made true.

- **`sfnt.rs`'s ligature guard could not see a lookup the shaper applies through
  LangSys `requiredFeatureIndex`** — a required feature is not in the feature
  list the guard walked, so a face carrying its substitution there would pass
  unnoticed. The property held for the vendored faces and still does; what
  changed is that the guard would now notice if it stopped.

- **The ring's arrowhead direction and a feature arc's circular coordinate are
  asserted for the first time.** Every ring feature could previously be rotated
  a thousand bases, or have its arrowhead moved to the wrong end, with the
  suite green — the same defect class that produced a real wrong coordinate in
  each of the two previous rounds.

- **A gate step reported ok after running zero tests** when its glob stopped
  matching, and the gate attributed the `<File>` ban to a portable step when
  only the Windows-and-`dist`-gated one asserts it.

- **The README and `docs/PLAN.md` named the wrong renderer** for the browser
  prototype's map — sending anyone fixing a prototype map bug to code the page
  never loads. Correcting it required stating what the sentence was hiding:
  **four things in this repository draw a plasmid map**, and of the six pairs
  between them, two are checked and four are not. The prototype's own renderer
  is checked against nothing. Both sentences were false on the day they were
  written, twelve days after the code they described had changed.

### Verification

Every fix carries a test or an executed proof. **The five Rust mutations were
run centrally and all five went red**, each targeted by line number because two
of the mutation strings also occur inside the doc comment that quotes them — a
trap one agent hit on its own first run and documented. The four prototype fixes
were proven in jsdom, each mutation reddening only its own assertions.

One honest note, because this project's first house rule cuts both ways: the
restore step of the mutation harness failed on `sfnt.rs` and left a deliberately
inverted guard in the working tree. It was caught by inspection rather than by
the harness. The restore now retries, verifies the bytes by SHA-256, and prints
the line it restored.


- **An exported PDF map had no background, but `--check-contrast` certified it
  against `#ffffff`.** `pdf::pdf_at` opened its content stream `q`, `1 J 1 j` and
  went straight to the backbone, so the file was transparent: a real
  `pl export --pdf` content stream is 9,989 bytes with a ` re` operator count of
  **0**. Composited onto a white page the certificate is true, which is why this
  survived as long as it did; on the dark slide a talk uses, the label ink is
  **1.05:1** against a requirement of 4.5 and the backbone **1.35:1** against 3.0.

  The stream now opens with the ground — `1 1 1 rg 0 0 w h re f`, bracketed by
  `q`/`Q` so white cannot leak into the graphics state, in SCENE units inside the
  `cm` matrix so a `--mm` printed width scales it once and not twice — with the
  colour taken from `pl_draw::PAPER` through the same `rgb` every other paint
  operator uses. All four writers now paint the same ground: EPS always did, PNG
  takes it as a parameter, SVG gained one in v0.10.2, PDF is the last and is
  here.

  **The certificate names the background it measured.** `pl export
  --check-contrast` passes `pl_draw::PAPER` rather than its own literal
  `"#ffffff"`, and a passing figure now prints `contrast ok on #ffffff (WCAG 2.2
  AA)`. The bare `contrast ok (WCAG 2.2 AA)` it replaces was byte-identical for
  the format that painted white, the format that painted nothing and the format
  that already had a ground, so it certified the transparent PDF in exactly the
  words it certified the SVG — and `#ffffff` reached the user only on a *failing*
  line, which is the one case where the reader can already see the colour.

  **v0.10.2's release note announced this fix on the day of a release that did
  not contain it**, and claimed the certificate "now states the background it
  assumed" when no format said any such thing — while `crates/pl-draw/src/lib.rs`,
  in the same commit, said the opposite under a heading reading "What still does
  not paint it". That entry has now been corrected twice: on 2026-08-14 to admit
  the PDF half had not shipped, and again once it had, because the admission went
  stale the moment the fix landed. Both corrections are left in the 0.10.2 entry
  rather than tidied away. A release note that runs ahead of the code is the
  failure this project's prose-versus-code rule exists to stop, and this is the
  case that rule was written from.

  Pinned by `pdf::file_tests::the_pdf_carries_the_ground_its_contrast_certificate_is_measured_against`,
  the twin of the SVG test v0.10.2 added, sharing its fixture so the two back ends
  are asserted to paint the same ground on the same scene. It reads the colour
  back out of the assembled file rather than asking the writer what it wrote,
  re-runs the audit against *that* string, and checks the rectangle covers the
  MediaBox, precedes the first `m` and `BT`, and stays in scene units at 89 mm.

## [0.10.2] - 2026-08-14

**v0.10.1 misplaced features on a double digest, and this is the release that
stops it.** A second audit round — eleven units over the v0.10.1 tree, and the
first round of the day that ran the repository's own oracles rather than reading
the code — raised 41 findings, of which 14 survived adversarial refutation. All
14 are fixed here.

### Fixed

- **A flipped fragment moved every feature laid down behind it.** `build`
  advanced its product cursor by the fragment's WATSON length in every case,
  while a flipped fragment contributes its CRICK length. The two differ by the
  difference in the two ends' overhang widths, so on a digest by enzymes of
  unequal width every feature after a flipped fragment shifted by that much.
  Measured over ten common enzymes swept pairwise on a four-fragment circle:
  **43 of 840 carried features landed on bases that are not their own** — shifted
  by two for NdeI against a four-base cutter, by four for blunt SmaI.

  v0.10.1 guarded this behind a comment asserting that "no digest is known that
  both produces such a fragment and seals it flipped, so it is guarded rather
  than corrected here". That sentence shipped a few hours earlier and was false
  when it was written. `place`'s bounds check does not catch it, because the
  shifted coordinate is still inside the product: the feature is placed,
  silently, on the wrong bases. A double digest is the most ordinary operation
  this panel performs.

  **A two-fragment fixture cannot see this**, which is why it survived: two
  fragments of a double digest carry one end of each enzyme, so neither can be
  sealed either way round and nothing ever flips. The test now sweeps four
  fragments and ASSERTS its own reach counters, so an edit that stops reaching
  the case fails loudly rather than going quietly green.

- **A multi-exon feature on an inverted fragment spliced its exons backwards.**
  Mirroring each span is not enough; the segment LIST is in join order and had to
  be reversed with it, or the feature exports as `complement(join(hi,lo))` and
  every outside reader — including this program's own GenBank reader — splices it
  in the wrong order.

- **`pl find` printed the retained-hit count as the site count**, so a record
  with 124,700 sites whose coordinates were dropped by a cap read as having 0.
  It now prints what the record HAS, says when the coordinates shown are a
  prefix, and tells the two records of one multi-record file apart instead of
  presenting record-local coordinates under identical labels.

- **`pl index --verify` announced that "every stored hash still matches the bytes
  on disk" after comparing zero hashes.** It now counts and names the rows it
  skipped, and says plainly when nothing in the index carries a hash to check.

- **`pl trace` and `pl sanger` told the user a damaged `.ab1` carries no quality
  values** — the exact sentence `pl-abif`'s own documentation calls false. The
  GUI had been fixed; the two CLI arms had not.

- **19 of 113 database rows had no id-to-content pin.** `audit_ids` never read
  `reference_aa`, so the peptide-only rows were unpinned while the audit printed
  that the ids "still mean the same sequence" about rows whose sequence it had
  not compared. Ids here come from an item's INDEX in a tuple, so a deletion
  renumbers everything after it; this audit is the only thing standing between
  that and a published id silently repointing at different content.

- **`legal/` was not pinned to LF**, so on a fresh Windows clone
  `archive_legal.py --check` declared six of the seven licence documents
  tampered with. A verifier that cries tamper on a clean checkout trains its
  reader to ignore it.

- **An exported SVG map had no background, but `--check-contrast` certified it
  against `#ffffff`** — the exact failure the EPS and PNG back ends of the same
  command were written to avoid. The SVG back end paints its ground now, so the
  certificate is about a colour the file actually has.

  **THE PDF HALF DID NOT SHIP IN THIS RELEASE, AND THIS ENTRY CLAIMED IT DID.**
  As first written, this sentence said "an exported SVG or PDF map" and "the
  certificate now states the background it assumed"; the second clause was true
  of neither back end and the first was true only of SVG. At v0.10.2
  `pdf::pdf_at` still opened its content stream straight onto the backbone, so
  `pl export --pdf --check-contrast` was a claim about a background the file did
  not have — which `crates/pl-draw/src/lib.rs:280-289` said in as many words, in
  the same commit, under a heading reading "What still does not paint it". The
  code was honest and the release note was not. Corrected 2026-08-14, after the
  round-3 audit read the two against each other.

  This correction has itself been corrected. The PDF ground and the named
  certificate landed later the same day and are recorded under [Unreleased];
  the paragraph above is left standing, in the past tense it belongs in, because
  what v0.10.2 shipped does not change retroactively and the sequence — claim,
  admission, fix — is the whole point of keeping it.

- **`packages/circular-map`**: a restriction site outside `1..length` was drawn
  at a wrapped base with its impossible coordinate printed on the figure and
  nothing in `malformed`; a linear molecule's backbone drew its free ends 10.8
  degrees inside the mapping every feature and ruler tick uses, so terminal
  features bridged the gap and the map read as closed — the one thing that gap
  exists to say.

- **The Recover banner could not see a crashed session's draft unless it sat in
  slot 0**, and advertised every draft of a document that WAS saved as newer than
  the user's file — offering to replace a saved file with a draft containing
  nothing the file does not.

### Fixed — two more checks that could not fail

- `a multi-segment feature draws one arrow, not one per exon` counted paths and
  never arrowheads. It now counts arrowheads and reads the tip's direction —
  the second half added because the prescribed fix alone still could not catch
  one of the three mutations it was written against.
- The `--verify` reassurance above is the other.

### Verification

Every fix carries a test, and **all fourteen mutations were run centrally, each
preceded by a control run of the unmutated tree**. That control caught three
proofs that would otherwise have passed vacuously — a flag that does not exist,
a test runner the package does not use, and a mutation that broke the build
rather than a test. Five mutations had to be targeted by line number, because
each string also occurs in the doc comment that quotes the fix.

Round 2 also ran all 24 shipped oracles against the release binary: 383 pydna
fragments, 8,657 fontTools outline commands, 4,312 SciPy spline points at worst
relative difference 4.9e-13, 1,268 live checks through the imported Python
module, 1.29 MB through zlib both one-shot and byte-at-a-time. **Zero
disagreements.** Vacuity was tested by driving every corpus-taking oracle with an
empty input set: 11 of 11 refused correctly.


## [0.10.1] - 2026-08-13

Nineteen defect fixes and nothing else — no new capability, no format
change, no CLI verb. The two that matter put a wrong coordinate in front of
a biologist: an origin-crossing feature that lost its wrapped half whenever
the plasmid was rotated, and a flipped fragment whose features landed four
bases off in every ordinary six-cutter subcloning.

### Fixed

Nineteen defects from [`docs/AUDIT-2026-08-13.md`](docs/AUDIT-2026-08-13.md),
the sixteen-unit audit of the v0.10.0 tree. Two of them put a wrong coordinate
in front of a biologist; the rest are a hedge lost at a boundary, a count that
was wrong on screen while the value underneath it was right, or a test whose
name claimed a distinction its body never drew.

**Every fix carries a test, and every one of those tests was proven to fail by
reverting only its own production change** — sixteen mutations applied one at a
time, each run against its crate's suite. That is worth stating because this
change fixes four tests that could not fail, and a fix for a check that cannot
fail is worth nothing if nobody runs the check.

- **A rotation lost the wrapped half of an origin-crossing feature.**
  `Feature::extent` recognised a crossing in exactly one *spelling* — the join
  `genbank::write` emits, whose last part starts at base 1 — while
  `Molecule::rotate` remaps endpoints in place and never re-normalises. Rotating
  a plasmid therefore produced a join whose FIRST part wraps, which the
  recogniser never tested for, and the fallback returned an inner subset where
  its own doc comment promised an outer bound. Measured: a 17 bp promoter on a
  2,686 bp circle became a 7 bp feature at bases that are not its own.

  It is now read off the SHAPE — a segment that itself wraps, or a cut at the
  origin found at any adjacent pair rather than only the last — so the answer no
  longer depends on which surface wrote the file. The ordinary multi-exon join
  `join(100..200,300..400)`, which really does run 100..400 and is not a wrap, is
  unchanged and tested beside it.

  The audit's own prescribed fix was **wrong** and was not taken: it would have
  returned the wrapping segment's start, which on the same fixture answers
  `(400, 399)` — a whole-plasmid span for a 60 bp CDS. Which segment wraps says
  where the *origin* fell, not where the feature begins.

- **A flipped fragment carried its features `|ovhg|` bases off — four, for every
  common six-cutter.** The clone panel mirrored a feature about `watson.len()`,
  which is the right basis for the LAYOUT and the wrong one for the mirror,
  because a sticky end is precisely the case where the two strands differ in
  length. Measured on an ordinary non-directional EcoRI subcloning: a gene read
  `AAGGGCCCTTTA` where the parent says `GCCCTTTAAAGG` — the same bases rotated by
  four — inside the construct, on the wrong bases, with nothing saying so. The
  unflipped path was correct, which is why it hid: no test in that file
  exercised a flipped fragment at all.

- **A fragment that wrapped the origin dropped every feature in its tail**, and
  reported a start coordinate that does not exist in the parent — in the fragment
  list and in the Copy-record methods text, which is written to be pasted into a
  paper. The interval test is modular now, and the rendered coordinate is folded
  back into `1..=n`.

- **A clean religation reported features dropped that were not.** The counter ran
  inside a loop over the parent's features once per FRAGMENT, so every feature
  was counted once for every piece it is not in.

- **GenBank export threw away its own loss report** and still cleared the dirty
  dot, in the GUI and in the wasm build. Both call `write_reporting` now, and a
  write that dropped a feature no longer marks the document saved. This closes an
  item `docs/AUDIT-2026-07-28.md:499` logged honestly as deferred.

- **Saving as FASTA under-reported what it destroys** — primers and notes were
  dropped uncounted, which cleared the dirty flag, stood the unsaved-changes
  guard down, and let exit delete the recovery draft that held the edited bases
  and the primers together.

- **Methylation read as a verdict when it was an absence.** Every non-`.dna`
  molecule reports all-false, and that was rendered identically to a `.dna` that
  genuinely says none. It now says "not recorded in this file (treated as
  unmethylated)". Wiring an actual control is a feature and is not in this
  change; the comment says so.

- **The MCP `annotate` tool dropped the hedges the CLI prints**, so an assistant
  received a bare `681 to 80` for a hit that crosses the origin and would either
  relay it or "correct" it to `80 to 681` — a 601-base arc containing none of the
  feature. Third time this class has been fixed in that file, and the first time
  the origin note was swept.

- **An index could silently empty itself.** `--follow-links` is not persisted, so
  opening the Library tab on a folder indexed with it walked without the flag,
  deleted the linked rows, and wrote `complete: true` — a positive assertion of
  completeness over a library it had just emptied. A skipped link is now treated
  as a partial walk for the deletion pass, so the worst case is a stale row.

- **`Dseq::to_string_full` sliced a strand off a UTF-8 boundary**, and `pcr`'s
  template guard ran three lines after the call that panics. Both fixed: the
  method operates on bytes and the guard moved ahead of it, so no public method
  of `Dseq` panics on a file-derived value.

- **A colour string could make an entire exported SVG unopenable.** The sanitiser
  passed U+000B and U+000C into an unescaped attribute, neither of which is legal
  in XML's `Char` production. Beyond removing those two, the well-formedness check
  now rejects anything outside `Char`, so the next hole in a sanitiser is caught
  by the existing hostile-input tests rather than by a user holding a figure no
  viewer will open.

- **The linear ruler labelled a base that does not exist**, `span + 1`, because
  its loop was inclusive where the circular one is exclusive.

- **A rejected keystroke's warning lived about one second instead of the
  documented five**, because `settle` destroyed the notice whose expiry rule
  `clear_notice` already owns.

### Fixed — four checks that could not fail

Each was green while proving nothing, which is worse than absent, because it
reports as coverage.

- **The wasm corpus comparison passed vacuously when the corpus was empty** — it
  reported success having compared nothing. It exits non-zero now, and
  `xcheck_oracles.py` actually runs the case its own header already named.
- **`a_faithful_save_clears_the_dirty_state_and_a_lossy_one_does_not` contained
  no lossy save.** The decision is split out of the file picker into
  `fasta_losses`, so both arms can be driven without a dialog.
- **The MCP melting-temperature test asserted no temperature.** It computes the
  expected value from `pl_thermo` inside the test now, so the assertion cannot
  drift from the implementation.
- **The gate's own Python-precondition audit mis-delimited a one-line `Step`**,
  exempting exactly the shape most likely to be written casually. It now has the
  planted-input control this project uses everywhere else — one naked one-line
  step that must be reported, one guarded one that must not.

### Fixed — prose asserting what the code does not do

- `no_double_digest_produces_a_fragment_with_no_base_pairs` claimed to sweep
  every shipped enzyme and named eight, all with the same overhang geometry. It
  sweeps all 58 now.
- `features/README.md` said "seven rows" two paragraphs after listing nine.
- `featedit.rs`'s module header described `extent`'s old recognition rule.

## [0.10.0] - 2026-08-13

Two gestures that promised something and delivered nothing now deliver it. Tab
reaching the sequence grid used to switch the keyboard off; it now hands the
keyboard over and draws a focus ring saying so. A cut site on the ring showed a
pointing hand over text that answered nothing, and the same cut's coordinate in
the Enzymes table was inert without even the false promise of a cursor; both now
select that base.

Both are behaviour changes to gestures a user already makes, which is what the
minor number is for. Neither changes a file format, an exported figure, a CLI
verb or an answer any of them gives.

### Fixed

- **The sequence grid stopped accepting keys the moment Tab reached it, and now
  it accepts them.** `Sense::click_and_drag()` is `CLICK | FOCUSABLE | DRAG` in
  egui 0.35, so the grid has always been in the tab order — measured as the
  fifteenth Tab press from a fresh window — while `sequence_keys` stood down on
  "anything is focused". Landing on the grid therefore switched off every arrow
  key, every typed base, Backspace, Delete and Ctrl+A, on the one widget holding
  the keyboard, with nothing on screen to say why.

  This is the second half of
  [`docs/UX-REVIEW-2026-07-31.md`](docs/UX-REVIEW-2026-07-31.md) finding 9. The
  first half — the accelerators, which the same over-broad test killed — was
  fixed at `a79a276`, before v0.1.0; the reviewer's conclusion about this half
  was "the reasonable conclusion is that sequence editing requires a mouse", and
  it was correct for every release so far.

  The guard now tests IDENTITY: the keys are the grid's while the grid holds the
  focus, and nobody else's. It is deliberately **not** the narrowing
  `global_shortcuts` took — `ctx.text_edit_focused()` — and the difference was
  measured rather than argued. An accelerator should fire from anywhere except a
  text box; this path types characters into a document, so it must yield to
  every widget that is not the grid. Under `text_edit_focused` a base typed with
  a tab-strip button focused is appended to the plasmid, taking an 8,117 bp
  fixture to 8,118.

- **The map's cut sites promised a click and answered none.** `MapResponse`
  carried `clicked` and `double_clicked`, both *feature* indices, and a
  `hovered_site` with nowhere to go — while `map.rs` set
  `CursorIcon::PointingHand` over every tick and every site label. On the
  8,117 bp plasmid this application was written for that is a pointing hand over
  22 of the ring's roughly 31 labels, every one of them inert. Before 0.9.0 it
  was merely inert; beside a band that now selects its bases it read as broken.

  **A click on a cut now selects that base**, from the ring and from the Enzymes
  table, whose coordinates were equally inert text four inches from the sequence
  they name.

  **The awkward part is a merged label, and it was measured before it was
  decided.** `ring::merge_sites` folds cuts whose ticks are the same tick, so one
  label can carry several enzymes at several positions and a click on it has no
  single answer. Instrumented on the user's own pKoV at five pane sizes: **up to
  3 of 19 labels are merged, covering 6 of the 22 enzymes** — and every merged
  pair on that file is `SalI/XbaI`, `SphI/NsiI` or `XmaI/SmaI`, the isoschizomer
  and polylinker pairs a cloner is choosing *between*. So "make only the
  unambiguous sites clickable" would leave a quarter of the ring dead, and the
  wrong quarter. Two more molecules and two more pairs, both at one shared base.

  Pointing at the nearer *tick* is not available at any price: the fold criterion
  is that the arc between the cuts is narrower than the tick's own stroke, one
  tick is drawn per label, and the two cuts of `XmaI  101 / SmaI  103` are
  **0.4 pt apart** on the ring measured here. What can be pointed at is the
  **text**, so the label is split into the runs that each name one cut — by
  `Site::label_runs`, the function `Site::label` is now built from, so the split
  and the drawing cannot drift — and a click answers with the enzyme whose digits
  the pointer is on. The status line names it, because nothing in the picture
  can.

  Two cuts 2 bp apart is not a rounding error: XmaI is `C^CCGGG` and SmaI is
  `CCC^GGG`, four bases of 5' overhang against a blunt end.

  **It takes you to the Sequence tab, always**, which is where a Sanger mismatch
  has always gone and not where a feature goes. 0.9.0's rule is unchanged —
  reveal it where you are if that is possible, otherwise take you somewhere it
  is — but a *base coordinate* has a representation in exactly one of the eight
  tabs. The Features list is 0.9.0's universal fallback because every feature has
  a row there; a cut has none.

  **And the cost is the same one 0.9.1 disclosed**: this replaces a selection you
  dragged out by hand, and undo does not reach it. Both surfaces say so on hover
  before the click.

  Checked on a real file as well as on the fixture written to make the proof
  easy: every one of the 26 cut ticks on `prototype/demo-construct.gb` is
  clicked, and the base it selects is compared against the number the Enzymes
  table *printed* beside that enzyme's name — **22 enzymes named by both
  surfaces, agreeing on all 22**, with neither number computed by the test.

  Two enzymes get no tick answer at all: **NotI** at 643 and **BamHI** at 1,829,
  each with a neighbour a few bases away whose hit box is the same fixed square
  and, where two overlap, wins. That is pre-existing and unchanged. What is new
  is that both are now known to be reachable by their own label instead of
  assumed to be — a cut whose tick nobody can hit and whose label nobody can hit
  would be this same defect surviving in the two places hardest to see it.

- **A multi-cutter's positions could have been printed backwards, and nothing
  would have noticed.** Making each coordinate clickable turned one string into
  one widget per number, laid out right-to-left — where the *first* widget added
  is the *rightmost*. That is a new way for `AvrII 830, 1,125, 2,069, 2,761` to
  come out as `2,761 2,069, 1,125, 830,`, with the commas hanging off the wrong
  ends and a reader planning a digest sent 1,931 bases away; the old single
  joined string could not be out of order because nothing ordered it. The order
  was right and stayed right — but the entire GUI suite, 676 tests at the time,
  passed with the reversal in place, because nothing else looks at a row with
  more than one number on it. It is now checked both ways: how the row reads,
  and which base each number actually answers with.

### Added

- **A focus ring on the sequence grid**, because a grid that holds the keyboard
  and looks identical to one that does not is a worse defect than dead keys.
  Drawn in `theme::accent_ink`, which the design system already reserves for
  focus strokes, and measured with `theme::contrast` against the panel behind
  it: **7.08:1 in the dark theme and 5.35:1 in the light one**, against the 3:1
  that WCAG 2.2 SC 1.4.11 asks of a graphical object. Two values and not one,
  because `#E69F00` is 2.25:1 on white and a single-constant ring is invisible
  in one of the two themes.

  The ring is clipped to the scroll **viewport** rather than to the band of rows
  underneath it, so all four of its edges are on screen at every scroll position
  — SC 2.4.11. On the fixture measured that moves its top edge 7.4 pt down from
  where the band starts, which is how far off screen the top of the indicator
  would otherwise have been.

- **A way out of the grid that is not a trap.** Arrow keys are locked to it
  while it has the focus: without that, egui reads them as focus navigation, and
  ArrowLeft handed the keyboard to the panel splitter on the first press while
  ArrowUp handed it to the genetic-code combo. Tab, Shift+Tab and Escape are
  left unlocked on purpose and all three are tested, because a keyboard user who
  can enter the sequence and not leave it would be worse off than one who never
  got in.

- **Ctrl+Z reaches a base typed from the keyboard**, and Ctrl+S, Ctrl+F, Ctrl+N
  and Ctrl+O all still fire while the sequence holds it. This is the first change
  that leaves both keyboard paths live in the same frame, so the question of
  whether a chord now reaches two handlers is answered rather than assumed: the
  two key sets are disjoint, and each accelerator is checked to fire alone.

### Verification

Nothing in this section changes what the application does. It is here because
the claims above are the kind that stay green while being wrong.

- **Four text boxes that can sit over an open molecule** now each have a
  keystroke aimed at them and the molecule measured afterwards: the Find bar,
  the Features filter, the feature editor and the New-document dialog. Two of
  the four turn out not to be held by this guard at all — the editor and the
  dialog have had stand-downs of their own since long before this branch — and
  the tests say so, because a test that is read as evidence for the wrong
  mechanism is worse than no test.

  **This sentence began "Every text box this application can put over an open
  molecule", and that was not true.** At least two more exist and neither is in
  the list or the tests: the Spacer box in the Design-primers window
  (`design.rs:513`) and the Cut-and-religate window's own box (`clone.rs:718`),
  both `egui::Context` windows that paint over whichever tab is showing. Four
  boxes covered is the fact; "every" was the assertion, and the difference is
  the whole reason this section exists.

  The measurement that matters is `effective_len` and not `molecule().len()`: a
  typed base sits in an open run before it reaches the op log, so the obvious
  assertion is one keystroke behind and can call a stolen base "nothing
  happened".

- **The focus ring's 7.08:1 and 5.35:1 are now tied to the frame at both ends.**
  One test already proved the ring's INK is what the application paints; nothing
  proved the same of the BACKGROUND, so the ratio was half anchored. Adding a
  single ordinary-looking fill under the ring leaves the contrast figures
  unchanged and passing while the ring is really on `#4A555C` at 3.40:1 — that
  gap is now closed by reading the fill out of a painted frame in both themes.

Not here: the go-to-base box. Finding **19** is what asks for one — the
genome-scale finding — and it notes in passing that the box "also solves the
keyboard-only case in finding 9"; this said finding 9 asked for it, which reads
the borrowed benefit as the request. It is a separate feature either way, and it
is now cheaper rather than dearer to build, because the surface such a box would
hand control back to can hold the keyboard and can say that it does.

- **A cut-site test measured the label a whole text width from where the widget
  was.** `egui::Label` takes its horizontal alignment from the enclosing layout,
  and inside `Layout::right_to_left` — which is how the Enzymes table's
  coordinate column is laid out — the galley's anchor is its RIGHT edge. The test
  helper read every drawn string as `from_min_size(pos, size)`, so it reported
  the column at 1,272..1,305 in a 1,280 pt window when the widget was really at
  1,239..1,272, and a click on the "centre of the text" pressed nothing. Nothing
  was wrong with the application; the reading of it was. It is fixed at the
  helper, where it is identical for every left-aligned galley the map tests use.

Also not here, and pre-existing: **the view does not follow the caret.** Only a
reflow or a map reveal sets a scroll offset, so a keyboard user can now drive the
caret off the bottom of the view and lose sight of it. It behaves exactly as it
did in 0.9.1 — what changed is that the caret can be driven from the keyboard at
all, which is what makes the gap reachable. It is the natural companion to the
go-to-base box and belongs with it.

One thing that WAS quietly wrong and is now fixed alongside it: `jump_to_base`
set the tab and no scroll offset, so a double-clicked Sanger mismatch at base
4,001 switched to a Sequence tab still sitting on row 1. It asks for
`Reveal::Base` now, which is what makes the new cut-site click land somewhere
you can see — and it fixes the Reads tab's jump on the way past.

### Audited before tagging

The release commit was read by six independent passes before the tag was cut,
each finding handed to a reader whose instruction was to refute it. Two dead
checks and five false counts survived that, and all seven are fixed above or
below. They are listed because a release that quietly repaired its own release
notes has learned nothing.

- **A version bump killed a test, silently, with the suite green.**
  `check_does_not_call_an_older_release_an_update` built its older release as
  `Version::new(current.major(), current.minor(), 0)` behind an
  `if older == current` guard **whose two arms were the same expression**. At
  any `x.y.0` release that value IS the current version — so from the moment
  `Cargo.toml` read `0.10.0`, the test named for the older case exercised only
  the equal one, and nothing anywhere went red. It had been true of every `.0`
  release before this one.

  `older` is now derived by decrementing the lowest non-zero component, and then
  **asserted to be older**, which is the part that stops this recurring. The
  helper returns the LARGEST version below the current one, so at 0.10.0 the
  fixture is `0.9.4294967295` — a string that sorts ABOVE `"0.10.0"`, making the
  same test a second trap for a comparison that ever became textual. Proven by
  three mutations: restoring the old derivation, making `update_available`
  lexical, and making it always true.

- **The MSI oracle claimed a comparison it never made.** `check-msi.ps1` said
  "`pl --version` must agree with the MSI's declared version" and then asserted
  only that the output held some three-number pattern; the package's declared
  version was never read. Both halves were already in scope — `$plVersion` and
  the `DisplayVersion` Windows Installer writes into Add/Remove Programs — and
  they are now compared. This is not hypothetical: both MSI steps read `dist/`,
  which the gate does not rebuild, so a stale payload inside a freshly-numbered
  package passed every assertion in the file.

- **Four counts and one attribution were wrong**, each now re-derived from the
  file it describes rather than from memory: the Stage 4 hold count in
  `features/README.md` said six and is one (`PLF:3019`, factor Xa — the other 27
  ship); the same file said the SV40 early poly(A) signal is `proposed` when
  `PLF:4011` was refused at 1 of 3 placements and `SV40` appears nowhere in
  `features.tsv`; it called the snapshot three ingest passes when
  `provenance.tsv` holds five distinct `retrieved` dates and `features/NOTICE`
  had already been corrected to say so; `tools/release-notes.md` — which the tag
  renders onto the release page — said nine embedded font faces where `NOTICE`
  and `check-archive.ps1` both say ten, and `NOTICE`'s copy is the one a test
  sums; and `tools/ci.ps1` said a machine without WiX still runs "the other 71
  steps" against its own stated rule of the file's step total minus one, which
  is 72.

  None of them was load-bearing. All of them were the same failure: a number in
  prose, drifting away from the number in the tree, with nothing joining them.

## [0.9.1] - 2026-08-13

**A patch number over a change to what a gesture does, and this paragraph is
here so that nobody is surprised by it.** 0.9.0 took a *minor* number for one
stated reason — "for the first time in four releases, something you do in the
app answers differently" — and the thing that answered differently was a click
on a feature. A click on a feature answers differently again here, on a second
surface and in a second way: it now takes the sequence selection. By the rule
0.9.0 wrote for itself this would be 0.10.0. It carries 0.9.1 because that is
the number the release was cut under, and the number is the only thing that was
made smaller — everything below is written at the size of the change rather than
at the size of the digit.

**What that means for somebody deciding whether to take it.** A gesture you
already use does something it did not do before, and it is destructive: clicking
a feature — on the map from any tab, or on a row in the Features list — replaces
whatever you had selected in the sequence, and undo does not reach a selection.
If you have a habit built on a hand-dragged range surviving a click on a feature
row, this release breaks it. That is the whole of the risk, and the app says so
on the hover of both surfaces, so it is met before it is paid rather than after.

**Nothing in the feature database moved.** No row was added, withdrawn or
edited, no sequence, extent, boundary rule or evidence citation changed, and
nothing was signed. The table is still **113 rows: 89 signed, 24 proposed**, the
same ids in the same order. Ten of those rows are Class B regulatory elements,
all ten are `proposed`, and **none of the ten ships**: `Db::reviewed()` serves
none of them and `pl annotate` searches none of them without
`--include-proposed`. Being in the table is not shipping, and this release adds
nothing to either.

**`features/SIGNOFF.tsv` is byte-identical to `main` and to `v0.6.0`** — 89
signature rows, sha256
`7cf86057c2b9b964976ad04788a764fd1882b56c2e4cdd427e3395a0fc858e97`, blob
`2d63b169d0de742154b5a7e87c830e12d5052be7`, the same blob 0.6.0, 0.7.0, 0.8.0,
0.8.1 and 0.9.0 shipped. **Nothing has been signed since 0.6.0**, all 89
signatures still verify, and no row digest was computed or published here.

### Changed — selecting a feature now selects its bases, from every path

- **Click a feature and its bases are selected, whatever tab is open.** 0.9.0
  made a click on the map *reveal* the feature in the panel you already had open,
  and it selected that feature's bases only when the open panel was the Sequence
  tab. A click on a Features ROW set the highlight and nothing else. So the map's
  selection arc and its end caps, the readout's GC and Tm, Ctrl+Shift+R (copy
  reverse complement) and Ctrl+Shift+P (copy protein) all keyed off a selection
  that the commonest gesture for picking a feature had never made — the GC of a
  feature you had picked in the list was reachable only by picking it again
  somewhere else.

  This is one rule where there were two: **selecting is unconditional, revealing
  stays tab-dependent.** Which bases are selected is a fact about the document;
  which panel moves to show them is a question about where you are standing, and
  0.9.0's answer to that is untouched. Every path into a feature — the map click,
  the Features row, the feature editor opening, Duplicate, Accept on a database
  proposal, and Save on a new feature — now goes through one setter,
  `App::select_feature`. The one exception is a double-click *in the sequence
  grid*, deliberately not routed: it selects the smallest covering **segment**
  rather than the whole feature — an exon, not the gene the exon is in — and
  routing it would have changed that gesture without anyone deciding to.

- **An origin-crossing feature selects the short arc.** A row click on a feature
  spelled `join(7900..8117,1..100)` on an 8,117 bp molecule selects the **318**
  bases the feature is on, not the **7,799** the same pair of carets also names.
  Both numbers are named in the assertion that holds it, because a message that
  prints only the one it wanted leaves a reader to work out whether the other one
  is what it got.

- **A second click on the same feature clears the bases as well as the
  highlight** — but only when they are still the ones that click put there. If
  you have dragged out something else since, that survives: clearing it would
  destroy a selection the gesture never made. The guard recognises its own work
  on a wrapped feature too, which is a different comparison and is now asserted
  rather than assumed.

- **The selected feature is first in the map's label budget**, ahead of the
  filter matches it already promoted. Otherwise on a dense map you click a row
  and the band you selected is the one band with no name against it — the finding
  0.9.0 closed, wearing different clothes. It goes ahead of the filter and not
  behind it because there is at most one selected feature, so it can never crowd
  the matches out, while a sixty-match filter could certainly crowd out the one
  feature you have in hand.

### Fixed — a click on a feature's NAME selected nothing at all

**This defect predates the change above and was found while making it.** It is
here at least as prominently as the feature, because it is the one entry in this
release that is a plain bug in shipped behaviour rather than a decision about
what a gesture ought to mean.

- **A click on a feature's name in the Features list selected nothing.** egui
  makes a `Label` click-and-drag-sensing so its text can be swept, and those
  labels are drawn inside the row and therefore on top of it; egui's hit test
  hands a press to the topmost widget that senses one. Measured with
  `Context::interaction_snapshot`, a press at the centre of `AmpR`'s name — the
  biggest target in the row and the one a user aims at — was won by the label,
  while the row itself reported `contains_pointer: true, hovered: false,
  clicked: false`. **The row only ever answered where no label had been drawn**,
  so the reliable way to pick a feature out of the list was to click the empty
  space beside its name. Text selection is now off for this row and nowhere
  else; the name is still selectable text in the feature editor, which is where
  it is editable.
- **The truncated-name tooltip had to move with it, onto the row.** A
  non-interactive label registered before the row that contains it is underneath
  it, and egui marks a non-interactive widget hovered only when it is on top of
  the topmost interactive one — so left where it was, the tooltip that shows the
  whole of a name too long for its row would silently have stopped appearing. One
  hover per row now carries both the full name and what a click does.

### What got worse

- **A range you dragged out by hand is destroyed when you click a feature,
  silently, and undo does not reach a selection.** Somebody with 300 bases
  dragged out in the Sequence tab who clicks a feature row to see what it is
  loses the drag, with no warning and nothing to press to get it back. It is
  accepted rather than overlooked: it is what the word "select" means, and it is
  the same cost Ctrl+F, a primer row and a Sanger mismatch have charged since
  each of those was built. A feature row charging it too is one rule instead of a
  fourth exception.

  **A cost this quiet has to be met before it is paid**, so it is now said in
  front of you: both the map band and the Features row say *"click to select its
  bases · double-click to edit"* on hover, in one shared sentence, and an
  annotation-only document — features and no bases, where there is nothing to
  select — says *"click to select · double-click to edit"* instead, rather than
  promising bases the file does not have.

- **The caret moves, and an open typing run is committed.** The selection is set
  through the one sanctioned door, which commits the run on the way past and
  leaves the caret at the feature's 3' end on the plus strand. Committing a run
  is not an edit you did not make — the run is bases you typed, and the commit is
  what turns them into one undo step — and all three sibling surfaces have always
  done both.

### Not changed

- **`features/SIGNOFF.tsv`, `features/features.tsv` and
  `features/provenance.tsv` are byte-identical to `main`**, verified with
  `git diff --exit-code main` on all three. This release moves version numbers,
  prose and this file; it does not run the feature build.
- **`docs/UX-REVIEW-2026-07-31.md` is not amended, and that is deliberate.** Its
  finding 10, *"Selecting a region answers none of the questions that follow
  it"*, was closed well before this release — the readout carries GC and Tm and
  the map draws a selection arc — and what changes here is not what a selection
  answers but what *makes* one. That file is a dated record of one binary at one
  commit, it says so in its own header, and it has not been edited since the day
  it was written; annotating one finding in it now would make it look like a
  status board, which it has never been.

## [0.9.0] - 2026-08-12

**A minor release rather than a patch, and the reason is the first bullet: for
the first time in four releases, something you do in the app answers
differently.** 0.6.0, 0.7.0, 0.8.0 and 0.8.1 each opened by saying that nothing
in them changed what Polylinker does — they moved prose, counts and checks, and
a plasmid opened under any of them got the same answer from the same code. This
one changes a gesture. **Clicking a feature on the map used to throw you into
the Features tab whatever you were doing; now it shows you the feature where you
already are.** That is a change to the one surface every user touches, so it
takes a minor number.

**The rest of the release is one database row**: `PLF:4015`, the mouse PGK
promoter, issued on 2026-08-12 because the curator instructed it and not because
any rule changed. No engine, no format reader or writer and no annotator rule
moved, and no existing row's sequence, extent, boundary rule or evidence
citation changed.

**The 89 signed rows are untouched, and so is the file that signs them.**
`features/SIGNOFF.tsv` is byte-identical to the one 0.6.0, 0.7.0, 0.8.0 and
0.8.1 shipped — the same blob, `2d63b169d0de742154b5a7e87c830e12d5052be7`, and
the same sha256
`7cf86057c2b9b964976ad04788a764fd1882b56c2e4cdd427e3395a0fc858e97` — so
**nothing has been signed since 0.6.0**, all 89 signatures still verify, and the
89 records `pl annotate` searches by default are the same 89. The table is now
**113 rows: 89 signed, 24 proposed.** Ten of those are Class B regulatory
elements and **none of the ten ships**: `Db::reviewed()` serves none of them, and
`pl annotate` searches none of them without `--include-proposed`.

### Changed

- **Click a feature on the map and it is revealed in the panel that is already
  open.** With the **Sequence** tab up, the click selects that feature's bases
  and scrolls the grid to where the feature *begins*. For a feature that crosses
  the origin, "begins" is the high coordinate, and what gets selected is the
  short arc across the origin — 318 bases on the 8,117 bp molecule the test uses
  — rather than the 7,799-base complement the same pair of carets also names.
  With the **Features** tab up, the list scrolls the row into view; before this,
  clicking a band whose row had scrolled out of sight changed nothing you could
  see, because the translucent wash on the row is the only per-row marker and it
  is clipped away. Either destination then lets go: the reveal is a one-shot, so
  the view comes to rest and you can wheel straight off the row or the bases it
  just brought up without being dragged back.
- **The six tabs that cannot show a feature still take you to the Features
  list** — Library, Enzymes, Primers, Reads, History and File. That is a
  decision and not a leftover: a cut list, an oligo, a trace, an edit log, a file
  header and a folder of other files all answer questions that are not about one
  feature, and a click whose only visible effect is on the surface you clicked
  reads as a click that did not work. The Sequence tab goes there too on a
  document with no bases — an annotation track, or an annotation-only GenBank —
  because it paints one sentence and no grid, so there is nothing there to reveal
  into.
- **Clicking the same feature a second time still clears the selection, and now
  does only that**: no tab switch, no scroll. The map draws the selection
  whichever tab is open, so a click on a band that is already highlighted is a
  deliberate "clear this" rather than a request to be taken anywhere.
  **Double-click still opens the feature editor**, and reveals once rather than
  twice.
- **A map click on a feature the Features filter hides says so** in the status
  line, naming the feature and the filter text, rather than emptying the box the
  user typed in or selecting something with no row to show for it.
- **Counts corrected by measurement, not by assumption**: the table is
  **113 rows — 89 signed, 24 proposed** (was 112/89/23), and Stage 5 now has
  **ten** Class B rows in the table (was nine) out of **sixteen** declared
  elements (was fifteen). Five are still refused by `MIN_PLACEMENTS` and
  `PLF:4006` is still withdrawn. Updated in `README.md`, `features/README.md`,
  `features/PROPOSED.md`, `features/NOTICE`, `docs/PLAN.md`,
  `bins/pl-gui/src/featuredb.rs`, `bins/pl-gui/src/settings.rs` and
  `crates/pl-features/src/lib.rs`. `features/PROPOSED.md` gains a section for
  `PLF:4015` with its `--show` command, its at-the-floor corroboration stated
  plainly, and the second-copy provenance.
- **`stage_classb.HELD` loses the PGK entry**, and nothing left in that file
  describes PGK as held or as failing on its evidence. The module docstring's
  every-copy paragraph now separates the two events it would otherwise blur: no
  row *returned* on the 2026-08-11 fix, and `PLF:4015` did not return — it was
  issued.

### Added

- **`PLF:4015`, the mouse PGK promoter, issued on the curator's instruction —
  not because the program changed its mind.** The element,
  `BX469914.4:13192-13699:+`, 508 nt, had sat in `stage_classb.HELD` since the
  Class B stage was written. On 2026-08-11 the second-copy fix took its measured
  corroboration from one exact placement to two, which cleared both floors, and
  it was **still refused** — the stage's own docstring says that a stage which
  promoted an element the moment its code stopped under-counting "would be
  adjusting its own membership", and that issuing a row "is a curator's decision
  and not this program's". That reasoning is unamended and still in the file. On
  2026-08-12 Lior Lobel took the decision, and the item was **appended** to
  `ITEMS`, taking `PLF:4015` so that none of `PLF:4000`–`PLF:4014` moves and
  `PLF:4006` stays retired. Verified by diffing the id→name mapping across the
  commit: **no published id changed meaning.**

  **It clears at exactly the floor, on both counts, with nothing to spare.**
  Re-measured by driving the stage's own `verify()` rather than taken from the
  hold text: the anchor, `BX469914.4` (Wellcome Trust Sanger Institute, clone
  RP23-217J7, chromosome X), annotates **nothing** — its entire feature table is
  one `source` line. `CR293496.1` (Sanger Centre) draws `regulatory 4137..4644`,
  5'+0/3'+0. `AB242435.1` (Central Institute for Experimental Animals, Kawasaki)
  carries the element **twice** and draws `regulatory 2089..2596` over the second
  copy, edge for edge, while the first copy at 374-881 carries a 516 nt feature,
  5'+8/3'+0. `same_submitter()` merges `BX469914` and `CR293496` — both Hinxton —
  so the anchor adds no third opinion. That leaves **2 submissions against
  `MIN_SUBMISSIONS = 2` and 2 placements against `MIN_PLACEMENTS = 2`: lose
  either witness and the row fails.** The row says so in its own `caveat` rather
  than leaving a reader to infer it, because "corroborated" and "corroborated
  twice over" are different claims. `MIN_PLACEMENTS`, `MIN_SUBMISSIONS`,
  `SUBMITTER_MERGE`, `same_submitter()` and the SnapGene screen are untouched.

  **The second placement exists only because of the 2026-08-11 second-copy fix,
  and the row's own note says so.** Under the previous implementation, which
  scored `occurrences()[0]` and nothing else, this element measured 2/1 and was
  refused. That is the honest provenance of this row's corroboration, and it
  travels with the row rather than living only in this file.

  **Neither edge is a landmark, and that is the whole content of
  `boundary_rule = consensus_of_insdc` here.** The primary source for the mouse
  Pgk-1 promoter is Adra, Boer & McBurney 1987, *Gene* 60:65-74, PMID 3440520,
  which is `M18735.1`'s own publication; the description is written from it and
  from measurement, not from any vendor page. Carrying `M18735.1`'s exon-1 and
  CDS annotation across the alignment, the element is **−431..+77** — 431 bases
  upstream of the 5' end of exon 1, through 77 bases of it, stopping 16 bases
  short of the ATG at `BX469914.4:13715`. It holds the whole promoter as that
  paper describes it: all five `GGGCGG` Sp1 hexamers, the one `CCAAT`, and no
  TATA box — counted in the shipped bases, and the only copies of either in the
  whole 1110 bp primary record. But **−431 is 196 bases beyond the
  outermost Sp1 site and nothing draws a line there**, and **+77 is a point
  inside exon 1 that misses both the transcription start and the initiation
  codon**. Both edges are a convention two depositors converged on. That is
  exactly what `consensus_of_insdc` claims, and this row must not be read as
  claiming more.

  **The old hold text's "~48 substitutions from the gene" is withdrawn and is
  not repeated anywhere in the tree.** It came of measuring against `M18735.1`
  rather than against the anchor. Against `BX469914.4` the element is a verbatim
  slice, re-sliced and re-checked on every build. Against `M18735.1` it is three
  exact blocks covering all 508 bases with **zero substitutions**, separated by
  two single-base indels; those indels are the whole reason the longest exact
  shared prefix is 64 nt. Which record is right was not determined, and the row
  says so.

  **The row carries `review_status = proposed` with an empty curator.**
  `features/SIGNOFF.tsv` was not written, and no row digest was computed or
  published here. Issuing a row puts it in the table; only a signature puts it
  inside `Db::reviewed()`, and only the first has happened.

### Fixed

- Two cross-references that named the PGK entry as an example of "a sequence
  that is in no primary record" — in `stage_classb.py`'s U6 hold text and the
  matching bullet in `features/PROPOSED.md`. That was already wrong before this
  change, since the "not a verbatim slice of anything" claim was retired on
  2026-08-11, and it is doubly wrong now: `PLF:4015` is an exact slice of a
  primary genomic record, so it is the counter-example. Both now point only at
  `PLF:4014`'s vector form, which is the case that really has this shape.
- **`PLF:4009`'s two population counts, left standing for one commit and then
  corrected.** Its `notes` said "the fourth shortest of the nine Class B rows in
  the table" and "the fifteen elements this stage declares"; issuing `PLF:4015`
  made those **ten** and **sixteen**. Every assertion the note makes survived the
  correction — 28 nt is **still** the fourth shortest, of 17, 17, 19, 28, 44, 47,
  225, 285, 508 and 1188 — and only the sizes of the populations moved. The note
  now carries the distinction that would have prevented the error: **the ordinal
  is a fact about this row, the populations are facts about the stage, and only
  the second kind moves when an element is issued, withdrawn or refused.** Two
  words changed, `nine` → `ten` and `fifteen` → `sixteen`; the row is `proposed`
  and unsigned, so its digest moved with them and nothing lapsed.
  `features/PROPOSED.md` records both the day it was left alone and the day it
  was fixed.
- **Six sentences that this release's own two changes falsified**, found by
  re-measuring every count in the tree against `features.tsv`,
  `stage_classb.py` and `provenance.tsv` rather than by reading for plausibility:
  - `bins/pl-gui/src/settings.rs` — the `annotate_unreviewed` doc comment, which
    warns in its own last line that it goes stale every time the table moves,
    and did: 112 rows and 23 records against 89 signatures, and nine regulatory
    elements. It is 113, 24 and ten, and the Class B half moved a **third** time
    on 2026-08-12.
  - `features/build/stage_classb.py` — the *"what is deliberately NOT here"*
    paragraph, which still said three of the nine held elements were rows and
    that `HELD` was ten entries over six refused elements. Measured at this
    commit: `len(HELD)` is **9**, over **five** refused elements — SV40 early,
    U6, H1, the AG/CAG/chicken-β-actin split and the tet split — and **four**
    are rows.
  - `features/README.md` — the id-block table's `PLF:4000`–`PLF:4999` row, which
    the `PLF:4015` commit's hunks skipped. "9 of 25 worked up" is **10 of 25**;
    25 is unchanged, being 16 `ITEMS` plus 9 `HELD`.
  - `features/PROPOSED.md` — "the fourth shortest of the **nine** Class B rows",
    in the correction table that fixed the superlative. Ten.
  - `features/NOTICE` — "over all **112** of our descriptions", the sibling of a
    line in `features/README.md` that the same commit did update. 113.
  - `bins/pl-gui/src/main.rs` — `feature_tip`'s doc comment, which explained the
    hover line as what makes "the tab jump on click" explicable. The tab no
    longer jumps on every click; the line itself, *"click to select ·
    double-click to edit"*, is unchanged and still accurate.
- **`features/NOTICE`'s ingest-pass banner, which had gone wrong in three
  places at once.** It listed four upstream retrieval dates; `provenance.tsv`
  carries **five**, because issuing `PLF:4015` fetched
  `ena/browser/api/fasta/BX469914?range=13192-13699` from EMBL-EBI on 2026-08-12
  — a URL no earlier pass had requested, and the source of that row's
  `reference_nt`. The cache's own metadata records the fetch date, and a verified
  cache hit returns without rewriting it, so this is a real retrieval on a fifth
  day and not the build clock leaking into an `ena` row. It is named as a
  **fetch** and given its size — two provenance rows, against 72, 249, 107 and 21
  for the four passes before it — rather than promoted to a fifth ingest pass.
  Two further figures in the same banner moved: the 2026-08-11 bucket now holds
  **24** rows over 18 accessions, not 21 over 15, because three of `PLF:4015`'s
  `boundary_evidence` rows cite records fetched that day while the element was
  still held; and the own-work rows the build clock stamps are **851**, not 843.
  Every provenance row the file already held survives byte for byte, sha256
  included, and no other upstream date's count moved.

### Not changed

- **`features/SIGNOFF.tsv` is byte-identical to `main` and to `v0.6.0`**, sha256
  `7cf86057c2b9b964976ad04788a764fd1882b56c2e4cdd427e3395a0fc858e97`, blob
  `2d63b169d0de742154b5a7e87c830e12d5052be7` — the same blob 0.6.0, 0.7.0, 0.8.0
  and 0.8.1 shipped. `git diff v0.6.0 HEAD -- features/SIGNOFF.tsv` is empty.
  Nothing has been signed since 0.6.0, all 89 signatures still verify, and no row
  digest was computed or published anywhere in this release.
- **The 89 signed rows and 22 of the 23 pre-existing proposed rows did not move
  by a byte**, and neither did any of the 1,292 provenance rows that existed at
  0.8.1. Rebuilt offline (`PLF_OFFLINE=1`) against the cache, then diffed against
  0.8.1 column by column: `features.tsv` gains one row, rewrites one header count
  and rewrites exactly one pre-existing cell; `provenance.tsv` gains thirteen rows
  and changes none. **The one cell is `PLF:4009`'s `notes`**, listed under *Fixed*
  above — column 14 of that row and nothing else on it, not `date_added`, not the
  `#!version` header, and not one cell of any other row. The thirteen new
  provenance rows all belong to `PLF:4015`.
- **The release commit itself does not touch `features.tsv`.** Everything above
  about that table landed in the two pull requests this release collects; cutting
  the release moves version numbers, prose and the changelog, and the generated
  tables are byte-identical before and after it.

## [0.8.1] - 2026-08-12

**This is a patch release, and nothing in it changes what Polylinker does to a
sequence.** No engine, no format reader or writer, no annotator rule and no
default moved. No row was added or withdrawn, and no sequence, extent, boundary
rule or evidence citation changed anywhere in the table. A plasmid opened under
0.8.0 and the same plasmid opened under 0.8.1 get the same answer, from the same
code, against the same 89 records. Each of the three entries below corrects a
sentence that was false when it was written, which is why this release has no
*Added*, *Changed* or *Removed* section at all.

**The 89 signed rows are untouched, and so is the file that signs them.**
`features/SIGNOFF.tsv` is byte-identical to the one 0.6.0, 0.7.0 and 0.8.0
shipped — the same blob, `2d63b169d0de742154b5a7e87c830e12d5052be7`, and the same
sha256 `7cf86057c2b9b964976ad04788a764fd1882b56c2e4cdd427e3395a0fc858e97` — so
**nothing has been signed since 0.6.0**, and all 89 signatures still verify. The
table is still **112 rows: 89 signed, 23 proposed**, the same ids in the same
order. The nine Class B rows it holds are all `proposed` and therefore **none of
them ships**: `Db::reviewed()` serves none of the nine, and `pl annotate`
searches none of them without `--include-proposed`.

**`features.tsv` is not byte-identical to 0.8.0, so here is what does differ.**
Exactly one cell of row *content* — `PLF:4009`'s `notes`, the only digest-covered
cell that moved anywhere in either table — and what changed inside it is a
sentence *about* the row, replacing a false superlative, rather than a different
description of what the row is. That row is `proposed` with no line in
`SIGNOFF.tsv`, so nothing was in a position to lapse. Everything else that
differs in either table is the build clock: `date_added`, `retrieved` and the
`#!version` header. The paragraph at the foot of this entry measures all of it
cell by cell, because "the tables did not move" is the claim this project checks
hardest.

### Fixed — a count in the worklist that nothing could reconstruct

- **`features/PROPOSED.md` said "Nine of these rows carried a sentence" the
  research contradicted, and nine matched nothing.** Its own table names
  **seven** distinct rows across eleven entries, plus one entry about the file
  itself; this file's `[0.7.0]` heading covers the same seven in seven of its
  eight bullets, the eighth being about that same file rather than about a row.
  Nothing there is nine. Seven would not have held either — `PLF:4006` was
  withdrawn on 2026-08-11, so six of the seven are among the rows the worklist
  now describes and the seventh is not in the table at all.

  The sentence now gives no number and says why: a bare count there has to be
  re-derived by hand every time a row is added, withdrawn or corrected, and it
  went stale within a day of being written. The table is the count. This is the
  fourth count in three releases to drift in prose while every test-asserted
  count held, so the fix is to stop asserting it rather than to correct it once
  more.

  `[0.7.0]`'s "nine rows" heading is **left standing** as the record of what that
  release claimed, in the same way `PLF:4009`'s note now quotes the superlative
  it replaces. This project records its corrections rather than overwriting them.

### Fixed — a superlative in `PLF:4009` that was false the day it was written

- **`PLF:4009` rrnB T2 no longer claims to be "the shortest".** Its `notes` said
  "it is the shortest and most sharply bounded of the twelve". The twelve were
  `PLF:4000`–`PLF:4011`, and `PLF:4000` and `PLF:4001` are **17 nt** each and are
  both declared above it — so the sentence was wrong when it was written and not
  overtaken later. It entered on 2026-08-10 with the row itself and shipped
  unchanged in 0.6.0, 0.7.0 and 0.8.0. `PLF:4012`, appended since, is 19 nt as
  well; measured against `reference_nt` in `features.tsv`, this row's 28 nt is
  the **fourth shortest** of the nine Class B rows in the table.
- **The bounding half is kept, and demoted from a superlative to the measurement
  it restates.** Every witness of this row that annotates anything over these
  bases draws `5'+0/3'+0` — and `PLF:4000` and `PLF:4001` read the same way, so
  "most sharply bounded" was not a ranking either. What distinguishes this row is
  that their notes *name* rival extents found elsewhere (19 nt and 21 nt for T7,
  a 19 nt published consensus running through +1 for SP6) and this row's names
  none. The row records that the claim it replaces was wrong, rather than
  overwriting it silently.
- **`README.md` now dates the table 2026.08.12.** `features.tsv`'s `#!version`
  line is the build clock too, so rebuilding moved it, and
  `the_readmes_state_the_signoff_count_the_database_has` asserts that the front
  page carries the live version alongside the live counts. The three counts in
  that sentence — 112, 89, 23 — are unchanged.

### Fixed — a build date written down in `features/NOTICE`, found by cutting this release

- **`features/NOTICE` said the own-work provenance rows "all read 2026-08-11
  today". They read 2026-08-12.** The sentence exists to explain that `retrieved`
  on an own-work field is stamped with the build date rather than with a fetch —
  and then named the date anyway, so the rebuild that this release documents
  falsified it on the same day, on all 843 of those rows. The date is **removed**
  rather than advanced, and the sentence now points at `features.tsv`'s
  `#!version` line, which is the same clock and is generated. That is the
  treatment `PROPOSED.md`'s count got above, applied to the same failure mode one
  file over.
- **The four upstream dates in that file are unaffected, and that was checked
  rather than assumed.** The 843 rows that moved are exactly the `polylinker`
  (731) and `insdc-ft` (112) own-work rows. Every upstream fetch row is unmoved
  and each date still carries the same number of rows it carried at 0.8.0:
  72 for 2026-07-27, 249 for 2026-07-28, 107 for 2026-08-10 and 21 for
  2026-08-11. The paragraph that distinguishes an ingest pass from a refresh is
  therefore still true as written.

**Nothing else in either table moved, and that was measured rather than assumed.**
The rebuild ran offline against the cached records (`PLF_OFFLINE=1`, the mode
`ci.yml` uses, so no upstream change could enter through it). Compared against
0.8.0 cell by cell: `features.tsv` differs in exactly **one** content cell,
`PLF:4009`'s `notes`, plus `date_added` on all 112 rows and the `#!version`
header; `provenance.tsv` differs in 843 cells, **all** of them the `retrieved`
date, and in nothing else. The id set and the id order are identical. Both of
those are the build clock and both are outside the content digest by
construction — see `SIGNED_COLUMNS` in `features/build/lib_columns.py` and
`content_digest()` in `features/build/build.py`. `PLF:4009` is `proposed` with no
line in `SIGNOFF.tsv`, so no signature was in a position to lapse, and **all 89
signatures still verify** against unchanged digests. No id moved; `PLF:4006`
stays retired. The table is still **112 rows: 89 signed, 23 proposed.**

## [0.8.0] - 2026-08-11

**Nothing in this release changes what Polylinker does to a sequence.** No
engine, no format reader or writer, no annotator rule and no default moved. A
plasmid opened under 0.7.0 and the same plasmid opened under 0.8.0 get the same
answer, from the same code.

**The 89 signed rows are untouched, and so is the file that signs them.**
`features/SIGNOFF.tsv` is byte-identical to the one 0.6.0 and 0.7.0 shipped —
the same blob, `2d63b169d0de742154b5a7e87c830e12d5052be7`, and the same
sha256 `7cf86057c2b9b964976ad04788a764fd1882b56c2e4cdd427e3395a0fc858e97` — so
**nothing has been signed since 0.6.0**, and all 89 signatures still verify. The
89 records `pl annotate` searches by default are the same 89.

**Three promoter rows joined the table, and none of them ships.** `PLF:4012`
the T3 promoter, `PLF:4013` the araBAD promoter and `PLF:4014` the human
EF-1alpha promoter are in `features.tsv` as `proposed`, which means a program
put them there and no human has read them. `Db::reviewed()` serves none of the
nine Class B rows the table now holds, and `pl annotate` searches none of them
without `--include-proposed`. The table is **112 rows: 89 signed, 23 proposed.**

**A scoring bug was fixed and it moved nothing.** `verify()` had been measuring
only a record's first copy of an element. Fixing it changed no row, no refusal
and no byte of either table — which was measured, not assumed, and is the whole
of the entry below.

### The three decisions this release did NOT take, because they are a curator's

Each of these is a place where the program could have acted and deliberately did
not. They are listed first because a release note that reported only what moved
would be reporting the easy half.

- **The mouse PGK promoter was not issued as a row.** Under the fixed loop it
  measures two independent submissions and two exact placements, and would clear
  both floors. It stays in `stage_classb.HELD`. A stage that promoted an element
  the moment its own code stopped under-counting it would be adjusting its own
  membership, which is the move every refusal in that file exists to refuse. The
  measurement is done and recorded; the judgement is the curator's.
- **The 276 nt chicken beta-actin promoter was refused although the code passes
  it.** `X00182.1:268-543` has the best boundary argument in the file — the
  record's own CAAT signal and TATA box put it at −276..−1 with +1 excluded,
  which is `PLF:4000`'s rule arrived at independently — and two submissions
  annotate it exactly. The second, `OP697986.1`, shares a submitting address
  with `OP697991.1`, which `SOURCING.md` §0.6 names as a **demonstrated false
  negative** of `record_is_snapgene_annotated` and deliberately declined to
  widen the screen for. `same_submitter()` returns True, the mechanical screen
  passes the record, and the author refused to count it anyway: one honest
  placement, held. This is the case where the program said yes and a human said
  no, and it is in these notes for that reason.
- **The T3 convention split was left standing.** The table now holds `-17..-1`
  for T7 and SP6 and `-17..+2` for T3. Three sibling rows using two conventions
  is not tidy, and the alternative is worse: the T3 extent that would match the
  T7 rule has exactly one submitting address behind it. `PLF:4012`'s worklist
  entry states the choice as *your call* rather than resolving it.

### Fixed — "ships" was being used to mean "reached the table"

This is the largest change in the release and it is entirely prose.

**Nothing in `PLF:4000`–`PLF:4014` has ever shipped.** All nine Class B rows in
the table are `proposed`; `Db::reviewed()` has never served one. Prose in this
repository had nonetheless been using *ships* for *is in the table* — in the
build source, in the curator worklist, in `features/README.md`, in
`features/SOURCING.md`, in two Rust doc comments and in this file — with the
result that a reader could come away believing the tool searches promoters it
has never searched. Two of those sentences were written by the previous release
and one, in `features/README.md`, had said since 0.7.0 that `PLF:4010` "still
ships".

The two events now have two names throughout: a row **reaches the table** when a
stage emits it, and a row **ships** when a curator's signature puts it inside
`Db::reviewed()`. `features/PROPOSED.md` and `stage_classb.py` both say so
explicitly at the point a reader meets the distinction, so the next person to
write the sentence has the wording in front of them.

Corrected with it, all measured against the tree rather than decremented:

- `README.md`'s status blockquote said the table holds **20** unread rows. It is
  **23**. (The blockquote's test-asserted counts were not touched.)
- `features/README.md`: the id-block table read *6 of 21 worked up* and is now
  *9 of 25* — `stage_classb.ITEMS` went 12 → 15 and `HELD` 9 → 10; the worklist
  paragraph said **20** rows twice; the Class B summary still read *6 … from the
  twelve*, which the 0.7.0 pass rewrote in its near-duplicate sixteen lines below
  and walked past here; *five of the twelve rows built* is *five of the fifteen*,
  the set of five being unchanged and re-measured against `features.tsv`; and the
  Class B ENA tally, *Six rows contributed 37 … out of 149*, is nine rows, 58
  and 170.
- `features/README.md` also claimed `PROPOSED.md` carries "the exact `--show`
  invocation per row". It did not: `PLF:4014` had no `--show` block. **One was
  added**, so the sentence is true rather than weakened, and the *How to sign*
  invocation — which listed six Class B ids and told the curator to read 20 of
  the 23 rows — now names all nine.
- `features/PROPOSED.md`: the Class B section was headed *The 6 Class B rows*
  and promised *Seven sections … Six of them are rows you can sign*; it is nine
  and ten. The summary table said **19** rows recommended for signing with all
  three 2026-08-11 additions *your call*; counted from the glance table it is
  **20**, and `PLF:4013` araBAD is **SIGN** in that table and in its own heading.
  The 0.7.0 notes record fixing this same heading when it read *The 7 Class B
  rows*; it went stale again inside one release, which is the argument for the
  test this file still does not have.
- `features/SOURCING.md`: *Seven of our twelve* → *Ten of our fifteen*; *Five of
  the twelve … do not ship* → *five of the fifteen … never became rows at all*.
- `features/NOTICE`: the taint gate's recorded scope was **109** descriptions
  and is 112.
- `bins/pl-gui/src/settings.rs`, `bins/pl-gui/src/featuredb.rs`,
  `crates/pl-features/src/lib.rs`: three doc comments describing a 109-row table
  against 89 signatures, and a `text` transcript claiming the table "holds 6
  Class B rows". That transcript is **interpolated live** from
  `records.filter(id.starts_with("PLF:4")).count()`, and its own doc comment
  insists it is "what reproducing the break prints *now*" — so it was refuting
  itself. It prints 9.

### Fixed — `features/NOTICE` denied a fourth ingest pass that had happened

The file said "Three dates because there have been three ingest passes" and, of
the `PLF:4006` withdrawal, that it was "a row leaving, not a fourth ingest pass".
Adding `PLF:4012`–`PLF:4014` fetched **fifteen ENA records that had not been
fetched before, contributing 21 provenance rows dated 2026-08-11** — a fourth
pass by the file's own definition. The snapshot line now names four dates, the
paragraph says what the fourth one is, and it records the check that
distinguishes a new pass from a refresh: the count of rows carrying each earlier
date is unchanged. The paragraph also now says that `retrieved` on an own-work
field is the build date and not a fetch, because that is why 731 `polylinker`
rows and 112 `insdc-ft` rows read 2026-08-11 today.

### Fixed — `stage_classb.verify()` scored only a record's FIRST copy of an element

- **`stage_classb.verify()` scored only a record's FIRST copy of an element, so
  a depositor who carries it twice and draws our edges over the SECOND copy was
  measured as having drawn nothing.** The dict the loop built already carried
  `occurrences`, so the code knew the other copies were there and never looked
  at them. Scoring moved to a new `place_in_record()`, which scores every copy;
  the copy reported is the first one drawn edge for edge, or copy 1 if none is.
  **One record is still ONE placement however many of its copies are drawn** —
  the unit of corroboration in this stage is the submission, and two copies
  inside one record are less independent than two records from one lab, which
  `corroborating_submissions()` already collapses to a single opinion.
- **What that changed for the rows: nothing, and it was measured rather than
  assumed.** All fourteen Class B items the stage offers were run through the
  real `verify()` under both loops. (Fifteen are declared; `build()` reports a
  withdrawn item and `continue`s **before** the `verify()` call, so `PLF:4006`
  is measured under neither loop. This entry claimed all fifteen were, which the
  build path contradicts — `features/PROPOSED.md` said fourteen and was right.)
  Every submission count and every exact-placement count is identical, all sixty
  witness evidence strings are byte-identical, the five rows refused on
  2026-08-10 (`PLF:4002`, `PLF:4003`, `PLF:4004`, `PLF:4005`, `PLF:4011`) are
  refused on the same evidence by the same numbers, **no refused row returned**,
  and `features.tsv` and `provenance.tsv` rebuild byte for byte: 112 rows, 1292
  provenance rows, 89 signatures still valid, 23 still proposed. Only four
  witness records in `ITEMS` carry their element more than once and all four
  annotate their copies alike, which is why. A row returning because the
  implementation changed is not the same thing as a row returning on new
  evidence, so `features/PROPOSED.md` now says how to tell them apart.
- **The one element the fix does move stayed held, and was not issued as a
  row** — the first of the three decisions above. Re-measured under the new
  loop, the mouse PGK promoter has two independent submissions and two exact
  placements where it had one, and would clear both floors. It stays in
  `stage_classb.HELD`: issuing a row is a curator's decision, and a stage that
  promoted an element the moment its own code stopped under-counting it would be
  adjusting its own membership. Ten further held extents were measured the same
  way and none of them moves; the SV40 early promoter cannot be reached by the
  fix at all, its 419 nt form occurring contiguously in no record.
- **One claim the fix itself made, withdrawn before it shipped.** The new
  disclosure said "this record's copies DISAGREE" whenever a record's copies
  were not annotated identically. Measured on `KX264176.1`, that is false: it
  carries the PLtetO-1 element twice, the depositor drew `regulatory` edge for
  edge over BOTH copies, and only a neighbouring `misc_feature` differs. The
  trigger is the annotation and not the extent, and the note now says only what
  was tested — that the record does not annotate its copies alike.
- **`stage_classb.HELD` and `features/PROPOSED.md` said the H1 promoter's 215 nt
  form is held by three independent submissions. It is four**: Neurology at the
  University of Goettingen holds it verbatim too, and draws it `5'+0/3'+1`,
  which is the whole 215-versus-216 question in one number. Found while taking
  this fix's blast radius. The hold is unaffected — zero submissions draw that
  extent, under either loop — and `PROPOSED.md`'s citation for `M18735.1` is
  corrected to Adra, Boer & McBurney.
- Seventeen checks added to `stage_classb.self_test()`, 43 before and 60 after,
  driven through the real `parse_embl()` on fixtures shaped like `AB242435.1`,
  `U13859.1`, `AJ318471.1` and `KX264176.1`. Each was shown to fail against the
  unfixed code: reverting `place_in_record()` to `hits[0]`, restoring the
  withdrawn wording, or suppressing the disclosure when every copy is exact each
  turns the whole stage self-test red, and the stage then emits no rows at all.

### Added — three Class B rows in the table, none of them shipping

- **Three Class B feature rows, `PLF:4012` the T3 promoter, `PLF:4013` the
  araBAD promoter and `PLF:4014` the human EF-1alpha promoter.** All three had
  been held out of the table with stated reasons; all three reasons turned out
  not to survive measurement. Each row clears the same two rules the six Class B
  rows before it clear — at least two INSDC submissions from different
  submitting addresses hold the exact bases, and at least two of those annotate
  a feature at exactly the extent the row carries — and each is anchored on a
  primary record, re-sliced on every build: the bacteriophage T3 genome
  `AJ318471.1`, the 1978 *E. coli* araBAD regulatory-region record `J01641.1`,
  and the human `EEF1A1` gene record `J04617.1`. They **carry**
  `review_status = proposed`, like everything a machine adds, which means they
  reached the table and did not ship: `Db::reviewed()` does not serve them and
  `pl annotate` does not search them. The database is 112 records, 89 signed and
  23 proposed.
- Adding them was the fourth genuine upstream ingest pass: fifteen ENA records
  fetched for the first time, 21 provenance rows dated 2026-08-11. See the
  `features/NOTICE` entry above.
- All three were **appended** to `stage_classb.ITEMS`, not inserted where they
  read best, so no published id moved; `self_test()`'s id pin now covers them.

### Changed

- **`stage_classb.HELD` rewritten.** Six elements are still refused and their
  reasons now say what measurement says rather than what the first pass guessed:
  two were backwards about which of the two rules they failed, one had been
  checking a transcript-level record where the gene record exists, and one failed
  only because `verify()` scored a record's first copy of an element and never
  its second — fixed in this same change, and that entry now records a
  measurement rather than a complaint. Two entries that were one name over
  several unrelated elements — CAG, and the dropped `tetO / TRE / Ptet` — are
  now six separately named entries, each with its own status; three of those
  name extents that clear the corroboration floor and say what is still
  missing before a row could exist.
- Row counts in `README.md`, `features/README.md`, `docs/PLAN.md` and
  `features/PROPOSED.md` updated for the three new rows, including the
  provenance licence and per-source tallies.
- **`docs/RELEASING.md`'s version check is now two greps.** Step 1 said
  `grep -c 'version = "<new>"' Cargo.toml   # must print 17, and <old> must
  print 0` — one command, with the half that catches a `sed` matching nothing
  left as a comment. Both are commands now. The step's concrete sed and tag
  numbers advanced to 0.7.0 → 0.8.0, which that file already documents as
  deliberately one release behind the moment a release lands.
- `PLF:4014`'s worklist entry now cites a **measurement** where it cited
  arithmetic. The claim that the gene form still annotates real pEF vectors was
  argued from 99.7% identity against a 0.96 floor; on review `pl annotate
  --include-proposed` was run against all three 1179 nt deposits (`MG547974.1`,
  `OQ300330.1`, `PP944532.1`) and each returns the row at 99.7% identity and
  100% coverage. One garbled sentence in `PLF:4012`'s entry, which left a
  mid-sentence self-correction in the text, is repaired.

### Known issue

- `stage_classb.parse_embl` cannot see a feature key fifteen characters wide:
  EMBL pads the key field to sixteen columns and `FT_LINE` is
  `^FT {3}(\S+) {2,}(\S.*)$`, which requires two spaces, so `prim_transcript`,
  `minus_35_signal` and `minus_10_signal` — the only three fifteen-character
  keys in `REGULATORY_KEYS` — are declared and unreachable. Found while
  anchoring `PLF:4014`, recorded in that row's caveat and in the module
  docstring, and deliberately **not** fixed in this change.
- **The reason that was given for deferring it was wrong, and correcting it is
  part of this release.** It read: "repairing it adds text to the measured
  `notes` of rows that already carry a curator signature, and those signatures
  would lapse." No signature would lapse. `parse_embl` is called from
  `stage_classb.py` and nowhere else, that stage emits only `PLF:4*` rows, and
  `SIGNOFF.tsv` contains **no `PLF:4` line at all** — the same fact
  `features/PROPOSED.md` states about the PGK entry. The honest reason to defer
  is narrower and survives checking: widening the regex changes the measured
  `notes` of nine rows, which changes `features.tsv`, which needs a rebuild
  against ENA and a fresh measurement — in a release whose entire claim is that
  nothing moved. It is deferred on that ground, not on a signature that does not
  exist.

## [0.7.0] - 2026-08-11

**Nothing in this release changes what Polylinker does to a sequence, and no row
was signed.** The 89 records `pl annotate` searches by default are unchanged in
every field their sign-off covers, and all 89 signatures still verify;
`features/SIGNOFF.tsv` is byte-identical to 0.6.0 — the same sha256 it carried at
that tag — and CI still proves the build never writes it. Of the 21 `proposed`
rows 0.6.0 shipped, **one has been withdrawn by the curator**; the other **20 are
still `proposed`, and still unread by any human**, byte-identical either side of
the withdrawal. What changed is the prose those rows carry, the worklist that
asks a human to read them, and two checks that turned out not to be running.

**One column did move on all 109 rows, and it is not content: `date_added`.** A
build stamps today's date into `#!version`, into every row's `date_added` and
into every own-work `retrieved` unless `PLF_BUILD_DATE` pins it — the mechanism
is `features/README.md`'s, not a change made here — so those bytes differ from
0.6.0 on every row, the signed ones included. The sign-off digest does not cover
that column, which is exactly why all 89 signatures still verify over rows whose
lines are *not* byte-identical. The 0.6.0 entry below calls the signed rows
unchanged "byte for byte"; what was true there was the digest-covered content,
and this entry says which rather than repeating the phrase.

### Removed — `PLF:4006`, the CMV enhancer, withdrawn by the curator

**The table is 109 rows; it was 110.** On 2026-08-11 Lior Lobel withdrew
`PLF:4006` rather than sign it. The reason is recorded beside the declaration in
`features/build/stage_classb.py` and printed by every build: its `notes`
referenced `PLF:4005`, which is not in the table, and asserted a shipping
condition — *ship this row and the enhancer together or not at all* — that the
table violates; of this project's own evidence only the SnapGene-shaped
submissions draw the 380/204 split, while every other deposit annotates the
region as a single element and calls it a promoter; and Boshart et al. 1985
(Cell 41:521–530, PMID 2985280) place the enhancer at −118..−524, which straddles
the split on either numbering, so neither of the row's edges is a literature
edge.

**The id is retired, not freed.** `PLF:4006` will never name anything else. The
declaration stays in `stage_classb.ITEMS` at its index, carrying the reason,
because that is what keeps the number spoken for — deleting it would have moved
the T7 terminator into `PLF:4006` and shifted four more published ids, which
0.6.0 measured and which the mechanism below now prevents.

Withdrawing this row **removes an instance and answers no question.** The posture
question underneath it — whether SnapGene-shaped corroboration counts for a Class
B extent — is exactly as open as it was, the SnapGene screen is unchanged, and
`PLF:4005` is still refused on the evidence that refused it.

**Six further counts moved with the row, in files no test reads.**
`features/NOTICE` said the taint gate ran over 110 descriptions and that seven
Class B rows carry the 2026-08-10 retrieval date; `README.md` said the table
holds 21 unread rows; `features/README.md` said 7 Class B elements; `PROPOSED.md` headed
its Class B section *The 7 Class B rows*; and two Rust doc comments described a
110-row table with 21 proposed and quoted a failure transcript reading *7 Class B
rows*. Only the two README headline claims are test-asserted, which is exactly
why the rest went stale — so each was recomputed from the tables rather than
decremented, and the taint gate was re-run to get the description count instead
of subtracting one from it: **1,367 of theirs against 109 of ours, no shared
five-token run, nothing above 60% containment**, and the same five rows above the
30% line with the same longest runs (1, 2, 4, 3, 3).

**That sweep was not complete, and cutting this release is what found the rest.**
Six more sentences had not moved with the row, and every one of them sits in a
file the paragraph above names by hand. In `features/README.md`: *seven ship*,
still listing the withdrawn CMV enhancer among rows the table holds; *Seven rows
contributed 42 ENA provenance rows … out of 154 in total*, where the same edit
that recomputed `ENA 149` eight lines earlier walked past the 154; *seven is what
survived the rules applied honestly*, which is true of the rules and reads, in
the present tense, one row better than the table is; and a `genbank_key` list of
`promoter`, `enhancer`, `terminator`, `polyA_signal` described as *all four*,
when no row anywhere in `features.tsv` carries `enhancer`. In `features/NOTICE`,
twice, the Class B rows are glossed as *promoters, enhancers, terminators and
poly(A) signals* — a kind with no row behind it, in the file that exists to
describe what is redistributed. Each was recomputed from the tables: six Class B
rows, 37 ENA provenance rows of 149, `promoter` (2), `terminator` (3),
`polyA_signal` (1). The lesson is the one the paragraph above already draws and
understated — untested prose does not stay true because a previous pass looked at
the file.

### Added — a row can be withdrawn, and a test proves the other ids do not move

`Convention` (Stage 5) and `Natural` (Stage 2) both gained a `withdrawn` field.
Both, deliberately: the two stages allocate ids the same way and carry the same
hazard, so a mechanism built for one would be a trap set for whoever first needed
it in the other. It takes the **reason**, not a bool — an id is permanent, so a
withdrawal is permanent, and a bare flag would record that somebody had decided
without recording what they decided. A marked item is dropped by `build()`, its
id is still consumed, and the drop is reported as `WITHDRAWN` with its reason
rather than as a check failing, because a decision is not a failure.

The check that matters is `stage_classb.self_test()` item 8: it pins
`PLF:4006`–`PLF:4010` to the elements they were published as, asserts that
marking one withdrawn moves none of them, and asserts against the same fixture
with the item **deleted** that the pin catches all five reassignments. Proven by
doing it: deleting the CMV enhancer declaration for real failed the pin on every
one of the five ids, and the build wrote nothing.

**And the failure now says so.** When the declaration is missing the check exits
early, and that early exit used to discard every label `must()` had recorded —
so reproducing the break printed one sentence about the enhancer and no evidence
at all that four further published ids had moved with it, while this entry and
`PROPOSED.md` both claimed the pin fails on all five. The five reassignments are
the entire subject of the check; a message that measures them and then throws
them away leaves its own claim unwitnessed. The exit now carries every pin that
failed, so the sentence above is something a reader reproduces rather than
something two files assert about each other.

### Fixed — two checks that were not running

Building the withdrawal turned up two gates that existed and were not being
exercised. Neither changes a byte of anybody's data, and both are why this entry
does not simply report that the checks held.

**`stage_classb.self_test()` ran on no build at all.** Its docstring had said
since it was written that its gates "run on every build". They ran only under
`python features/build/stage_classb.py`; `build.py` called `build()` and never
`self_test()` — so any test written there would have been a test CI never runs,
which is what the id pin above would have been had both not landed together.
They need no network, so there was never a reason for it.
`stage_classb.build()` now runs `self_test()` before it emits a row, and returns
its report.

**`build.py`'s `audit_ids` would have refused this release.** It treated *any*
published id disappearing as fatal — correct for a silent repointing, wrong for a
decision, and a legitimate withdrawal is exactly what this release is. So the
mechanism above would have been built and then blocked by the audit meant to
protect it. It now separates an absence a stage *explains* from one nothing
explains; every other absence stays fatal, and `--allow-id-drift` is still the
only way past a genuine repointing. Four cases in `build.self_test()` drive that
hole, including a withdrawal declared for the wrong id and a row that is present
and repointed.

### Changed — the curator worklist now carries the evidence, not just the questions

[`features/PROPOSED.md`](features/PROPOSED.md) was a list of open questions. It
is now a list of decisions: every one of the 21 rows it was written for carries
the primary source that settles it, what that source settles, and a
recommendation — *sign*, *withdraw*, or *your call* with the options and their
consequences spelled out. Nineteen are recommended for signature, three of those
only after reading a named paragraph; one was recommended for withdrawal and has
since been withdrawn, so the worklist is 20 rows. Three naming and scope
questions and three unadjudicated patent flags are collected at the top as the
things no further research can decide — there were four, and the CMV question is
the one that is now closed. Its
*Claims*, *Anchor*, *Sources* and witness lines are read out of `features.tsv`
rather than retyped, so they cannot drift from the table, and it still carries no
digests for the reason it always did.

**One row was recommended for withdrawal: `PLF:4006`, the CMV enhancer** — a
recommendation, because withdrawing a row is a curator's call too. *He took it;
see* Removed *above.* Its
`notes` sent the reader to "the promoter row above" for why the two halves of the
584 nt block ship together; that row is `PLF:4005`, which the
extent-corroboration rule refused in 0.6.0, so the table is in the state the note
forbids. A user annotating a pcDNA3-type CMV region sees the upstream 380 nt
light up and the promoter half stay dark, and `Db::absent_common_kinds` cannot
say so, because `PLF:4000` and `PLF:4001` supply the literal `promoter` key it
probes for. The evidence turned out to say something stronger than the note did:
of the six records this stage fetches that contain the 380 bases, the only three
that draw the 380/204 split are the three already identified as SnapGene-shaped,
and every submission that is *not* SnapGene-shaped annotates the region as one
element and calls it a promoter. Boshart et al. 1985 (PMID 2985280) put the
enhancer at −524..−118, which straddles the split on either numbering
convention, so no primary source draws this edge either. `PROPOSED.md` sets out
all three options — restore the promoter row, re-cut to one 584 nt row, or
withdraw — with what each costs.

### Fixed — nine rows carried a sentence the evidence does not support

`SIGNOFF.tsv` defines a signature as a human who "wrote or checked its
description from the primary source", so a description written from nothing in
particular is precisely what a signature is supposed to catch. These were caught
before anyone signed, which is the order that rule exists to produce. Each is a
`description` or `notes` change on an unsigned row; no signed row's prose moved.

- **`PLF:4008` rrnB T1 — "nothing in the primary source says 'T1'" is false.**
  Brosius 1984 (PMID 6202587), who sequenced this operon and built the pKK
  vectors these extents come from, reports that T1 and T2 "each function
  separately in vivo"; Orosz et al. 1991 (PMID 1718749) subcloned them
  individually and found "T1 and T2 are both efficient terminators in isolated
  forms" — and that paper is in `J01695`'s **own reference list**. The narrower
  true claim, that the record's *feature table* annotates no terminator, is what
  the row says now. Its "rival extents run 43 to 98 nt" also does not survive
  re-measurement: no rival longer than 44 nt exists in anything checked, and the
  lone 43 nt feature is `U13859` annotating the same bases `rrnB T1` three times
  at 44, 44 and 43 — one submission disagreeing with itself.
- **`PLF:4000` T7 promoter — a note that pointed at evidence which is not
  there.** It said a 20 nt convention "is measured against this row in the
  witness offsets above"; all four offsets above are `5'+0/3'+0`, and no 20 nt
  form exists anywhere checked. The rivals that do exist are 19 nt and 21 nt,
  sit in other rows' records, and are now named. The description also called the
  row "the 17 bp class III promoter" while its own caveat described it as the
  −17..−1 part with +1 excluded; the description now says which. Added: across
  all 17 T7 promoters the anchor annotates, every column from −17 to −3 agrees
  in 14 of 17 records or better while no column from −24 to −18 exceeds 12 of 17
  — a better 5'-edge argument than the row was making.
- **`PLF:4007` T7 terminator — "neither is wrong" understated the row.**
  Macdonald et al. 1994 (PMID 8158645) put termination "at a 3' G residue just
  downstream of the U run"; that G is 24210, this row's last base and the single
  coordinate the anchor annotates as Tphi. Only the 5' edge is a convention, and
  the two rival 48 nt forms are not equally defensible. A claim that some deposit
  gives the name to a different part of the T7 genome was removed: no record
  checked shows it and none was ever named.
- **`PLF:4006` CMV enhancer — a rival "378 nt convention" attributed to a record
  that was neither named nor retained.** Not re-derivable from anything in the
  repository, so it is gone rather than left standing.
- **`PLF:1016` bsr — the organism conflict is resolved and written in.** UniProt
  said *Bacillus cereus*, ENA `S81409` said `/organism="Escherichia coli"
  /strain="TK121"`, and the row named no organism at all. The paper the record
  was created from (Kobayashi et al. 1991, PMID 1368770 — no PubMed abstract, so
  the full text is the only route) says the authors isolated a blasticidin S
  resistant *Bacillus cereus* K55-S1, took the plasmid pBSR8 from it, subcloned
  the gene into pUC19 as pTK17, and grew pTK17 in *E. coli* TK121. **The
  `/organism` is the expression host.** Four corroborations re-derived from the
  record itself: the promoter coordinates a 1998 *Bacillus* paper reports
  (91TTGATC, 113TAAAAT, start at 125) are exact in `S81409`; those are σ^A/σ^B
  promoters and *E. coli* has no σ^B; the CDS is 37.4 % GC with 25.5 % at third
  positions; and the record's ends are the paper's NdeI and HincII sites. The
  organism is written into the description in a sentence of its own, because the
  obvious phrasing is a five-token run of exactly the shape SOURCING.md §0.4
  hard-fails — the same trap `PLF:1015` was already rewritten around.
- **`PLF:1014` pac — the pinned record is flagged `UNVERIFIED_ORGANISM` by the
  archive and the row said nothing about it.** `M25346.1`'s own first line reads
  "UNVERIFIED:", its comment says GenBank staff could not verify the source
  organism, the sequence and/or the annotation, and `P13249` has exactly one EMBL
  cross-reference. The flag is discharged — the organism from Lacalle et al. 1989
  (PMID 2676728) and the `ATCC:12461` culture collection, the extent from that
  same paper's "600-nt open reading frame, starting with an ATG codon", the
  sequence from the stage's forced translation — but a signature that does not
  mention the flag would be a signature saying a curator read the record and did
  not notice its first line.
- **`PLF:1019` LEU2 — presented as uncontested; it is not.** Every pRS vector in
  INSDC carries a LEU2 that differs from this row (`A69V`, `N300D`; plus `G78A`
  and `V195L` in pRS405 and pRS425), and those five records are **one submitter
  on two consecutive days in November 1993** — a single vector series that does
  not agree with itself. `A69V` is also in `X03840`, a genomic record, so part of
  it is allelic rather than error.
- **`PROPOSED.md`'s own instructions for rejecting a row were wrong**, and wrong
  about the thing that matters most right now. "Dropping one does not renumber
  anything after it" is true of a row *dropped by a check* — the five refused
  Class B elements keep their index and are re-measured every build — and false
  of an item *deleted from a stage's `ITEMS`*, because both stages allocate
  `PLF:{ID_BASE + i}` from the tuple index. Measured: deleting the CMV enhancer
  would make `PLF:4006` mean the T7 terminator and would shift four more ids.
  `build.py` catches it and refuses to write, so the failure is loud, but it is
  still a failure. `stage_classb.ITEMS` now carries a comment recording this at
  the point where somebody would reach for the delete key. That section said the
  gate it describes did not exist yet; it does now, and it is the `withdrawn`
  field and its test under *Added* above.

### Added — primary citations for rows that had none

- **`PLF:4009` rrnB T2** now cites Brosius 1984 and Orosz 1991 for the *name*, so
  only the extent rests on vector records. **`PLF:4010` bGH poly(A)** now cites
  its anchor's own publication (Gordon et al. 1983, PMID 6357899) and Goodwin &
  Rottman 1992 (PMID 1644817), and records that the anchor's `exon 2138..2439`
  places the cleavage site 18 bases after the hexamer — which turns "enough
  flanking sequence" from a judgement into a measurement, with the positioning
  element those authors require sitting inside the row with 84 bases to spare.
- **`PLF:4001` SP6 promoter** was described as bounded by analogy to T7. It is
  not: the anchor record's own publication (Dobbins et al. 2004, PMID 15028677)
  identifies ten SP6 promoters and publishes the consensus
  `KAWTTARGKGACACTATAG`, whose −17..−1 window resolves to this row base for
  base, and Brown et al. 1986 (PMID 3010240) fixes the register at `CACTA` =
  −7..−3, which this row satisfies. **`PLF:1015` bsd** now cites Kimura et al.
  1994 (PMID 8159161) for "an open reading frame of 393 bp, encoding a
  polypeptide of 130 amino acids" — this row's extent to the base.

Literature was checked against PubMed and Europe PMC; no source on
`SOURCING.md`'s NO-GO list was consulted. Two papers carry no PubMed abstract
and are marked as such wherever they are used: Dunn & Studier 1983, which
nothing here relies on, and Kobayashi et al. 1991, read from the publisher's
full text.

## [0.6.0] - 2026-08-10

**Nothing in this release changes what Polylinker does to a sequence**, and the
89 feature records `pl annotate` searches by default are unchanged, byte for
byte, with every signature still valid. If you upgrade and change nothing else,
nothing you get out of the tool moves. The database grew from 89 rows to 110;
the 21 new rows are `proposed`, which means a program put them there and no
human has read them, so they are searched only if you ask for them by name with
`--include-proposed` or the equivalent tick-box in the app.

Those 21 are the story, and it has two halves that arrived four hours apart.

**The database got its first promoters, terminators and yeast markers** — 14
selection markers and 12 Class B regulatory elements, the first elements of
those classes it has ever held.

**Then a rule applied honestly took five of the twelve back.** `SOURCING.md` §4
has always required "≥2 independent GenBank exemplars *showing where depositors
actually place it*", and only the first half of that sentence was ever executed:
the build checked that two independent submissions held the *bases*, measured
where each drew the *edges*, wrote the answer into a note, and tested nothing. A
row could therefore ship `boundary_rule = consensus_of_insdc` on a consensus of
one, and four did. Making the second half executable refused `lac`, `tac`,
`trc`, the CMV promoter and the SV40 early poly(A) — each corroborated by
exactly one submission, which is one lab's opinion. Seven Class B rows ship as
`proposed`. That is the finding, not a shortfall.

Two limits belong up here rather than in a footnote, because a release about not
overclaiming cannot overclaim:

- **The new posture check does not detect a coordinate, and nothing here could.**
  It is a process rule: every build stage that emits a boundary must declare how
  it avoided taking one from a vendor, and the gate checks that the declaration
  exists and matches the code. The artifact the taint gate is pinned to has four
  columns — no sequence, no coordinates, no lengths — so there is nothing in it
  to compare an extent against. The taint gate remains a check on the
  **description** column and must not be described as a check on the database.
- **There is a demonstrated false negative inside the shipping witness set.**
  `OP697991.1`, one of the two submissions corroborating `PLF:4006`, carries
  `/note` text byte-identical to the flagged `MH325107.1` but for the token
  `label: `. The screen passes it — correctly by its own rule, wrongly as a
  matter of fact. Widening the screen to that shape was deliberately not done,
  because honest records share it. So "2 independent submissions" must not be
  read as "2 SnapGene-free submissions".

### Added — 21 proposed feature records, and none of them ship yet

`features/features.tsv` goes from 89 rows to 110. **The 89 rows the tool
searches by default are unchanged, byte for byte; every one of their signatures
is still valid.** The 21 new rows are `proposed`, which means a program put them
there and no human has read them, so `pl annotate` ignores them unless you pass
`--include-proposed` and the desktop app ignores them unless you tick the box.
This is what "the tool may propose and never assert" looks like when it is
actually exercised, rather than when the table happens to be fully signed.

**14 further selection markers** (`PLF:1014`–`PLF:1027`), each verified by the
same chain as the existing natural-protein rows — translate the nucleotides,
require an exact residue-for-residue match to a UniProt canonical, cite the
depositor's own coordinates — and each dropped rather than corrected if any leg
disagreed: `pac`, `bsd`, `bsr`, `dhfrI`, `URA3`, `LEU2`, `HIS3`, `TRP1`, HSV
`TK`, mouse `Dhfr`, `gpt`, `bar`, `pat` and `rpsL`. They give the database its
first yeast markers of any kind. They **narrow** the eukaryotic selection-marker
gap `features/SOURCING.md` names as Gap 6 without closing it, and the earlier
draft of this entry said "close", which was an overclaim on two counts: three of
Gap 6's five named markers were already signed before today, the two these add
(`pac`, `bsd`) are `proposed` and so are searched by nobody, and Gap 6 also names
the codon-optimised forms, of which this adds none — every row here is a native
CDS. Gap 6's own entry now records that.

**7 Class B regulatory elements** — the T7 and SP6 promoters (`PLF:4000`,
`PLF:4001`), the CMV enhancer (`PLF:4006`), the T7, rrnB T1 and rrnB T2
terminators (`PLF:4007`–`PLF:4009`) and the bGH poly(A) signal (`PLF:4010`).
These are the first promoters and terminators of any kind in the database. A
Class B boundary is a *convention* and not a fact, so each row ships a coordinate
slice of one named INSDC record, and two claims are re-checked on every build:
that at least two records **from different submitting addresses** hold those
exact bases, and that at least two of those submissions annotate a feature at
**exactly** the shipped extent. The second of those is new — see below — and it
is why this is seven rows and not twelve.

Three things that came out of building it and are documented rather than
smoothed over:

- **INSDC records carry SnapGene annotation, and the CI taint gate cannot see
  it.** ENA folds SnapGene's `/label` into the `/note`, so its editorial prose
  arrives through a source this project cleared. The gate compares descriptions
  and can never notice a *coordinate* arriving that way. The stage therefore
  reads no `/note`, `/label`, `/gene`, `/product` or `/standard_name` at all,
  and refuses to count a SnapGene-annotated deposit as an independent witness.
  Two of the seven surviving rows have a witness excluded on those grounds, and
  three more of the five that were refused did too.
  **This is no longer an open hole — see "the coordinate route" below.**
- **The taint gate fired for real, for the second time in this project's
  history**, on the blasticidin deaminase description, whose first draft shared
  a five-token run with their file. Nothing was copied; the row was rewritten
  anyway, because the rule is mechanical on purpose.
- **Nine more elements were worked up and are not here**, each with its reason
  recorded in `features/build/stage_classb.py`: T3, the SV40 early promoter, U6,
  H1, EF-1α, PGK, CAG and araBAD are held, and tetO/TRE is dropped outright
  because the name covers four unrelated elements. `SOURCING.md` budgets about
  forty Class B rows; seven is what survives both rules applied honestly, and
  that is the finding rather than a shortfall.

**A curator worklist, [`features/PROPOSED.md`](features/PROPOSED.md).** Twenty-one
rows nobody has read is a request for several hours of a specialist's attention,
and "here is the table, good luck" is not how to ask for it. The file gives each
row's claim, the accessions to check it against, the boundary chosen and its
basis, and the exact `--show` invocation — and it leads with the rows where the
exemplars *disagreed*, because those need a decision rather than a check: the two
T7-terminator forms that are offset from each other by one base, rrnB T1's rivals
running 43 to 98 nt, and `PLF:1016`, which should not be signed at all until an
unresolved organism conflict between UniProt and the ENA record is settled. It
carries no digests on purpose — `SIGNOFF.tsv` says signing a digest nobody has
read is not an attestation, and a worklist you could copy twenty-one hashes out of
without opening a row would be a machine for producing exactly that. It now also
carries the five elements that were refused, so the work of rescuing them is
asked for rather than left implicit.

### Added — the coordinate route, declared by every stage that could carry it

The bullet above says the taint gate cannot see a coordinate arriving from
SnapGene through INSDC. That sentence described an open hole for one release.
This closes it — and the first thing to say is what "closes" means, because the
obvious repair is not available and pretending otherwise would be the exact
defect this project keeps catching in itself.

**A coordinate-level taint check cannot be built here, and the reason is not
effort.** `features/build/insdc_posture.py` carries the argument in full and
`features/SOURCING.md` §0.6 carries the measurements. In short: the artifact the
gate is pinned to, pLannotate's `snapgene.csv`, is four columns of `sseqid`,
`Feature`, `Type` and `Description` — **no sequence and no coordinate**, so there
is nothing in it to compare an extent against. SnapGene's feature bases live in a
separate bulk asset that carries no licence and sits on a host the build refuses,
and fetching a complete copy of their extents in order to prove we did not copy
their extents would be a larger act of copying than the one being disproved. And
the sequences are biology: the T7 promoter is the T7 promoter, an exact match
proves nothing about copying, and a rule keyed on agreement fires on **84%** of
the distinct extents in a 481-record survey of this database's own witnesses —
100% for the rarer ones. That is a check that gets switched off in a week, which
is a check that proves nothing.

**So the enforcement is structural rather than statistical.** Every stage in
`build.STAGES` must declare `INSDC_POSTURE`, naming one of four postures and
saying in its own words what it does about a depositor's coordinates. The gate
refuses a stage that declares nothing — the same shape, and for the same reason,
as the existing rule that refuses a stage that does not declare its id block —
and then checks the mechanical half of whatever was declared:

- a `no_insdc` stage must name no INSDC host;
- a `no_feature_table` stage must name no record flat-file endpoint, because a
  feature table is only served by the flat-file view;
- a `feature_table_forced` stage must **name** the test that forces its extents,
  and the gate drives that test with a CDS that translates to its protein and one
  that does not;
- a `feature_table_convention` stage must name its SnapGene screen, which the
  gate drives against a record carrying the tell and one without, and its
  corroboration floor, which may not go below two.

Each of those was proven to fail by breaking the real tree seven ways — deleting
a declaration, adding an ENA fetch to the stage that says it makes none, pointing
a FASTA-only stage at a flat file, blinding the SnapGene screen, making it fire
on everything, lowering the floor to one, and neutering the translation check
that `stage_uniprot`'s whole posture rests on. All seven go red. The gate runs
inside `taint_gate.py` **before** the fetch, so it still reports on a day the pin
is unreachable, and it is now a step in `tools/ci.ps1` — the first half of the
taint gate to have a local twin, since it needs no network at all.

**Say plainly what this does not buy.** It does not show that no coordinate in
`features.tsv` agrees with SnapGene's, and nothing in this repository can. It
shows that no stage reached the table without a human answering the question, and
that four named mechanisms still work. `SOURCING.md` §6 now forbids describing
the taint gate as a check on the database: it is a check on the description
column, and saying more than that is the overclaim.

### Changed — a Class B row must now show that depositors put the edges where we did

`features/SOURCING.md` §4 has always asked for "≥2 independent GenBank exemplars
**showing where depositors actually place it**", and only the first half of that
sentence was executed. `stage_classb.verify()` required two independent
submissions to *contain the bases* — a fact about the sequence, not about a
boundary — then measured where each of them drew the edges, wrote the answer into
`notes`, and tested nothing. A row could therefore ship
`boundary_rule = consensus_of_insdc` on a consensus of one, and four did.

`MIN_PLACEMENTS` makes the second half executable: two independent submissions
must annotate a feature at **exactly** the shipped extent, edge for edge, with no
tolerance — a tolerance would be a knob to widen until the row passed. **Five of
the twelve candidates fail and do not ship:**

| Row | Element | Submissions holding the bases | Placing it exactly |
|---|---|---|---|
| `PLF:4002` | lac promoter | 4 | 1 |
| `PLF:4003` | tac promoter | 3 | 1 *(two records, one lab)* |
| `PLF:4004` | trc promoter | 2 | 1 *(the anchor itself)* |
| `PLF:4005` | CMV promoter | 3 | 1 |
| `PLF:4011` | SV40 early poly(A) | 3 | 1 |

Two things about that table. The rows stay in the stage's allow-list rather than
moving to `HELD`, so they keep their ids, are re-measured on every build, and come
back by themselves the day a curator cites evidence that corroborates the extent
— or re-cuts it to one the evidence already corroborates. And `PLF:4005` is worth
looking at twice: the CMV promoter's *only* exact corroboration is `LC897329`, a
record whose feature table is SnapGene's Common Feature naming throughout with no
`label:` tell in it. That is the blind spot at the top of this entry, biting a
real row — caught by a rule that names no vendor and reads no `/note`.

**On review, that blind spot is wider than the sentence above admits, and the
one row it was said to spare is not spared.** `PLF:4006`, which ships, has
exactly two corroborating submissions and **both** carry a SnapGene fingerprint
the screen passes. `LC897329.1` is the naming case again. `OP697991.1` is
sharper and was measured on 2026-08-10: four of its `/note`s — over the CMV
enhancer, the CMV promoter, the ColE1 origin and the AmpR CDS — have a
descriptive half **byte-identical** to the corresponding `/note` in
`MH325107.1`, the record the screen does flag, in the same two-part shape ENA
emits when it folds a `/label`, differing by the token `label: ` and nothing
else. Neither observation refuses the row — an extent two independent
submissions publish is attested whatever tool drew it — but "2 of 3 independent
submissions" must not be read as "2 of 3 SnapGene-free submissions".
`features/PROPOSED.md` and `features/SOURCING.md` §0.6 now say so, and §0.6 also
now separates which of its four rejection grounds a reader can re-derive from
this tree (point 1, re-measured against the pinned artifact, along with §0.5's
figures) from which they cannot (points 3 and 4, a one-off 481-record survey
whose record list was not preserved).

**This rule is not a taint check and must not be described as one.** It cannot
show that an extent came from SnapGene. It answers the narrower question that is
answerable: did our own evidence force this extent, or is it one lab's opinion?

### Fixed

- The desktop app's "no promoter is in this database yet" line is computed by
  probing the table for literal INSDC feature keys, and the twelve new rows were
  invisible to that probe for the length of one build because they used the
  current INSDC spelling (`regulatory`) rather than the retired one. The app
  would have gone on saying "no promoter" after promoters were signed off. The
  rows now carry `promoter`, `enhancer`, `terminator` and `polyA_signal`, and a
  test pins the disclosure to the table it describes in both directions.
- Two counts in `features/README.md` were wrong before this change and are
  corrected by measurement: the alias-collision table said twelve colliding
  strings when there were more, and listed `smR` as resolving to two records
  when it resolves to three.
- **Every Class B row told the curator the anchor record annotated nothing near
  it, when what had been measured was narrower than that.** The sentence read
  "ANCHOR RECORD'S OWN ANNOTATION within 80 nt of this interval: none", but
  `parse_embl()` keeps only regulatory-type feature keys, so a CDS, gene or exon
  over the interval was never looked at and could not appear. It is not
  hypothetical: `X17403.1` annotates `CDS complement(173505..>173909)` straight
  across `PLF:4005`'s interval, and the row said "none". The rows are still
  right — a CDS is not a rival promoter boundary — but a curator reading "none"
  had been told the region was bare, which it is not. The note now says which
  keys were counted and that it is silent about the rest.
- `features/README.md` described `build/stage_curated.py` as "Stage 5" when it
  is and always was Stage 4. Harmless while there were four stages; actively
  misleading the moment a real Stage 5 (`stage_classb.py`) landed underneath it.
  Both rows are now in the file table, with the correct numbers.

## [0.5.0] - 2026-08-10

**Nothing in this release changes what Polylinker does to a sequence.** No
parser, no renderer, no digest, no annotation and no file format is touched.
What changes is what "CI is green" is worth: for six releases it was worth
nothing, because the gate `docs/RELEASING.md` names before tagging was run by
no workflow at all, and the number that gate produces was a terminal on one
Windows machine.

One thing here is user-facing, and it is first for that reason.

### Fixed

- **The Windows installer refused a correct download as incomplete.**
  `Install-Polylinker.ps1` checks the extracted files against
  `SHA256SUMS.txt` before it installs anything, and it built the relative name
  of each file by subtracting the source directory's path from
  `FileInfo.FullName` — two strings produced by two different normalisers.
  `Resolve-Path` hands back the string it was given; `Get-ChildItem` reports
  what the volume holds. Where those differ, every name came out wrong, nothing
  matched the manifest, and the installer stopped with *"this copy is
  incomplete — 21 file(s) the manifest lists are not here"* over a download in
  which all 21 were present.

  They differ in two measured ways. An **8.3 short name** anywhere in the path
  is shorter than what the volume reports — `C:\PROGRA~1` is 11 characters and
  `C:\Program Files` is 16 — so the subtraction left the tail of the source
  directory welded to the front of every filename. A **trailing separator**
  needs no alias at all and reproduces on any machine: `Substring(len + 1)` cut
  one character too many, and `a.txt` came back as `.txt`.

  Which invocations were exposed is measured rather than assumed, because the
  first draft of the fix's own comment got it wrong. Running the installer the
  way `README-WINDOWS.txt` and `Install.cmd` tell you to is **safe**: `-Source`
  defaults to `$PSScriptRoot`, and PowerShell hands that over already expanded
  and without a trailing separator, on 5.1 and on 7 alike. What is exposed is
  an explicit `-Source`, and the obvious thing a wrapper script passes to it is
  cmd's `%~dp0`, which carries both defects at once — the alias verbatim and a
  trailing backslash always.

  The same wrong subtraction sat in two more places in that file, where the
  failure would have been worse than a refusal: an uninstall decides whether to
  copy itself out of the directory it is about to delete by asking whether
  `$PSCommandPath` is under `$Prefix`, and `Stop-IfRunning` decides whether the
  app is running out of the prefix by comparing against `Process.Path`. Windows
  reports both of those expanded, so a short `$Prefix` answered "no" to both:
  the uninstaller would have been deleting its own running file, and an upgrade
  would have overwritten files mapped into a running process instead of
  refusing. All of it now goes through one `Get-DirectoryPrefix`.

- **What did *not* reach a shipped release, said plainly rather than left to be
  inferred.** Two defects of the same family turned up beside the one above,
  and the reason neither touched a published artifact is where they could
  reach, not luck.

  The same path arithmetic was in `tools/release.ps1`.
  `.github/workflows/release.yml` invokes that script as
  `release.ps1 -Out dist` — a relative path, resolved under the runner's
  workspace, which carries no 8.3 component — so the defect had nothing to bite
  on there. Checked rather than reasoned: the Windows archive published for
  v0.4.0 verifies against its own manifest, all 21 entries matching the bytes.

  Separately, `RUSTFLAGS: -D warnings` set at workflow level was *replacing*
  `.cargo/config.toml`'s `-C target-feature=+crt-static` rather than merging
  with it, putting a VC++ redistributable dependency back into anything built
  under it — a dependency whose installer needs administrator rights, which the
  user this project is aimed at does not have. That was in `ci.yml` only;
  `release.yml` has never set `RUSTFLAGS`, so no shipped binary ever carried
  it. Both were found because the gate started running in the places where the
  defects were reachable.

- **The gate now runs in CI, on all three platforms.** `tools/ci.ps1` is a
  72-step gate and [`docs/RELEASING.md`](docs/RELEASING.md) names it as the
  thing to run before tagging. **No workflow invoked it.** It appeared in
  `.github/workflows/ci.yml` six times and every one of those was on a comment
  line, so from v0.1.2 until now it had been failing on a clean tree and
  nothing reported that: 0.1.2, 0.1.3, 0.2.0, 0.3.0, 0.3.1 and 0.3.2 were all
  tagged with it red. `ci.yml` has a `gate` job now — `windows-latest`,
  `ubuntu-latest` and `macos-latest`, `fail-fast: false` — and a red gate is a
  red build.

  The three legs of that job, read out of the per-leg ledgers of run
  31364592031 — this release's parent commit — rather than described:

  | Leg | Steps | Ran | Skipped | `not windows` | corpus |
  |---|---|---|---|---|---|
  | `windows-latest` | 72 | 67 | 5 | 0 | 5 |
  | `ubuntu-latest` | 72 | 60 | 12 | 7 | 5 |
  | `macos-latest` | 72 | 60 | 12 | 7 | 5 |

  **The port did not weaken the Windows leg**, which is the leg that has been
  standing between a mistake and a published release: 67 steps ran before it
  and 67 ran after, the same five skipped for want of a corpus both times, and
  the two lists of what ran differ on two names — both of them renames of a
  step that ran either way. It was done by spelling rather than by skipping: an
  `$exe` suffix for 25 hardcoded `pl.exe` sites, `[IO.Path]::GetTempPath()` for
  17 unguarded `$env:TEMP` uses, a three-way `.pyd`/`.so`/`.dylib` table,
  `tar$exe`. Two things nothing had ever exercised now run for real: the tar.gz
  writer, on the two platforms whose users are the reason it exists; and the
  zip's "entry order is sorted" claim, which off Windows is fed ext4 and APFS
  enumeration orders instead of only NTFS's.

- **The gate's skips are checked, not printed.** A step whose tooling is
  missing SKIPs rather than failing — right on a workstation, dangerous on a
  runner, and the reason the gate passed on the author's machine for six
  releases *with six steps skipped*: the wasm-versus-native comparison, both
  chromatogram oracles, digest versus Biopython on real plasmids, the
  Rust-versus-Python reader, and the MSI install test. The job now installs
  what a runner can be given — eleven Python oracles, Node 24, the
  `wasm32-unknown-unknown` target, the TypeScript toolchain, and on Windows the
  WiX Toolset and a `dist/` for the MSI steps to read — and passes
  `-ExpectedSkips`, a new parameter that fails the run on any difference from
  [`.github/ci-expected-skips.txt`](.github/ci-expected-skips.txt) in either
  direction. Set equality, not a count: a count of five is satisfied by the
  wrong five skipping, and a name matching no step in the gate is itself a
  failure, so a renamed step cannot drift off the list unnoticed.

  Five of the six skips remain, and they are one skip five times: those steps
  need real `.dna` and `.ab1` files, and a lab's plasmids are not ours to
  publish. It is the same five on all three legs, which is why that file needs
  no platform column. The sixth, the MSI install-and-uninstall test, now runs
  on every push — the only check that puts a real `msiexec` against a real
  registry, and until now its first execution on any given release was after
  the tag.

- **Three gate steps that had never run in CI at all now do**, because each
  needs a Python package the existing `oracles` job does not install: *gel
  calibration spline vs SciPy*, *PDF is a PDF, and matches the SVG* against
  PyMuPDF and pypdf, and *the release workflow parses and covers three
  platforms*, which parses `release.yml` with PyYAML.

- **`Get-DirectoryPrefix` is one function copied three times, and that is now
  checked rather than asserted.** It is defined in `tools/ci.ps1`,
  `tools/release.ps1` and `tools/installer/Install-Polylinker.ps1`, and the
  duplication is legitimate and stays: `release.ps1` copies the installer into
  the archive root as a single flat file with nothing beside it to dot-source.
  What was missing is that nothing compared the copies while `ci.ps1`'s comment
  called `release.ps1`'s the "identical function" — prose standing in for a
  check, sitting directly on top of the defect above. Two steps hold it now.
  One finds every definition under `tools/` by parsing rather than by path,
  and compares the bodies as token streams, so a reformat is invisible and a
  dropped `TrimEnd` is not. The other is the one that matters, because three
  identically wrong copies pass a drift guard: it drives each extracted
  definition over a **real** 8.3 alias, discovered on the machine rather than
  minted, and requires the arithmetic it replaced to still come back mangled on
  the same input.

### Added

- **A `reconcile` job, because no leg of a matrix can see the others.** Each
  leg writes a ledger — every step, `ran` or `skipped`, and the reason a skip
  gave for itself — and `tools/reconcile-ledgers.ps1` compares the three. It is
  the only place a step that skipped on **all three** platforms can be seen
  (`not windows` on two legs is only honest if the third ran it), and the only
  place a step deleted from one leg at file level can be seen. The job runs the
  reconciler's own self-test first, because a reconciler whose parser stopped
  matching reports every push clean.

### Changed

- **`ci.yml`'s `test` matrix drops `windows-latest`** and runs on
  `ubuntu-latest` and `macos-latest`. That leg was the one piece of genuine
  duplication, because the `gate` job runs those steps on the same runner
  image. What it is **not** is a copy of the gate, and the cost is stated in
  the workflow rather than glossed: every cargo invocation in `test` passes
  `--locked` and no cargo invocation in the gate does, so a rewritten lockfile
  is a red build there and nowhere else, and `pl-draw/tests/memory.rs` and
  `pl-features/tests/schema_pin.rs` are run by no step in the gate at all. Lock
  drift is a property of the tree and not of an operating system, and the two
  suites count `Layout` sizes and compare a Python file to a Rust constant, so
  none of the three has an operating system in it; they run on two platforms
  now instead of three. `tools/ci.ps1`'s step *every integration suite is run
  by a gate* is what keeps that division honest — it reads the tree, and every
  `tests/*.rs` target under `crates/` and `bins/` must be run, whole, by one of
  the two files.

### Known limitations at this release

- **A mislabelled skip is held by review alone, and nothing mechanical catches
  it.** The skip rules catch a step that stops running for a reason *nobody
  declared* — a wheel that stops building, a `pip install` line edited in a
  hurry — which is the failure that actually happens. They do not catch a human
  hand-writing `WindowsOnly` into a portable step's own precondition. Such a
  step goes on running on Windows, so every per-leg rule is satisfied (the
  reason was declared, it agrees with the platform, it is not a corpus skip)
  and so is the reconciler's requirement that every step ran somewhere. Two
  platforms of coverage disappear and nothing anywhere turns red.

  This is measured, not reasoned. It was done on purpose to *gel calibration
  spline vs SciPy* and pushed: run 31361991651's Linux leg
  reported **eight** `not windows` skips where seven is the honest count, and
  it passed. The sentence that used to say there was "no line anybody can add
  anywhere to quiet a platform skip" was wrong and now says what is true: no
  line in `.github/ci-expected-skips.txt` can, which is what the split between
  that file and the preconditions buys, and it is the whole of what it buys.
  What holds the rest is that `WindowsOnly` is one greppable identifier in
  seven places and `tools/ci.ps1` is reviewed. The two obvious repairs — a
  file per platform, or a platform column — are both a second, unverified claim
  about where a step *ought* to run laid on top of the one thing that is
  actually verified, which is what it did; `.github/ci-expected-skips.txt` sets
  that argument out at length.

## [0.4.0] - 2026-08-09

The minor number moves rather than the patch because the window looks
different on launch, the minimum window size changed, and a font was added
to the archive. Nothing about a file Polylinker reads or writes changed.

### Changed

- **The desktop window has a design system.** Colour, spacing, typography,
  rounding and shadow are ported from the author's other eframe application so
  the two programs read as one piece of software: an orange accent taken from
  Polylinker's own icon, softer and more downward shadows, 6 pt widgets in 10 pt
  windows, and slightly larger body text. No panel moved, no tab was renamed and
  no screen was reorganised. Two of the ported colours were overruled by
  measurement rather than copied — see *Accessibility* below.
- **The light/dark choice is remembered.** The toolbar switch has been there for
  a while and died with the process, because this application deliberately does
  not use `eframe`'s own persistence. It is now written to the layout file on
  the click, and **Help ▸ Follow the desktop's theme** puts it back to following
  the system, which egui's two-state switch offers no way to do.
- **The minimum window is 990 × 560, up from 880 × 560.** This is the design
  system's one measured cost and it is paid rather than hidden. The toolbar's
  fixed run is priced in button padding and button text size, and the ported
  values are larger than egui's on both; measured through the real toolbar at
  880 pt, the run's right edge moves 86.9 pt and the title block it leaves room
  for falls from 193 pt to about 48, at which point the status line is not
  truncated but **absent** — and the status line is where an export says "this
  drops 9 feature(s) and the topology". The width was swept in 10 pt steps: the
  status returns at 960 and reaches the length the old minimum used to show at
  980. The default size is unchanged at 1280 × 840.
- **A destructive button stays legible while it is pressed.** *Delete feature*
  carried its red in the string, which pins one colour through every widget
  state; with the accent behind a pressed control that measured 2.10:1 in dark
  mode. The red now lives in the widget's resting ink, and egui's own label
  colour takes over under the pointer.

### Added

- **Inter SemiBold**, SIL OFL 1.1, for headings only — window and dialog titles.
  It is in a font family of its own and in neither text chain, on purpose: its
  capital `I` and its lowercase `l` are the same bare stem, and this
  application's proportional text is enzyme names like `AflII` and `BspLU11III`.
  IBM Plex Sans keeps the body. Inter Regular is not vendored at all. The
  archive now carries eight font licence texts, and `licences/Inter-OFL.txt` is
  required by name by the packaging gate.

### Accessibility

- **Every ink the window paints is now measured against every surface it is
  painted on**, in both themes, by a test rather than by a review. Two of the
  ported tones did not clear WCAG AA and were moved: the light striped-row
  colour, on which the feature editor's inverted-span sentence measured
  **4.48:1**, and the dark one, on which the tertiary text role measured
  4.5007:1 — passing by seven ten-thousandths, which is not a margin. Both moved
  one notch towards their own panel, to values already in the design system.
- The accent is a pair, because one value cannot do both jobs: `#E69F00` is
  2.25:1 on white and unusable as light-mode ink, so light mode uses
  `rgb(140, 97, 0)` — the same colour scaled, hue unchanged — and the two swap
  roles with the theme. Every accent fill takes its foreground by measurement.

### Fixed

- **Layout tests were measuring a `Style` the binary does not install.** The
  test context installed the shipped fonts and then left egui's default spacing
  and text sizes in place; making it honest turned eleven green tests red at
  once, two of them against the style 0.3.2 shipped. All eleven were traced
  rather than re-baselined: six read the sequence grid's geometry from an
  unsettled first frame and then clicked with it, landing up to three bases
  away; one read a window's footer before the window had finished growing; one
  asserted a panel width egui had stopped being able to grant; one duplicated a
  `pl-doc` invariant through a 420 pt viewport.

## [0.3.2] - 2026-08-09

A 32-agent audit raised 25 findings; 8 were refuted and 17 survived an
independent skeptic. All 17 are fixed here, each with a test shown to fail
against the unfixed code first.

### Fixed

- **Saving one tab could destroy the work in another, silently.** With two
  documents open and both edited, answering the quit dialog's
  "Save as .dna…" closed the window over every *other* dirty tab — and
  deleted their crash drafts on the way out. No prompt, no draft, no way
  back. **If you have been running 0.3.1 or earlier with more than one tab
  open, this is the reason to update.**

  The guard asked only the *active* document, which had been marked clean
  four lines earlier, so the check could never fail. The GenBank arm of the
  same dialog already did the right thing and its comment described this
  exact hole; one arm had been fixed and its sibling left. The condition now
  lives in the single place both arms pass through, because two half-guards
  that must agree is the defect, not the asymmetry.

- **Closing a tab with unsaved work then quitting lost it.** `Ctrl+W`
  deleted the tab's crash draft and hid its edits from the quit guard, so
  the app exited without asking. Closed-but-dirty tabs are now put back on
  the bench before the guard reads it.

- **A `.dna` file could round-trip to a different molecule.** Strings taken
  from the input were written into GenBank lines where column position
  carries meaning, so a name or description containing the wrong character
  moved the fields around it. Everything interpolated into a line is now
  flattened first, and what had to change is reported rather than done
  quietly.

- **`join(complement(a),complement(b))` was rewritten as
  `complement(join(a,b))`.** Those name different spliced products. A
  feature read from one file and written back out could describe a
  construct nobody built.

- **A `>` inside a sequence split an exported FASTA into two records.**

- **Methylation was judged at an enzyme's first site only**, so rotating a
  plasmid changed whether the app said Dam or Dcm blocked it. The verdict is
  now per site, and says how many of how many are affected.

- **`pl-clone::digest` gave different fragments depending on the order the
  enzymes were passed** — blunt or sticky ends could turn on an argument
  order the caller has no reason to think matters.

- **Four buttons' explanations were unreachable**, a failed update download
  left its partial file behind while reporting that nothing was written, and
  a superseded Sanger comparison ran to completion because its cancel flag
  never reached the worker.

### Changed

- **Gate steps that were running nothing now run something.** 52 of the 53
  command-line integration tests were executed by no gate; one step filtered
  on a test name that no longer existed and ran zero tests; a `pl-fileio`
  suite was referenced nowhere. A step that runs no tests and one that runs
  53 look identical in a green log, which is how they survived.

- Four comments claimed guarantees the code no longer honoured, including
  `pl-draw` promising byte-identical output on every platform while its own
  raster module records that the PNG path is not.


## [0.3.1] - 2026-08-08

### Fixed

- **Four greyed-out buttons could not say why they were greyed out.** They
  attached their explanation with `on_hover_text`, which on a *disabled*
  widget shows nothing at all: egui 0.35 routes it through
  `Tooltip::for_enabled`, and that opens the popup only when the response is
  enabled. So every one of those sentences was written in the branch that
  runs precisely *because* the button is grey, and could never be read.

  It is the shape of mistake that leaves nothing behind — no warning, no
  wrong answer on screen, just a hint nobody can reach. The user hovers the
  grey button asking what to do, and the app says nothing back. Now
  `on_disabled_hover_text`, in "Design primers…", "New feature…", "Copy
  rev-comp" and the primer designer's "Add to document". "Copy protein" in
  the same row was already right, and carried the comment explaining the
  trap that its three neighbours had fallen into.

### Changed

- **The feature-database gate no longer depends on anyone else's uptime.** The
  CI step that proves the build never writes `features/SIGNOFF.tsv` did it by
  running the real build against live EBI, NCBI, UniProt and RCSB. On
  2026-08-07 EBI timed out twice in a row and main went red twice, both times
  for a reason no commit under test had caused. The step was not wrong to fail —
  the build had died before reaching the writer, so it had genuinely checked
  nothing — but a red gate that says nothing about the code teaches people to
  ignore red gates, which costs more than the check is worth.

  The rule is unchanged and is now proved twice, offline, on every push.
  `features/build/check_writer.py` drives the real writer over the real shipped
  rows with the real signatures applied; the end-to-end run sets `PLF_OFFLINE=1`
  so `fetch` refuses the network instead of hoping for it. Neither can be turned
  red by a third party, and both now run in `tools/ci.ps1` too, which they could
  not when they needed a network.

  The check got stricter in three places on the way. It now also proves the
  build *reads* the sign-off — the step was named "The build reads SIGNOFF.tsv
  and never writes it" and only ever tested the second clause, so a build that
  ignored the file entirely passed it. It looks for a stray `SIGNOFF.tsv` at any
  depth and in any case, where the old `test ! -e` saw neither a subdirectory nor
  `signoff.tsv` on a case-insensitive filesystem. And it plants five misbehaving
  writers and requires itself to catch each one before it will certify the real
  one, then requires itself to pass a writer that does nothing wrong — because a
  check that fires on everything proves as little as one that fires on nothing.

  Verification against live sources still happens, in a new scheduled
  `features (live sources)` workflow that also reports whether the shipped table
  still reproduces from upstream. It is not a gate, and an unreachable source
  there is reported as *not checked* rather than as a failure.

- **`build.py` tells an outage apart from a defect.** A failed fetch raised
  `SystemExit`, which killed the interpreter before `write_outputs` ran and was
  indistinguishable from `check_fetch_host` refusing a source no licence covers.
  It now raises `SourceUnavailable`, the stage drops out, the build reaches its
  writer and exits **3** with `build-source-unavailable`. A sourcing violation
  still stops everything, and an HTTP 4xx — a withdrawn or mistyped accession,
  which *is* this repository's defect — is now fatal instead of being retried
  four times and then excused as an outage. Nothing can ship from a short build:
  the id-stability audit already refuses to overwrite a published table when rows
  go missing, and that, not the abort, was always what protected it.

  A per-host circuit breaker stops a real outage costing four timeouts on every
  one of the hundreds of accessions `stage_curated` requests; the first failure
  against a host is remembered and the rest give up at once.

## [0.3.0] - 2026-08-07

### Added

- **Linear molecules get a linear figure.** Exporting a PCR product, a
  linearised vector, a gene fragment or a gBlock produced a C-shaped ring with a
  gap in it. The gap was correct about topology and was still the wrong picture:
  every FASTA and every assembly opens linear, so this was not an edge case. A
  linear molecule now exports as a horizontal track — features as boxes and
  arrowheads on a band the backbone runs through, cut sites as ticks with their
  coordinates above, a ruler beneath — in SVG, PDF, EPS and PNG, from `pl
  export` and from the app's Map items alike. **Circular molecules are
  untouched, byte for byte.**

  It is one geometry, not a second renderer. The figure is built as the same
  `Scene` the ring is, out of the same three primitives, so no writer changed and
  the app's on-screen `Scene` painter can consume it; labels are packed by the
  same isotonic regression, features resolved by the same `ranges`/`mid_base`
  pass, and a base's position along the molecule now comes from one function that
  the ring multiplies by a turn and the track by its width. What is new is a band
  of label rows above the track, filled nearest-first, where a row that cannot
  hold a label hands it to the next row out — so a polylinker's twelve cutters
  cost nothing rather than eleven names.

  The shape is `Options::shape`, which defaults to asking the molecule. Both
  overrides have a user and neither was reachable before: `Shape::Linear` on a
  plasmid is the cut map, and says so in the figure and in `Report::cut_open`,
  because nothing in the geometry of a track distinguishes a linearised plasmid
  from a molecule that really is a line. `Shape::Circular` on a linear molecule
  is the gapped ring, unchanged.

  `Options::height` is a budget on a linear figure rather than a canvas: the
  scene comes back as tall as it needed. At the 720 × 720 default a PCR product
  is a 138 pt drawing, not a 138 pt drawing centred in 720 pt of white that
  `page::Fit` would print as an 89 × 89 mm block and a raster export would pay
  for in pixels. A budget too small for the caption, the band and the ruler
  yields a figure taller than it rather than a figure with its scale cropped
  off.

  A feature spanning the origin of a plasmid drawn cut open is **split**, one box
  per span, at the two ends of the track — those are the bases a reader would
  find there — and the caption saying the circle was cut at base 1 is what makes
  two boxes under one name read as a wrap rather than as two copies.

  `pl methods map` is a new methods paragraph for the figure, with the defaults
  interpolated from `pl_draw::Options` and its limits stated: overlapping
  features overprint in one band rather than being separated into lanes, and a
  map missing three names looks exactly like a molecule with three fewer
  features, so the count printed beside the export is part of the figure.

- **The gate renders the same molecule from two processes and compares bytes.**
  "Byte-identical on every platform" is on the front of this project and nothing
  in the gate had ever compared two separate *runs* — the renderer's own
  determinism tests loop inside one process, which holds constant every single
  thing that varies between them: the allocator, `RandomState`'s per-process
  seed, the environment, the locale. Demonstrated rather than assumed: a
  `std::process::id()` term added to the linear figure's height leaves the
  in-process test green and turns the new step red. Both shapes, all four
  formats, no Python and no corpus, so it runs everywhere the gate runs.

- **File ▸ New (Ctrl+N): a molecule that never came from a file.** Every door
  into the app was a file, so bases that arrive as bases — a gBlock in an email,
  a synthesis vendor's plain sequence, 300 bp pasted into a message — had to be
  written out as a FASTA in a text editor before Polylinker would look at them.
  The dialog takes a name and a block of bases and makes a document: line breaks
  and indentation, a FASTA header line, the coordinates off a numbered sequence
  listing, lower case and U are all accepted, and it says on screen what it
  ignored rather than dropping characters quietly. Anything that is not a
  nucleotide is **refused**, with the character and the position it first appears
  at, instead of being silently removed — a molecule with a hole in it is one
  nobody can check afterwards. **Circular or linear is chosen at creation**,
  because it changes the digest, the origin-crossing features and the gel, and
  because FASTA has no field to say it in. The bases go in through the same
  content-sniffed loader every file uses, so the new document undoes, autosaves,
  gets annotated on open and prompts for a location when you save it, exactly
  like one read from disk.

- **You can take the protein out of the desktop app.** It has painted a
  six-frame amino-acid track since 0.2.0 and there was no way to get a residue
  string onto the clipboard or into a file — so the most routine downstream step
  there is, pasting a protein into BLAST or a structure predictor or a
  colleague's email, ran through retyping it off the screen. There are now three
  doors, and they share one translator with the track rather than adding a
  second: **Copy protein** beside the sequence readout (Ctrl+Shift+P) takes the
  selection's reading, **Copy protein** in the Features toolbar takes the
  selected feature's, and **Save ▸ Protein FASTA…** writes every reading the
  document has plus the selection, one record each, through the same atomic
  writer every other save uses.

  **The genetic code travels with the protein.** Polylinker offers all 27 NCBI
  tables with a per-feature `/transl_table` override, and thirteen of the 27 do
  not treat `TGA` as a stop — so a residue string on its own does not determine
  its own bases, and a protein produced under table 11 and pasted somewhere that
  assumes table 1 is a wrong answer that looks right. Every header carries
  `transl_table=`, GenBank's own spelling of the number, alongside the reading's
  location in GenBank's own notation: `location=complement(join(1976..3310,3311..3397))`
  says the strand, the bases and the fact that there is more than one piece. The
  clipboard gets a FASTA record for the same reason rather than bare letters —
  it is the only form in which the number can travel, and everything that takes
  a protein takes FASTA.

  **The awkward cases are stated rather than guessed at.** A selection whose
  length is not a multiple of three says how many bases were left over; a
  reverse-strand or multi-segment reading says so in its location and again in
  words; an internal stop codon is counted and its residues named; a partial CDS
  running off the end of a linear molecule says how many bases the annotation
  claims that the molecule does not have, which was previously clamped in
  silence and read as a merely shorter protein; and an initiator that does not
  spell M — `GTG` under table 11, `V` under table 1 — says that the letter is a
  substitution and not what the codon spells.

  Help ▸ "Open reading frames and translation" now says where all three doors
  are, which is the half of that page that had a method and no location. Its
  methods paragraph — the one written to be pasted into a paper — also states
  the residue convention for the first time: the first codon of a reading is
  written `M` wherever the code permits initiation there whatever the codon
  spells, and a termination codon is written `*`. Both `pl orfs` and the desktop
  app have always done that, through the same `translate_cds`, and neither said
  so.

- **The desktop app can now find where an oligo binds.** Paste a primer into the
  new **Primers** tab and it lists every place that oligo anneals on the open
  molecule, on both strands, including sites that cross the origin of a circular
  plasmid. Each site is drawn on the map and boxed in the sequence view, and
  clicking one selects the bases it pairs with. This is the thing a cloner does
  most often with a primer they already have, and until now `pl primers <file>
  --primer SEQ` was the only way to do it: `pl-primer` reached the desktop binary
  only *transitively*, inside `pl-design`'s off-target prefilter, so the engine
  shipped with no caller a user could reach. The app also shipped a Help page
  titled "Primer binding sites" describing a search it could not run — the same
  defect, one crate over, that feature annotation had a day earlier, and it is
  recorded as such in `bins/pl-gui/Cargo.toml` beside the dependency.

- What the panel shows, and why each part is there. The **annealed footprint is
  kept visibly apart from any 5' tail**, because a 20 nt primer with a 20 nt
  Gibson arm is a 40-mer whose annealing temperature is the 20-mer's — the
  melting temperature is computed over the footprint alone, and a tool that
  prints one string cannot say so. The **number of sites is stated before the
  list**, in a warning colour and in words, because a primer that binds twice is
  a failed PCR and a panel that leads with the best site answers a question
  nobody asked. No melting temperature is reported for a footprint carrying a
  mismatch or an ambiguity code; the row says which of the two it is and what
  that means, rather than leaving a blank cell. Annealing temperatures are
  offered per polymerase for the selected site, over that site's footprint
  length, and labelled as vendor advice rather than a measurement.

- The panel exposes the same controls as `pl primers` — `--seed`,
  `--seed-mismatch` and `--exact` — with the same defaults, and the seed bounds
  are now a pair of constants in `pl-primer` that both surfaces read, so a GUI
  that accepted a seed the CLI refuses is arranged against rather than asserted.
  `the_primers_panel_and_the_cli_agree_about_the_same_primer_and_molecule`
  compares whole binding lists against the expression `cmd_primers` evaluates,
  and `the_panel_and_pl_tm_agree_about_the_footprint_and_not_the_whole_oligo`
  does the same for the temperature against the expression `pl tm` evaluates.

### Changed

- **Three things came off the roadmap on 2026-08-06, and none of them changes a
  byte that ships.** Code signing and macOS notarisation; Bar-Ilan
  technology-transfer clearance; and the rule that v1.0 waits for a second
  maintainer holding commit and release keys. None is planned work any more —
  not deferred, not blocked on money, not waiting on an office or on a person.
  They are struck rather than deleted in `docs/PLAN.md` (§4, §9.2, §10 risks 1,
  10 and 12, §11.1, §12), because a withdrawn plan that leaves no trace is
  indistinguishable from one that was never made. This entry exists so that a
  reader comparing two versions finds a gate disappearing here, rather than by
  diffing the plan.

- **The builds are unsigned, exactly as before, and it costs you exactly what it
  cost you before.** What changed is the tense: "not done yet", "outstanding"
  and "when a certificate arrives" implied one was on its way, and none is.
  `docs/RELEASING.md`, `SECURITY.md`, `README.md`, `README-WINDOWS.txt`,
  `README-MACOS.txt`, `tools/release-notes.md` and `Install-Polylinker.ps1` now
  say that plainly. Nothing was removed from any of them, and the gate in
  `tools/ci.ps1` that reads the shipped text still passes: Windows SmartScreen
  shows *"Windows protected your PC"* on first run and what that means is still
  explained; macOS Gatekeeper still refuses a downloaded binary and
  `xattr -d com.apple.quarantine` on the named files is still the remedy given;
  the SHA-256 still proves the bytes and nothing about who produced them; the
  Ed25519 signature over `SHA256SUMS.txt` is still a *manifest* signature and
  still not code signing; and a managed or locked-down machine may still refuse
  unsigned software outright, where the answer is still to ask the
  administrator rather than work around it. `README-LINUX.txt` was not touched,
  because it never described a future.

- `PROVENANCE.md` gains a dated amendment rather than an edit: the record of
  what was decided in July 2026 stands, and the note beneath it says which half
  of it stopped being planned work. Legal advice on the trademark and Israeli
  §24 questions did not stop being owed.

### Fixed

- **The disclosure line on a linear figure counted a different figure from the
  one it was printed on.** `pl export` and the app both build the "*N of M
  cutters labelled*" line in two passes — render once to learn how many labels
  fit, render again with a line saying so — and both carried a comment claiming
  this cannot change what it counts. On the ring it cannot: the note reaches
  `centre_room` → `keep_clear` → the ruler's radius and stops. On the track it
  reached the caption, which is one of the four terms fixing how many rows of
  labels there is room for, so drawing the note stole a row. Measured on a 6 kb
  track with 40 cut sites at 720 × 180: the line said 33 enzymes named and 7
  hidden, and the figure it was printed on named 24 and hid 16.
  `debug_assert!(Disclosure::closes)` passed on both, because 24 + 16 and 33 + 7
  both reach 40 — the arithmetic closed over numbers taken from the wrong
  picture. The linear figure now reserves the note's line in that arithmetic
  whether or not there is a note, which costs at most one row on a figure whose
  height is already binding and names every label it costs.

- **"in the PDF annotation" was never true.** Two comments justified shortening a
  feature name rather than a cut coordinate by saying the whole name survives
  "in the SVG `<title>`, in the PDF annotation and in the app's Features tab".
  There is no PDF annotation: `pdf.rs`'s own module doc has always said an
  annotation "would be furniture in a figure", and the writer emits no `/Annots`
  array at all. Traced one writer at a time and written down as measured — the
  SVG carries a real `<title>`, the EPS carries the text as a comment nothing
  renders, and a PDF and a PNG carry no copy whatsoever, so on those two the
  only surviving record is `Report::labels_truncated`. The conclusion the
  comments were reaching for holds on the true premise, because a reported loss
  is still a different thing from a silent one; a test now pins where the name
  does and does not appear, in all four formats.

- **A melting temperature in a methods paragraph now always carries the
  conditions it was computed under.** `pl methods primers` (and the same page in
  the app's Help window) said the temperature "is computed from the footprint
  alone" and then named no nearest-neighbour table, no salt correction and no
  concentration at all. These paragraphs exist to be pasted into a paper — the
  Help page has a "Copy this paragraph" button — and the same 20 nt footprint
  reads 53.9 °C on this model's 50 mM Na+ scale and about five degrees higher in
  an ordinary PCR buffer, so a reader given the number without the scale could
  neither reproduce it nor compare it. The paragraph now interpolates the
  conditions, states that no temperature is reported for a mismatched footprint
  and why, and says the extension rule the `--exact` flag switches. A test sweeps
  every topic and fails any paragraph that reports a temperature without naming
  its table and its sodium.

- The Design panel's conditions line and the `/note` it writes into your file
  now read the thermodynamics the pair was actually **scored** under, instead of
  re-deriving `Constraints::default()`. Same string today, because that panel
  puts no control on the salt; the point is that the note is saved into the
  document, so it had to be true by construction rather than by coincidence.

## [0.2.0] - 2026-08-06

### Added

- **The desktop app annotates.** Opening or pasting a molecule now searches it
  against the 89-record features database that was already compiled into
  `polylinker.exe`, and lists what it found at the top of the Features tab.
  Until now the app shipped a methods page *describing* an annotation it could
  not perform: `bins/pl-gui` had no dependency on `pl-features` at all, and the
  flagship item in `docs/PLAN.md` §v1.0 was reachable only from `pl annotate` on
  the command line.

  **They are proposals, and your document does not contain them.** Nothing is
  added until you press Accept — one row, or all of them — and each accepted
  feature is one undo step carrying the same provenance note
  `pl annotate --genbank` writes, from the same function, so a `.gb` written by
  the app and one written by the command line cannot come to say different
  things about the same hit. That is `features/SIGNOFF.tsv`'s rule in the
  interface: the tool may propose and may not assert. An implementation that
  silently wrote the hits into the file on open would demo better and would be
  asserting on somebody's behalf.

  Every row shows its identity **and** its coverage, never one without the
  other — the first 300 bp of a 600 bp marker copied perfectly is 100% identity
  at 50% coverage, and "100%" alone reads as "this is that feature". Rows also
  carry whether the match was nucleotide or protein, the record's `PLF:` id, and
  whether a curator has ever checked that record; an unreviewed record is
  marked in warning ink, and accepting it writes that caveat into your file.

  Defaults match `pl annotate` exactly: reviewed rows only, partial matches
  hidden, both one click away. The scan runs on a worker thread, is thrown away
  rather than remapped whenever an edit moves bases, and never touches the
  network. "Annotate on open" ships **on** — unlike the update check, which
  ships off, there is no privacy question here, only time, and the time was
  measured rather than assumed.

- **The app says what the database has no rows for.** There is not one promoter,
  terminator or origin of replication among the 89 records — those three classes
  have no automatable source that gives a defensible boundary, which
  `features/README.md` has always been candid about and which nobody reads
  before opening a plasmid. A user who watches `AmpR` light up and sees no `ori`
  concludes their plasmid has no `ori`, and the tool caused that by having just
  demonstrated that it knows what features are. The proposals panel, the About
  page and `pl methods annotate` all say so now, each computed from the shipped
  table by one function (`Db::absent_common_kinds`) rather than written down, so
  the sentence shortens by itself the day a `promoter` row lands.

- `CHANGELOG.md` — this file.
- `CITATION.cff`, so the repository is citable by people who have to cite their
  tools. There is no DOI; see the file.

- **A security policy, and a way to report a flaw.** `SECURITY.md` did not
  exist. The project now ships an embedded signing key and code that executes on
  it, and there was no channel to report a problem in either; reports go through
  GitHub private vulnerability reporting. It is specific rather than generic:
  the highest-value report is named, file parsers are in scope because a plasmid
  map arrives by email, and the key-compromise section states plainly that
  anyone who can push to the repository, anyone who can read the Actions secret,
  and GitHub itself can sign a release every installed copy will accept. It
  gives a rotation procedure and then says the procedure is untested.

- **`CITATION.cff`**, for an audience that cites things. There is no DOI, and
  the file says so rather than inventing one.

### Fixed

- **The minimum Rust version was a guess, and the guess was wrong.**
  `Cargo.toml` declared `rust-version = "1.82"`, and `README-LINUX.txt` and the
  release notes told anyone whose glibc is too old for the Linux binaries that
  building from source needs Rust 1.82. That is advice aimed precisely at the
  people who cannot check it cheaply, and nothing had ever compiled this tree
  with 1.82: every toolchain step in both workflows is
  `dtolnay/rust-toolchain@stable`. 1.82 does not get as far as compiling —
  `indexmap 2.14.0` in `Cargo.lock` is edition 2024, which cargo 1.82 refuses
  at the manifest. The floor is **1.92**, bounded from both sides: 1.92 checks
  the whole workspace clean, and 1.91.1 is rejected by the eight egui 0.35
  crates the editor depends on. All three copies of the number now say 1.92, a
  new `msrv` job in CI installs whatever `rust-version` declares and runs
  `cargo check --workspace --locked` on it, and a gate step fails if the prose
  and the manifest disagree.

  Nothing about the published binaries changes. What changes is that the
  number a reader acts on is now compiled against on every push.

- **The 200 ms annotation budget had never been measured.**
  `docs/PLAN.md` §v1.0 item 5 has claimed "under 200 ms for a 10 kb plasmid"
  since the plan was written, with nothing computing it — the same shape of
  unchecked number as the `1.82` above, on a claim more people quote.
  `crates/pl-features/tests/budget.rs` measures it now, on two 10 kb circular
  plasmids built out of real records from the shipped table. **The budget holds:
  11 ms and 103 ms, release build**, against 106 ms and 1,075 ms debug.

  The interesting part is that the two differ by nine times, and in the
  direction nobody would pick: the plasmid with four multi-kilobase CDSs costs
  nine times the one carrying 37 short parts, because the cost is the aligner
  and the aligner is the product of the two lengths. Measuring only the busy
  plasmid — the one that looks harder — would have reported 11 ms and declared
  the budget met with eighteen times the room it actually has.

- **A GUI build spawning `curl` opened a console window on Windows.**
  `polylinker.exe` is a windows-subsystem binary with no console of its own, so
  Windows makes one for a console child; `curl` finishes in well under a second,
  and what a user saw was a black window appearing and vanishing. On a tool
  whose whole claim is that it does not touch the network unless asked, an
  unexplained terminal flashing at launch is the worst possible thing to show.
  `CREATE_NO_WINDOW` is now set on both `curl` invocations, which
  `bins/pl-gui/src/recover.rs` had already been doing for its own child process
  since long before the updater was written. Nobody had seen the flash because
  the update check ships off: the defect was real, latent, and reserved for the
  first person ever to switch the setting on.

Nothing else has landed since v0.1.3 that changes what the programs do: a
`cargo fmt` of the release-signature test, and a recount of the line count the
README asserts about itself.

- **The MIT half of "MIT OR Apache-2.0" did not exist.** `Cargo.toml`, the
  release notes and the npm package all offered a dual licence; only the Apache
  text was committed, and `packages/circular-map/package.json` listed a
  `LICENSE-MIT` in its `files` array that was not there. Anyone who chose the
  MIT half was offered a licence they could not read. Both texts now ship in
  every archive and the MSI, and `tools/check-archive.ps1` requires them **by
  name** — this project has lost licence texts from a packaging step twice, and
  a count cannot tell which one went missing.

- **The README claimed a relationship with REBASE that does not exist.** It said
  restriction-enzyme data "is REBASE, redistributed under its own terms". It is
  not: `NOTICE` says REBASE data *will be* sourced into a separate repository,
  and what ships is 58 enzymes transcribed from published references,
  "not a reproduction of any database". Claiming to redistribute a database the
  project has not licensed is the wrong direction to be wrong in.

- **`Cargo.toml` pointed at a GitHub organisation that does not exist**
  (`polylinker/polylinker`). The identical mistake shipped in the updater in
  0.1.2 and made `pl update` fail with a 404.

- **`CONTRIBUTING.md` described a different project** — "there is no build. The
  reference implementation is Python with no dependencies beyond the standard
  library" — against 21 workspace crates, three-OS CI and a 65-step gate.

## [0.1.3] - 2026-08-06

### Fixed

- **`pl update` reaches a repository that exists.** The compiled-in
  `RELEASE_BASE_URL` was `https://github.com/polylinker/polylinker`, an
  organisation nobody registered, so `pl update --check` returned 404 the first
  time it was pointed at the real internet. Every unit test passed throughout:
  they assert that a URL is *built* correctly from the constant, and none of
  them asserted that the constant was right.

  **If you are running 0.1.2, its updater cannot work and cannot tell you this
  release exists.** Download 0.1.3 from the releases page by hand.

### Added

- The signature CI actually produced is now a committed test fixture, together
  with the manifest it covers: the real `SHA256SUMS.txt` from the v0.1.2 release
  page and the real 64 signature bytes `openssl` produced on the runner from
  `POLYLINKER_RELEASE_KEY`. Everything else about signing was tested against
  keys the tests invent, which proves self-consistency — exactly what a pipeline
  signing with the wrong key would also have proved. This is the first test in
  which the private half and the compiled-in public half meet.
- Negative controls for it, because a test that only checks a valid signature
  passes against a verifier that returns `true`: a flipped bit in the manifest,
  a flipped bit in the signature, and a public key one bit away from the release
  key are each required to be refused.
- `.gitattributes` pins those two fixtures to LF. Their bytes are the message
  the signature was made over, so a CRLF checkout would change what was signed
  and fail the test on Windows alone — announcing that the release key does not
  match the compiled-in key, which would be false and is the most alarming thing
  this repository could say.

## [0.1.2] - 2026-08-06

The first signed release. Manifest signing and the updater's verification of it
had never met: nothing on a development machine can sign with the CI secret, and
the publish job only runs on a tag. This is the tag that tested it.

**Broken in this release:** `pl update` points at a GitHub organisation that
does not exist and returns 404 for every check. Fixed in 0.1.3, which 0.1.2
cannot tell you about. Everything else below works.

### Added

- **An Ed25519 signature over the release manifest.** Every release page now
  carries `SHA256SUMS.txt.sig` beside `SHA256SUMS.txt`, and prints the OpenSSL
  command to check it by hand. A checksum proves your download matches the
  release page; the signature proves the release page came from whoever holds
  the release key. The private half is a GitHub Actions secret,
  `POLYLINKER_RELEASE_KEY`, and is on no machine here.
- **The public key, compiled into `pl` and `polylinker`.** Only those two:
  `pl-mcp`, the Python extension module and the wasm build do not carry it,
  because none of them can update anything. The trust anchor is in the binary
  being replaced rather than fetched from the network, which is the whole point.
  Rotating that key needs every installed copy to be replaced by hand — there is
  **no revocation channel**, because a revocation channel is a network call.
  [`docs/RELEASING.md`](docs/RELEASING.md) records that cost.
- **`crates/pl-update`, an opt-in updater**, meeting the four conditions
  `docs/RELEASING.md` had set for one before it was allowed to exist. It fetches
  `SHA256SUMS.txt` and `SHA256SUMS.txt.sig` into memory and verifies the
  signature *before it requests the platform artifact at all*; a failed
  signature means the artifact is never asked for and nothing is written. The
  download lands on a `.part` file and is renamed into place only if its
  SHA-256 matches the entry in the verified manifest. It then prints the path
  and stops: it replaces nothing, refuses to write into the directory it is
  running from, and running the new file is yours to do.
- **`pl update` and `pl update --check`.** One request, made because somebody
  typed the verb. No thread, no timer, no stored "last checked".
- **An update check in the desktop app, under Help, shipped off.** Turned on it
  asks once per launch and shows a notice pointing at the release page; it never
  downloads. A new installation contacts nothing, and a truncated or hand-edited
  settings file falls back to off rather than on.
- **SHA-512 and Ed25519 verification in `pl-core`**, hand-written and with no
  dependency, checked against Wycheproof vectors and adversarially tested.

### Changed

- The release notes and `docs/RELEASING.md` no longer say "no updater". They say
  no *auto*-updater, and describe the two opt-in paths. The distinction is the
  point: nothing runs on a timer and nothing installs anything.

## [0.1.1] - 2026-08-05

### Added

- **A Windows MSI installer**, `polylinker-0.1.1-windows-x64.msi`. It installs
  for you alone by default — no administrator, no elevation prompt — with "for
  everyone" offered for machines where you are one. It puts Polylinker in the
  Start Menu and in Settings → Apps and offers to put `pl` on your PATH. It
  **adds** Polylinker to the "Open with" list for eight extensions — `.dna`,
  `.gb`, `.gbk`, `.genbank`, `.fasta`, `.fa`, `.fna` and `.ab1` — and takes none
  of them away: it writes `OpenWithProgids` entries and never an extension's own
  default, so if SnapGene owns `.dna` on your machine it still does afterwards.
  The installer contacts nothing and registers no service, no scheduled task and
  no auto-updater — nothing it puts on the machine ever runs on its own, and
  `tools/ci.ps1` fails the build if any network or scheduling facility appears in
  the installer sources.
- The MSI's file list is generated from the same `SHA256SUMS.txt` the zip is
  verified against, rather than written out a second time, because a second list
  is how a licence text stops shipping.
- Install, verify and uninstall are exercised on a CI runner: every payload file
  under `LocalAppData`, the installed `pl.exe` reporting its own version, the
  Start Menu shortcut, the `.dna` handler registration, and a planted foreign
  default handler surviving both install and uninstall.

The portable zip is unchanged and still ships `Install-Polylinker.ps1` for
anyone who would rather run something they can read.

## [0.1.0] - 2026-08-05

First public release.

### Added

- **Three platforms**, each built on the operating system it runs on:
  `polylinker-0.1.0-windows-x64.zip`, `polylinker-0.1.0-macos-universal.tar.gz`
  (one binary for Apple Silicon and Intel) and
  `polylinker-0.1.0-linux-x64.tar.gz` (glibc 2.39 or newer).
- Each archive contains `polylinker` (the desktop editor), `pl` (the command
  line), `pl-mcp` (a read-only MCP server), the Python extension module, the
  licence texts that have to accompany every copy — `LICENSE.txt`, `NOTICE.txt`,
  `TRADEMARKS.md`, `features/NOTICE.txt` and seven font licences under
  `licences/` — a per-platform read-me, and a `SHA256SUMS.txt` covering all of
  them. The release page carries a second `SHA256SUMS.txt` over the three
  archives themselves. The Windows zip adds `Install-Polylinker.ps1`,
  `Install.cmd`, `README-WINDOWS.txt` and the icon.
- `tools/check-archive.ps1` asserts the required members of each archive **by
  name and per platform**, not by count: an empty archive agrees perfectly with
  an empty manifest, and a count cannot tell a missing binary from a missing
  licence.

### Known limitations at this release

- **Unsigned, on every platform.** No code-signing certificate and no Apple
  Developer ID. macOS Gatekeeper refuses the files until
  `com.apple.quarantine` is removed from them by hand; Windows SmartScreen warns
  on first run. Neither is an oversight, and the words for clicking past a
  security warning appear in nothing this project ships.
- **No updater of any kind.** `pl --version` printed the version and the commit
  and asked nobody anything. One was added in 0.1.2.
- **No manifest signature.** `SHA256SUMS.txt` shipped unsigned, so the release
  page proved integrity and not origin. Added in 0.1.2.

[Unreleased]: https://github.com/liorlobel/polylinker/compare/v0.10.3...HEAD
[0.10.3]: https://github.com/liorlobel/polylinker/releases/tag/v0.10.3
[0.10.2]: https://github.com/liorlobel/polylinker/releases/tag/v0.10.2
[0.10.1]: https://github.com/liorlobel/polylinker/releases/tag/v0.10.1
[0.10.0]: https://github.com/liorlobel/polylinker/releases/tag/v0.10.0
[0.9.1]: https://github.com/liorlobel/polylinker/releases/tag/v0.9.1
[0.9.0]: https://github.com/liorlobel/polylinker/releases/tag/v0.9.0
[0.8.1]: https://github.com/liorlobel/polylinker/releases/tag/v0.8.1
[0.8.0]: https://github.com/liorlobel/polylinker/releases/tag/v0.8.0
[0.7.0]: https://github.com/liorlobel/polylinker/releases/tag/v0.7.0
[0.6.0]: https://github.com/liorlobel/polylinker/releases/tag/v0.6.0
[0.5.0]: https://github.com/liorlobel/polylinker/releases/tag/v0.5.0
[0.4.0]: https://github.com/liorlobel/polylinker/releases/tag/v0.4.0
[0.3.2]: https://github.com/liorlobel/polylinker/releases/tag/v0.3.2
[0.3.1]: https://github.com/liorlobel/polylinker/releases/tag/v0.3.1
[0.3.0]: https://github.com/liorlobel/polylinker/releases/tag/v0.3.0
[0.2.0]: https://github.com/liorlobel/polylinker/releases/tag/v0.2.0
[0.1.3]: https://github.com/liorlobel/polylinker/releases/tag/v0.1.3
[0.1.2]: https://github.com/liorlobel/polylinker/releases/tag/v0.1.2
[0.1.1]: https://github.com/liorlobel/polylinker/releases/tag/v0.1.1
[0.1.0]: https://github.com/liorlobel/polylinker/releases/tag/v0.1.0
