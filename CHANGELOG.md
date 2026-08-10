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

### Added — 26 proposed feature records, and none of them ship yet

`features/features.tsv` goes from 89 rows to 115. **The 89 rows the tool
searches by default are unchanged, byte for byte; every one of their signatures
is still valid.** The 26 new rows are `proposed`, which means a program put them
there and no human has read them, so `pl annotate` ignores them unless you pass
`--include-proposed` and the desktop app ignores them unless you tick the box.
This is what "the tool may propose and never assert" looks like when it is
actually exercised, rather than when the table happens to be fully signed.

**14 further selection markers** (`PLF:1014`–`PLF:1027`), each verified by the
same chain as the existing natural-protein rows — translate the nucleotides,
require an exact residue-for-residue match to a UniProt canonical, cite the
depositor's own coordinates — and each dropped rather than corrected if any leg
disagreed: `pac`, `bsd`, `bsr`, `dhfrI`, `URA3`, `LEU2`, `HIS3`, `TRP1`, HSV
`TK`, mouse `Dhfr`, `gpt`, `bar`, `pat` and `rpsL`. These close the eukaryotic
selection-marker gap `features/SOURCING.md` names as Gap 6, and they give the
database its first yeast markers.

**12 Class B regulatory elements** (`PLF:4000`–`PLF:4011`) — the T7, SP6, lac,
tac, trc and CMV promoters, the CMV enhancer, the T7, rrnB T1 and rrnB T2
terminators, and the bGH and SV40 early poly(A) signals. These are the first
promoters and terminators of any kind in the database. A Class B boundary is a
*convention* and not a fact, so each row ships a coordinate slice of one named
INSDC record, and the claim that at least two further records **from different
submitting addresses** hold those exact bases is re-checked on every build, with
each depositor's own edges measured against ours and written into the row.

Three things that came out of building it and are documented rather than
smoothed over:

- **INSDC records carry SnapGene annotation, and the CI taint gate cannot see
  it.** ENA folds SnapGene's `/label` into the `/note`, so its editorial prose
  arrives through a source this project cleared. The gate compares descriptions
  and can never notice a *coordinate* arriving that way. The new stage therefore
  reads no `/note`, `/label`, `/gene`, `/product` or `/standard_name` at all,
  and refuses to count a SnapGene-annotated deposit as an independent witness.
  Five of the twelve rows have a witness excluded on those grounds.
- **The taint gate fired for real, for the second time in this project's
  history**, on the blasticidin deaminase description, whose first draft shared
  a five-token run with their file. Nothing was copied; the row was rewritten
  anyway, because the rule is mechanical on purpose.
- **Nine more elements were worked up and are not here**, each with its reason
  recorded in `features/build/stage_classb.py`: T3, the SV40 early promoter, U6,
  H1, EF-1α, PGK, CAG and araBAD are held, and tetO/TRE is dropped outright
  because the name covers four unrelated elements. `SOURCING.md` budgets about
  forty Class B rows; twelve is what survived the two-independent-submissions
  rule applied honestly, and that is the finding rather than a shortfall.

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

[Unreleased]: https://github.com/liorlobel/polylinker/compare/v0.5.0...HEAD
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
