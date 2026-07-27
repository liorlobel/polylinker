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
    pl export  <file>... [options]       plasmid map as SVG

CONVERT OPTIONS:
    --to <genbank|gb|fasta|fa>   output format (default: genbank)
    -o, --outdir <dir>           where to write (default: beside the input)
    --stdout                     write to stdout instead of files

EXPORT OPTIONS:
    --width <px>                 canvas width  (default: 720)
    --height <px>                canvas height (default: 720)
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

        let (svg, drawn) = pl_draw::circular_svg(&mol, opts);

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
            println!("{svg}");
            written += 1;
            continue;
        }

        let dir = outdir.clone().unwrap_or_else(|| {
            path.parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."))
        });
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;

        let stem = genbank::locus_name(&title_of(path));
        let mut dest = dir.join(format!("{stem}.svg"));
        if same_file(path, &dest) {
            return Err(format!(
                "{}: writing the map here would overwrite the input. Use --outdir <dir> or --stdout.",
                path.display()
            ));
        }
        if claimed.contains(&dest) {
            let mut k = 2;
            loop {
                let candidate = dir.join(format!("{stem}-{k}.svg"));
                if !claimed.contains(&candidate) {
                    dest = candidate;
                    break;
                }
                k += 1;
            }
            renamed += 1;
        }
        claimed.push(dest.clone());
        std::fs::write(&dest, &svg).map_err(|e| format!("{}: {e}", dest.display()))?;

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
