//! `pl` — inspect, convert and digest sequence files.
//!
//! Everything is local. No network, no account, no telemetry.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

// `load` is deliberately absent: it drops the `LoadReport` by construction, and
// every verb here that used it answered about record 1 of a multi-record file
// with nothing to say so. Import `load_with_report` and report, or refuse.
use pl_fileio::{detect, fasta, genbank, load_with_report, snapgene, Format};

const USAGE: &str = "\
pl -- Polylinker command line

USAGE:
    pl info    <file>...                 summarise each file
    pl convert <file>... [options]       convert to GenBank or FASTA
    pl digest  <file> [--enzyme NAME]    restriction sites
    pl blocks  <file.dna>                anatomy of a SnapGene container
    pl checksum <file>...                SEGUID v2 checksums
    pl export  <file>... [options]       plasmid map as SVG or PDF
    pl find-motif <IUPAC> <file>         search a sequence, both strands
    pl tm      <OLIGO>...                melting temperature
    pl goldengate <OVERHANG>...          check a Type IIS overhang set
    pl primers <file> --primer SEQ       where primers anneal
    pl design  <file> --region A..B      pick a PCR primer pair for a region
    pl trace   <file.ab1>... [--svg F]   read or draw a Sanger chromatogram
    pl orfs    <file> [--table N]        open reading frames, six frames
    pl sanger  <read>... --ref <file>    did the clone work?
    pl annotate <file>...                find known features in a plasmid
    pl gel     <file> --cut A --cut B    will the digest be readable?
    pl methods [topic]                   what to write in your methods section
    pl licences                          who the annotation data belongs to

    pl index   <dir>... [options]        build or refresh a folder's index
    pl find    <dir> [filters]           search it (--motif/--enzyme/--text/...)
    pl library <dir> [options]           what is indexed, and what could not be

CONVERT OPTIONS:
    --to <genbank|fasta|dna>     output format (default: genbank)
    -o, --outdir <dir>           where to write (default: beside the input)
    --stdout                     write to stdout instead of files

INDEX OPTIONS:
    --verify                     re-read every file and check its stored hash
    --rebuild                    ignore any existing index
    --index-at <dir>             keep the index here instead of the OS cache
    --follow-links               follow symbolic links (off by default)
    --max-depth <n>              default 32

GEL OPTIONS:
    --cut <ENZYME>               add to the digest; repeat for a double digest
    --lane <A+B>                 a whole lane, enzymes joined by '+'
    --agarose <percent>          default 1.0
    --ladder <1kb|100bp|1kb-plus>  default 1kb
    --band-mm <mm>               how wide a band is (default 1.5) — this is
                                 what decides whether two fragments resolve
    --run-mm <mm>                how far the dye front ran (default 80)
    --svg <file>                 draw it

ANNOTATE OPTIONS:
    --include-proposed           search rows no human has signed off on
    --min-identity <0..1>        default 0.96
    --min-coverage <0..1>        default 0.30
    --code <transl_table>        genetic code, default 11 (bacterial). Decides
                                 which codons may open a reading frame, and so
                                 whether a peptide tag counts as fused to one:
                                 table 1 does not accept GTG, which five of the
                                 shipped markers begin with
    --no-protein                 skip six-frame protein matching
    --fragments                  list partial hits too
    --genbank                    write an annotated GenBank record to stdout
    --db                         describe the shipped database and exit

TRACE OPTIONS:
    --svg <file>                 draw the chromatogram
    --bases <FIRST..LAST>        which called bases to draw (default all)
    --width <points>             drawing width (default 1200)
    --accessible                 Okabe-Ito colours, not the red/green classic

SANGER OPTIONS:
    --ref <file>                 the reference to compare against
    --ref-seq <ACGT>             a reference sequence instead of a file
    --read <ACGT>                a read instead of a trace file
    --min-quality <n>            Phred at or above which to believe a
                                 difference (default 20)
    --all                        list low-confidence differences too

ORFS OPTIONS:
    --table <n>                  NCBI genetic code (default 11, bacterial)
    --min-aa <n>                 shortest ORF to report (default 30)
    --any-start                  stop-to-stop runs, not just real start codons
    --complete-only              drop frames that run off the end
    --seq <ACGT>                 a sequence instead of a file
    --circular                   with --seq, treat it as a circle
    --translate                  print the protein
    --tables                     list every genetic code and exit

FIND OPTIONS:
    --motif <IUPAC>              both strands, origin-aware
    --enzyme <NAME>              the site of a shipped enzyme
    --name <S>                   substring of the path or molecule name
    --text <S>                   substring of features, primers and notes
    --absent                     invert the sequence criteria only
    --topology <circular|linear|undeclared>
    --state <ok|no-bases|annotation-track|...>
    --length <MIN..MAX>          bases
    --features <MIN..MAX>
    --limit <N>                  default 200
    --no-index                   answer from the files, ignoring the index

LIBRARY OPTIONS:
    --problems                   every record that could not be fully read

DESIGN OPTIONS:
    --region <A..B>              the target, 1-based inclusive. Required.
                                 On a circle A > B means the region crosses the
                                 origin, which is ordinary on a plasmid.
    --seq <BASES>                template, instead of a file
    --circular                   with --seq
    --mode <contain|within>      contain: the product contains all of A..B and a
                                 primer may begin outside it (cloning an ORF).
                                 within: both primers lie inside A..B (a qPCR
                                 amplicon inside a gene). Default contain.
    --flank <N>                  how far outside A..B a primer's OUTER end may
                                 sit, contain only. Default 200. --flank 0 pins
                                 the two outer ends exactly to A and B, which is
                                 what seamless cloning wants -- and is also the
                                 setting most likely to return nothing, because
                                 it leaves only one 5' end and 10 lengths per
                                 side. Measured on 35 plasmids: 12 designed, 23
                                 refused. Raise it a few bases before relaxing
                                 anything physical.
    --rt                         RT-PCR preset. Prints the bacteria caveat: this
                                 CANNOT exclude genomic DNA, because bacterial
                                 genes have no introns to span.
    --len <MIN..MAX>             primer length, default 18..27
    --len-opt <N>                default 20
    --tm <MIN..MAX>              footprint Tm, default 52..58 -- ON THIS MODEL'S
                                 50 mM Na+ SCALE, where an ordinary PCR buffer
                                 sits about 5 C higher. Pass --na 150 to design
                                 on the bench scale instead.
    --tm-opt <C>                 default 55
    --tm-diff <C>                max |Tm_f - Tm_r|, default 5
    --gc <MIN..MAX>              default 40..60, REPORTED and not a gate
    --gc-hard                    make it a gate. Measured, on a 22% GC template
                                 it cuts the forward survivors from 17 to 1 --
                                 one candidate from an empty result, on a
                                 criterion the Tm window already covers. That
                                 is why it is off.
    --gc-clamp <MIN..MAX>        G or C among the LAST 5 bases, default 1..3
    --max-poly <N>               longest run of one base, default 4 (G: 3)
    --product <MIN..MAX>         amplicon length, default 100..3000. The
                                 AMPLICON's, so any 5' tails count toward it --
                                 that is the molecule that runs on the gel.
    --product-opt <N>            penalise distance from this, on a log scale
    --max <N>                    pairs to report, default 5
    --add-5 <ENZYME>             add this enzyme's site as a 5' tail, forward
    --add-3 <ENZYME>             ... reverse
    --spacer <BASES>             bases 5' of the added site. None by default; a
                                 warning says why you may want some. Needs
                                 --add-5 or --add-3: with no added site there is
                                 nothing for a spacer to sit 5' of
    --vector <file>              count the added enzymes' sites in a vector too,
                                 with Dam/Dcm methylation applied: a blocked
                                 site is not a usable single cutter. Needs
                                 --add-5 or --add-3 -- those are the sites that
                                 get counted -- and the file must hold one record
    --vector-circular            read the vector as a plasmid. FASTA carries no
                                 topology, and a site straddling the origin of a
                                 vector read as linear is reported as absent
    --dam- / --dcm-              the vector prep is dam- / dcm-. Both are
                                 assumed PRESENT, which is an ordinary lab strain
    --cpg                        the vector is CpG methylated (a mammalian cell
                                 line, or M.SssI). Off by default
                                 (--vector-circular, --dam-, --dcm- and --cpg all
                                 describe a vector, so each needs --vector)
    --off-seed <N>               3'-anchored seed for the off-target scan, 8-32,
                                 default 12. Raising it makes the scan faster
                                 and BLINDER: 12 is pl-clone's own MIN_ANNEAL,
                                 so above it this tool would offer pairs its own
                                 simulator refuses as not specific
    --no-specificity             skip the off-target scan, and say so
    --table <1998|2004>          } spelled exactly as `pl tm` spells them
    --na <mM>                    }
    --oligo <nM>                 }
    --salt <santalucia|schildkraut|none>

PRIMERS OPTIONS:
    --primer <SEQ>               a primer, repeatable
    --seq <BASES>                template, instead of a file
    --circular                   with --seq
    --seed <N>                   3'-anchored seed length, 8-35 (default 14)
    --seed-mismatch              allow one, never at the 3' end
    --exact                      footprint stops at the first mismatch
                                 (pydna and SnapGene behave this way)

GOLDENGATE OPTIONS:
    --enzyme <NAME>              digest a file with a Type IIS enzyme instead

TM OPTIONS:
    --table <1998|2004>          SantaLucia parameter set (default: 1998)
    --na <mM>                    monovalent cation (default: 50)
    --oligo <nM>                 strand concentration (default: 50)
    --salt <santalucia|schildkraut|none>

FIND-MOTIF OPTIONS:
    --seq <BASES>                search this literal sequence instead of a file
    --topology <circular|linear> overrides the file's own topology; with --seq,
                                 which carries none, the default is linear

EXPORT OPTIONS:
    --width <px>                 canvas width  (default: 720)
    --height <px>                canvas height (default: 720)
    --sites <unique|dual|all|none>
                                 which restriction sites to label (default:
                                 unique -- the same rule the desktop map
                                 applies, so the two agree)
    --pdf                        write PDF instead of SVG
    --eps                        write EPS instead of SVG
    --mm <width>                 final printed width in millimetres
    --journal <name>             column width and type floor from a preset
    --column <single|double>     which of the preset's two column widths
                                 (default single). Needs --journal, since the
                                 widths come from the preset, and cannot be
                                 combined with --mm, which sets the width itself
    --check-contrast             measure every colour against WCAG 2.2 AA
    --no-ruler                   omit the base-position ruler
    -o, --outdir <dir>           where to write (default: beside the input)
    --stdout                     write to stdout instead of files

DIGEST OPTIONS:
    --enzyme <NAME>              restrict to one enzyme (repeatable)
    --unique                     only enzymes that cut exactly once
    --non-cutters                only enzymes that do not cut

GLOBAL:
    --json                       machine-readable output (info, digest)

Formats are detected from content, never from the file extension.
";

