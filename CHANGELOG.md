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

Nothing yet.

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

[Unreleased]: https://github.com/liorlobel/polylinker/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/liorlobel/polylinker/releases/tag/v0.3.0
[0.2.0]: https://github.com/liorlobel/polylinker/releases/tag/v0.2.0
[0.1.3]: https://github.com/liorlobel/polylinker/releases/tag/v0.1.3
[0.1.2]: https://github.com/liorlobel/polylinker/releases/tag/v0.1.2
[0.1.1]: https://github.com/liorlobel/polylinker/releases/tag/v0.1.1
[0.1.0]: https://github.com/liorlobel/polylinker/releases/tag/v0.1.0
