//! `pl` — inspect, convert and digest sequence files.
//!
//! Everything is local. No network, no account, no telemetry.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use pl_fileio::{detect, fasta, genbank, load, load_with_report, snapgene, Format};

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
    pl trace   <file.ab1>...             read a Sanger chromatogram
    pl orfs    <file> [--table N]        open reading frames, six frames
    pl sanger  <read>... --ref <file>    did the clone work?

    pl index   <dir>... [options]        build or refresh a folder's index
    pl find    <dir> [query] [filters]   search it
    pl library <dir> [options]           what is indexed, and what could not be

CONVERT OPTIONS:
    --to <genbank|gb|fasta|fa>   output format (default: genbank)
    -o, --outdir <dir>           where to write (default: beside the input)
    --stdout                     write to stdout instead of files

INDEX OPTIONS:
    --verify                     re-read every file and check its stored hash
    --rebuild                    ignore any existing index
    --index-at <dir>             keep the index here instead of the OS cache
    --follow-links               follow symbolic links (off by default)
    --max-depth <n>              default 32

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
    --topology <circular|linear> default: linear

EXPORT OPTIONS:
    --width <px>                 canvas width  (default: 720)
    --height <px>                canvas height (default: 720)
    --pdf                        write PDF instead of SVG
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
        "orfs" => cmd_orfs(rest),
        "sanger" => cmd_sanger(rest),
        "trace" => cmd_trace(rest),
        "index" => cmd_index(rest),
        "find" => cmd_find(rest),
        "library" => cmd_library(rest),
        "bench-adapter" => cmd_bench_adapter(rest),
        "cut-adapter" => cmd_cut_adapter(rest),
        "-V" | "--version" => {
            println!("pl {}", env!("CARGO_PKG_VERSION"));
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

fn parse_args(args: &[String], valued: &[&str]) -> Result<Args, String> {
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

fn title_of(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "sequence".into())
}

// ---------------------------------------------------------------------------

fn cmd_info(args: &[String]) -> Result<(), String> {
    let a = parse_args(args, &[])?;
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
                    let sites: usize = mol.primers.iter().map(|p| p.sites.len()).sum();
                    let lower = mol.seq.iter().filter(|b| b.is_ascii_lowercase()).count();
                    let feats: Vec<String> = mol
                        .features
                        .iter()
                        .map(|f| {
                            format!(
                                "{{{}: {}, {}: {}, {}: {}, {}: {}, {}: {}, {}: {}}}",
                                json_str("name"),
                                json_str(&f.name),
                                json_str("kind"),
                                json_str(&f.kind),
                                json_str("start"),
                                f.start(),
                                json_str("end"),
                                f.end(),
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
                        "  {{{}: {}, {}: {}, {}: {}, {}: {}, {}: {}, {}: {}, {}: {}, {}: {}, {}: {}, {}: {}, {}: {}, {}: {}, {}: [{}]}}",
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
                        json_str("features"), feats.join(", ")
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

    for path in &a.files {
        let data = read(path)?;
        match load_with_report(&data) {
            Err(e) => println!("{}\n   ERROR: {e}\n", path.display()),
            Ok((mol, fmt, report)) => {
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
    Ok(())
}

fn cmd_convert(args: &[String]) -> Result<(), String> {
    let a = parse_args(args, &["to", "outdir", "o"])?;
    a.require_files()?;

    let to = a.get("to").unwrap_or("genbank").to_ascii_lowercase();
    let (ext, is_gb) = match to.as_str() {
        "genbank" | "gb" | "gbk" => ("gb", true),
        "fasta" | "fa" | "fna" => ("fa", false),
        other => return Err(format!("unknown output format '{other}'")),
    };
    let outdir = a.get("outdir").or_else(|| a.get("o")).map(PathBuf::from);
    let to_stdout = a.has("stdout");
    let date = today();

    // Two inputs can share a basename. Silently overwriting one with the other
    // is data loss, so collisions get a suffix and are reported.
    let mut claimed: Vec<PathBuf> = Vec::new();
    let mut converted = 0usize;
    let mut renamed = 0usize;

    for path in &a.files {
        let data = read(path)?;
        let (mol, _fmt, report) =
            load_with_report(&data).map_err(|e| format!("{}: {e}", path.display()))?;
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
        let text = if is_gb {
            genbank::write(&mol, &title, date)
        } else {
            fasta::write(&mol, &title, 70)
        };

        // GenBank cannot express an unoriented or bidirectional feature, so
        // those are written as forward. Say so rather than letting the export
        // publish a directional claim the source never made.
        if is_gb {
            let lossy = mol.features_without_expressible_orientation();
            if !lossy.is_empty() {
                eprintln!(
                    "pl: {}: {} feature(s) have no GenBank-expressible strand and are written as forward: {}",
                    path.display(),
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

        if to_stdout {
            print!("{text}");
            converted += 1;
            continue;
        }

        let dir = outdir.clone().unwrap_or_else(|| {
            path.parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."))
        });
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;

        let stem = genbank::locus_name(&title);
        let mut dest = dir.join(format!("{stem}.{ext}"));

        // Never write over the file we just read.
        //
        // Converting a `.gb` to genbank in place computes a destination equal
        // to the input, and `claimed` only ever guarded output against other
        // output. A multi-record file made that destructive rather than merely
        // redundant: `load` keeps only the first record, so a 124-record 36 KB
        // `.gbk` was rewritten as a 28 KB single-record file with 1,879
        // features gone — and the CLI reported success.
        if same_file(path, &dest) {
            return Err(format!(
                "{}: converting to {ext} here would overwrite the input file. \
                 Use --outdir <dir> to write elsewhere, or --stdout.",
                path.display()
            ));
        }

        if claimed.contains(&dest) {
            let mut n = 2;
            loop {
                let candidate = dir.join(format!("{stem}-{n}.{ext}"));
                if !claimed.contains(&candidate) {
                    dest = candidate;
                    break;
                }
                n += 1;
            }
            renamed += 1;
        }
        claimed.push(dest.clone());
        std::fs::write(&dest, text).map_err(|e| format!("{}: {e}", dest.display()))?;

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
    let a = parse_args(args, &["enzyme"])?;
    a.require_files()?;
    let path = &a.files[0];
    let data = read(path)?;
    let (mol, _) = load(&data).map_err(|e| format!("{}: {e}", path.display()))?;
    if mol.seq.is_empty() {
        return Err(format!("{}: no bases to digest", path.display()));
    }

    let wanted = a.get_all("enzyme");
    let mut results = pl_enzymes::digest_all(&mol);
    if !wanted.is_empty() {
        results.retain(|d| wanted.iter().any(|w| w.eq_ignore_ascii_case(d.enzyme.name)));
        if results.is_empty() {
            return Err(format!(
                "no such enzyme in the built-in set: {}",
                wanted.join(", ")
            ));
        }
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
    let a = parse_args(args, &[])?;
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
    let a = parse_args(args, &[])?;

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
        let (mol, _) = load(&data).map_err(|e| format!("{}: {e}", path.display()))?;
        // SEGUID is defined over unambiguous uppercase DNA. Say what was done
        // rather than quietly folding case or dropping ambiguity codes: a
        // checksum is an identity claim, and a silently altered input makes it
        // a false one.
        let seq: String = String::from_utf8_lossy(&mol.seq).to_uppercase();
        println!("{}", title_of(path));
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
fn cmd_export(args: &[String]) -> Result<(), String> {
    let a = parse_args(args, &["outdir", "o", "width", "height"])?;
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
    let opts = pl_draw::Options {
        width: num("width", 720.0)?,
        height: num("height", 720.0)?,
        ruler: !a.has("no-ruler"),
        ..Default::default()
    };

    let outdir = a.get("outdir").or_else(|| a.get("o")).map(PathBuf::from);
    let to_stdout = a.has("stdout");
    let mut claimed: Vec<PathBuf> = Vec::new();
    let (mut written, mut renamed) = (0usize, 0usize);

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

        let as_pdf = a.has("pdf");
        let (bytes, drawn, font) = if as_pdf {
            let (b, d, f) = pl_draw::circular_pdf(&mol, opts);
            (b, d, Some(f))
        } else {
            let (s, d) = pl_draw::circular_svg(&mol, opts);
            (s.into_bytes(), d, None)
        };

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

        let dir = outdir.clone().unwrap_or_else(|| {
            path.parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."))
        });
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;

        let ext = if as_pdf { "pdf" } else { "svg" };
        let stem = genbank::locus_name(&title_of(path));
        let mut dest = dir.join(format!("{stem}.{ext}"));
        if same_file(path, &dest) {
            return Err(format!(
                "{}: writing the map here would overwrite the input. Use --outdir <dir> or --stdout.",
                path.display()
            ));
        }
        if claimed.contains(&dest) {
            let mut k = 2;
            loop {
                let candidate = dir.join(format!("{stem}-{k}.{ext}"));
                if !claimed.contains(&candidate) {
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
    let a = parse_args(args, &["seq", "topology", "motif"])?;

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

    let circular = match a.get("topology").unwrap_or("linear") {
        "circular" => true,
        "linear" => false,
        other => return Err(format!("--topology {other:?}: expected circular or linear")),
    };

    // `--seq` for a literal sequence; otherwise the remaining files, whose
    // first record is used.
    let (seq, label) = match a.get("seq") {
        Some(s) => (s.as_bytes().to_vec(), "<--seq>".to_string()),
        None => {
            let path = a
                .files
                .get(if a.get("motif").is_some() { 0 } else { 1 })
                .ok_or("give a sequence with --seq, or a file")?;
            let data = read(path)?;
            let (mol, _, _) =
                load_with_report(&data).map_err(|e| format!("{}: {e}", path.display()))?;
            (mol.seq.clone(), path.display().to_string())
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

fn scan_options(a: &Args, previous: Option<pl_index::codec::Library>) -> pl_scan::ScanOptions {
    let mut walk = pl_scan::WalkOptions {
        follow_links: a.has("follow-links"),
        ..Default::default()
    };
    if let Some(d) = a.get("max-depth").and_then(|v| v.parse().ok()) {
        walk.max_depth = d;
    }
    pl_scan::ScanOptions { walk, previous }
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
    let a = parse_args(args, &["index-at", "max-depth"])?;
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

        let (lib, report) = pl_scan::scan(root, now_ns(), &scan_options(&a, previous));
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
        let (lib, report) = pl_scan::scan(root, now_ns(), &scan_options(a, None));
        if let Some(why) = &report.incomplete {
            eprintln!("pl: scan incomplete: {why}");
        }
        return Ok((lib, false));
    }
    let path = index_location(a, root)?;
    let previous = previous_index(&path)?;
    let had_index = previous.is_some();
    let (lib, report) = pl_scan::scan(root, now_ns(), &scan_options(a, previous));

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
    )?;
    let root = a.files.first().ok_or("no folder given")?.clone();
    if !root.is_dir() {
        return Err(format!("{}: not a folder", root.display()));
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
            // absurd, and `BsaI` is not in the shipped table.
            let e = pl_enzymes::by_name(name).ok_or_else(|| {
                format!(
                    "--enzyme {name:?}: not in the shipped table of {} Type IIP enzymes.\n\
                     there is no BsaI, BsmBI, BbsI or SapI yet — use --motif GGTCTC to ask \
                     about the site itself.",
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
    let a = parse_args(args, &["index-at", "max-depth"])?;
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
    let a = parse_args(args, &["table", "na", "oligo", "salt"])?;

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
    for s in &seqs {
        match pl_thermo::tm(s.as_bytes(), &m) {
            Ok(t) => {
                tms.push(t.tm);
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
                tms.clear();
                println!(
                    "{:>8}  {:>6}  {:>9}  {:>9}  {s}  --  {e}",
                    "-", "-", "-", "-"
                );
            }
        }
    }

    // Annealing advice, separately and per polymerase, exactly as the plan
    // insists: a Tm is a property of a duplex, a Ta is protocol advice.
    if !tms.is_empty() {
        let low = tms.iter().cloned().fold(f64::INFINITY, f64::min);
        println!(
            "
annealing advice, from the lowest Tm ({low:.1}C):"
        );
        for p in pl_thermo::POLYMERASES {
            let (lo, hi) = pl_thermo::anneal(low, None, p);
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
    let a = parse_args(args, &["enzyme"])?;

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
        let (mol, _, _) =
            load_with_report(&data).map_err(|f| format!("{}: {f}", path.display()))?;
        let seq = String::from_utf8_lossy(&mol.seq).to_string();
        let frags = pl_clone::cut(&pl_clone::Dseq::new(&seq, mol.topology.is_circular()), e);
        for f in &frags {
            if let Some(o) = pl_clone::goldengate::left_overhang(f) {
                overhangs.push(o);
            }
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
fn cmd_sanger(args: &[String]) -> Result<(), String> {
    let a = parse_args(args, &["ref", "ref-seq", "read", "min-quality"])?;

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
            let (mol, _, _) =
                load_with_report(&data).map_err(|e| format!("{}: {e}", path.display()))?;
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
    for (name, seq, qual) in &reads {
        let r = match pl_sanger::compare(seq, qual, &reference, circular, &p) {
            Some(r) => r,
            None => {
                println!("{name}: could not be placed on this reference");
                worst += 1;
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
            reads.len()
        );
    }
    Ok(())
}

fn cmd_orfs(args: &[String]) -> Result<(), String> {
    let a = parse_args(args, &["table", "min-aa", "seq"])?;

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
            let (mol, _, _) =
                load_with_report(&data).map_err(|e| format!("{}: {e}", path.display()))?;
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
                "    {{\"start\": {}, \"end\": {}, \"strand\": {}, \"frame\": {}, \"aa_len\": {}, \"start_codon\": {}, \"complete\": {}, \"wrapped\": {}, \"protein\": {}}}{}",
                o.start,
                o.end,
                json_str(if o.strand == pl_core::Strand::Reverse { "-" } else { "+" }),
                o.frame,
                o.aa_len,
                json_str(&String::from_utf8_lossy(&o.start_codon)),
                o.complete,
                o.wrapped,
                json_str(&String::from_utf8_lossy(&code.translate(&bases_of(o)))),
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
            if o.wrapped { "  crosses origin" } else { "" },
            if o.complete {
                ""
            } else {
                "  no stop — runs off the end"
            }
        );
        if a.has("translate") {
            let aa = code.translate(&bases_of(o));
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
    let a = parse_args(args, &["seed", "primer", "seq"])?;

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
            let (mol, _, _) =
                load_with_report(&data).map_err(|e| format!("{}: {e}", path.display()))?;
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

/// Read a Sanger chromatogram.
fn cmd_trace(args: &[String]) -> Result<(), String> {
    let a = parse_args(args, &[])?;
    a.require_files()?;
    for path in &a.files {
        let data = read(path)?;
        let t = match pl_abif::parse(&data) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("pl: {}: {e}", path.display());
                continue;
            }
        };
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
        let frags: Vec<String> = pl_clone::cut(&d, e)
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
    let a = parse_args(args, &[])?;

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
        )
        .unwrap();
        assert_eq!(a.get_all("enzyme"), vec!["EcoRI", "BamHI"]);
    }

    #[test]
    fn a_valued_flag_without_its_value_is_an_error() {
        assert!(parse_args(&["--to".into()], &["to"]).is_err());
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
}