/// Escape a string for JSON. Hand-written to keep the dependency list readable.
fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "-h" || args[0] == "--help" {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let (cmd, rest) = args.split_first().unwrap();
    // `--help` after the verb, not just before it. Every verb now rejects an
    // option it does not know (see `parse_args`), and `pl convert --help` is a
    // habit, not a typo -- answering it with "unknown option" would be a worse
    // reply than the silent shrug it replaced.
    if rest.iter().any(|a| a == "-h" || a == "--help") {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let result = match cmd.as_str() {
        "info" => cmd_info(rest),
        "convert" => cmd_convert(rest),
        "digest" => cmd_digest(rest),
        "blocks" => cmd_blocks(rest),
        "checksum" => cmd_checksum(rest),
        "export" => cmd_export(rest),
        "find-motif" => cmd_find_motif(rest),
        "tm" => cmd_tm(rest),
        "goldengate" => cmd_goldengate(rest),
        "primers" => cmd_primers(rest),
        "design" => cmd_design(rest),
        "orfs" => cmd_orfs(rest),
        "sanger" => cmd_sanger(rest),
        "annotate" => cmd_annotate(rest),
        "gel" => cmd_gel(rest),
        "methods" => cmd_methods(rest),
        "licences" | "licenses" => cmd_licences(rest),
        "trace" => cmd_trace(rest),
        "index" => cmd_index(rest),
        "find" => cmd_find(rest),
        "library" => cmd_library(rest),
        "bench-adapter" => cmd_bench_adapter(rest),
        "cut-adapter" => cmd_cut_adapter(rest),
        "-V" | "--version" => {
            // The commit, not just the version. There is no auto-updater by
            // design (docs/RELEASING.md), so this line is the whole mechanism
            // by which anyone establishes which build they are running — and
            // every build between two releases says 0.1.0. `build.rs` stamps
            // it, and marks it `-dirty` when the tree it was built from had
            // uncommitted changes, because then the commit does not describe
            // this binary.
            println!("pl {} ({})", env!("CARGO_PKG_VERSION"), env!("PL_COMMIT"));
            Ok(())
        }
        other => Err(format!("unknown command '{other}'\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("pl: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Split arguments into positional files and flags.
struct Args {
    files: Vec<PathBuf>,
    flags: Vec<(String, Option<String>)>,
}

/// Split arguments into positional files and flags.
///
/// `valued` names the options that consume the next argv token, `boolean` the
/// rest; together they are the verb's whole vocabulary, and anything else is a
/// hard error.
///
/// It used to accept any `-`/`--` token with no allowed-name list, and no verb
/// ever looked at `Args.flags` for names it did not recognise, so a mistyped
/// option was dropped and the command answered with its defaults. `pl orfs
/// plasmid.gb --min-a 50` made `min-a` a flag nobody reads and pushed `"50"`
/// onto `files`, where `a.files.first()` discarded it: `min_aa` stayed at the
/// default 30 and every 30-49 aa ORF the user had just asked to exclude was
/// printed, exit 0. `pl digest x.gb --uniqe` listed every cut site instead of
/// the unique cutters, leaving no stray positional to notice. A typo that
/// changes the answer must not be indistinguishable from the answer.
fn parse_args(args: &[String], valued: &[&str], boolean: &[&str]) -> Result<Args, String> {
    let mut files = Vec::new();
    let mut flags = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if let Some(name) = a.strip_prefix("--").or_else(|| a.strip_prefix('-')) {
            let (name, inline) = match name.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (name, None),
            };
            if !valued.contains(&name) && !boolean.contains(&name) {
                let mut known: Vec<String> = valued
                    .iter()
                    .chain(boolean.iter())
                    .map(|n| format!("--{n}"))
                    .collect();
                known.sort();
                return Err(format!(
                    "unknown option '{a}'; this command takes {}",
                    if known.is_empty() {
                        "no options".to_string()
                    } else {
                        known.join(", ")
                    }
                ));
            }
            let value = if inline.is_some() {
                inline
            } else if valued.contains(&name) {
                i += 1;
                Some(
                    args.get(i)
                        .cloned()
                        .ok_or_else(|| format!("--{name} needs a value"))?,
                )
            } else {
                None
            };
            flags.push((name.to_string(), value));
        } else {
            files.push(PathBuf::from(a));
        }
        i += 1;
    }
    Ok(Args { files, flags })
}

impl Args {
    fn has(&self, name: &str) -> bool {
        self.flags.iter().any(|(k, _)| k == name)
    }
    fn get(&self, name: &str) -> Option<&str> {
        self.flags
            .iter()
            .find(|(k, _)| k == name)
            .and_then(|(_, v)| v.as_deref())
    }
    fn get_all(&self, name: &str) -> Vec<&str> {
        self.flags
            .iter()
            .filter(|(k, _)| k == name)
            .filter_map(|(_, v)| v.as_deref())
            .collect()
    }
    fn require_files(&self) -> Result<(), String> {
        if self.files.is_empty() {
            Err("no input files".into())
        } else {
            Ok(())
        }
    }
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Do these two paths name the same file on disk?
///
/// Compared after canonicalisation, so `./x.gb` and `x.gb` are recognised as
/// one file. `canonicalize` requires the path to exist, which is true of the
/// input and false of a destination that has not been written yet — so the
/// destination falls back to canonicalising its parent directory and comparing
/// the file name.
/// Pick a destination that does not overwrite an input or an earlier output.
///
/// `pl convert` and `pl export` have always de-collided their outputs and
/// refused to write over an input. `pl trace --svg` and `pl gel --svg` grew the
/// same multi-input handling without either guard, so two inputs sharing a file
/// stem produced two "-> X.svg" success lines and one file: the first
/// molecule's picture was overwritten and the CLI reported success for both.
/// Reproduced with two distinct fixtures before this existed.
///
/// The suffix scheme matches `cmd_convert`, so the behaviour is one thing to
/// learn rather than three.
fn claim_output(
    desired: PathBuf,
    input: &Path,
    claimed: &mut Vec<PathBuf>,
    renamed: &mut usize,
) -> Result<PathBuf, String> {
    if same_file(input, &desired) {
        return Err(format!(
            "{}: writing here would overwrite the input file. Choose another --svg path.",
            desired.display()
        ));
    }
    if !claimed.contains(&desired) {
        claimed.push(desired.clone());
        return Ok(desired);
    }
    let stem = desired
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "out".into());
    let ext = desired
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_else(|| "svg".into());
    let mut n = 2;
    loop {
        let candidate = desired.with_file_name(format!("{stem}-{n}.{ext}"));
        if !claimed.contains(&candidate) && !same_file(input, &candidate) {
            *renamed += 1;
            claimed.push(candidate.clone());
            return Ok(candidate);
        }
        n += 1;
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    let real = |p: &Path| -> Option<PathBuf> {
        match std::fs::canonicalize(p) {
            Ok(c) => Some(c),
            Err(_) => {
                let dir = std::fs::canonicalize(p.parent().filter(|d| !d.as_os_str().is_empty())?)
                    .ok()?;
                Some(dir.join(p.file_name()?))
            }
        }
    };
    match (real(a), real(b)) {
        (Some(x), Some(y)) => x == y,
        // If either side cannot be resolved, fall back to the literal
        // comparison already made above rather than guessing.
        _ => false,
    }
}

/// Say so when a file held more records than the verb looked at.
///
/// `load` returns record 1 and every verb below then analyses it as though it
/// were the file. Nine of them bound the `LoadReport` to `_`, so `pl digest
/// multi.gbk --json` emitted `{"file":…, "bp":…, "circular":…, "digests":[…]}`
/// and the text path asserted "N unique cutter(s)" — statements true of one
/// record, presented as facts about the file, with no record count and no
/// warning. 8 of 303 GenBank files and 351 FASTA files in this project's corpus
/// hold more than one record, and `pl-fileio`'s own worked example is a
/// 124-record `.gbk`. `convert` and `export` refuse outright; these verbs
/// answer about record 1, which is defensible only if they say so.
///
/// stderr, not stdout, so a `--json` consumer still gets one parseable document
/// and the warning is not swallowed by a redirect of the answer.
fn note_first_record_only(label: &str, report: &pl_fileio::LoadReport, what: &str) {
    if report.truncated() {
        eprintln!(
            "pl: {label}: {} records in this file; only the first was {what}",
            report.records
        );
    }
}

/// Say when the molecule contradicts itself, before answering questions about it.
///
/// `Molecule::validate()` was reached from exactly one place in the whole
/// workspace — `pl-gui`'s document loader — so every check it performs was
/// invisible to anyone using the terminal. The reachable case is dull and
/// common: a GenBank record whose `//` terminator is missing and which has
/// something after the sequence. The ORIGIN loop reads on to end of file, so a
/// 12 bp record with a mail-client footer loads as
/// `acgtacgtacgt--SentfrommyiPhone`, 30 bases, against a LOCUS line that says
/// 12. Nothing was suspect, no location was unrepresentable, and `pl info`
/// printed `30 bp` beside `declared_bp 12` without ever remarking that a file
/// disagreeing with itself is a file to look at.
///
/// A notice, not a refusal: the bases may well be the ones the user wants, and
/// `pl convert` writing a 30 bp record is a defensible thing to do as long as
/// it says the header claimed 12. stderr for the same reason as
/// `note_first_record_only` — a `--json` consumer keeps one parseable document.
fn note_self_contradiction(label: &str, mol: &pl_core::Molecule) {
    for problem in mol.validate() {
        eprintln!("pl: {label}: {problem}");
    }
}

/// Refuse to answer about a molecule that carries no bases.
///
/// `pl digest` has always done this ("no bases to digest") and `pl design`
/// refuses both no-bases classes in `pl-design`'s own words. Six other verbs
/// answered instead. On `anno.gb` — a GenBank record whose LOCUS line declares
/// 4000 bp and whose ORIGIN is empty, which `pl info` correctly describes as
/// "4000 bp DECLARED, but this file carries no bases" — `pl gel --cut EcoRI`
/// printed "none of these enzymes cuts this molecule", `pl orfs` "no ORF of 30
/// aa or more", `pl primers` "no binding site", `pl annotate` "nothing found",
/// `pl find-motif` "no hits" and `pl goldengate --enzyme BsaI` "no structural
/// fault found": six negative verdicts computed from zero bases, every one at
/// exit 0, and every one printing `0 bp` for a record that declares 4000.
/// `pl find` over the same file already says "0 of 1 records searched — 1 have
/// no sequence (a declared length, no bases)"; these verbs are the
/// single-molecule form of that question and have to be as honest.
///
/// The predicate is `seq.is_empty()`, the one `cmd_digest` uses — **not**
/// `is_annotation_track()`, which requires `declared_len` to be 0 and is
/// therefore false for exactly the commonest case, a GenBank record that
/// declares a length and ships none of it.
fn refuse_without_bases(label: &str, mol: &pl_core::Molecule, what: &str) -> Result<(), String> {
    if !mol.seq.is_empty() {
        return Ok(());
    }
    let declared = mol.declared_len.unwrap_or(0);
    if declared > 0 {
        return Err(format!(
            "{label}: this file declares {declared} bases and carries none of them -- \
             annotation-only GenBank. There is nothing here to {what}."
        ));
    }
    if !mol.features.is_empty() {
        return Err(format!(
            "{label}: this is an annotation track: it carries {} feature{} and no bases, so \
             there is nothing here to {what}. Open the sequence these coordinates describe.",
            mol.features.len(),
            if mol.features.len() == 1 { "" } else { "s" }
        ));
    }
    Err(format!(
        "{label}: this record carries no bases, so there is nothing here to {what}."
    ))
}

/// Say what the record just written could not carry, from both directions.
///
/// Four channels, and `pl convert` learned each of them separately: what the
/// *writer* could not carry (the `_reporting` variants fill a `Vec<String>` that
/// the plain wrappers throw away), what the *reader* could not build out of the
/// source's notes block or its exotic locations, and which features have no
/// GenBank-expressible strand and go out as forward. `pl annotate --genbank`
/// writes the same record with the same writer to the same stream and said none
/// of the four: a `misc_feature join(1..10,J00194.1:200..300)` came out as
/// `misc_feature 1..10` — 10 bp where the source claimed 111 — and a
/// `gap(unk100)` feature vanished outright, with empty stderr and exit 0.
/// Factored here so a fourth writer cannot forget them one at a time.
///
/// `written` is the molecule actually encoded, not the one loaded: `pl annotate`
/// adds features before writing, and the orientation question is about what
/// went into the file.
fn note_output_losses(
    label: &str,
    format_name: &str,
    unwritable: &[String],
    report: &pl_fileio::LoadReport,
    written: &pl_core::Molecule,
    is_genbank: bool,
) {
    if !unwritable.is_empty() {
        eprintln!(
            "pl: {label}: {} item(s) the {format_name} writer could not carry: {}",
            unwritable.len(),
            unwritable
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join("; ")
        );
    }
    // The *reader's* half of the same sentence, and it has to be said by the
    // verb that writes the lossy copy rather than only by `pl info`. A `.dna`
    // whose block 6 holds `<References><Reference pubMedID=".." title=".."/>`
    // — a published citation, on 3 of the 33 real files this was checked
    // against — went through with exit 0, an empty stderr, and
    // `<References></References>` in the output. Re-reading that output reports
    // nothing, because by then there is nothing left to report: one hop and the
    // loss is both total and invisible.
    //
    // Not folded into the line above: that one says "the writer could not
    // carry", this says "the file held something the reader could not build".
    // Ungated by format — GenBank and FASTA discard the notes block wholesale,
    // so the statement is no less true there.
    if !report.unrepresentable_notes.is_empty() {
        eprintln!(
            "pl: {label}: {} part(s) of the source's notes block this model cannot hold, so \
             the output does not carry them: {}",
            report.unrepresentable_notes.len(),
            report
                .unrepresentable_notes
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if !report.unrepresentable_locations.is_empty() {
        eprintln!(
            "pl: {label}: {} location(s) the reader could not represent, so the output does \
             not carry them: {}",
            report.unrepresentable_locations.len(),
            report
                .unrepresentable_locations
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    // A different statement, and GenBank-only: GenBank cannot express an
    // unoriented or bidirectional feature, so those are written as forward. Say
    // so rather than letting the export publish a directional claim the source
    // never made.
    if is_genbank {
        let lossy = written.features_without_expressible_orientation();
        if !lossy.is_empty() {
            eprintln!(
                "pl: {label}: {} feature(s) have no GenBank-expressible strand and are written as forward: {}",
                lossy.len(),
                lossy
                    .iter()
                    .take(3)
                    .map(|(_, f)| f.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }
}

/// Which input, if any, this destination would land on top of.
///
/// The whole input list, not just the file being converted right now: `pl
/// convert seqA.fa seqA.gb --to genbank` derives `seqA.gb` from `seqA.fa`, and
/// the file it would clobber is one the run has not opened yet.
fn collides_with_input(dest: &Path, inputs: &[PathBuf]) -> Option<PathBuf> {
    inputs.iter().find(|i| same_file(i, dest)).cloned()
}

/// Where a per-input output goes: `--outdir` if given, beside the input if not.
fn destination_dir(input: &Path, outdir: &Option<PathBuf>) -> PathBuf {
    outdir.clone().unwrap_or_else(|| {
        input
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    })
}

fn title_of(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "sequence".into())
}

// ---------------------------------------------------------------------------

fn cmd_info(args: &[String]) -> Result<(), String> {
    let a = parse_args(args, &[], &["json"])?;
    a.require_files()?;

    if a.has("json") {
        // Accumulate, then print once.
        //
        // `println!("[")` ran before `read(path)?`, so an unreadable file
        // aborted mid-document: `pl info --json missing.fa ok.fa ok2.fa` emitted
        // exactly "[" plus a newline, processed neither valid file, and left a
        // unparseable array behind. A *parse* failure was already reported
        // per-file; only an I/O failure had this behaviour. `cmd_checksum
        // --stdin-json` already accumulates, which makes truncation structurally
        // impossible — this is that pattern.
        let mut out = String::from("[\n");
        let mut first = true;
        let mut failed = false;
        for path in &a.files {
            if !first {
                out.push_str(",\n");
            }
            first = false;
            let data = match read(path) {
                Ok(d) => d,
                Err(e) => {
                    // Same shape as a parse failure, so a consumer has one
                    // error form to handle rather than two.
                    failed = true;
                    out.push_str(&format!(
                        "  {{{}: {}, {}: {}}}",
                        json_str("file"),
                        json_str(&path.display().to_string()),
                        json_str("error"),
                        json_str(&e)
                    ));
                    continue;
                }
            };
            match load_with_report(&data) {
                Err(e) => out.push_str(&format!(
                    "  {{{}: {}, {}: {}}}",
                    json_str("file"),
                    json_str(&path.display().to_string()),
                    json_str("error"),
                    json_str(&e.to_string())
                )),
                Ok((mol, fmt, report)) => {
                    note_self_contradiction(&path.display().to_string(), &mol);
                    let sites: usize = mol.primers.iter().map(|p| p.sites.len()).sum();
                    let lower = mol.seq.iter().filter(|b| b.is_ascii_lowercase()).count();
                    // Notes, with their attributes. This is the only machine-
                    // readable view of block 6 the CLI has, and it exists so
                    // `reference/python/tests/xcheck_rust.py` can compare it:
                    // two independently written parsers had silently disagreed
                    // about that block for as long as both existed — Python kept
                    // `<Empty/>` and Rust dropped it, Python collapsed a
                    // repeated tag and Rust kept both — because the cross-check
                    // compared bases, features and primer counts and nothing
                    // else. An `.dna` attribute is exactly the kind of detail
                    // one implementation notices and the other does not.
                    let notes: Vec<String> = mol
                        .notes
                        .iter()
                        .map(|n| {
                            let attrs: Vec<String> = n
                                .attrs
                                .iter()
                                .map(|(k, v)| {
                                    format!(
                                        "{{{}: {}, {}: {}}}",
                                        json_str("name"),
                                        json_str(k),
                                        json_str("value"),
                                        json_str(v)
                                    )
                                })
                                .collect();
                            format!(
                                "{{{}: {}, {}: {}, {}: [{}]}}",
                                json_str("name"),
                                json_str(&n.key),
                                json_str("value"),
                                json_str(&n.value),
                                json_str("attrs"),
                                attrs.join(", ")
                            )
                        })
                        .collect();
                    // `Feature::start`/`end` are a min and a max over the
                    // segments, and this is the format that suffers most for
                    // it. `genbank::write` emits an origin-crossing feature as
                    // `join(2677..2686,1..7)`, so the min start is always
                    // exactly 1 and the max end is always exactly the molecule
                    // length: a 17 bp promoter came out of a shipped
                    // machine-readable format as `"start": 1, "end": 2686`,
                    // spanning the whole plasmid, with only `"segments": 2`
                    // beside it and the real coordinates unrecoverable. Worse,
                    // nothing about the record looks wrong. `extent` knows the
                    // wrap and reports it the way `Molecule::subseq` reads a
                    // pair — `end < start` means it crosses the origin — and
                    // falls back to the same min/max for an ordinary spliced
                    // join, where the min/max really is the extent.
                    let circular = mol.topology.is_circular();
                    let span = mol.span();
                    let feats: Vec<String> = mol
                        .features
                        .iter()
                        .map(|f| {
                            let (start, end) =
                                f.extent(span, circular).unwrap_or((f.start(), f.end()));
                            format!(
                                "{{{}: {}, {}: {}, {}: {}, {}: {}, {}: {}, {}: {}}}",
                                json_str("name"),
                                json_str(&f.name),
                                json_str("kind"),
                                json_str(&f.kind),
                                json_str("start"),
                                start,
                                json_str("end"),
                                end,
                                json_str("segments"),
                                f.segments.len(),
                                json_str("strand"),
                                json_str(match f.strand {
                                    pl_core::Strand::Forward => "+",
                                    pl_core::Strand::Reverse => "-",
                                    pl_core::Strand::Both => "both",
                                    pl_core::Strand::Unoriented => "none",
                                })
                            )
                        })
                        .collect();
                    out.push_str(&format!(
                        "  {{{}: {}, {}: {}, {}: {}, {}: {}, {}: {}, {}: {}, {}: {}, {}: {}, {}: {}, {}: {}, {}: {}, {}: {}, {}: [{}], {}: [{}], {}: [{}]}}",
                        json_str("file"), json_str(&path.display().to_string()),
                        json_str("format"), json_str(fmt.name()),
                        json_str("bp"), mol.len(),
                        json_str("declared_bp"), mol.declared_len.unwrap_or(0),
                        json_str("sequence_absent"), mol.seq.is_empty(),
                        json_str("records_in_file"), report.records,
                        json_str("span"), mol.span(),
                        json_str("circular"), mol.topology.is_circular(),
                        json_str("lowercase"), lower,
                        json_str("n_primers"), mol.primers.len(),
                        json_str("n_binding_sites"), sites,
                        json_str("n_features"), mol.features.len(),
                        json_str("features"), feats.join(", "),
                        json_str("notes"), notes.join(", "),
                        json_str("unrepresentable_notes"),
                        report.unrepresentable_notes.iter()
                            .map(|s| json_str(s)).collect::<Vec<_>>().join(", ")
                    ));
                }
            }
        }
        out.push_str("\n]");
        println!("{out}");
        // A read failure still exits non-zero: `reference/python/tests/
        // xcheck_rust.py` bails on a non-zero status, so returning 0 here would
        // turn a hard stop into a soft mismatch.
        if failed {
            return Err("one or more files could not be read".into());
        }
        return Ok(());
    }

    // A file that cannot be read is reported and the run carries on, exactly as
    // the `--json` branch above does.
    //
    // `read(path)?` propagated out of the loop, so `pl info missing.fa pUC19.gb
    // pET28a.gb` printed one OS error and summarised neither readable plasmid,
    // while the same argv with `--json` reported the failure *and* both records.
    // The parse-failure arm three lines below has always been per-file, so the
    // asymmetry lived inside one loop body. docs/AUDIT-2026-07.md:165 asks for
    // both branches to be fixed together "or you create a new asymmetry".
    // The exit status is unchanged: a read failure still ends non-zero.
    let mut failed = false;
    for path in &a.files {
        let data = match read(path) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("pl: {e}");
                failed = true;
                continue;
            }
        };
        match load_with_report(&data) {
            Err(e) => println!("{}\n   ERROR: {e}\n", path.display()),
            Ok((mol, fmt, report)) => {
                note_self_contradiction(&path.display().to_string(), &mol);
                println!("{}", path.display());
                println!("   format     {}", fmt.name());
                if report.truncated() {
                    // Saying nothing here is how 1,879 features went missing
                    // from a 124-record file without anyone noticing.
                    println!(
                        "   records    {} in this file; showing the first",
                        report.records
                    );
                }
                if !report.unrepresentable_locations.is_empty() {
                    println!(
                        "   skipped    {} location(s) this reader cannot represent: {}",
                        report.unrepresentable_locations.len(),
                        report
                            .unrepresentable_locations
                            .iter()
                            .take(3)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
                // Its own line with its own noun. Folding these into the
                // "location(s)" line above would have the CLI say something
                // false about coordinates. "part(s) of the notes block" rather
                // than "element(s) nested deeper" because the channel carries
                // three shapes — a nested element, a note's text after one and
                // an attribute on the `<Notes>` root — and a noun naming only
                // the first would misdescribe `Notes/Comments/text()`.
                if !report.unrepresentable_notes.is_empty() {
                    println!(
                        "   skipped    {} part(s) of the notes block this model cannot hold: {}",
                        report.unrepresentable_notes.len(),
                        report
                            .unrepresentable_notes
                            .iter()
                            .take(3)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
                if mol.sequence_absent() {
                    println!(
                        "   length     {} bp DECLARED, but this file carries no bases",
                        mol.span()
                    );
                } else {
                    println!("   length     {} bp", mol.len());
                }
                println!("   topology   {}", mol.topology.as_str());
                match mol.gc_percent() {
                    Some(gc) => println!("   GC         {gc:.1}%"),
                    None => println!("   GC         n/a"),
                }
                let comp = mol.composition();
                if comp.other > 0 {
                    println!("   ambiguous  {} base(s) outside ACGT", comp.other);
                }
                let lower = mol.seq.iter().filter(|b| b.is_ascii_lowercase()).count();
                if lower > 0 {
                    println!("   lowercase  {lower} base(s) -- masked or low-confidence");
                }
                println!("   features   {}", mol.features.len());
                let sites: usize = mol.primers.iter().map(|p| p.sites.len()).sum();
                if !mol.primers.is_empty() {
                    println!(
                        "   primers    {} ({sites} binding site(s))",
                        mol.primers.len()
                    );
                }
                if fmt == Format::SnapGene {
                    if let Ok(doc) = snapgene::parse(&data) {
                        let total = doc.total_bytes();
                        let derived = doc.derived_bytes();
                        if total > 0 && derived > 0 {
                            println!(
                                "   container  {total} bytes, {:.0}% regenerable cache",
                                100.0 * derived as f64 / total as f64
                            );
                        }
                        if doc.history_present {
                            println!(
                                "   history    present{}",
                                if doc.history_compressed {
                                    " (xz-compressed)"
                                } else {
                                    ""
                                }
                            );
                        }
                    }
                }
                println!();
            }
        }
    }
    if failed {
        return Err("one or more files could not be read".into());
    }
    Ok(())
}

fn cmd_convert(args: &[String]) -> Result<(), String> {
    let a = parse_args(args, &["to", "outdir", "o"], &["stdout"])?;
    a.require_files()?;

    let to = a.get("to").unwrap_or("genbank").to_ascii_lowercase();
    #[derive(PartialEq)]
    enum Out {
        GenBank,
        Fasta,
        Dna,
    }
    let (ext, out_fmt) = match to.as_str() {
        "genbank" | "gb" | "gbk" => ("gb", Out::GenBank),
        "fasta" | "fa" | "fna" => ("fa", Out::Fasta),
        "dna" | "snapgene" => ("dna", Out::Dna),
        other => return Err(format!("unknown output format '{other}'")),
    };
    let is_gb = out_fmt == Out::GenBank;

    let outdir = a.get("outdir").or_else(|| a.get("o")).map(PathBuf::from);
    let to_stdout = a.has("stdout");
    let date = today();

    // Two inputs can share a basename. Silently overwriting one with the other
    // is data loss, so collisions get a suffix and are reported.
    let mut claimed: Vec<PathBuf> = Vec::new();
    let mut converted = 0usize;
    let mut renamed = 0usize;

    // One SnapGene container per stream.
    //
    // `--stdout` writes each payload back to back with no separator, which is
    // meaningful for GenBank and FASTA -- both are record streams -- and
    // silently corrupting for `.dna`. `snapgene::read_blocks` checks the HEADER
    // kind and magic only for the *first* block, so `pl convert a.gb b.gb --to
    // dna --stdout > merged.dna` parses without error: b's header is absorbed
    // as an ordinary block of a's document and `parse` applies blocks
    // last-writer-wins, leaving a's title over b's sequence and features. The
    // run printed nothing at all, because the summary below is inside
    // `if !to_stdout`.
    if to_stdout && out_fmt == Out::Dna && a.files.len() > 1 {
        return Err(format!(
            "--stdout would write {} SnapGene containers into one stream, and a reader takes the \
             result for one valid document with the first file's name over the last file's \
             sequence. Convert one file at a time, or drop --stdout.",
            a.files.len()
        ));
    }

    // Never write over a file that is still on the command line.
    //
    // The guard inside the loop compared the destination only against `path`,
    // the input of *that* iteration, and `claimed` only ever compared output
    // against output. Inputs are read and written one at a time, and
    // `locus_name` strips the extension, so `pl convert seqA.fa seqA.gb --to
    // genbank` computed `seqA.gb` from `seqA.fa`, saw no collision, and wrote
    // the FASTA-derived record over the user's `seqA.gb` -- which iteration 2
    // then read back and refused. The original record was already gone,
    // features and all, before the multi-record refusal above ever parsed it,
    // and the run still ended with "nothing was overwritten". Checked for every
    // input before anything is written, so a collision costs no files at all.
    if !to_stdout {
        for path in &a.files {
            let dir = destination_dir(path, &outdir);
            let dest = dir.join(format!("{}.{ext}", genbank::locus_name(&title_of(path))));
            if let Some(victim) = collides_with_input(&dest, &a.files) {
                return Err(format!(
                    "{}: converting to {ext} here would overwrite {}. \
                     Use --outdir <dir> to write elsewhere, or --stdout.",
                    path.display(),
                    if same_file(&victim, path) {
                        "the input file".to_string()
                    } else {
                        format!("{}, which is also being converted", victim.display())
                    }
                ));
            }
        }
    }

    for path in &a.files {
        let data = read(path)?;
        let (mol, _fmt, report) =
            load_with_report(&data).map_err(|e| format!("{}: {e}", path.display()))?;
        note_self_contradiction(&path.display().to_string(), &mol);
        if report.truncated() {
            // Converting would write record 1 and silently discard the rest.
            // A 124-record 36 KB .gbk became a 28 KB single-record file with
            // 1,879 features gone, reported as success.
            return Err(format!(
                "{}: holds {} records and this would write only the first. Split the file first, or use --stdout to see what would be written.",
                path.display(),
                report.records
            ));
        }
        let title = title_of(path);
        // `.dna` is bytes rather than text, so the payload is a Vec<u8> for
        // all three formats and the destination logic below — which guards
        // against overwriting the input and against two inputs claiming one
        // output name — is shared rather than reimplemented per format.
        //
        // What the `.dna` writer deliberately omits is stated at
        // `snapgene::from_molecule`: the two regenerable caches, 78% of a
        // typical file and dangerous when stale, and the history tree, which
        // is a provenance graph this file does not have. Anything else missing
        // from the output is reported below rather than assumed absent — from
        // *both* directions, which took two goes to get right: what the writer
        // could not carry, and what the reader could not build in the first
        // place. Only the writer's half was wired at first, and the reader's
        // half is where a `.dna`'s nested citation goes.
        //
        // The `_reporting` variants, not `write`/`from_molecule`. Both writers
        // have filled a `Vec<String>` of un-writable items since block 5 was
        // implemented and both plain wrappers throw it away, so the only callers
        // of the reporting forms in this workspace were unit tests: a primer
        // binding site starting before base 1 was dropped by the `.dna` writer
        // and `pl convert` printed nothing and exited 0. docs/PLAN.md §6.4.5 is
        // blunt about which way this goes — "a writer that drops unknown blocks
        // while appearing to succeed is the single worst outcome in this
        // document".
        let mut unwritable: Vec<String> = Vec::new();
        let bytes: Vec<u8> = match out_fmt {
            Out::GenBank => {
                let (text, rep) = genbank::write_reporting(&mol, &title, date);
                unwritable = rep;
                text.into_bytes()
            }
            Out::Fasta => fasta::write(&mol, &title, 70).into_bytes(),
            Out::Dna => {
                let (b, rep) = pl_fileio::snapgene::from_molecule_reporting(&mol);
                unwritable = rep;
                b
            }
        };
        // All four loss channels, in `note_output_losses` so that `pl annotate
        // --genbank` — which writes the same record with the same writer — says
        // the same things. It said none of them until this was shared.
        note_output_losses(
            &path.display().to_string(),
            ext,
            &unwritable,
            &report,
            &mol,
            is_gb,
        );

        if to_stdout {
            use std::io::Write;
            std::io::stdout()
                .write_all(&bytes)
                .map_err(|e| format!("stdout: {e}"))?;
            converted += 1;
            continue;
        }

        let dir = destination_dir(path, &outdir);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;

        let stem = genbank::locus_name(&title);
        let mut dest = dir.join(format!("{stem}.{ext}"));

        // The unsuffixed destination was cleared against every input before the
        // loop started. A *suffixed* one has to be cleared here, because
        // `seqA-2.gb` can be a file on the command line too.
        if claimed.contains(&dest) {
            let mut n = 2;
            loop {
                let candidate = dir.join(format!("{stem}-{n}.{ext}"));
                if !claimed.contains(&candidate)
                    && collides_with_input(&candidate, &a.files).is_none()
                {
                    dest = candidate;
                    break;
                }
                n += 1;
            }
            renamed += 1;
        }
        claimed.push(dest.clone());
        std::fs::write(&dest, &bytes).map_err(|e| format!("{}: {e}", dest.display()))?;

        println!(
            "{:>10} bp  {:>8}  {:>4} feat  {}",
            mol.span(),
            mol.topology.as_str(),
            mol.features.len(),
            dest.display()
        );
        converted += 1;
    }

    if !to_stdout {
        eprintln!("\nconverted {converted} file(s)");
        if renamed > 0 {
            eprintln!(
                "{renamed} output name(s) were suffixed because inputs shared a basename -- \
                 nothing was overwritten"
            );
        }
    }
    Ok(())
}

fn cmd_digest(args: &[String]) -> Result<(), String> {
    let a = parse_args(args, &["enzyme"], &["unique", "non-cutters", "json"])?;
    a.require_files()?;
    let path = &a.files[0];
    let data = read(path)?;
    let (mol, _, report) =
        load_with_report(&data).map_err(|e| format!("{}: {e}", path.display()))?;
    note_first_record_only(&path.display().to_string(), &report, "digested");
    if mol.seq.is_empty() {
        return Err(format!("{}: no bases to digest", path.display()));
    }

    let wanted = a.get_all("enzyme");
    let mut results = pl_enzymes::digest_all(&mol);
    if !wanted.is_empty() {
        // Every name is resolved before anything is digested, because the guard
        // that used to stand here was per *call* and not per *name*: `retain`
        // followed by `if results.is_empty()` only fires when EVERY name is
        // unknown, so `pl digest x.fa --enzyme HaeIII --enzyme EcoRI` printed
        // the EcoRI row, exited 0, and dropped HaeIII without a word. The
        // answer was then a statement about one enzyme wearing the heading of
        // two. DpnI and HaeIII are both absent from the 58-row table and are
        // both ordinary things to ask for, so this is reachable by typing a
        // real enzyme name rather than by mistyping one.
        //
        // All the misses are named at once — a caller who got three names wrong
        // should not have to run this three times. `by_name` and `digest_all`
        // search the same `ENZYMES` table with the same case-insensitive
        // comparison, so no name that resolves here can vanish in the retain
        // below. bins/pl-mcp and crates/pl-py already resolved per name; this
        // was the last of the three that did not.
        let missing: Vec<&str> = wanted
            .iter()
            .copied()
            .filter(|w| pl_enzymes::by_name(w).is_none())
            .collect();
        if !missing.is_empty() {
            return Err(format!(
                "no enzyme named {} in the built-in set of {}. `pl digest {}` \
                 with no --enzyme lists every one of them",
                missing.join(", "),
                pl_enzymes::ENZYMES.len(),
                path.display()
            ));
        }
        results.retain(|d| wanted.iter().any(|w| w.eq_ignore_ascii_case(d.enzyme.name)));
    }
    if a.has("unique") {
        results.retain(|d| d.is_unique_cutter());
    }
    if a.has("non-cutters") {
        results.retain(|d| d.is_non_cutter());
    }

    if a.has("json") {
        // Positions are 1-based indices of the base 3' of the nick, matching
        // Biopython's Restriction module so comparison is a plain equality.
        let items: Vec<String> = results
            .iter()
            .map(|d| {
                let pos: Vec<String> = d.positions.iter().map(u64::to_string).collect();
                format!(
                    "  {{{}: {}, {}: {}, {}: [{}]}}",
                    json_str("enzyme"),
                    json_str(d.enzyme.name),
                    json_str("site"),
                    json_str(d.enzyme.site),
                    json_str("positions"),
                    pos.join(", ")
                )
            })
            .collect();
        println!(
            "{{{}: {}, {}: {}, {}: {},\n {}: [\n{}\n ]}}",
            json_str("file"),
            json_str(&title_of(path)),
            json_str("bp"),
            mol.len(),
            json_str("circular"),
            mol.topology.is_circular(),
            json_str("digests"),
            items.join(",\n")
        );
        return Ok(());
    }

    println!(
        "{} -- {} bp {}\n",
        title_of(path),
        mol.len(),
        mol.topology.as_str()
    );
    println!(
        "{:<10} {:>5}  {:<28} largest fragments",
        "enzyme", "cuts", "positions"
    );
    for d in &results {
        if d.positions.is_empty() && !a.has("non-cutters") {
            continue;
        }
        let pos: Vec<String> = d.positions.iter().take(6).map(u64::to_string).collect();
        let mut shown = pos.join(", ");
        if d.positions.len() > 6 {
            shown.push_str(", ...");
        }
        let frags = d.fragments(mol.len(), mol.topology);
        let f: Vec<String> = frags.iter().take(4).map(u64::to_string).collect();
        println!(
            "{:<10} {:>5}  {:<28} {}",
            d.enzyme.name,
            d.count(),
            shown,
            f.join(" / ")
        );
    }

    let uniq = results.iter().filter(|d| d.is_unique_cutter()).count();
    let non = results.iter().filter(|d| d.is_non_cutter()).count();
    println!(
        "\n{uniq} unique cutter(s), {non} non-cutter(s), from {} enzymes",
        results.len()
    );
    Ok(())
}

fn cmd_blocks(args: &[String]) -> Result<(), String> {
    let a = parse_args(args, &[], &[])?;
    a.require_files()?;
    let path = &a.files[0];
    let data = read(path)?;
    if detect(&data) != Some(Format::SnapGene) {
        return Err(format!("{}: not a SnapGene .dna file", path.display()));
    }
    let doc = snapgene::parse(&data).map_err(|e| e.to_string())?;
    let total = doc.total_bytes().max(1);

    println!(
        "{}  --  {} bytes in {} blocks\n",
        title_of(path),
        doc.total_bytes(),
        doc.blocks.len()
    );
    println!(
        "{:>4}  {:<20} {:>10} {:>7}",
        "type", "meaning", "bytes", "share"
    );
    for b in &doc.blocks {
        let meaning = match b.kind {
            snapgene::block::SEQUENCE => "sequence",
            snapgene::block::CUTSITE_CACHE => "cut-site cache",
            snapgene::block::ENZYME_TABLE => "enzyme table",
            snapgene::block::PRIMERS => "primers",
            snapgene::block::NOTES => "notes",
            snapgene::block::HISTORY_TREE => "history tree",
            snapgene::block::EXTRA_PROPS => "extra properties",
            snapgene::block::HEADER => "header",
            snapgene::block::FEATURES => "features",
            snapgene::block::HISTORY_NODE => "history node",
            _ => "unknown",
        };
        println!(
            "{:>4}  {:<20} {:>10} {:>6.1}%{}",
            b.kind,
            meaning,
            b.size_on_disk(),
            100.0 * b.size_on_disk() as f64 / total as f64,
            if b.is_derived() { "   regenerable" } else { "" }
        );
    }
    let derived = doc.derived_bytes();
    println!(
        "\n{:.0}% of this file is a cache of (sequence x enzyme set) and is a pure function \
         of data held elsewhere in it.",
        100.0 * derived as f64 / total as f64
    );
    Ok(())
}

/// SEGUID v2 checksums for a molecule.
///
/// Reports the form whose invariances are the molecule's own: `cdseguid` for a
/// circular duplex, `ldseguid` for a linear one.
///
/// `--stdin-json` takes `[{"label":..,"seq":..}]` and emits all five forms for
/// each. That mode exists for the cross-check against the reference
/// implementation, not for interactive use.
fn cmd_checksum(args: &[String]) -> Result<(), String> {
    let a = parse_args(args, &[], &["stdin-json"])?;

    if a.has("stdin-json") {
        let mut input = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut input)
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for (label, seq) in parse_label_seq_json(&input)? {
            let rc =
                String::from_utf8_lossy(&pl_core::reverse_complement(seq.as_bytes())).into_owned();
            let f = |r: Result<String, pl_core::seguid::Error>| match r {
                Ok(v) => json_str(&v),
                Err(e) => json_str(&format!("ERROR: {e}")),
            };
            out.push(format!(
                "{{{}: {}, {}: {}, {}: {}, {}: {}, {}: {}, {}: {}}}",
                json_str("label"),
                json_str(&label),
                json_str("seguid"),
                f(pl_core::seguid::seguid(&seq)),
                json_str("lsseguid"),
                f(pl_core::lsseguid(&seq)),
                json_str("csseguid"),
                f(pl_core::csseguid(&seq)),
                json_str("ldseguid"),
                f(pl_core::ldseguid(&seq, &rc)),
                json_str("cdseguid"),
                f(pl_core::cdseguid(&seq, &rc)),
            ));
        }
        println!("[{}]", out.join(",\n "));
        return Ok(());
    }

    a.require_files()?;
    for path in &a.files {
        let data = read(path)?;
        let (mol, _, report) =
            load_with_report(&data).map_err(|e| format!("{}: {e}", path.display()))?;
        // SEGUID is defined over unambiguous uppercase DNA. Say what was done
        // rather than quietly folding case or dropping ambiguity codes: a
        // checksum is an identity claim, and a silently altered input makes it
        // a false one.
        let seq: String = String::from_utf8_lossy(&mol.seq).to_uppercase();
        println!("{}", title_of(path));
        // Dropped records are exactly that kind of silent alteration, and this
        // is the one identity-asserting verb that used to skip the check. An
        // 8-contig Plasmidsaurus `assemblies.fa` printed the file's basename
        // and one ldseguid/lsseguid pair from contig 1, shape-identical to a
        // single-record file's output, and the user filed that checksum as the
        // identity of the whole file. On stdout beside the checksum, not on
        // stderr, because the claim and its scope have to travel together
        // through a redirect.
        if report.truncated() {
            println!(
                "   records    {} in this file; the checksum below covers only the first",
                report.records
            );
        }
        if seq.is_empty() {
            println!("   no sequence to checksum");
            continue;
        }
        if let Some(bad) = seq.chars().find(|c| !matches!(c, 'A' | 'C' | 'G' | 'T')) {
            println!("   contains {bad:?}; SEGUID is defined over unambiguous DNA only");
            continue;
        }
        let lower = mol.seq.iter().filter(|b| b.is_ascii_lowercase()).count();
        if lower > 0 {
            println!("   note: {lower} lowercase base(s) upper-cased for the checksum");
        }
        let rc = String::from_utf8_lossy(&pl_core::reverse_complement(seq.as_bytes())).into_owned();
        let duplex = if mol.topology.is_circular() {
            pl_core::cdseguid(&seq, &rc)
        } else {
            pl_core::ldseguid(&seq, &rc)
        };
        match duplex {
            Ok(v) => println!("   {v}"),
            Err(e) => println!("   {e}"),
        }
        match pl_core::lsseguid(&seq) {
            Ok(v) => println!("   {v}   (this strand alone)"),
            Err(e) => println!("   lsseguid: {e}"),
        }
    }
    Ok(())
}

/// Render each molecule's map to a standalone SVG.
///
/// SVG rather than PNG because a map is a figure: it goes into a paper at
/// whatever size the journal asks for, and a raster of it does not. The output
/// is self-contained — no external stylesheet, no font file, no script — so it
/// opens in Illustrator, Inkscape and a browser alike.
/// Which cutters `pl export` puts on the figure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sites {
    Unique,
    Dual,
    All,
    None,
}

impl Sites {
    /// Every `(enzyme, cut position)` this filter admits, and what it excluded.
    ///
    /// Sorted by position and then by name so the figure is byte-identical for
    /// identical input, which is the whole point of `pl-draw`.
    ///
    /// The `Disclosure` comes back with it because the filter is the only thing
    /// that knows what it turned away, and the figure is where that has to be
    /// said: `--sites unique` is the default, so `pl export` on the user's own
    /// plasmid omitted twelve dual and six multi cutters and neither the SVG nor
    /// stderr mentioned any of them. A dual cutter is exactly what you reach for
    /// to excise an insert.
    fn of(self, mol: &pl_core::Molecule) -> (Vec<(String, u64)>, pl_draw::ring::Disclosure) {
        let mut out: Vec<(String, u64)> = Vec::new();
        let mut d = pl_draw::ring::Disclosure::default();
        for r in pl_enzymes::digest_all(mol) {
            let n = r.count();
            if n == 0 {
                continue;
            }
            d.cutters += 1;
            let keep = match self {
                Sites::Unique => n == 1,
                Sites::Dual => (1..=2).contains(&n),
                Sites::All => true,
                Sites::None => false,
            };
            if keep {
                out.extend(r.positions.iter().map(|p| (r.enzyme.name.to_string(), *p)));
            } else if n == 1 {
                // A single cutter the filter turned away, which only `--sites
                // none` produces. Without this arm it fell through to `multi`,
                // and pKoV exported `0 of 40 cutters labelled · 12 dual, 28
                // multi not drawn` — 22 of those 28 cut exactly once — while
                // `closes()` passed, because the sum reached 40 over the wrong
                // classes. See `ring::Disclosure::single`.
                d.single += 1;
            } else if n == 2 {
                d.dual += 1;
            } else {
                d.multi += 1;
            }
        }
        // `--sites dual` admits both, so nothing is a "dual cutter not drawn";
        // the buckets have to describe this filter and not the default one.
        out.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
        (out, d)
    }
}

fn cmd_export(args: &[String]) -> Result<(), String> {
    let a = parse_args(
        args,
        &[
            "outdir", "o", "width", "height", "mm", "journal", "column", "sites",
        ],
        &["pdf", "eps", "stdout", "no-ruler", "check-contrast"],
    )?;
    a.require_files()?;

    let num = |name: &str, default: f64| -> Result<f64, String> {
        match a.get(name) {
            None => Ok(default),
            Some(v) => v
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite() && (16.0..=20000.0).contains(v))
                .ok_or_else(|| format!("--{name} '{v}' is not a size between 16 and 20000")),
        }
    };
    let base_opts = pl_draw::Options {
        width: num("width", 720.0)?,
        height: num("height", 720.0)?,
        ruler: !a.has("no-ruler"),
        ..Default::default()
    };

    // Which restriction sites go on the figure.
    //
    // `pl-draw` held no reference to an enzyme anywhere, so every exported map
    // had none: a user reads 22 unique cutters off the desktop map, exports the
    // figure, and gets a picture with nothing to plan a digest from. `unique`
    // is the default because it is the rule the desktop map applies, so the two
    // produce the same figure for the same molecule.
    //
    // Refused positively, the way `--column`, `--topology`, `--salt` and `--to`
    // all are: a mistyped filter that silently means something else is how a
    // user comes to believe a site is absent.
    let sites = match a.get("sites").map(|s| s.to_ascii_lowercase()) {
        None => Sites::Unique,
        Some(v) if v == "unique" => Sites::Unique,
        Some(v) if v == "dual" => Sites::Dual,
        Some(v) if v == "all" => Sites::All,
        Some(v) if v == "none" => Sites::None,
        Some(v) => return Err(format!("--sites {v:?}: expected unique, dual, all or none")),
    };

    // Physical size. A figure exported at "720 pixels" arrives in a manuscript
    // as whatever width the template gives it, and every label scales with it:
    // an 8 pt name at half size is 4 pt, below every journal's floor, which is
    // invisible on screen and caught by a copy editor.
    let journal = match a.get("journal") {
        Some(j) => Some(pl_draw::page::preset(j).ok_or_else(|| {
            format!(
                "--journal {j:?}: known presets are {}",
                pl_draw::page::PRESETS
                    .iter()
                    .map(|p| p.name)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?),
        None => None,
    };
    // Every value that was not the exact lowercase `double` used to select the
    // single-column width in silence, so `--column Double` — with `--journal`
    // next door accepting `Nature` case-insensitively — exported the EPS at
    // 89 mm instead of the requested 183 (measured: BoundingBox 0 0 253 253
    // rather than 0 0 519 519) and then ran the journal's type floor against the
    // wrong width, printing "smallest type is 3.0 pt, below nature's 5 pt
    // minimum / raise --font-size, drop labels, or use --column double" — the
    // remedy the user believed they had just applied. Refused positively, the
    // way `--topology`, `--salt`, `--mode`, `--state` and `--to` all are.
    let double = match a.get("column") {
        None => false,
        Some(v) if v.eq_ignore_ascii_case("double") => true,
        Some(v) if v.eq_ignore_ascii_case("single") => false,
        Some(v) => return Err(format!("--column {v:?}: expected single or double")),
    };
    // ... and a `--column` nothing reads is no better than a mistyped one: the
    // width comes from `--mm` when it is given and from the journal preset
    // otherwise, so with `--mm`, or with no preset at all, this flag was
    // accepted and discarded.
    if a.has("column") {
        if a.has("mm") {
            return Err(
                "--column and --mm both set the printed width; --mm wins, so passing \
                        both means one of them is not doing what you think. Give one."
                    .into(),
            );
        }
        if journal.is_none() {
            return Err(
                "--column picks between a journal's single- and double-column widths, \
                        so it needs --journal <name>. Use --mm <width> to set a width directly."
                    .into(),
            );
        }
    }
    let width_mm: Option<f64> = match (a.get("mm"), journal) {
        (Some(v), _) => Some(
            v.parse::<f64>()
                .ok()
                .filter(|x| (5.0..=500.0).contains(x))
                .ok_or_else(|| format!("--mm {v:?}: expected 5 to 500"))?,
        ),
        (None, Some(p)) => Some(if double { p.double_mm } else { p.single_mm }),
        (None, None) => None,
    };

    let outdir = a.get("outdir").or_else(|| a.get("o")).map(PathBuf::from);
    let to_stdout = a.has("stdout");
    let mut claimed: Vec<PathBuf> = Vec::new();
    let (mut written, mut renamed) = (0usize, 0usize);

    // One picture per stream, for every format this verb writes.
    //
    // `--stdout` wrote each payload back to back with no separator and no
    // file-count check: two `<svg>` roots is not well-formed XML, a second PDF
    // leaves a trailing xref pointing into the first document, and EPS is one
    // document structure too. Nothing downstream reports the concatenation, and
    // the run summary is inside `if !to_stdout`, so the whole run said nothing.
    if to_stdout && a.files.len() > 1 {
        return Err(format!(
            "--stdout writes one figure to one stream, and {} inputs would be concatenated into a \
             file no viewer reads as {} pictures. Export one file at a time, or use --outdir <dir>.",
            a.files.len(),
            a.files.len()
        ));
    }

    // Never write over a file that is still on the command line. Same defect,
    // same fix, same reasoning as `cmd_convert`: `pl export map.gb map.svg`
    // wrote the map over the user's `map.svg` in iteration 1 and only then
    // discovered, in iteration 2, that `map.svg` is not a sequence file.
    if !to_stdout {
        let ext = if a.has("eps") {
            "eps"
        } else if a.has("pdf") {
            "pdf"
        } else {
            "svg"
        };
        for path in &a.files {
            let dir = destination_dir(path, &outdir);
            let dest = dir.join(format!("{}.{ext}", genbank::locus_name(&title_of(path))));
            if let Some(victim) = collides_with_input(&dest, &a.files) {
                return Err(format!(
                    "{}: writing the map here would overwrite {}. Use --outdir <dir> or --stdout.",
                    path.display(),
                    if same_file(&victim, path) {
                        "the input".to_string()
                    } else {
                        format!("{}, which is also an input", victim.display())
                    }
                ));
            }
        }
    }

    for path in &a.files {
        let data = read(path)?;
        let (mol, _fmt, report) =
            load_with_report(&data).map_err(|e| format!("{}: {e}", path.display()))?;
        if report.truncated() {
            // One file, one picture. Drawing record 1 and calling the result
            // "the map of this file" is the same silent-truncation mistake
            // `convert` had.
            return Err(format!(
                "{}: holds {} records and this would draw only the first. Split the file first.",
                path.display(),
                report.records
            ));
        }

        // One `Options` per file: the title, the site list and the disclosure
        // line are properties of *this* molecule, not of the run.
        let (site_list, filtered) = sites.of(&mol);
        let mut opts = pl_draw::Options {
            title: Some(pl_fileio::caption_of(&title_of(path)).to_string()),
            sites: site_list,
            ..base_opts.clone()
        };
        // Built in two passes, because the line has to name how many labels the
        // ring could not fit and only placing them answers that.
        //
        // Exact rather than approximate, and provably so: the note reaches only
        // `centre_room` -> `keep_clear` -> the ruler's radius, and nothing there
        // feeds back into the reserve, the geometry or the packing. So pass one's
        // counts describe pass two's picture. `the_note_does_not_change_what_it_counts`
        // asserts that rather than leaving it as a claim in a comment.
        let note = {
            let (_, first) = pl_draw::scene(&mol, opts.clone());
            let d = pl_draw::ring::Disclosure {
                labelled: first.sites_named,
                hidden: first.sites_dropped,
                shortened: first.sites_shortened,
                ..filtered
            };
            // The invariant, checked and not assumed: every cutting enzyme in
            // exactly one bucket. A line whose arithmetic does not close tells
            // the reader enzymes went missing that did not.
            debug_assert!(d.closes(), "{d:?} does not account for every cutter");
            (d.cutters > 0).then_some(d)
        };
        // On stderr as well as in the figure, because a filter that hides is the
        // one thing this command must not be quiet about — and stderr has no
        // width limit, so it always gets the long form.
        if let Some(d) = &note {
            eprintln!("pl: {}: {}", path.display(), d.long());
        }
        opts.note = note;

        let as_pdf = a.has("pdf");
        let as_eps = a.has("eps");
        let (bytes, drawn, font) = if as_eps {
            let (scene, d) = pl_draw::scene(&mol, opts.clone());
            let fit = width_mm.map(|mm| pl_draw::page::Fit::to_width_mm(&scene, mm));
            let (text, f) = pl_draw::eps::to_eps(&scene, fit.map_or(1.0, |f| f.scale));
            (text.into_bytes(), d, Some(f))
        } else if as_pdf {
            let (b, d, f) = pl_draw::circular_pdf(&mol, opts.clone());
            (b, d, Some(f))
        } else {
            let (s, d) = pl_draw::circular_svg(&mol, opts.clone());
            (s.into_bytes(), d, None)
        };

        // Contrast, measured against the background the figure is drawn on.
        // Feature colours come out of the file, so this is not something the
        // renderer can simply fix -- but an unreadable label is a defect in the
        // figure whoever authored the colour, and saying so is the only way it
        // gets noticed before print.
        if a.has("check-contrast") {
            let (scene, _) = pl_draw::scene(&mol, opts.clone());
            let scale =
                width_mm.map_or(1.0, |mm| pl_draw::page::Fit::to_width_mm(&scene, mm).scale);
            let findings = pl_draw::contrast::audit(&scene, "#ffffff", scale);
            if findings.is_empty() {
                eprintln!("pl: {}: contrast ok (WCAG 2.2 AA)", path.display());
            }
            for f in findings.iter().take(12) {
                eprintln!(
                    "pl: {}: {:.2}:1 needs {:.1}:1 — {} {} on {}",
                    path.display(),
                    f.ratio,
                    f.required,
                    f.foreground,
                    if f.what.is_empty() { "shape" } else { &f.what },
                    f.background
                );
            }
            if findings.len() > 12 {
                eprintln!("pl: {}: and {} more", path.display(), findings.len() - 12);
            }
        }

        // The check the physical size exists for. Report it once per file,
        // whatever the format, because it is a property of the figure and not
        // of the encoder.
        if let Some(mm) = width_mm {
            let (scene, _) = pl_draw::scene(&mol, opts.clone());
            let fit = pl_draw::page::Fit::to_width_mm(&scene, mm);
            eprintln!(
                "pl: {}: {:.1} x {:.1} mm at final size",
                path.display(),
                fit.width_mm,
                fit.height_mm
            );
            if let Some(p) = journal {
                if fit.type_too_small(&p) {
                    eprintln!(
                        "pl: {}: smallest type is {:.1} pt, below {}'s {:.0} pt minimum",
                        path.display(),
                        fit.min_font_pt.unwrap_or(0.0),
                        p.name,
                        p.min_font_pt
                    );
                    eprintln!("     raise --font-size, drop labels, or use --column double");
                }
            }
        }

        // Helvetica is one of the fourteen fonts every PDF viewer provides, so
        // nothing is embedded -- at the cost of WinAnsi, which has no Greek.
        // Say which names lost characters rather than shipping a figure full of
        // question marks with nothing to explain them.
        if let Some(f) = &font {
            if !f.unencodable.is_empty() {
                eprintln!(
                    "pl: {}: {} name(s) hold characters Helvetica cannot show and were                      written with '?': {}",
                    path.display(),
                    f.unencodable.len(),
                    f.unencodable.join(", ")
                );
                eprintln!("     export SVG instead to keep them");
            }
        }

        // A map missing three labels looks exactly like a plasmid with three
        // fewer features, so say which ones went.
        if !drawn.labels_hidden.is_empty() {
            eprintln!(
                "pl: {}: {} label(s) did not fit and are not shown: {}{}",
                path.display(),
                drawn.labels_hidden.len(),
                drawn
                    .labels_hidden
                    .iter()
                    .take(5)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", "),
                if drawn.labels_hidden.len() > 5 {
                    ", ..."
                } else {
                    ""
                }
            );
            eprintln!("     a larger --width/--height fits more of them");
        }
        // And which CUTTERS went, which is a different question from which labels
        // went and the one a reader planning a digest is asking. `labels_hidden`
        // is label TEXTS: a multi cutter dropped at five of its nine ticks is
        // named there five times while being plainly on the figure, so it answers
        // "is DraI on this map?" with "no, five times". `sites_hidden` is the
        // enzymes the filter admitted that appear nowhere.
        if !drawn.sites_hidden.is_empty() {
            eprintln!(
                "pl: {}: {} cutter(s) are on no label: {}{}",
                path.display(),
                drawn.sites_hidden.len(),
                drawn
                    .sites_hidden
                    .iter()
                    .take(8)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", "),
                if drawn.sites_hidden.len() > 8 {
                    ", ..."
                } else {
                    ""
                }
            );
        }
        // A shortened label is not a hidden one and had no print site at all,
        // so the shortening happened silently in a figure headed for a journal.
        // `pCMV-WPRE` going out as `pCMV-WP...` is a different plasmid's name.
        if !drawn.labels_truncated.is_empty() {
            eprintln!(
                "pl: {}: {} label(s) were shortened with '...' to fit: {}{}",
                path.display(),
                drawn.labels_truncated.len(),
                drawn
                    .labels_truncated
                    .iter()
                    .take(5)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", "),
                if drawn.labels_truncated.len() > 5 {
                    ", ..."
                } else {
                    ""
                }
            );
            eprintln!("     the SVG's <title> still carries each whole name");
        }
        // The molecule's own name is not a label and has its own field, but a
        // caption cut short on a printed figure is the same class of wrongness:
        // `NC_000913.3 Escherichia coli str. K-12...` is recognisable and
        // `pCMV-WP...` is a different plasmid.
        if drawn.title_truncated {
            eprintln!(
                "pl: {}: the caption was too wide for the ring and was shortened; \
                 the <title> carries the whole name",
                path.display()
            );
        }
        // Half a feature is a worse lie than no feature: a 101 bp arrow drawn
        // from one segment of `join(100..200,5000..6000)` is indistinguishable
        // from a real 101 bp feature of that name.
        if !drawn.partly_drawn.is_empty() {
            eprintln!(
                "pl: {}: {} feature(s) were drawn from only some of their segments: {}",
                path.display(),
                drawn.partly_drawn.len(),
                drawn
                    .partly_drawn
                    .iter()
                    .take(5)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        if !drawn.malformed.is_empty() {
            eprintln!(
                "pl: {}: {} feature(s) have coordinates outside the molecule and are not drawn: {}",
                path.display(),
                drawn.malformed.len(),
                drawn.malformed.join(", ")
            );
        }

        if to_stdout {
            use std::io::Write;
            std::io::stdout()
                .write_all(&bytes)
                .map_err(|e| e.to_string())?;
            written += 1;
            continue;
        }

        let dir = destination_dir(path, &outdir);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;

        let ext = if as_eps {
            "eps"
        } else if as_pdf {
            "pdf"
        } else {
            "svg"
        };
        let stem = genbank::locus_name(&title_of(path));
        let mut dest = dir.join(format!("{stem}.{ext}"));
        // The unsuffixed destination was cleared against every input before the
        // loop started; a suffixed one has to be cleared here.
        if claimed.contains(&dest) {
            let mut k = 2;
            loop {
                let candidate = dir.join(format!("{stem}-{k}.{ext}"));
                if !claimed.contains(&candidate)
                    && collides_with_input(&candidate, &a.files).is_none()
                {
                    dest = candidate;
                    break;
                }
                k += 1;
            }
            renamed += 1;
        }
        claimed.push(dest.clone());
        std::fs::write(&dest, &bytes).map_err(|e| format!("{}: {e}", dest.display()))?;

        println!(
            "{:>10} bp  {:>8}  {:>4} label  {}",
            mol.span(),
            mol.topology.as_str(),
            drawn.labels_placed,
            dest.display()
        );
        written += 1;
    }

    if !to_stdout {
        eprintln!("\nwrote {written} map(s)");
        if renamed > 0 {
            eprintln!(
                "{renamed} output name(s) were suffixed because inputs shared a basename -- \
                 nothing was overwritten"
            );
        }
    }
    Ok(())
}

/// Search one sequence for an IUPAC motif, on both strands.
///
/// The single-molecule form of the library search, and the way the differential
/// test reaches it: `reference/python/tests/xcheck_motif.py` drives this against
/// a regex built from Biopython's own IUPAC table. Before it existed, every
/// cross-check in this project used restriction sites — and every site in the
/// shipped table is a non-degenerate palindrome, so nothing compared a
/// degenerate pattern or a minus-strand hit against an outside implementation.
fn cmd_find_motif(args: &[String]) -> Result<(), String> {
    let a = parse_args(args, &["seq", "topology", "motif"], &["json"])?;

    // The pattern is the first bare word, or `--motif`.
    let pattern = match a.get("motif") {
        Some(p) => p.to_string(),
        None => a
            .files
            .first()
            .map(|p| p.to_string_lossy().to_string())
            .ok_or("no motif given")?,
    };
    let motif =
        pl_index::scan::Motif::new(&pattern).map_err(|e| format!("--motif {pattern:?}: {e}"))?;

    // `--topology` *overrides* what the file declared; it does not stand in for
    // it. This used to default to linear for a file as well as for `--seq`, so a
    // 42 bp GenBank record whose LOCUS line says `circular` and whose only EcoRI
    // site spans bases 40,41,42,1,2,3 printed "no hits" at exit 0 -- while `pl
    // info`, `pl digest`, `pl primers` and `pl find` on the same bytes all read
    // it as a circle and all found the site. `--seq` is bare bases and carries
    // no topology, so linear stays the default there and only there.
    let asked = match a.get("topology") {
        None => None,
        Some("circular") => Some(true),
        Some("linear") => Some(false),
        Some(other) => return Err(format!("--topology {other:?}: expected circular or linear")),
    };

    // `--seq` for a literal sequence; otherwise the remaining files, whose
    // first record is used.
    let (seq, circular, label) = match a.get("seq") {
        Some(s) => (
            s.as_bytes().to_vec(),
            asked.unwrap_or(false),
            "<--seq>".to_string(),
        ),
        None => {
            let path = a
                .files
                .get(if a.get("motif").is_some() { 0 } else { 1 })
                .ok_or("give a sequence with --seq, or a file")?;
            let data = read(path)?;
            let (mol, _, report) =
                load_with_report(&data).map_err(|e| format!("{}: {e}", path.display()))?;
            note_first_record_only(&path.display().to_string(), &report, "searched");
            refuse_without_bases(&path.display().to_string(), &mol, "search")?;
            (
                mol.seq.clone(),
                asked.unwrap_or(mol.topology.is_circular()),
                path.display().to_string(),
            )
        }
    };

    let row = pl_index::Row {
        state: pl_index::State::Ok,
        topology: if circular {
            pl_index::Topology::Circular
        } else {
            pl_index::Topology::Linear
        },
        seq_off: 0,
        seq_bases: seq.len() as u64,
        length: seq.len() as u64,
        ..Default::default()
    };
    let packed = pl_index::nibble::pack(&seq);
    let hits = pl_index::scan::find_in_row(&motif, &packed, &row);

    if a.has("json") {
        let mut out = String::from("{\n");
        out.push_str(&format!("  \"motif\": {},\n", json_str(&motif.text)));
        out.push_str(&format!("  \"bp\": {},\n", seq.len()));
        out.push_str(&format!("  \"circular\": {circular},\n"));
        out.push_str(&format!("  \"palindromic\": {},\n", motif.palindromic));
        out.push_str("  \"hits\": [\n");
        for (i, h) in hits.iter().enumerate() {
            out.push_str(&format!(
                "    {{\"start\": {}, \"end\": {}, \"strand\": {}, \"wrapped\": {}}}{}\n",
                h.start,
                h.end,
                json_str(match h.strand {
                    pl_index::scan::Strand::Forward => "+",
                    pl_index::scan::Strand::Reverse => "-",
                    pl_index::scan::Strand::Both => "both",
                }),
                h.wrapped,
                if i + 1 == hits.len() { "" } else { "," }
            ));
        }
        out.push_str("  ]\n}\n");
        print!("{out}");
        return Ok(());
    }

    // The header states what was actually searched, so an empty result reads as
    // "searched and absent" rather than "did not search".
    println!(
        "{}  —  {label}, {} bp, {}",
        motif.describe(),
        seq.len(),
        if circular { "circular" } else { "linear" }
    );
    if hits.is_empty() {
        println!("\nno hits");
        return Ok(());
    }
    println!("\n{:>10}  {:>10}  {:>6}  bases", "start", "end", "strand");
    for h in &hits {
        let bases = pl_index::scan::hit_bases(&packed, &row, h, motif.len());
        println!(
            "{:>10}  {:>10}  {:>6}  {}{}",
            h.start,
            h.end,
            h.strand.as_str(),
            String::from_utf8_lossy(&bases),
            if h.wrapped {
                "   (wraps the origin)"
            } else {
                ""
            }
        );
    }
    println!("\n{} hit(s)", hits.len());
    Ok(())
}

/// Resolve where a root's index lives, honouring `--index-at`.
fn index_location(a: &Args, root: &Path) -> Result<PathBuf, String> {
    let dir = match a.get("index-at") {
        Some(d) => PathBuf::from(d),
        None => pl_scan::cache_dir()?,
    };
    Ok(pl_scan::index_path(&dir, root))
}

fn now_ns() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// Read the existing index for a root, or `None`.
///
/// A stale or damaged index is **not** an error: the file is derived, so the
/// only right response is to say so on stderr and rebuild. A *newer* index is
/// the exception — acting on it would overwrite work this build cannot
/// reproduce.
fn previous_index(path: &Path) -> Result<Option<pl_index::codec::Library>, String> {
    match pl_scan::load(path) {
        Ok(v) => Ok(v),
        Err(e) if e.rebuildable() => {
            eprintln!("pl: {}: {e}", path.display());
            Ok(None)
        }
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

fn scan_options(
    a: &Args,
    previous: Option<pl_index::codec::Library>,
) -> Result<pl_scan::ScanOptions, String> {
    let mut walk = pl_scan::WalkOptions {
        follow_links: a.has("follow-links"),
        ..Default::default()
    };
    // Refused, not shrugged off. This was `.and_then(|v| v.parse().ok())`, so an
    // unparseable value left `max_depth` at the default 32 and the run was
    // character-for-character identical to one with no `--max-depth` at all --
    // the only evidence a bound applied is the "scan incomplete: ... deeper than
    // --max-depth N" line, which never printed. `parse_args`' own rule is that a
    // typo which changes the answer must not be indistinguishable from the
    // answer, and every other numeric option here (`--min-aa`, `--agarose`,
    // `--seed`, `--mm`, ...) already refuses. It also catches `pl index root
    // --max-depth $DEPTH --rebuild` with `$DEPTH` unset, where `--rebuild` is
    // swallowed as the value and then discarded: both flags lost, exit 0. With
    // `--follow-links` the stake is larger than a wider walk -- `WalkOptions`
    // names `max_depth` the only thing standing between a link cycle and an
    // endless one.
    if let Some(v) = a.get("max-depth") {
        walk.max_depth = v
            .parse()
            .map_err(|_| format!("--max-depth {v:?}: expected a number"))?;
    }
    Ok(pl_scan::ScanOptions { walk, previous })
}

/// Print what a scan did. Never silent about what it could not do.
fn report_scan(root: &Path, r: &pl_scan::ScanReport) {
    if let Some(why) = &r.incomplete {
        eprintln!(
            "pl: scan incomplete: {why}\n    nothing was removed from the index — a folder that \
             became unreachable is not a folder whose files were deleted"
        );
    }
    for (path, err) in r.unreadable.iter().take(10) {
        eprintln!("pl: {path}: {err}");
    }
    if r.unreadable.len() > 10 {
        eprintln!("pl: ... and {} more unreadable", r.unreadable.len() - 10);
    }
    println!(
        "{}\n{:>8} files, {} records\n{:>8} parsed, {} reused, {} restamped, {} removed",
        root.display(),
        r.files_seen,
        r.records,
        r.parsed,
        r.reused,
        r.touched_only,
        r.removed
    );
}

/// Say how long a search over this library will take, when it is not instant.
///
/// Measured throughput is ~335 Mbase/s single-threaded, so the 100 ms that
/// makes a search box feel immediate is spent at roughly 33 Mbase. A real lab
/// drive comfortably exceeds that, and the user should learn it here rather
/// than by waiting for the first query.
fn report_size(lib: &pl_index::codec::Library) {
    println!("{:>8} bases indexed", lib.packed_bases);
    if lib.packed_bases > pl_scan::INTERACTIVE_BASES {
        eprintln!(
            "pl: {} bases indexed, so a motif search will take roughly {} ms.\n    \
             point 'pl index' at the folder your constructs are actually in if that matters.",
            lib.packed_bases,
            lib.packed_bases / 335_000
        );
    }
}

/// Build or refresh a folder's index.
fn cmd_index(args: &[String]) -> Result<(), String> {
    let a = parse_args(
        args,
        &["index-at", "max-depth"],
        &["rebuild", "verify", "follow-links"],
    )?;
    a.require_files()?;

    for root in &a.files {
        if !root.is_dir() {
            return Err(format!("{}: not a folder", root.display()));
        }
        let path = index_location(&a, root)?;
        let previous = if a.has("rebuild") {
            None
        } else {
            previous_index(&path)?
        };

        if a.has("verify") {
            // Re-read every file and check its stored hash. This is what makes
            // the mtime shortcut a *checkable* limitation rather than a
            // theoretical one.
            let Some(lib) = previous.as_ref() else {
                return Err(format!("{}: no index to verify", root.display()));
            };
            let mut checked = 0usize;
            let mut wrong = Vec::new();
            let mut seen: Vec<&str> = Vec::new();
            for row in &lib.rows {
                if seen.contains(&row.path.as_str()) || row.content.is_empty() {
                    continue;
                }
                seen.push(&row.path);
                checked += 1;
                match std::fs::read(pl_scan::abs(root, &row.path)) {
                    Ok(bytes) if pl_scan::content_id(&bytes) != row.content => {
                        wrong.push(row.path.clone())
                    }
                    Err(e) => wrong.push(format!("{} ({e})", row.path)),
                    _ => {}
                }
            }
            println!("{}\n{checked} file(s) re-read", root.display());
            if wrong.is_empty() {
                println!("every stored hash still matches the bytes on disk");
            } else {
                println!(
                    "{} file(s) changed without the index noticing:",
                    wrong.len()
                );
                for w in wrong.iter().take(20) {
                    println!("  {w}");
                }
                println!("run 'pl index {} --rebuild'", root.display());
            }
            continue;
        }

        let (lib, report) = pl_scan::scan(root, now_ns(), &scan_options(&a, previous)?);
        report_scan(root, &report);
        report_size(&lib);
        pl_scan::save(&path, &lib).map_err(|e| e.to_string())?;
        println!("{:>8} {}", "index:", path.display());
    }
    Ok(())
}

/// Open a library, refreshing it first if the folder has moved on.
///
/// A stale answer to a search is a wrong answer, so the changed subset is always
/// re-read before answering — and the refresh is stated rather than hidden.
fn open_library(a: &Args, root: &Path) -> Result<(pl_index::codec::Library, bool), String> {
    if a.has("no-index") {
        let (lib, report) = pl_scan::scan(root, now_ns(), &scan_options(a, None)?);
        if let Some(why) = &report.incomplete {
            eprintln!("pl: scan incomplete: {why}");
        }
        return Ok((lib, false));
    }
    let path = index_location(a, root)?;
    let previous = previous_index(&path)?;
    let had_index = previous.is_some();
    let (lib, report) = pl_scan::scan(root, now_ns(), &scan_options(a, previous)?);

    if !had_index {
        eprintln!(
            "pl: no index for {}; scanned {} file(s). run 'pl index {}' to save it.",
            root.display(),
            report.files_seen,
            root.display()
        );
    } else if report.parsed > 0 || report.removed > 0 {
        eprintln!(
            "pl: {} file(s) changed since the index was built and were re-read{}",
            report.parsed,
            if report.removed > 0 {
                format!("; {} removed", report.removed)
            } else {
                String::new()
            }
        );
    }
    if let Some(why) = &report.incomplete {
        eprintln!("pl: scan incomplete: {why}; nothing was removed");
    }
    // Best-effort: a read-only cache directory answers from memory and says so
    // rather than failing the query.
    if had_index || report.files_seen > 0 {
        if let Err(e) = pl_scan::save(&path, &lib) {
            eprintln!("pl: could not save the index ({e}); answering from memory");
        }
    }
    Ok((lib, true))
}

/// Search a library.
fn cmd_find(args: &[String]) -> Result<(), String> {
    let a = parse_args(
        args,
        &[
            "index-at",
            "max-depth",
            "motif",
            "enzyme",
            "text",
            "name",
            "topology",
            "state",
            "length",
            "features",
            "limit",
        ],
        &["absent", "no-index", "follow-links", "json"],
    )?;
    let root = a.files.first().ok_or("no folder given")?.clone();
    if !root.is_dir() {
        return Err(format!("{}: not a folder", root.display()));
    }
    // USAGE advertised `pl find <dir> [query] [filters]` and no query was ever
    // read: only `files[0]` is used, and `files[1..]` went nowhere. `pl find .
    // GAATTC` and `pl find . ZZZZZZ` -- the second not even valid IUPAC -- both
    // printed every record in the library and "1 record matched", so a search
    // written the way `pl find-motif <IUPAC> <file>` is written returned the
    // whole library as the answer, at exit 0. Refused rather than implemented:
    // `--motif` and `--enzyme` are the two ways to ask, they disagree about
    // which one a bare word would mean, and a wrong answer is worse than a
    // missing shorthand. The USAGE line no longer promises a positional query.
    if a.files.len() > 1 {
        return Err(format!(
            "pl find takes one folder and named filters, not a positional query -- {:?} was \
             read as neither and would have been dropped, leaving every record a match. \
             Use --motif {:?} for a sequence, --enzyme for a site, or --text/--name.",
            a.files[1].display(),
            a.files[1].display()
        ));
    }

    // The motif, from `--motif` or an enzyme's site.
    let motif = match (a.get("motif"), a.get("enzyme")) {
        (Some(_), Some(_)) => return Err("give --motif or --enzyme, not both".into()),
        (Some(m), None) => {
            Some(pl_index::scan::Motif::new(m).map_err(|e| format!("--motif {m:?}: {e}"))?)
        }
        (None, Some(name)) => {
            // Never fall through to treating the name as a motif: `--enzyme
            // BsaI` silently searching for the literal bases B-s-a-I would be
            // absurd.
            //
            // Both counts are computed, because the sentence that stood here was
            // hard-coded and every clause of it had gone false: it called the 58
            // entries "58 Type IIP enzymes" when 8 of them are Type IIS, and it
            // told anyone who mistyped any name at all that "there is no BsaI,
            // BsmBI, BbsI or SapI yet — use --motif GGTCTC to ask about the site
            // itself", while `--enzyme BsaI` resolved and searched, `pl digest
            // --enzyme BsaI` worked, and the whole `pl goldengate --enzyme BsaI`
            // verb shipped. The workaround it offered was not even a different
            // search: `--enzyme BsaI` hands `Motif::new` the same "GGTCTC".
            let e = pl_enzymes::by_name(name).ok_or_else(|| {
                let iis = pl_enzymes::ENZYMES
                    .iter()
                    .filter(|e| e.cuts_outside_site())
                    .count();
                format!(
                    "--enzyme {name:?}: not in the shipped table. {} enzymes are available, \
                     {iis} of them Type IIS; pl digest --enzyme lists them.",
                    pl_enzymes::ENZYMES.len()
                )
            })?;
            Some(pl_index::scan::Motif::new(e.site).map_err(|e| e.to_string())?)
        }
        (None, None) => None,
    };

    let range = |flag: &str| -> Result<(Option<u64>, Option<u64>), String> {
        let Some(v) = a.get(flag) else {
            return Ok((None, None));
        };
        let (lo, hi) = v.split_once("..").unwrap_or((v, v));
        let p = |s: &str| -> Result<Option<u64>, String> {
            if s.is_empty() {
                Ok(None)
            } else {
                s.parse()
                    .map(Some)
                    .map_err(|e| format!("--{flag} {v:?}: {e}"))
            }
        };
        Ok((p(lo)?, p(hi)?))
    };
    let (min_len, max_len) = range("length")?;
    let (min_f, max_f) = range("features")?;

    let filters = pl_index::query::Filters {
        topology: match a.get("topology") {
            Some(t) => Some(pl_index::Topology::from_name(t).ok_or_else(|| {
                format!("--topology {t:?}: expected circular, linear or undeclared")
            })?),
            None => None,
        },
        state: match a.get("state") {
            Some(s) => Some(
                pl_index::State::from_name(s)
                    .ok_or_else(|| format!("--state {s:?}: not a known state"))?,
            ),
            None => None,
        },
        min_len,
        max_len,
        min_features: min_f.map(|v| v as u32),
        max_features: max_f.map(|v| v as u32),
    };

    if a.has("absent") && motif.is_none() {
        return Err("--absent needs --motif or --enzyme: it inverts the sequence criteria".into());
    }

    let q = pl_index::query::Query {
        name: a.get("name").map(str::to_string),
        text: a.get("text").map(str::to_string),
        motif: motif.clone(),
        filters,
        absent: a.has("absent"),
    };

    let (lib, _) = open_library(&a, &root)?;
    let results = pl_index::query::run(&lib.rows, &lib.packed, &q);
    // Lenient on purpose, unlike `--max-depth` in `scan_options`, and the
    // asymmetry is the point rather than an oversight. `--max-depth` is the only
    // evidence that a walk was bounded, so a value that fails to parse erases the
    // fact that anything was skipped. `--limit` is a display cap over a result
    // set whose true size is stated either way -- the text path prints "showing
    // N of M matching records" below, and `--json` emits `matched` from the full
    // set, separately from the `matches` array -- so a bad value can only show
    // more rows than asked for, never hide a record behind a claim that it is
    // not there.
    let limit: usize = a.get("limit").and_then(|v| v.parse().ok()).unwrap_or(200);

    if a.has("json") {
        let mut out = String::from("{\n  \"matches\": [\n");
        for (i, m) in results.matches.iter().take(limit).enumerate() {
            out.push_str(&format!(
                "    {{\"path\": {}, \"record\": {}, \"name\": {}, \"length\": {}, \
                 \"topology\": {}, \"state\": {}, \"hits\": {}}}{}\n",
                json_str(&m.row.path),
                m.row.record,
                json_str(&m.row.name),
                m.row.length,
                json_str(m.row.topology.as_str()),
                json_str(m.row.state.as_str()),
                m.hits.len(),
                if i + 1 == results.matches.len().min(limit) {
                    ""
                } else {
                    ","
                }
            ));
        }
        out.push_str(&format!(
            "  ],\n  \"matched\": {},\n  \"total_hits\": {},\n  \"searched\": {},\n  \"total\": {}\n}}\n",
            results.matches.len(),
            results.total_hits,
            results.coverage.searched,
            results.coverage.total
        ));
        print!("{out}");
        return Ok(());
    }

    // The header says what was actually searched, so an empty result reads as
    // "searched and absent" rather than "did not search".
    if let Some(m) = &motif {
        println!(
            "{}{}",
            m.describe(),
            if q.absent {
                "  —  showing records WITHOUT it"
            } else {
                ""
            }
        );
        println!();
    }

    for m in results.matches.iter().take(limit) {
        let hits = if m.hits.is_empty() {
            String::new()
        } else {
            let first: Vec<String> = m
                .hits
                .iter()
                .take(4)
                .map(|h| {
                    format!(
                        "{}{}{}",
                        h.start,
                        h.strand.as_str(),
                        if h.wrapped { "~" } else { "" }
                    )
                })
                .collect();
            format!(
                "  {} hit{} at {}{}",
                m.hits.len(),
                if m.hits.len() == 1 { "" } else { "s" },
                first.join(", "),
                if m.hits.len() > 4 { ", ..." } else { "" }
            )
        };
        println!(
            "{:>10} bp  {:>10}  {}{}",
            if m.row.length > 0 {
                m.row.length
            } else {
                m.row.declared_len
            },
            m.row.topology.as_str(),
            m.row.path,
            hits
        );
    }

    println!();
    if results.matches.len() > limit {
        println!(
            "showing {limit} of {} matching records — raise --limit to see the rest",
            results.matches.len()
        );
    }
    println!(
        "{} record{} matched{}",
        results.matches.len(),
        if results.matches.len() == 1 { "" } else { "s" },
        if results.total_hits > 0 {
            format!(", {} hit(s) in total", results.total_hits)
        } else {
            String::new()
        }
    );
    if motif.is_some() {
        println!("---\n{}", results.coverage.describe());
    }
    Ok(())
}

/// What is indexed, and what could not be.
fn cmd_library(args: &[String]) -> Result<(), String> {
    let a = parse_args(
        args,
        &["index-at", "max-depth"],
        &["problems", "no-index", "follow-links", "json"],
    )?;
    let root = a.files.first().ok_or("no folder given")?.clone();
    if !root.is_dir() {
        return Err(format!("{}: not a folder", root.display()));
    }
    let (lib, _) = open_library(&a, &root)?;

    if a.has("problems") {
        let problems: Vec<&pl_index::Row> = lib
            .rows
            .iter()
            .filter(|r| !r.state.searchable() || !r.problem.is_empty())
            .collect();
        // An empty list says so out loud. Printing nothing is
        // indistinguishable from not having looked.
        if problems.is_empty() {
            println!("0 problems across {} record(s)", lib.rows.len());
            return Ok(());
        }
        for r in &problems {
            println!(
                "{:>20}  {}{}",
                r.state.as_str(),
                r.path,
                if r.problem.is_empty() {
                    String::new()
                } else {
                    format!("  —  {}", r.problem)
                }
            );
        }
        println!("\n{} of {} record(s)", problems.len(), lib.rows.len());
        return Ok(());
    }

    let mut counts: Vec<(&str, usize)> = Vec::new();
    for r in &lib.rows {
        match counts.iter_mut().find(|(s, _)| *s == r.state.as_str()) {
            Some((_, n)) => *n += 1,
            None => counts.push((r.state.as_str(), 1)),
        }
    }
    counts.sort_unstable();

    if a.has("json") {
        let mut out = format!(
            "{{\n  \"root\": {},\n  \"records\": {},\n  \"bases\": {},\n  \"complete\": {},\n  \"states\": {{\n",
            json_str(&lib.root),
            lib.rows.len(),
            lib.packed_bases,
            lib.complete
        );
        for (i, (s, n)) in counts.iter().enumerate() {
            out.push_str(&format!(
                "    {}: {n}{}\n",
                json_str(s),
                if i + 1 == counts.len() { "" } else { "," }
            ));
        }
        out.push_str("  }\n}\n");
        print!("{out}");
        return Ok(());
    }

    println!("{}", lib.root);
    println!("{:>10} record(s)", lib.rows.len());
    println!("{:>10} base(s) indexed", lib.packed_bases);
    if !lib.complete {
        println!("           the last scan did not finish; nothing was removed");
    }
    for (s, n) in &counts {
        println!("{n:>10} {s}");
    }
    Ok(())
}

/// Melting temperature for one or more oligos.
///
/// Reports a Tm and, separately, annealing advice per polymerase -- never a Tm
/// with a buffer correction folded in, because a number like that can never be
/// explained when it differs from another tool's.
fn cmd_tm(args: &[String]) -> Result<(), String> {
    let a = parse_args(args, &["table", "na", "oligo", "salt"], &["json"])?;

    let mut m = match a.get("table").unwrap_or("1998") {
        "1998" => pl_thermo::Method::default(),
        "2004" => pl_thermo::Method::santalucia_2004(),
        other => return Err(format!("--table {other:?}: expected 1998 or 2004")),
    };
    if let Some(v) = a.get("na") {
        m.na_molar = v.parse::<f64>().map_err(|e| format!("--na: {e}"))? * 1e-3;
    }
    if let Some(v) = a.get("oligo") {
        m.oligo_molar = v.parse::<f64>().map_err(|e| format!("--oligo: {e}"))? * 1e-9;
    }
    m.salt = match a.get("salt").unwrap_or("santalucia") {
        "santalucia" => pl_thermo::SaltCorrection::SantaLucia1998,
        "schildkraut" => pl_thermo::SaltCorrection::SchildkrautLifson,
        "none" => pl_thermo::SaltCorrection::None,
        other => {
            return Err(format!(
                "--salt {other:?}: expected santalucia, schildkraut or none"
            ))
        }
    };

    // Oligos come from the command line, or one per line on stdin.
    let mut seqs: Vec<String> = a
        .files
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    if seqs.is_empty() {
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)
            .map_err(|e| e.to_string())?;
        seqs = buf
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect();
    }
    if seqs.is_empty() {
        return Err("give one or more oligos, or pipe them in one per line".into());
    }

    if a.has("json") {
        for s in &seqs {
            println!("{}", pl_thermo::tm_report_line(s, &m));
        }
        return Ok(());
    }

    println!("{}", m.describe());
    println!();
    println!("{:>8}  {:>6}  {:>9}  {:>9}  oligo", "Tm", "GC%", "dH", "dS");
    let mut tms = Vec::new();
    // Counted, not cleared.
    //
    // The Err arm used to call `tms.clear()`, so a failure suppressed the advice
    // only when it was the *last* oligo: later successes repopulated the vector
    // and the "lowest Tm" was then minimised over the post-failure subset alone.
    // `pl tm ATATATATATATATAT GGGGNGGGG GGGGGGCCGGGGCCGGGG` printed a 17.8C row
    // and then advised "from the lowest Tm (69.2C)" with Phusion Ta 72C -- 50C
    // above where the AT-rich primer anneals, and contradicting a number two
    // lines above it. Deleting the middle oligo gave the correct 17.8C basis.
    let mut unevaluated = 0usize;
    for s in &seqs {
        match pl_thermo::tm(s.as_bytes(), &m) {
            Ok(t) => {
                // The length rides along with the Tm because the vendor rule
                // printed two lines below has a length clause in it, and
                // discarding the length made that clause unenforceable — see
                // the `anneal_sized` call. `s.len()` is a sound base count:
                // `pl_thermo::tm` returns `Ok` only after checking every
                // ASCII-uppercased byte is A, C, G or T, so no multi-byte
                // character can be in here.
                tms.push((t.tm, s.len()));
                println!(
                    "{:>7.1}C  {:>5.1}%  {:>9.1}  {:>9.1}  {}{}",
                    t.tm,
                    t.gc_percent,
                    t.dh,
                    t.ds,
                    s,
                    if t.self_complementary {
                        "   (self-complementary)"
                    } else {
                        ""
                    }
                );
            }
            Err(e) => {
                unevaluated += 1;
                println!(
                    "{:>8}  {:>6}  {:>9}  {:>9}  {s}  --  {e}",
                    "-", "-", "-", "-"
                );
            }
        }
    }

    // An oligo with no Tm is a hole in the set, and the lowest Tm of a set with
    // a hole in it is unknown. Withheld rather than guessed, and said out loud:
    // a missing paragraph is easy to miss, and this one decides a thermocycler
    // setting.
    if unevaluated > 0 {
        println!(
            "
no annealing advice: {unevaluated} of {} oligo(s) could not be evaluated, so the lowest Tm of \
this set is unknown. Fix or drop them and run it again.",
            seqs.len()
        );
        return Ok(());
    }

    // Annealing advice, separately and per polymerase, exactly as the plan
    // insists: a Tm is a property of a duplex, a Ta is protocol advice.
    if !tms.is_empty() {
        // Which oligo produced the minimum, not just the value. A fold to the
        // bare `f64` threw the length away one line before the rule that needs
        // it, so `pl tm ATTTAGGTGACACTATAG` — the 18 nt SP6 primer, Tm 38.9C —
        // advised "Phusion 42C" on the same printed line as the rule reserving
        // that +3 for primers over 20 nt. The tie-break on length matches
        // `anneal_sized`'s own documented rule: equal Tms pick the shorter
        // primer, which is the cooler and therefore safer answer.
        let &(low, low_nt) = tms
            .iter()
            .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
            .expect("non-empty");
        println!(
            "
annealing advice, from the lowest Tm ({low:.1}C):"
        );
        for p in pl_thermo::POLYMERASES {
            let (lo, hi) = pl_thermo::anneal_sized((low, low_nt), None, p);
            let range = if (lo - hi).abs() < 0.01 {
                format!("{lo:.0}C")
            } else {
                format!("{lo:.0}-{hi:.0}C")
            };
            println!("{:>10}  {range:<10}  {}", p.name, p.note);
        }
        println!(
            "
this is advice, not a measurement; the Tm above is the measurement"
        );
    }
    Ok(())
}

/// Check a Golden Gate overhang set, or the one a file would produce.
///
/// Reports the faults that can be found from the overhangs alone, and says
/// plainly that it is not reporting a fidelity percentage — the measured
/// ligation rates that would justify one are not shipped.
fn cmd_goldengate(args: &[String]) -> Result<(), String> {
    let a = parse_args(args, &["enzyme"], &["json"])?;

    // Either bare overhangs on the command line, or a file plus --enzyme.
    let mut overhangs: Vec<pl_clone::goldengate::Overhang> = Vec::new();
    let source;

    let bare: Vec<String> = a
        .files
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .filter(|s| !s.is_empty() && s.bytes().all(|b| pl_core::iupac::code_mask(b) != 0))
        .collect();

    if let Some(name) = a.get("enzyme") {
        let e = pl_enzymes::by_name(name)
            .ok_or_else(|| format!("--enzyme {name:?}: not in the shipped table"))?;
        if !e.cuts_outside_site() {
            return Err(format!(
                "{} is not a Type IIS enzyme: it cuts inside its own site, so every                  fragment gets the same ends and there is no overhang set to check",
                e.name
            ));
        }
        let path = a
            .files
            .iter()
            .find(|p| p.exists())
            .ok_or("give a file to digest with --enzyme")?;
        let data = read(path)?;
        let (mol, _, report) =
            load_with_report(&data).map_err(|f| format!("{}: {f}", path.display()))?;
        note_first_record_only(&path.display().to_string(), &report, "digested");
        refuse_without_bases(
            &path.display().to_string(),
            &mol,
            "digest, and so no overhang set to check",
        )?;
        let seq = String::from_utf8_lossy(&mol.seq).to_string();
        // `try_cut`, so a refusal names its own reason. `cut` returns an empty
        // Vec for a molecule that is not DNA, which then fell through to the
        // empty-overhang guard below and blamed the enzyme: a file carrying one
        // stray non-ASCII byte was told "BsaI leaves no overhang here -- 0
        // fragment(s)" about a digest that never ran at all. `CutError::NotDna`
        // names the offending character instead.
        let frags = pl_clone::try_cut(&pl_clone::Dseq::new(&seq, mol.topology.is_circular()), e)
            .map_err(|err| format!("{}: {err}", path.display()))?;
        for f in &frags {
            if let Some(o) = pl_clone::goldengate::left_overhang(f) {
                overhangs.push(o);
            }
        }
        // `check(&[])` has an early return for the empty slice, so a digest that
        // recovered no overhang at all came back with no faults and
        // `"usable": true` -- a clean bill of health for a junction set that was
        // never examined. Reproduced two ways: a file with no site for the
        // enzyme, and audit #42's own fixture `AAAAAAAAAAAAGGTCTCAC`, a linear
        // part whose BsaI site sits too close to the end for the overhang to
        // form, which after #43 stopped being cut at all and so stopped
        // reaching the Fault::Incompatible that #42 added. The printed
        // "-> 1 fragment(s)" does not distinguish the two cases -- a circular
        // molecule with one genuine BsaI junction prints exactly the same line
        // -- and `--json` never emits the fragment count at all. Refused rather
        // than noted, because exit 1 is the only channel a `--json` consumer
        // can see.
        if overhangs.is_empty() {
            return Err(format!(
                "{}: {} leaves no overhang here -- {} fragment(s), none of them with a \
                 four-base end this could check. There is no set to report on, and an empty \
                 set passes every structural check by default.",
                path.display(),
                e.name,
                frags.len()
            ));
        }
        source = format!(
            "{} cut with {} -> {} fragment(s)",
            path.display(),
            e.name,
            frags.len()
        );
    } else {
        if bare.is_empty() {
            return Err("give overhangs (e.g. AATG GCTT CAGG) or a file with --enzyme".into());
        }
        for b in &bare {
            overhangs.push(pl_clone::goldengate::Overhang {
                bases: b.to_ascii_uppercase().into_bytes(),
                five_prime: true,
            });
        }
        source = format!("{} overhang(s) given", overhangs.len());
    }

    let report = pl_clone::goldengate::check(&overhangs);

    if a.has("json") {
        let mut out = String::from(
            "{
  \"overhangs\": [",
        );
        for (i, o) in report.overhangs.iter().enumerate() {
            out.push_str(&format!("{}{}", if i > 0 { ", " } else { "" }, json_str(o)));
        }
        out.push_str(
            "],
  \"faults\": [
",
        );
        for (i, f) in report.faults.iter().enumerate() {
            out.push_str(&format!(
                "    {{\"fatal\": {}, \"detail\": {}}}{}
",
                f.is_fatal(),
                json_str(&f.to_string()),
                if i + 1 == report.faults.len() {
                    ""
                } else {
                    ","
                }
            ));
        }
        out.push_str(&format!(
            "  ],
  \"usable\": {}
}}
",
            report.is_usable()
        ));
        print!("{out}");
        return Ok(());
    }

    println!("{source}");
    println!("{}", report.overhangs.join("  "));
    println!();
    if report.faults.is_empty() {
        println!("no structural fault found");
    } else {
        for f in &report.faults {
            println!("{}  {f}", if f.is_fatal() { "STOPS  " } else { "reduces" });
        }
        println!();
        println!(
            "{}",
            if report.is_usable() {
                "the assembly should still give the intended construct, with a wrong minor product"
            } else {
                "this set will not give one construct"
            }
        );
    }
    println!(
        "
{}",
        report.caveat()
    );
    Ok(())
}

/// Where primers anneal on a template, with footprint and tail kept apart.
/// The paragraph a user pastes into a paper.
///
/// Generated from the parameters the code actually uses rather than written
/// out, so a changed default changes the text. Prose in a manual drifts from
/// the code the first time a constant moves, and the drift is invisible: the
/// sentence still reads correctly and is no longer true.
fn cmd_licences(_args: &[String]) -> Result<(), String> {
    let (db, errs) = pl_features::Db::builtin();
    // If the compiled-in table did not load cleanly, say so rather than
    // printing an attribution table computed from a partial parse.
    for e in &errs {
        eprintln!("warning: {} line {}: {}", e.file, e.line, e.problem);
    }
    println!(
        "Polylinker {} - annotation data: sources, licences and attribution\n",
        env!("CARGO_PKG_VERSION")
    );
    println!(
        "The feature database (release {}) is compiled into this program: {} record(s),\n\
         {} per-field provenance row(s). A single record legitimately mixes licences,\n\
         which is why provenance is recorded per field rather than per row.\n",
        db.version,
        db.records.len(),
        db.provenance.len()
    );

    let mut by_source: BTreeMap<(String, String), usize> = BTreeMap::new();
    for p in &db.provenance {
        *by_source
            .entry((p.source_db.clone(), p.licence.clone()))
            .or_insert(0) += 1;
    }
    println!("  {:<16} {:<36} FIELDS", "SOURCE", "LICENCE");
    for ((source, licence), n) in &by_source {
        println!("  {source:<16} {licence:<36} {n}");
    }

    println!(
        "
ATTRIBUTION

  UniProt data are (c) 2002-2024 UniProt Consortium, used under CC BY 4.0
  (https://creativecommons.org/licenses/by/4.0/). Changes were made: entries
  were resolved to a verified coding sequence and reduced to the fields above.

  Courtesy of the U.S. National Library of Medicine.
  NCBI's Disclaimer and Copyright notice:
  https://www.ncbi.nlm.nih.gov/home/about/policies/

  Nucleotide sequences come from INSDC records and carry a credit expectation to
  the original submitters; each record's accession is in its provenance row.
  ENA and Rfam are services of EMBL-EBI. Rfam is CC0 1.0, with per-family
  primary-source credit carried in each record's notes.

  The residue strings of the designed peptide parts are read out of deposited
  polymer entities of the PDB archive, which the wwPDB Usage Policy places under
  CC0 1.0 (https://www.wwpdb.org/about/usage-policies). Attribution to the
  original depositors is encouraged rather than required; each record's
  provenance row names the exact entity its residues were located in.

  This dataset is a dated snapshot and does not reflect the most current data
  available from NLM.

  The full notice, including the per-family credit table and the statement of
  changes, is features/NOTICE in the source distribution."
    );

    let reviewed = db.reviewed().records.len();
    println!(
        "
SIGN-OFF: {reviewed} of {} record(s) have been reviewed by a named curator.
`pl annotate` searches only reviewed records unless you pass --include-proposed.",
        db.records.len()
    );
    Ok(())
}

fn cmd_methods(args: &[String]) -> Result<(), String> {
    let a = parse_args(args, &[], &[])?;
    let names: Vec<String> = a.files.iter().map(|p| p.display().to_string()).collect();

    if names.is_empty() {
        println!(
            "Polylinker {} — what each operation does, and what it does not
",
            env!("CARGO_PKG_VERSION")
        );
        for t in pl_doc::TOPICS {
            println!("  {:<12} {}", t.name, t.title);
            for line in wrap(pl_doc::help(*t), 66) {
                println!("               {line}");
            }
            println!();
        }
        println!("  pl methods <topic>   the full text, for a methods section");
        return Ok(());
    }

    for (i, name) in names.iter().enumerate() {
        let t = pl_doc::topic(name).ok_or_else(|| {
            format!(
                "unknown topic {name:?}: try one of {}",
                pl_doc::TOPICS
                    .iter()
                    .map(|t| t.name)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
        if i > 0 {
            println!();
        }
        println!(
            "{}
{}
",
            t.title,
            "-".repeat(t.title.len())
        );
        for para in pl_doc::methods(t).split(
            "

",
        ) {
            for line in wrap(para, 78) {
                println!("{line}");
            }
            println!();
        }
    }
    Ok(())
}

/// Wrap on whitespace, collapsing the runs that come from a Rust string
/// continuation. Nothing clever: a methods paragraph is prose.
fn wrap(s: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut line = String::new();
    for word in s.split_whitespace() {
        if !line.is_empty() && line.len() + 1 + word.len() > width {
            out.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

fn cmd_gel(args: &[String]) -> Result<(), String> {
    let a = parse_args(
        args,
        &[
            "cut", "lane", "agarose", "ladder", "band-mm", "run-mm", "svg",
        ],
        &[],
    )?;
    a.require_files()?;

    let mut conditions = pl_gel::Conditions::default();
    if let Some(v) = a.get("agarose") {
        conditions.agarose_percent = v
            .trim_end_matches('%')
            .parse()
            .ok()
            .filter(|x: &f64| (0.3..=4.0).contains(x))
            .ok_or_else(|| format!("--agarose {v:?}: expected 0.3 to 4"))?;
    }
    if let Some(v) = a.get("run-mm") {
        conditions.run_mm = v
            .parse()
            .ok()
            .filter(|x: &f64| (10.0..=400.0).contains(x))
            .ok_or_else(|| format!("--run-mm {v:?}: expected 10 to 400"))?;
    }
    if let Some(v) = a.get("band-mm") {
        conditions.band_mm = v
            .parse()
            .ok()
            .filter(|x: &f64| (0.1..=20.0).contains(x))
            .ok_or_else(|| format!("--band-mm {v:?}: expected 0.1 to 20"))?;
    }

    let ladder_name: &str = a.get("ladder").map_or("1kb", |s| s);
    let ladder = pl_gel::ladder(ladder_name).ok_or_else(|| {
        format!(
            "--ladder {ladder_name:?}: known ladders are {}",
            pl_gel::LADDERS
                .iter()
                .map(|l| l.name)
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;

    // One lane per --lane, plus one lane for all the --cut enzymes together.
    // They go in the same tube, so their cuts merge: running each separately
    // and stacking the results is a different experiment.
    let mut lane_specs: Vec<Vec<String>> = a
        .get_all("lane")
        .iter()
        .map(|s| s.split('+').map(|e| e.trim().to_string()).collect())
        .collect();
    let cuts: Vec<String> = a.get_all("cut").iter().map(|s| s.to_string()).collect();
    if !cuts.is_empty() {
        lane_specs.push(cuts);
    }
    if lane_specs.is_empty() {
        return Err("give --cut <ENZYME> (repeatable) or --lane <A+B>".into());
    }

    let gel = pl_gel::Gel::modelled(conditions);
    // Two inputs sharing a file stem must not silently overwrite each other.
    let mut claimed: Vec<PathBuf> = Vec::new();
    let mut renamed = 0usize;
    for path in &a.files {
        let data = read(path)?;
        let (mol, _, report) =
            load_with_report(&data).map_err(|e| format!("{}: {e}", path.display()))?;
        note_first_record_only(&path.display().to_string(), &report, "run on the gel");
        refuse_without_bases(&path.display().to_string(), &mol, "digest and run")?;

        let mut lanes = vec![pl_gel::render::Lane {
            label: format!("{} ladder", ladder.name),
            sim: gel.run(ladder.sizes),
            is_ladder: true,
        }];
        let mut uncut = true;
        for spec in &lane_specs {
            let mut positions = Vec::new();
            for name in spec {
                let e =
                    pl_enzymes::by_name(name).ok_or_else(|| format!("unknown enzyme {name:?}"))?;
                let cuts = pl_enzymes::cut_positions(&mol.seq, mol.topology, e);
                if !cuts.is_empty() {
                    uncut = false;
                }
                positions.extend(cuts);
            }
            let frags = pl_enzymes::fragments_from_cuts(&positions, mol.len(), mol.topology);
            lanes.push(pl_gel::render::Lane {
                label: spec.join("+"),
                sim: gel.run(&frags),
                is_ladder: false,
            });
        }

        println!(
            "{}  {} bp {}   {}% agarose, {} ladder",
            path.display(),
            mol.len(),
            mol.topology.as_str(),
            conditions.agarose_percent,
            ladder.name
        );
        for lane in lanes.iter().filter(|l| !l.is_ladder) {
            println!("\n  {}", lane.label);
            if lane.sim.groups.is_empty() && lane.sim.bands.is_empty() {
                println!("    no fragments");
            }
            for g in &lane.sim.groups {
                println!(
                    "    {:>6.1} mm  {}{}",
                    g.mm,
                    g.sizes
                        .iter()
                        .map(u64::to_string)
                        .collect::<Vec<_>>()
                        .join(" / "),
                    if g.is_merged() {
                        "   one band — these will not separate"
                    } else {
                        ""
                    }
                );
            }
            for bp in lane.sim.too_large() {
                println!("        --      {bp}   too large for this gel to place");
            }
            for bp in lane.sim.too_small() {
                println!("        --      {bp}   too small for this gel to place");
            }
            let merged: usize = lane.sim.merged().iter().map(|g| g.sizes.len()).sum();
            if merged > 0 {
                println!(
                    "    {} fragment(s) hide in {} band(s)",
                    merged,
                    lane.sim.merged().len()
                );
            }
        }
        if uncut {
            println!("\n  none of these enzymes cuts this molecule");
        }
        println!("\n  {}", lanes[1].sim.caveat());

        if let Some(out) = a.get("svg") {
            let scene = pl_gel::render::to_scene(
                &lanes,
                &pl_gel::render::Options::default(),
                &title_of(path),
            );
            let desired = if a.files.len() > 1 {
                std::path::PathBuf::from(out).with_file_name(format!(
                    "{}.svg",
                    path.file_stem().unwrap_or_default().to_string_lossy()
                ))
            } else {
                std::path::PathBuf::from(out)
            };
            let out = claim_output(desired, path, &mut claimed, &mut renamed)?;
            std::fs::write(&out, pl_draw::svg_of(&scene).as_bytes())
                .map_err(|e| format!("{}: {e}", out.display()))?;
            println!("  -> {}", out.display());
        }
        println!();
    }
    Ok(())
}

fn cmd_annotate(args: &[String]) -> Result<(), String> {
    let a = parse_args(
        args,
        &["min-identity", "min-coverage", "code"],
        &[
            "db",
            "include-proposed",
            "no-protein",
            "fragments",
            "genbank",
        ],
    )?;

    let (all, errors) = pl_features::Db::builtin();
    for e in &errors {
        eprintln!("pl: {}:{}: {}", e.file, e.line, e.problem);
    }
    let counts = all.review_counts();
    let n_reviewed = all.reviewed().records.len();

    if a.has("db") {
        println!("feature database {}\n", all.version);
        for (status, n) in &counts {
            println!("  {:>4}  {}", n, status.as_str());
        }
        println!(
            "\n  {} record(s), {n_reviewed} of them shippable",
            all.records.len()
        );
        if n_reviewed == 0 {
            println!(
                "\n  Nothing here has been signed off. Every row was assembled by machine\n  \
                 from public sources and no human has checked one against its cited\n  \
                 accession. Writing a gene's name onto a map is an assertion, so the\n  \
                 default finds nothing until a curator reviews these rows."
            );
        }
        return Ok(());
    }
    a.require_files()?;

    // The default searches only reviewed rows, which today means nothing at
    // all. Printing "no features found" over an unapproved database would be
    // true and useless, so the reason is printed instead.
    let proposed = a.has("include-proposed");
    let db = if proposed {
        all.clone()
    } else {
        all.reviewed()
    };

    let fraction = |flag: &str| -> Result<Option<f64>, String> {
        match a.get(flag) {
            None => Ok(None),
            Some(v) => v
                .parse::<f64>()
                .ok()
                .filter(|x| (0.0..=1.0).contains(x))
                .map(Some)
                .ok_or_else(|| format!("--{flag} {v:?}: expected 0 to 1")),
        }
    };
    let mut config = pl_features::annotate::Config::default();
    if let Some(x) = fraction("min-identity")? {
        config.min_identity = x;
    }
    if let Some(x) = fraction("min-coverage")? {
        config.min_coverage = x;
    }
    config.protein = !a.has("no-protein");
    // The genetic code, exposed because it is no longer cosmetic. It decides
    // which codons open a reading frame, and the fusion rule admits a peptide
    // tag only inside one -- so a user annotating a eukaryotic construct can
    // ask for table 1's three initiators instead of table 11's seven, and a
    // user with a mitochondrial or ciliate construct can have the right stops.
    if let Some(v) = a.get("code") {
        let n: u8 = v
            .parse()
            .map_err(|_| format!("--code {v:?}: expected a GenBank transl_table number"))?;
        config.code = pl_core::translate::table(n).ok_or_else(|| {
            format!(
                "--code {n}: not an NCBI translation table. Known: {}",
                pl_core::translate::all_tables()
                    .map(|c| c.id.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
    }

    if db.records.is_empty() {
        println!(
            "no records to search: {} of {} rows are reviewed\n",
            n_reviewed,
            all.records.len()
        );
        println!(
            "  The shipped database is entirely 'proposed' — machine-assembled from\n  \
             public sources, with no human sign-off. Pass --include-proposed to search\n  \
             it anyway, and treat anything it finds as a suggestion to check."
        );
        return Ok(());
    }

    let annotator = pl_features::annotate::Annotator::new(&db, config);
    let unseedable = annotator.unseedable();
    if !unseedable.is_empty() {
        eprintln!(
            "pl: {} record(s) are too short to seed and cannot be found: {}",
            unseedable.len(),
            unseedable
                .iter()
                .map(|r| r.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    for path in &a.files {
        let data = read(path)?;
        let (mol, _, report) =
            load_with_report(&data).map_err(|e| format!("{}: {e}", path.display()))?;
        note_first_record_only(&path.display().to_string(), &report, "annotated");
        refuse_without_bases(&path.display().to_string(), &mol, "annotate")?;
        let found = annotator.annotate(&mol);
        let shown: Vec<&pl_features::annotate::Annotation> = found
            .iter()
            .filter(|f| a.has("fragments") || !f.is_fragment)
            .collect();

        if a.has("genbank") {
            let mut out = mol.clone();
            for f in &shown {
                let r = &db.records[f.record];
                let mut feat = pl_core::Feature::new(r.name.clone(), r.genbank_key.clone());
                feat.strand = f.strand;
                feat.segments = vec![pl_core::Segment::new(f.start, f.end)];
                // Provenance travels with the annotation. A map that cannot say
                // where a name came from is a map nobody can check, and an
                // unreviewed row must carry that fact into the file it lands in
                // — otherwise the caveat stops at the terminal.
                feat.qualifiers.push((
                    "note".into(),
                    Some(format!(
                        "{} {}: {:.1}% identity, {:.0}% coverage, polylinker feature db {}{}",
                        r.id,
                        if f.via_protein {
                            "protein match"
                        } else {
                            "nucleotide match"
                        },
                        f.identity * 100.0,
                        f.coverage * 100.0,
                        db.version,
                        if r.review_status == pl_features::ReviewStatus::Proposed {
                            "; PROPOSED, not reviewed by a human"
                        } else {
                            ""
                        }
                    )),
                ));
                // The evidence a peptide part was admitted on, carried into the
                // written file rather than left in the terminal. A tag called
                // by the fusion rule with no ORF drawn under it is otherwise
                // unexplainable to whoever opens the file next.
                if let Some(o) = f.fusion_orf {
                    feat.qualifiers.push((
                        "note".into(),
                        Some(format!(
                            "peptide reference, admitted because it lies in frame inside \
                             a {} aa ORF at {}..{} on the {} strand",
                            o.aa_len,
                            o.start,
                            o.end,
                            if o.strand == pl_core::Strand::Reverse {
                                "minus"
                            } else {
                                "plus"
                            }
                        )),
                    ));
                }
                out.features.push(feat);
            }
            // `write_reporting`, not `write`: the plain wrapper drops the
            // writer's `Vec<String>` of items it could not carry, and this verb
            // is one of the four call sites audit #77 recorded as still doing
            // that. Both of the reader's channels are said here too — a
            // `misc_feature join(1..10,J00194.1:200..300)` was written as
            // `misc_feature 1..10`, claiming 10 bp where the source claimed 111,
            // and a `gap(unk100)` feature disappeared, with empty stderr and
            // exit 0. `pl convert <f> --to genbank --stdout` emits a
            // byte-identical record and reported all four; only this verb was
            // silent.
            let (text, unwritable) =
                pl_fileio::genbank::write_reporting(&out, &title_of(path), today());
            note_output_losses(
                &path.display().to_string(),
                "gb",
                &unwritable,
                &report,
                &out,
                true,
            );
            print!("{text}");
            continue;
        }

        println!(
            "{}  {} bp {}",
            path.display(),
            mol.seq.len(),
            if mol.topology.is_circular() {
                "circular"
            } else {
                "linear"
            }
        );
        if shown.is_empty() {
            println!("  nothing found");
            // Bounded, because the unbounded reading is the wrong one. Until the
            // table was signed off on 2026-07-28 an empty result carried the
            // "no rows are reviewed" notice and could not be mistaken for a
            // statement about the molecule; now that it can, say what was
            // actually searched. 84 records is not the set of features that
            // exist, and a user who reads "nothing found" as "no features here"
            // has been misled by us, not by their plasmid.
            println!(
                "  ({} curated record(s) searched; this database is not \
                 comprehensive)",
                db.records.len()
            );
        }
        for f in &shown {
            let r = &db.records[f.record];
            println!(
                "  {:>7}..{:<7} {} {:<14} {:>5.1}% id  {:>4.0}% cov{}{}{}",
                f.start,
                f.end,
                if f.strand == pl_core::Strand::Reverse {
                    "-"
                } else {
                    "+"
                },
                r.name,
                f.identity * 100.0,
                f.coverage * 100.0,
                if f.is_fragment { "  fragment" } else { "" },
                if f.via_protein { "  via protein" } else { "" },
                if f.wraps_origin {
                    "  crosses origin"
                } else {
                    ""
                },
            );
            // Why a peptide part was called at all. Without this line the
            // fusion rule is correct and inexplicable: ORF display is a
            // separate feature, so a user sees a FLAG tag appear with no
            // visible protein under it and no way to check the reasoning.
            // SOURCING.md §3's stated differentiator is "a hit plus how we
            // found it", and this is the how.
            if let Some(o) = f.fusion_orf {
                println!(
                    "                              in frame with a {} aa ORF at {}..{} {}",
                    o.aa_len,
                    o.start,
                    o.end,
                    if o.strand == pl_core::Strand::Reverse {
                        "-"
                    } else {
                        "+"
                    },
                );
            }
        }
        if proposed && !shown.is_empty() {
            println!(
                "\n  These come from unreviewed rows. Check each against its source\n  \
                 before putting a name on a map: `pl annotate --db` lists the state."
            );
        }
        println!();
    }
    Ok(())
}

fn cmd_sanger(args: &[String]) -> Result<(), String> {
    let a = parse_args(
        args,
        &["ref", "ref-seq", "read", "min-quality"],
        &["circular", "all", "json"],
    )?;

    let mut p = pl_sanger::Params::default();
    if let Some(v) = a.get("min-quality") {
        p.min_quality = v
            .parse()
            .map_err(|_| format!("--min-quality {v:?}: expected 0 to 93"))?;
    }

    let (reference, circular, ref_label) = match a.get("ref-seq") {
        Some(s) => (
            s.as_bytes().to_vec(),
            a.has("circular"),
            "<--ref-seq>".into(),
        ),
        None => {
            let path = a.get("ref").ok_or("give --ref <file> or --ref-seq")?;
            let path = std::path::PathBuf::from(path);
            let data = read(&path)?;
            let (mol, _, report) =
                load_with_report(&data).map_err(|e| format!("{}: {e}", path.display()))?;
            note_first_record_only(
                &path.display().to_string(),
                &report,
                "used as the reference",
            );
            (
                mol.seq.clone(),
                mol.topology.is_circular(),
                path.display().to_string(),
            )
        }
    };

    // Reads: either sequences on the command line or chromatograms on disk.
    let mut reads: Vec<(String, Vec<u8>, Vec<u8>)> = Vec::new();
    for s in a.get_all("read") {
        reads.push((
            format!("<--read {}nt>", s.len()),
            s.as_bytes().to_vec(),
            vec![],
        ));
    }
    for path in &a.files {
        let data = read(path)?;
        let t = pl_abif::parse(&data).map_err(|e| format!("{}: {e}", path.display()))?;
        reads.push((
            path.file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_default(),
            t.sequence.clone(),
            t.quality.clone(),
        ));
    }
    if reads.is_empty() {
        return Err("give a .ab1 file, or --read".into());
    }

    if a.has("json") {
        println!("{{\n  \"reads\": [");
        for (i, (name, seq, qual)) in reads.iter().enumerate() {
            let comma = if i + 1 == reads.len() { "" } else { "," };
            match pl_sanger::compare(seq, qual, &reference, circular, &p) {
                None => println!(
                    "    {{\"name\": {}, \"placed\": false}}{comma}",
                    json_str(name)
                ),
                Some(r) => {
                    print!(
                        "    {{\"name\": {}, \"placed\": true, \"reversed\": {}, \"wrapped\": {}, \"score\": {}, \"ref_start\": {}, \"ref_end\": {}, \"identity\": {:.6}, \"differences\": [",
                        json_str(name),
                        r.reversed,
                        r.wrapped,
                        r.alignment.score,
                        r.alignment.ref_start,
                        r.alignment.ref_end,
                        r.identity
                    );
                    for (k, d) in r.discrepancies.iter().enumerate() {
                        print!(
                            "{}{{\"ref_pos\": {}, \"read_pos\": {}, \"kind\": {}, \"ref\": {}, \"read\": {}, \"quality\": {}}}",
                            if k == 0 { "" } else { ", " },
                            d.ref_pos,
                            d.read_pos,
                            json_str(match d.kind {
                                pl_sanger::Op::Mismatch => "mismatch",
                                pl_sanger::Op::Insertion => "insertion",
                                pl_sanger::Op::Deletion => "deletion",
                                pl_sanger::Op::Match => "match",
                            }),
                            json_str(&(d.ref_base as char).to_string()),
                            json_str(&(d.read_base as char).to_string()),
                            match d.quality {
                                Some(q) => q.to_string(),
                                None => "null".into(),
                            }
                        );
                    }
                    println!("]}}{comma}");
                }
            }
        }
        println!("  ]\n}}");
        return Ok(());
    }

    println!(
        "reference {ref_label}, {} bp {}\n",
        reference.len(),
        if circular { "circular" } else { "linear" }
    );

    let mut worst = 0usize;
    // A read that could not be placed is not a difference.
    //
    // `worst += 1` in this arm shared the accumulator with the discrepancy
    // count below, so the closing line reported a base difference for a read
    // where zero bases were ever compared: one perfect read plus one unplaced
    // read printed "no difference worth acting on" and then "1 difference(s)
    // not dismissible at Q20 across 2 read(s)", which reads as a mutation in
    // the clone. The --json path of this same function has always reported
    // these honestly as {"placed": false} with no differences.
    let mut unplaced = 0usize;
    for (name, seq, qual) in &reads {
        let r = match pl_sanger::compare(seq, qual, &reference, circular, &p) {
            Some(r) => r,
            None => {
                println!("{name}: could not be placed on this reference");
                unplaced += 1;
                continue;
            }
        };
        // Unknown counts here too: a file with no qualities gives no grounds
        // to dismiss anything.
        let high = r.count(pl_sanger::Confidence::High) + r.count(pl_sanger::Confidence::Unknown);
        worst += high;
        println!(
            "{name}  {} nt, {} strand, covers {}..{}{}  {:.2}% identity",
            seq.len(),
            if r.reversed { "reverse" } else { "forward" },
            r.covered.0,
            r.covered.1,
            if r.wrapped { " (crosses origin)" } else { "" },
            r.identity * 100.0
        );
        match r.reliable {
            Some((s, e)) => println!("  basecaller stands behind read bases {s}..{e}"),
            None if qual.is_empty() => println!("  no quality values in this file"),
            None => println!("  no stretch of this read is reliable"),
        }

        // High-confidence first and always; the rest only on request. Both
        // counts are printed either way, so nothing is hidden by the default.
        let show: Vec<&pl_sanger::Discrepancy> = r
            .discrepancies
            .iter()
            .filter(|d| a.has("all") || d.confidence != pl_sanger::Confidence::Low)
            .collect();
        for d in &show {
            println!(
                "  {:>8}  ref {} -> read {}   {}{}",
                d.ref_pos,
                d.ref_base as char,
                d.read_base as char,
                match d.quality {
                    Some(q) => format!("Q{q}"),
                    None => "Q?".into(),
                },
                match d.confidence {
                    pl_sanger::Confidence::High => String::new(),
                    pl_sanger::Confidence::Low => "  low confidence".into(),
                    pl_sanger::Confidence::Unknown => "  no quality".into(),
                }
            );
        }
        let low = r.count(pl_sanger::Confidence::Low);
        if r.clean() {
            println!("  no difference worth acting on");
        }
        if low > 0 && !a.has("all") {
            println!("  {low} more below Q{} — see --all", p.min_quality);
        }
        println!();
    }

    if worst > 0 {
        println!(
            "{worst} difference(s) not dismissible at Q{} across {} read(s)",
            p.min_quality,
            reads.len() - unplaced
        );
    }
    if unplaced > 0 {
        println!(
            "{unplaced} read(s) could not be placed on this reference; no bases were compared for \
             {}",
            if unplaced == 1 { "it" } else { "them" }
        );
    }
    Ok(())
}

/// An ORF's protein, in the convention a CDS record uses.
///
/// Two departures from a raw per-codon translation, both following from the
/// fact that an ORF has a *decided* beginning and end:
///
///   * The initiator becomes `M`. A ribosome starting at GTG, TTG or ATT still
///     puts methionine there; GenBank CDS records show it, and Biopython does
///     the same behind `cds=True`. `tet(A)` was being reported as starting with
///     valine.
///   * The terminal codon renders as `*`, not as its residue. That only differs
///     in tables 27, 28 and 31, where a codon is both a stop and an amino acid
///     — but there the ORF finder has already ruled that translation stops
///     here, and printing `W` for the last codon would contradict the boundary
///     the same table just drew.
fn orf_protein(code: pl_core::translate::Code, bases: &[u8], complete: bool) -> Vec<u8> {
    if !complete {
        return code.translate_cds(bases);
    }
    let body = &bases[..bases.len().saturating_sub(3)];
    let mut out = code.translate_cds(body);
    out.push(b'*');
    out
}

fn cmd_orfs(args: &[String]) -> Result<(), String> {
    let a = parse_args(
        args,
        &["table", "min-aa", "seq"],
        &[
            "tables",
            "any-start",
            "complete-only",
            "circular",
            "translate",
            "json",
        ],
    )?;

    if a.has("tables") {
        if a.has("json") {
            println!("{{\n  \"tables\": [");
            let all: Vec<_> = pl_core::translate::all_tables().collect();
            for (i, c) in all.iter().enumerate() {
                println!(
                    "    {{\"id\": {}, \"name\": {}, \"aas\": {}, \"starts\": {}}}{}",
                    c.id,
                    json_str(c.name()),
                    json_str(c.amino_acids()),
                    json_str(c.start_codons()),
                    if i + 1 == all.len() { "" } else { "," }
                );
            }
            println!("  ]\n}}");
            return Ok(());
        }
        println!("NCBI genetic codes\n");
        for c in pl_core::translate::all_tables() {
            println!(
                "  {:>2}  {:<44}{}",
                c.id,
                c.name(),
                if c.is_stop(b"TGA") {
                    ""
                } else {
                    "  TGA is not a stop"
                }
            );
        }
        return Ok(());
    }

    // 11, not 1: this is a plasmid tool, and its molecules are read in
    // bacteria. Table 11 differs only in allowing more initiation codons, so
    // defaulting to 1 would silently miss the GTG- and TTG-started markers that
    // fill the vectors people actually clone with.
    let id: u8 = match a.get("table") {
        Some(v) => v
            .parse()
            .map_err(|_| format!("--table {v:?}: expected a number"))?,
        None => 11,
    };
    let code = pl_core::translate::table(id)
        .ok_or_else(|| format!("--table {id}: no such NCBI code (try --tables)"))?;

    let mut p = pl_core::orf::Params::default();
    if let Some(v) = a.get("min-aa") {
        p.min_aa = v
            .parse()
            .map_err(|_| format!("--min-aa {v:?}: expected a number"))?;
    }
    p.require_start = !a.has("any-start");
    p.include_incomplete = !a.has("complete-only");

    let (seq, circular, label) = match a.get("seq") {
        Some(s) => (
            s.as_bytes().to_vec(),
            a.has("circular"),
            "<--seq>".to_string(),
        ),
        None => {
            let path = a.files.first().ok_or("give a file, or --seq")?;
            let data = read(path)?;
            let (mol, _, report) =
                load_with_report(&data).map_err(|e| format!("{}: {e}", path.display()))?;
            note_first_record_only(&path.display().to_string(), &report, "read for ORFs");
            refuse_without_bases(&path.display().to_string(), &mol, "read for ORFs")?;
            (
                mol.seq.clone(),
                mol.topology.is_circular(),
                path.display().to_string(),
            )
        }
    };

    let orfs = pl_core::orf::find_orfs(&seq, code, circular, &p);
    let n = seq.len();

    // Read an ORF's bases the way its coordinates say to. `start..end` always
    // runs low-to-high along the plus strand, wrapping the origin when
    // `end < start`; a reverse ORF is that same span read the other way.
    let bases_of = |o: &pl_core::orf::Orf| -> Vec<u8> {
        let span: Vec<u8> = (0..o.bases())
            .map(|j| seq[(o.start as usize - 1 + j) % n])
            .collect();
        if o.strand == pl_core::Strand::Reverse {
            pl_core::reverse_complement(&span)
        } else {
            span
        }
    };

    if a.has("json") {
        println!("{{\n  \"table\": {id},\n  \"orfs\": [");
        for (i, o) in orfs.iter().enumerate() {
            print!(
                // `bp` and `laps` are not decoration. `start..end` is an
                // inclusive span round the circle, and it stops being the ORF's
                // extent the moment `laps > 0`: a stop-free run longer than the
                // whole molecule comes back as 5..18 on a 19 bp circle for an
                // ORF of 33 bases, with `start < end`, so nothing in the record
                // looks wrong and the reader is 19 bases short with no way to
                // tell. `bases_of` above already reads `o.bases()`, which is
                // why `protein` was right while the coordinates printed beside
                // it were a whole lap short — one line disagreeing with itself.
                // Audit 2026-07-28 #6 added `Orf::laps` and `Orf::bases()` to
                // pl-core and stopped there; this is the reader they were for.
                "    {{\"start\": {}, \"end\": {}, \"bp\": {}, \"laps\": {}, \"strand\": {}, \"frame\": {}, \"aa_len\": {}, \"start_codon\": {}, \"complete\": {}, \"wrapped\": {}, \"protein\": {}}}{}",
                o.start,
                o.end,
                o.bases(),
                o.laps,
                json_str(if o.strand == pl_core::Strand::Reverse { "-" } else { "+" }),
                o.frame,
                o.aa_len,
                json_str(&String::from_utf8_lossy(&o.start_codon)),
                o.complete,
                o.wrapped,
                json_str(&String::from_utf8_lossy(&orf_protein(code, &bases_of(o), o.complete))),
                if i + 1 == orfs.len() { "" } else { "," }
            );
            println!();
        }
        println!("  ]\n}}");
        return Ok(());
    }

    println!(
        "{label}, {n} bp {}",
        if circular { "circular" } else { "linear" }
    );
    println!("table {id} — {}\n", code.name());
    if orfs.is_empty() {
        println!("  no ORF of {} aa or more", p.min_aa);
    }
    for o in &orfs {
        println!(
            "  {} {:>7}..{:<7} {:>5} aa  {}{}{}",
            if o.strand == pl_core::Strand::Reverse {
                "-"
            } else {
                "+"
            },
            o.start,
            o.end,
            o.aa_len,
            String::from_utf8_lossy(&o.start_codon),
            // A lap has to be said out loud, because the coordinates cannot say
            // it: `start..end` is an inclusive span round the circle, so an ORF
            // that runs past the origin and on past its own start reads as the
            // short arc it ends on. On a 19 bp circle a 33-base ORF printed
            // "5..18 + 10 aa", a range of 14 bases, with `start < end` so not
            // even the wrap was visible. `bases_of` above already reads
            // `o.bases()`, so the translation under `--translate` was the full
            // length while the coordinates on this line were not.
            match o.laps {
                0 if o.wrapped => "  crosses origin".to_string(),
                0 => String::new(),
                l => format!(
                    "  crosses origin, and laps the molecule {l} more time(s) — {} bp in all",
                    o.bases()
                ),
            },
            if o.complete {
                ""
            } else {
                "  no stop — runs off the end"
            }
        );
        if a.has("translate") {
            let aa = orf_protein(code, &bases_of(o), o.complete);
            for chunk in aa.chunks(60) {
                println!("      {}", String::from_utf8_lossy(chunk));
            }
        }
    }
    if !orfs.is_empty() {
        println!("\n  {} ORF(s)", orfs.len());
    }
    if !code.is_stop(b"TGA") {
        println!("  note: table {id} reads TGA as an amino acid, not a stop");
    }
    for c in [b"TAA", b"TAG", b"TGA"] {
        if code.is_ambiguous_stop(c) {
            println!(
                "  note: {} is both a stop and {} in table {id} — where it \
                 terminates depends on context this tool does not have",
                String::from_utf8_lossy(c),
                code.codon(c) as char
            );
        }
    }
    // A frame with no stop anywhere on a circle has no ORF to report, because
    // every start in it is equally first. Say so rather than leave a silent gap.
    for (st, f) in pl_core::stopless_frames(&seq, code, circular) {
        println!(
            "  note: frame {}{f} has no stop codon anywhere on this circle",
            if st == pl_core::Strand::Reverse {
                "-"
            } else {
                "+"
            }
        );
    }
    Ok(())
}

fn cmd_primers(args: &[String]) -> Result<(), String> {
    let a = parse_args(
        args,
        &["seed", "primer", "seq"],
        &["circular", "seed-mismatch", "exact", "json"],
    )?;

    let mut params = pl_primer::Params::default();
    if let Some(v) = a.get("seed") {
        params.seed_len = v
            .parse()
            .ok()
            .filter(|n| (8..=35).contains(n))
            .ok_or_else(|| format!("--seed {v:?}: expected 8 to 35"))?;
    }
    params.seed_mismatch = a.has("seed-mismatch");
    // `--exact` is the pydna/SnapGene rule: the footprint stops at the first
    // mismatch rather than extending through isolated ones.
    params.extend_mismatches = !a.has("exact");

    let primers: Vec<String> = a.get_all("primer").iter().map(|s| s.to_string()).collect();
    if primers.is_empty() {
        return Err("give at least one --primer".into());
    }

    let (seq, circular, label) = match a.get("seq") {
        Some(s) => (
            s.as_bytes().to_vec(),
            a.has("circular"),
            "<--seq>".to_string(),
        ),
        None => {
            let path = a.files.first().ok_or("give a template file, or --seq")?;
            let data = read(path)?;
            let (mol, _, report) =
                load_with_report(&data).map_err(|e| format!("{}: {e}", path.display()))?;
            note_first_record_only(&path.display().to_string(), &report, "used as the template");
            refuse_without_bases(&path.display().to_string(), &mol, "anneal a primer to")?;
            (
                mol.seq.clone(),
                mol.topology.is_circular(),
                path.display().to_string(),
            )
        }
    };

    if a.has("json") {
        println!(
            "{{
  \"bindings\": ["
        );
        let mut first = true;
        for pr in &primers {
            for b in pl_primer::find_bindings(pr.as_bytes(), &seq, circular, &params) {
                if !first {
                    println!(",");
                }
                first = false;
                print!(
                    "    {{\"primer\": {}, \"start\": {}, \"end\": {}, \"strand\": {},                      \"footprint\": {}, \"tail\": {}, \"mismatches\": {}}}",
                    json_str(pr),
                    b.start,
                    b.end,
                    json_str(b.strand.as_str()),
                    json_str(&b.footprint_str()),
                    json_str(&b.tail_str()),
                    b.mismatches.len()
                );
            }
        }
        println!(
            "
  ]
}}"
        );
        return Ok(());
    }

    println!(
        "{label}, {} bp {}",
        seq.len(),
        if circular { "circular" } else { "linear" }
    );
    println!(
        "3'-anchored seed of {} bases{}
",
        params.seed_len,
        if params.seed_mismatch {
            ", one mismatch allowed (never at the 3' end)"
        } else {
            ", exact"
        }
    );
    let mut any = false;
    for pr in &primers {
        let bindings = pl_primer::find_bindings(pr.as_bytes(), &seq, circular, &params);
        println!("{pr}  ({} nt)", pr.len());
        if bindings.is_empty() {
            println!("     no binding site");
            continue;
        }
        any = true;
        for b in &bindings {
            let tm = match b.tm {
                Some(t) => format!("{t:5.1}C"),
                None => "    -".into(),
            };
            println!(
                "  {} {:>7}..{:<7} {tm}  footprint {}{}{}",
                b.strand.as_str(),
                b.start,
                b.end,
                b.footprint_str(),
                if b.has_tail() {
                    format!("   tail {}", b.tail_str())
                } else {
                    String::new()
                },
                if b.mismatches.is_empty() {
                    String::new()
                } else {
                    format!("   {} mismatch(es)", b.mismatches.len())
                }
            );
        }
        if bindings.len() > 1 {
            println!(
                "     {} sites: this primer is not specific to one place",
                bindings.len()
            );
        }
    }
    if any {
        println!(
            "
Tm is over the annealed footprint only; a 5' tail never contributes to it"
        );
    }
    Ok(())
}

/// Pick a primer pair for a region.
///
/// Every numeric option is validated with a **positive** test —
/// `v.is_finite() && (lo..=hi).contains(&v)` — never `!(v <= 0.0)`. NaN fails
/// every comparison and slipped past exactly that guard in `pl-thermo`, where
/// `--na nan` parsed, failed `NaN > 0.0`, and printed the 1 M number under a
/// method line reading "0 mM Na+".
fn cmd_design(args: &[String]) -> Result<(), String> {
    let a = parse_args(
        args,
        &[
            "region",
            "seq",
            "mode",
            "flank",
            "len",
            "len-opt",
            "tm",
            "tm-opt",
            "tm-diff",
            "gc",
            "gc-clamp",
            "max-poly",
            "product",
            "product-opt",
            "max",
            "add-5",
            "add-3",
            "spacer",
            "vector",
            "off-seed",
            "table",
            "na",
            "oligo",
            "salt",
        ],
        &[
            "circular",
            "rt",
            "gc-hard",
            "no-specificity",
            "json",
            "vector-circular",
            "dam-",
            "dcm-",
            "cpg",
        ],
    )?;

    // The Tm knobs, spelled exactly as `pl tm` spells them. Two spellings of
    // one model is how a user ends up unable to reproduce their own number.
    let mut c = pl_design::Constraints {
        tm_method: match a.get("table").unwrap_or("1998") {
            "1998" => pl_thermo::Method::default(),
            "2004" => pl_thermo::Method::santalucia_2004(),
            other => return Err(format!("--table {other:?}: expected 1998 or 2004")),
        },
        ..Default::default()
    };
    if let Some(v) = a.get("na") {
        c.tm_method.na_molar = number(v, "--na", 0.000_001, 5_000.0)? * 1e-3;
    }
    if let Some(v) = a.get("oligo") {
        c.tm_method.oligo_molar = number(v, "--oligo", 0.000_001, 1e9)? * 1e-9;
    }
    c.tm_method.salt = match a.get("salt").unwrap_or("santalucia") {
        "santalucia" => pl_thermo::SaltCorrection::SantaLucia1998,
        "schildkraut" => pl_thermo::SaltCorrection::SchildkrautLifson,
        "none" => pl_thermo::SaltCorrection::None,
        other => {
            return Err(format!(
                "--salt {other:?}: expected santalucia, schildkraut or none"
            ))
        }
    };

    // `--rt` is applied BEFORE the explicit options, so an explicit --product
    // or --tm still wins. A preset that overrode what the user typed would be
    // the worst of both.
    if a.has("rt") {
        c = c.rt_pcr();
    }

    if let Some(v) = a.get("mode") {
        c.mode = pl_design::Mode::parse(v)
            .ok_or_else(|| format!("--mode {v:?}: expected contain or within"))?;
    }
    if let Some(v) = a.get("flank") {
        c.flank = number(v, "--flank", 0.0, 100_000.0)? as u64;
    }
    if let Some(v) = a.get("len") {
        // From the constants, not from 8.0/60.0 written out again. pl-design's
        // widening advice now NAMES this lower bound as what the tool accepts
        // ("--len accepts down to 8"), so a literal here and a constant there
        // are two spellings of one contract that can drift apart silently — and
        // the way it would show is advice telling the user to pass a value this
        // parser then rejects.
        let (lo, hi) = range(
            v,
            "--len",
            pl_design::Constraints::LEN_HARD_MIN as f64,
            pl_design::Constraints::LEN_HARD_MAX as f64,
        )?;
        c.len_min = lo as usize;
        c.len_max = hi as usize;
        c.len_opt = c.len_opt.clamp(c.len_min, c.len_max);
    }
    if let Some(v) = a.get("len-opt") {
        c.len_opt = number(v, "--len-opt", c.len_min as f64, c.len_max as f64)? as usize;
    }
    if let Some(v) = a.get("tm") {
        let (lo, hi) = range(v, "--tm", -50.0, 150.0)?;
        c.tm_min = lo;
        c.tm_max = hi;
        c.tm_opt = (c.tm_min + c.tm_max) / 2.0;
    }
    if let Some(v) = a.get("tm-opt") {
        c.tm_opt = number(v, "--tm-opt", c.tm_min, c.tm_max)?;
    }
    if let Some(v) = a.get("tm-diff") {
        c.tm_diff_max = number(v, "--tm-diff", 0.0, 100.0)?;
    }
    if let Some(v) = a.get("gc") {
        let (lo, hi) = range(v, "--gc", 0.0, 100.0)?;
        c.gc_min = lo;
        c.gc_max = hi;
    }
    c.gc_hard = a.has("gc-hard");
    if let Some(v) = a.get("gc-clamp") {
        let (lo, hi) = range(v, "--gc-clamp", 0.0, 5.0)?;
        c.gc_clamp_min = lo as usize;
        c.gc_clamp_max = hi as usize;
    }
    if let Some(v) = a.get("max-poly") {
        c.max_poly = number(v, "--max-poly", 1.0, 30.0)? as usize;
        c.max_poly_g = c.max_poly_g.min(c.max_poly);
    }
    if let Some(v) = a.get("product") {
        let (lo, hi) = range(v, "--product", 20.0, 100_000.0)?;
        c.product_min = lo as u64;
        c.product_max = hi as u64;
    }
    if let Some(v) = a.get("product-opt") {
        c.product_target = Some(number(v, "--product-opt", 20.0, 100_000.0)? as u64);
    }
    // A target outside the window is kept and DISCLOSED rather than dropped.
    //
    // This used to be a `.filter()` on the `--product` arm above, which could
    // only ever prune the `--rt` preset's target: argument order in this
    // function is fixed, so a user's `--product-opt` was always read afterwards
    // and never saw it. Two sources of the same field, one silently pruned and
    // one not.
    //
    // Dropping the filter rather than moving it, because pl-design now says
    // what it was hiding: `Constraints::describe()` prints
    // "target N bp OUTSIDE that window", `Report::warnings` names it, and the
    // size term still ranks monotonically, so an out-of-window target degrades
    // to a visible preference instead of vanishing. Moving the filter down here
    // would have made the two sources consistent by silencing both.
    // bins/pl-gui/src/design.rs clamps in the panel, where the user can see the
    // number move.
    if let Some(v) = a.get("max") {
        c.max_pairs = number(v, "--max", 1.0, 200.0)? as usize;
    }
    if let Some(v) = a.get("off-seed") {
        c.off_seed = number(v, "--off-seed", 8.0, 32.0)? as usize;
    }
    c.specificity = !a.has("no-specificity");

    let spacer = a.get("spacer").unwrap_or("").as_bytes().to_vec();
    if let Some(b) = spacer
        .iter()
        .find(|b| !matches!(b.to_ascii_uppercase(), b'A' | b'C' | b'G' | b'T'))
    {
        return Err(format!(
            "--spacer contains {:?}, which is not a DNA base. A tail is real DNA that has to \
             be ordered.",
            *b as char
        ));
    }
    for (flag, slot) in [("add-5", 0usize), ("add-3", 1usize)] {
        let Some(name) = a.get(flag) else { continue };
        let e = pl_enzymes::by_name(name).ok_or_else(|| {
            format!(
                "--{flag} {name:?}: not in the shipped table. {} enzymes are available; \
                 pl digest --enzyme lists them.",
                pl_enzymes::ENZYMES.len()
            )
        })?;
        let spec = pl_design::params::Tailspec {
            enzyme: e,
            spacer: spacer.clone(),
        };
        if slot == 0 {
            c.tail_five = Some(spec);
        } else {
            c.tail_three = Some(spec);
        }
    }

    // A flag that silently does nothing is a defect here for the same reason an
    // unknown flag is (see `parse_args`). Everything `--vector` produces comes
    // out of the `[tail_five, tail_three]` loop at the end of this function, so
    // with neither `--add-5` nor `--add-3` given, the vector was read, parsed
    // and put through both no-bases gates -- and then never mentioned: `pl
    // design t.fa --region 400..1000 --vector pUC19.gb` printed a full report in
    // which the word "vector" did not occur once, exit 0, and `--json`'s
    // `warnings` carried nothing about it either. Because the gates run first,
    // an unusable vector errored loudly while a usable one produced total
    // silence, so the silence read as a clean bill of health rather than as an
    // ignored flag. `--spacer` is inert the same way: validated as DNA, then
    // discarded.
    if c.tail_five.is_none() && c.tail_three.is_none() {
        if a.has("vector") {
            return Err(
                "--vector needs --add-5 or --add-3: what is counted in the vector is \
                        the sites those flags add, and with neither there is nothing to count"
                    .into(),
            );
        }
        if a.has("spacer") {
            return Err(
                "--spacer needs --add-5 or --add-3: a spacer is the bases 5' of an \
                        added site, and with neither flag no site is added"
                    .into(),
            );
        }
    }
    for flag in ["vector-circular", "dam-", "dcm-", "cpg"] {
        if a.has(flag) && !a.has("vector") {
            return Err(format!(
                "--{flag} needs --vector: it says how to read the vector, and no vector was given"
            ));
        }
    }

    let region_text = a
        .get("region")
        .ok_or("give --region A..B, the target to amplify")?;
    let (rs, re) = region_text
        .split_once("..")
        .ok_or_else(|| format!("--region {region_text:?}: expected A..B"))?;
    let region = pl_design::Region::new(
        rs.trim()
            .parse::<u64>()
            .map_err(|e| format!("--region: {e}"))?,
        re.trim()
            .parse::<u64>()
            .map_err(|e| format!("--region: {e}"))?,
    );

    // The template, and the two no-bases files refused in their own terms by
    // pl-design rather than by a sentence restated here.
    let (mol, label) = match a.get("seq") {
        Some(s) => (
            pl_core::Molecule {
                name: "<--seq>".into(),
                seq: s.as_bytes().to_vec(),
                topology: if a.has("circular") {
                    pl_core::Topology::Circular
                } else {
                    pl_core::Topology::Linear
                },
                ..Default::default()
            },
            "<--seq>".to_string(),
        ),
        None => {
            let path = a.files.first().ok_or("give a template file, or --seq")?;
            let data = read(path)?;
            let (mol, _, report) =
                load_with_report(&data).map_err(|e| format!("{}: {e}", path.display()))?;
            note_first_record_only(&path.display().to_string(), &report, "used as the template");
            (mol, path.display().to_string())
        }
    };

    // Said BEFORE the scan, not after it. The off-target check is what this
    // tool does that a designer deferring specificity to BLAST cannot, and on
    // a bacterial chromosome it is also the whole runtime: measured, 3 s at
    // 500 kb, 17 s at 1 Mb, 65 s at 2 Mb of random sequence and about five and
    // a half minutes on a real 9.1 Mb chromosome. The GUI refuses templates
    // over 200 kb and sends the user here, so here is where the cost has to be
    // stated. It is not lowered by shortening the seed -- see
    // `Constraints::OFF_SEED` for why raising it would buy the time by making
    // the answer worse -- so the honest move is to name the knobs.
    if c.specificity && mol.seq.len() as u64 > pl_design::Constraints::SCAN_NOTICE_BP {
        eprintln!(
            "pl: {} bp template: the off-target scan is the slow part here and grows faster \
             than the template -- measured on random sequence, 3 s at 500 kb, 17 s at 1 Mb, \
             65 s at 2 Mb, about 5 minutes at 9 Mb. --no-specificity skips it and the report \
             says so; a smaller --flank or a narrower --region enumerates fewer candidates.",
            mol.seq.len()
        );
    }

    let mut r = pl_design::design_molecule(&mol, region, &c).map_err(|e| e.to_string())?;

    // A vector, if one was given, is a separate molecule and gets a separate
    // line. "Absent from the product, present 3x in the vector" is a different
    // problem from an internal site and collapsing the two makes it unfixable.
    //
    // Methylation is asked about the VECTOR only, and is now actually asked.
    // A PCR product is synthesised from dNTPs in vitro and is unmethylated, so
    // running the Dam/Dcm rules over it would spuriously reject ClaI, XbaI and
    // the rest for a molecule they cut perfectly -- `tail.rs` says why. A
    // vector, though, came out of a miniprep, and an ordinary lab strain is
    // dam+ dcm+. Until a reviewer checked, this block called `cut_positions`
    // and nothing else while two comments claimed methylation was applied
    // here: a ClaI site sitting inside GATCGATC was reported as "1 time --
    // which is what you want: that is where the insert goes", and the user
    // linearises a dam+ prep, gets no cut, and loses a week.
    if let Some(v) = a.get("vector") {
        let data = read(Path::new(v))?;
        let (mut vec_mol, _, vec_report) =
            load_with_report(&data).map_err(|e| format!("{v}: {e}"))?;
        // Refused, not noted. The load report was bound to `_` here -- the only
        // `_`-bound one left in this binary -- so a two-record `multi.fa` whose
        // first record has no EcoRI site and whose second has three was judged
        // on record 1 and the verdict printed as a statement about the file:
        // "EcoRI reads 0 sites in multi.fa (2000 bp, read as LINEAR) and cuts 0
        // of them -- so it cannot open this vector", exit 0, empty stderr, and
        // the same sentence inside `--json`'s `warnings[]` where a stderr note
        // could not be seen at all. Passing the same file as template and
        // vector in one command reported it honestly once and silently once.
        // `note_first_record_only` is not enough here the way it is for
        // `digest`: "which backbone" is not a question record 1 can answer, so
        // this refuses the way `convert` and `export` refuse a multi-record
        // input.
        if vec_report.truncated() {
            return Err(format!(
                "--vector {v}: holds {} records and only the first would be scanned. \
                 Which record is the backbone is not something this can guess -- split the \
                 file and name the one you mean.",
                vec_report.records
            ));
        }
        // The same two no-bases gates the template gets, in the same words --
        // an annotation track scored as "cuts 0 times -- so it cannot open
        // this vector" is a verdict derived from zero bases, and a user
        // comparing backbones eliminates one on it.
        if vec_mol.is_annotation_track() {
            return Err(format!(
                "--vector {v}: {}",
                pl_design::DesignError::AnnotationTrack {
                    features: vec_mol.features.len(),
                }
            ));
        }
        if vec_mol.sequence_absent() {
            return Err(format!(
                "--vector {v}: {}",
                pl_design::DesignError::SequenceAbsent {
                    declared: vec_mol.declared_len.unwrap_or(0),
                }
            ));
        }
        // FASTA carries no topology, so a site straddling a plasmid's origin
        // read as linear is reported as absent -- measured: a 2,400 bp vector
        // whose only EcoRI site spans base 1 came back "cuts 0 times".
        // `--vector-circular` says so explicitly, and the line below states
        // which topology was assumed, the way `pl digest`'s header does.
        if a.has("vector-circular") {
            vec_mol.topology = pl_core::Topology::Circular;
        }
        let meth = pl_core::Methylation {
            dam: !a.has("dam-"),
            dcm: !a.has("dcm-"),
            ecoki: false,
            // Not a flag any container carries, and not the ordinary case for
            // a plasmid grown in E. coli, so it is opt-in.
            cpg: a.has("cpg"),
        };
        for spec in [&c.tail_five, &c.tail_three].into_iter().flatten() {
            // `cut_sites` rather than `cut_positions`, because methylation is a
            // question about the recognition site and recovering the site
            // start back from the cut is the arithmetic that was already wrong
            // once in the GUI, on origin-straddling sites.
            let sites = pl_enzymes::cut_sites(&vec_mol.seq, vec_mol.topology, spec.enzyme);
            let mut affected: Vec<(u64, pl_enzymes::methylation::SiteEffect)> = Vec::new();
            for s in &sites {
                if let Some(e) = pl_enzymes::methylation::site_effect(
                    spec.enzyme,
                    &vec_mol.seq,
                    (s.site_start - 1) as usize,
                    vec_mol.topology,
                    &meth,
                ) {
                    affected.push((s.site_start, e));
                }
            }
            affected.sort_by_key(|(p, _)| *p);
            affected.dedup_by_key(|(p, _)| *p);
            let cuts = sites.len();
            let dead = affected
                .iter()
                .filter(|(_, e)| e.effect == pl_enzymes::methylation::Effect::Blocked)
                .count();
            // The verdict is about the sites that will actually be cut. A
            // blocked site is not "where the insert goes"; it is a site that
            // does nothing.
            let live = cuts.saturating_sub(dead);
            let mut line = format!(
                "{} reads {} site{} in {} ({} bp, {}) and cuts {} of them -- {}",
                spec.enzyme.name,
                cuts,
                if cuts == 1 { "" } else { "s" },
                v,
                vec_mol.seq.len(),
                if vec_mol.topology.is_circular() {
                    "circular".to_string()
                } else {
                    "read as LINEAR; pass --vector-circular if it is a plasmid".to_string()
                },
                live,
                match live {
                    0 => "so it cannot open this vector".to_string(),
                    1 => "which is what you want: that is where the insert goes".to_string(),
                    n => format!("so the digest fragments the vector into {n} pieces"),
                }
            );
            for (pos, e) in &affected {
                line.push_str(&format!(
                    ". The site at {pos} is {} by {} methylation in the dam{} dcm{} prep \
                     assumed here -- grow the vector in a dam-/dcm- strain, or pass \
                     --dam-/--dcm- if this prep already is",
                    e.effect.as_str(),
                    e.methylase.name(),
                    if meth.dam { "+" } else { "-" },
                    if meth.dcm { "+" } else { "-" },
                ));
            }
            r.warnings.push(line);
        }
    }

    if a.has("json") {
        print!("{}", r.json(&label));
    } else {
        print!("{}", r.text(&label));
    }
    Ok(())
}

/// A finite number inside a range, refused positively.
fn number(v: &str, flag: &str, lo: f64, hi: f64) -> Result<f64, String> {
    let x: f64 = v.trim().parse().map_err(|e| format!("{flag} {v:?}: {e}"))?;
    // A positive test, not `!(x < lo)`: NaN fails every comparison and would
    // slip through the negation.
    if x.is_finite() && (lo..=hi).contains(&x) {
        Ok(x)
    } else {
        Err(format!("{flag} {v:?}: expected a number from {lo} to {hi}"))
    }
}

fn range(v: &str, flag: &str, lo: f64, hi: f64) -> Result<(f64, f64), String> {
    let (a, b) = v
        .split_once("..")
        .ok_or_else(|| format!("{flag} {v:?}: expected MIN..MAX"))?;
    let a = number(a, flag, lo, hi)?;
    let b = number(b, flag, lo, hi)?;
    if a > b {
        return Err(format!("{flag} {v:?}: {a} is greater than {b}"));
    }
    Ok((a, b))
}

/// Read a Sanger chromatogram.
fn cmd_trace(args: &[String]) -> Result<(), String> {
    let a = parse_args(args, &["svg", "bases", "width"], &["accessible", "json"])?;
    a.require_files()?;
    let mut claimed: Vec<PathBuf> = Vec::new();
    let mut renamed = 0usize;
    for path in &a.files {
        let data = read(path)?;
        let t = match pl_abif::parse(&data) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("pl: {}: {e}", path.display());
                continue;
            }
        };
        if let Some(out) = a.get("svg") {
            let mut o = pl_draw::trace::Options {
                palette: if a.has("accessible") {
                    pl_draw::trace::Palette::Accessible
                } else {
                    pl_draw::trace::Palette::Classic
                },
                ..Default::default()
            };
            if let Some(w) = a.get("width") {
                o.width = w
                    .parse()
                    .map_err(|_| format!("--width {w:?}: expected a number"))?;
            }
            if let Some(r) = a.get("bases") {
                let (s, e) = r
                    .split_once("..")
                    .ok_or_else(|| format!("--bases {r:?}: expected FIRST..LAST"))?;
                o.bases = Some((
                    s.parse().map_err(|_| format!("--bases {r:?}"))?,
                    e.parse().map_err(|_| format!("--bases {r:?}"))?,
                ));
            }
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            let view = pl_draw::trace::View {
                channels: [
                    &t.channels[0],
                    &t.channels[1],
                    &t.channels[2],
                    &t.channels[3],
                ],
                base_order: t.base_order,
                peaks: &t.peaks,
                sequence: &t.sequence,
                quality: &t.quality,
                title: &name,
            };
            let (scene, rep) = view.to_scene(&o);
            let svg = pl_draw::svg_of(&scene);
            // With several inputs, one `--svg` path would overwrite itself; a
            // silent last-writer-wins is the wrong answer to that.
            let desired = if a.files.len() > 1 {
                std::path::PathBuf::from(out).with_file_name(format!(
                    "{}.svg",
                    path.file_stem().unwrap_or_default().to_string_lossy()
                ))
            } else {
                std::path::PathBuf::from(out)
            };
            let out = claim_output(desired, path, &mut claimed, &mut renamed)?;
            std::fs::write(&out, svg.as_bytes()).map_err(|e| format!("{}: {e}", out.display()))?;
            println!(
                "{} -> {}  ({} bases, {} samples{})",
                path.display(),
                out.display(),
                rep.bases_drawn,
                rep.samples,
                if rep.decimated_to > 0 {
                    format!(", decimated to {} points", rep.decimated_to)
                } else {
                    String::new()
                }
            );
            for n in &rep.notes {
                println!("  note: {n}");
            }
            continue;
        }
        if a.has("json") {
            println!(
                "{{\"file\": {}, \"sequence\": {}, \"length\": {}, \"edited\": {},                  \"mean_quality\": {}, \"sample\": {}}}",
                json_str(&path.display().to_string()),
                json_str(&String::from_utf8_lossy(&t.sequence)),
                t.sequence.len(),
                t.edited(),
                match t.mean_quality() {
                    // Full precision: rounding here made a differential
                    // against Biopython report 292 disagreements that were
                    // entirely this format string.
                    Some(q) => format!("{q}"),
                    None => "null".into(),
                },
                json_str(&t.sample_name)
            );
            continue;
        }
        println!("{}", path.display());
        println!(
            "{:>10} bases   {:>4} ambiguous   {}",
            t.sequence.len(),
            t.ambiguous(),
            match t.mean_quality() {
                Some(q) => format!("mean quality {q:.1}"),
                None => "no quality in this file".into(),
            }
        );
        if !t.sample_name.is_empty() {
            println!("{:>10}   {}", "sample", t.sample_name);
        }
        // A human's correction is a fact the file carries, and it differs from
        // the machine's call in most real traces. Showing one and hiding the
        // other is how a user reads a sequence nobody meant them to read.
        match (t.edited(), t.edit_distance()) {
            (true, Some(n)) => println!(
                "{:>10}   a human edited {n} base(s); the machine's call is shown",
                "edited"
            ),
            (true, None) => println!(
                "{:>10}   a human's version differs in length from the machine's",
                "edited"
            ),
            _ => {}
        }
        println!("{}", String::from_utf8_lossy(&t.sequence));
        println!();
    }
    Ok(())
}

/// Emit digest fragments for the differential test against pydna.
///
/// Reads `id \t enzyme \t topology \t sequence` and writes one JSON object per
/// line with every fragment's watson, crick and overhang. All three are
/// reported because a fragment can have the right length and the wrong end
/// shape, and only the overhang shows it.
fn cmd_cut_adapter(_args: &[String]) -> Result<(), String> {
    let mut input = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut input).map_err(|e| e.to_string())?;

    for line in input.lines() {
        let f: Vec<&str> = line.trim_end().split('\t').collect();
        if f.len() < 4 {
            continue;
        }
        let (id, enzyme, topology, seq) = (f[0], f[1], f[2], f[3]);
        let Some(e) = pl_enzymes::by_name(enzyme) else {
            println!(
                "{{{}: {}, {}: []}}",
                json_str("id"),
                json_str(id),
                json_str("fragments")
            );
            continue;
        };
        let d = pl_clone::Dseq::new(seq, topology == "circular");
        // A refusal is reported, not spelled the same as "this enzyme does not
        // cut". `cut` hands back an empty Vec when the molecule is not DNA, and
        // this adapter printed that as `"fragments": []` — indistinguishable
        // from a genuine non-cutter, in the one output whose whole job is to be
        // compared against pydna. The success shape is untouched, so
        // `reference/python/tests/xcheck_clone.py` reads exactly what it read
        // before; the `error` key appears only on a case that could not run.
        let cut = match pl_clone::try_cut(&d, e) {
            Ok(f) => f,
            Err(err) => {
                println!(
                    "{{{}: {}, {}: {}, {}: []}}",
                    json_str("id"),
                    json_str(id),
                    json_str("error"),
                    json_str(&err.to_string()),
                    json_str("fragments")
                );
                continue;
            }
        };
        let frags: Vec<String> = cut
            .iter()
            .map(|fr| {
                format!(
                    "{{{}: {}, {}: {}, {}: {}}}",
                    json_str("watson"),
                    json_str(&fr.watson),
                    json_str("crick"),
                    json_str(&fr.crick),
                    json_str("ovhg"),
                    fr.ovhg
                )
            })
            .collect();
        println!(
            "{{{}: {}, {}: [{}]}}",
            json_str("id"),
            json_str(id),
            json_str("fragments"),
            frags.join(", ")
        );
    }
    Ok(())
}

/// The polylinker-bench adapter.
///
/// The bench is meant to be run against any tool, so the interface it asks of a
/// tool is deliberately the smallest thing that works: tab-separated lines in,
/// tab-separated lines out, no JSON parser required on this side. Anyone
/// wanting to score SnapGene, Benchling or UGENE writes the same twenty lines.
///
/// ```text
/// pl bench-adapter --capabilities        -> the operations this tool answers
/// pl bench-adapter                       -> reads cases on stdin, answers on stdout
/// ```
///
/// Each input line is `id \t operation \t topology \t sequence [\t key=value]...`
/// and each output line is `id \t key=value...`, or `id \t unsupported` for a
/// case this tool cannot attempt. Saying *unsupported* is a first-class answer:
/// a benchmark that lets a tool quietly skip what it cannot do measures nothing.
fn cmd_bench_adapter(args: &[String]) -> Result<(), String> {
    let a = parse_args(args, &[], &["capabilities"])?;

    if a.has("capabilities") {
        println!("identity");
        println!("digest");
        println!("pcr");
        println!("assembly");
        return Ok(());
    }

    let mut input = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut input).map_err(|e| e.to_string())?;

    for line in input.lines() {
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 4 {
            continue;
        }
        let (id, operation, topology, seq) = (f[0], f[1], f[2], f[3]);
        let params: Vec<(&str, &str)> = f[4..].iter().filter_map(|kv| kv.split_once('=')).collect();
        let param = |k: &str| params.iter().find(|(pk, _)| *pk == k).map(|(_, v)| *v);

        let circular = topology == "circular";
        let upper = seq.to_ascii_uppercase();

        match operation {
            "identity" => {
                let rc = String::from_utf8_lossy(&pl_core::reverse_complement(upper.as_bytes()))
                    .into_owned();
                if circular {
                    match pl_core::cdseguid(&upper, &rc) {
                        Ok(v) => println!("{id}\tcdseguid={v}"),
                        Err(e) => println!("{id}\terror={e}"),
                    }
                } else {
                    let ld = pl_core::ldseguid(&upper, &rc);
                    let ls = pl_core::lsseguid(&upper);
                    match (ld, ls) {
                        (Ok(d), Ok(s)) => println!("{id}\tldseguid={d}\tlsseguid={s}"),
                        (Err(e), _) | (_, Err(e)) => println!("{id}\terror={e}"),
                    }
                }
            }
            "assembly" => {
                // Fragments arrive comma-separated in the sequence column.
                let method = param("method").unwrap_or("homologous");
                if method != "homologous" {
                    println!("{id}	unsupported");
                    continue;
                }
                let limit: usize = param("min_homology")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(25);
                let frags: Vec<pl_clone::Dseq> = upper
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .map(|s| pl_clone::Dseq::new(s, false))
                    .collect();
                let opts = pl_clone::assembly::Options {
                    limit,
                    ..Default::default()
                };
                match pl_clone::assembly::assemble(&frags, circular, opts) {
                    Err(e) => println!("{id}	error={e}"),
                    Ok(products) => match products.len() {
                        0 => println!("{id}	error=no assembly"),
                        // More than one distinct product is a real answer about
                        // the reaction, not a tie to be broken silently.
                        1 => {
                            let p = &products[0];
                            match p.checksum() {
                                Some(c) => {
                                    println!("{id}	cdseguid={c}	length={}", p.seq.watson.len())
                                }
                                None => println!("{id}	error=product is not plain ACGT"),
                            }
                        }
                        n => println!("{id}	error={n} distinct products"),
                    },
                }
            }
            "digest" => {
                let Some(name) = param("enzyme") else {
                    println!("{id}\terror=no enzyme given");
                    continue;
                };
                match pl_enzymes::by_name(name) {
                    None => println!("{id}\tunsupported"),
                    Some(e) => {
                        let topo = if circular {
                            pl_core::Topology::Circular
                        } else {
                            pl_core::Topology::Linear
                        };
                        let pos = pl_enzymes::cut_positions(upper.as_bytes(), topo, e);
                        let list: Vec<String> = pos.iter().map(u64::to_string).collect();
                        println!(
                            "{id}\tcut_positions={}\tcut_count={}",
                            list.join(","),
                            pos.len()
                        );
                    }
                }
            }
            "pcr" => {
                let (Some(fwd), Some(rev)) = (param("forward_primer"), param("reverse_primer"))
                else {
                    println!("{id}\terror=missing a primer");
                    continue;
                };
                let template = pl_clone::Dseq::new(&upper, circular);
                match pl_clone::pcr(fwd, rev, &template) {
                    Err(e) => println!("{id}\terror={e}"),
                    Ok(product) => {
                        let w = &product.watson;
                        let rc =
                            String::from_utf8_lossy(&pl_core::reverse_complement(w.as_bytes()))
                                .into_owned();
                        match pl_core::ldseguid(w, &rc) {
                            Ok(v) => {
                                println!("{id}\tproduct_length={}\tldseguid={v}", w.len())
                            }
                            Err(e) => println!("{id}\terror={e}"),
                        }
                    }
                }
            }
            _ => println!("{id}\tunsupported"),
        }
    }
    Ok(())
}

/// Minimal reader for `[{"label": "..", "seq": ".."}, ...]`.
///
/// Hand-rolled to keep the binary dependency-free. It only has to survive the
/// cross-check harness's own output, which is generated rather than typed.
fn parse_label_seq_json(s: &str) -> Result<Vec<(String, String)>, String> {
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let mut key: Option<String> = None;
    let (mut label, mut seq) = (None, None);
    while i < chars.len() {
        match chars[i] {
            '"' => {
                let start = i + 1;
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    if chars[i] == '\\' {
                        i += 1;
                    }
                    i += 1;
                }
                let text: String = chars[start..i.min(chars.len())].iter().collect();
                match key.take() {
                    Some(k) if k == "label" => label = Some(text),
                    Some(k) if k == "seq" => seq = Some(text),
                    Some(_) => {}
                    None => key = Some(text),
                }
            }
            '}' => {
                if let (Some(l), Some(q)) = (label.take(), seq.take()) {
                    out.push((l, q));
                }
                key = None;
            }
            _ => {}
        }
        i += 1;
    }
    if out.is_empty() {
        return Err("no {label, seq} objects found on stdin".into());
    }
    Ok(out)
}

/// Local date as `(day, month_index, year)`.
///
/// Computed from the system clock without pulling in a date crate: days since
/// the Unix epoch through the civil-calendar algorithm (Howard Hinnant's
/// `civil_from_days`). UTC, which is the right choice for a file header.
fn today() -> (u32, usize, i32) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (d, (m - 1) as usize, y as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_parse_in_both_spellings() {
        let a = parse_args(
            &[
                "x.dna".into(),
                "--to".into(),
                "fasta".into(),
                "--outdir=out".into(),
                "--stdout".into(),
            ],
            &["to", "outdir"],
            &["stdout"],
        )
        .unwrap();
        assert_eq!(a.files.len(), 1);
        assert_eq!(a.get("to"), Some("fasta"));
        assert_eq!(a.get("outdir"), Some("out"));
        assert!(a.has("stdout"));
    }

    #[test]
    fn repeated_flags_all_survive() {
        let a = parse_args(
            &[
                "--enzyme".into(),
                "EcoRI".into(),
                "--enzyme".into(),
                "BamHI".into(),
            ],
            &["enzyme"],
            &[],
        )
        .unwrap();
        assert_eq!(a.get_all("enzyme"), vec!["EcoRI", "BamHI"]);
    }

    #[test]
    fn a_valued_flag_without_its_value_is_an_error() {
        assert!(parse_args(&["--to".into()], &["to"], &[]).is_err());
    }

    #[test]
    fn an_option_the_verb_does_not_know_is_an_error_not_a_silent_default() {
        // `pl orfs plasmid.gb --min-a 50`: "min-a" became a flag nobody reads
        // and "50" a positional that `a.files.first()` threw away, so `min_aa`
        // stayed at the default 30 and every 30-49 aa ORF the user had just
        // asked to exclude was printed, exit 0.
        let Err(e) = parse_args(
            &["plasmid.gb".into(), "--min-a".into(), "50".into()],
            &["table", "min-aa", "seq"],
            &["any-start"],
        ) else {
            panic!("--min-a was accepted as if it were an option this verb has");
        };
        assert!(e.contains("--min-a"), "{e}");
        // The reply has to say what the verb does take, or the user is left
        // guessing which of --min-a/--min-aa/--minaa was meant.
        assert!(e.contains("--min-aa"), "{e}");

        // A mistyped *boolean* leaves no stray positional behind, so nothing
        // downstream could ever have noticed it: `pl digest x.gb --uniqe`
        // listed every cut site instead of only the unique cutters.
        assert!(parse_args(
            &["x.gb".into(), "--uniqe".into()],
            &["enzyme"],
            &["unique", "non-cutters", "json"],
        )
        .is_err());
    }

    #[test]
    fn a_flag_the_verb_does_know_is_still_accepted_in_every_spelling() {
        // The control on the check above: rejecting unknown names must not cost
        // us the known ones, in any of the three spellings the parser supports.
        let a = parse_args(
            &[
                "--min-aa".into(),
                "50".into(),
                "--table=11".into(),
                "--any-start".into(),
                "plasmid.gb".into(),
            ],
            &["table", "min-aa", "seq"],
            &["any-start"],
        )
        .unwrap();
        assert_eq!(a.get("min-aa"), Some("50"));
        assert_eq!(a.get("table"), Some("11"));
        assert!(a.has("any-start"));
        assert_eq!(a.files.len(), 1);
    }

    #[test]
    fn a_value_that_looks_like_a_flag_is_still_a_value() {
        // `--name --absent` is a search for the literal text "--absent", not a
        // second option: the value is taken by index, before any name check.
        let a = parse_args(
            &["--name".into(), "--absent".into()],
            &["name"],
            &["absent"],
        )
        .unwrap();
        assert_eq!(a.get("name"), Some("--absent"));
        assert!(!a.has("absent"));
    }

    #[test]
    fn civil_calendar_matches_known_dates() {
        // Sanity-check the epoch and a leap day via the same algorithm.
        fn from_secs(secs: i64) -> (u32, usize, i32) {
            let z = secs.div_euclid(86_400) + 719_468;
            let era = z.div_euclid(146_097);
            let doe = z.rem_euclid(146_097);
            let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
            let y = yoe + era * 400;
            let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
            let mp = (5 * doy + 2) / 153;
            let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
            let m = if mp < 10 { mp + 3 } else { mp - 9 };
            let y = if m <= 2 { y + 1 } else { y };
            (d, (m - 1) as usize, y as i32)
        }
        assert_eq!(from_secs(0), (1, 0, 1970));
        assert_eq!(from_secs(951_782_400), (29, 1, 2000)); // 2000-02-29
        assert_eq!(from_secs(1_774_483_200), (26, 2, 2026)); // 2026-03-26
    }

    #[test]
    fn the_version_carries_the_commit_it_was_built_from() {
        // `docs/RELEASING.md` says the update path is that a user checks when
        // they want to, and that `pl --version` tells them which build they
        // have. That sentence was false for a while: the binary printed
        // "pl 0.1.0" and nothing else, and every build between two releases
        // says 0.1.0. This test is what keeps the document honest.
        let commit = env!("PL_COMMIT");
        assert!(!commit.is_empty());
        // Either a real short hash, or the documented fallback for a source
        // tarball with no .git — never silently blank.
        let core = commit.strip_suffix("-dirty").unwrap_or(commit);
        assert!(
            core == "unknown" || (core.len() >= 7 && core.chars().all(|c| c.is_ascii_hexdigit())),
            "{commit:?} is neither a short hash nor 'unknown'"
        );
    }
}
