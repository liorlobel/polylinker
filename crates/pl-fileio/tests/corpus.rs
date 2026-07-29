//! Validation against a real corpus of files.
//!
//! Unit tests on synthetic input prove the code does what I thought it did.
//! These prove it survives files written by somebody else's software over
//! fifteen years of format drift, which is a different and harder claim.
//!
//! Point `PL_CORPUS` at a directory and these run; otherwise they skip, so the
//! suite stays green for contributors who do not have a pile of `.dna` files:
//!
//! ```text
//! PL_CORPUS="/mnt/c/Users/me/plasmids" cargo test -p pl-fileio --test corpus -- --nocapture
//! ```

use std::path::{Path, PathBuf};

use pl_core::Topology;
use pl_fileio::{detect, genbank, snapgene, xml, Format};

fn corpus_root() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var_os("PL_CORPUS")?);
    p.is_dir().then_some(p)
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 12 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        match e.file_type() {
            Ok(t) if t.is_dir() => walk(&p, out, depth + 1),
            Ok(t) if t.is_file() => out.push(p),
            _ => {}
        }
    }
}

fn files_with(ext: &[&str]) -> Vec<PathBuf> {
    let Some(root) = corpus_root() else {
        return Vec::new();
    };
    let mut all = Vec::new();
    walk(&root, &mut all, 0);
    all.retain(|p| {
        p.extension()
            .and_then(|e| e.to_str())
            .map(|e| ext.iter().any(|w| w.eq_ignore_ascii_case(e)))
            .unwrap_or(false)
    });
    all.sort();
    all
}

fn skip(what: &str) {
    eprintln!("SKIP {what}: set PL_CORPUS to a directory of real files to enable");
}

#[test]
fn dna_files_round_trip_byte_exactly() {
    let files = files_with(&["dna"]);
    if files.is_empty() {
        return skip("dna_files_round_trip_byte_exactly");
    }

    let mut ok = 0usize;
    let mut failures = Vec::new();
    let mut total_bytes = 0usize;

    for path in &files {
        let Ok(raw) = std::fs::read(path) else {
            continue;
        };
        total_bytes += raw.len();
        match snapgene::parse(&raw) {
            Err(e) => failures.push(format!("{}: parse: {e}", path.display())),
            Ok(doc) => {
                let out = snapgene::write(&doc, false);
                if out == raw {
                    ok += 1;
                } else {
                    failures.push(format!(
                        "{}: rewrote {} bytes, original was {}",
                        path.display(),
                        out.len(),
                        raw.len()
                    ));
                }
            }
        }
    }

    eprintln!(
        "byte-exact .dna round-trip: {ok}/{} files, {:.1} MB",
        files.len(),
        total_bytes as f64 / 1e6
    );
    assert!(
        failures.is_empty(),
        "round-trip failures:\n  {}",
        failures.join("\n  ")
    );
}

#[test]
fn dropping_derived_blocks_keeps_the_molecule_intact() {
    let files = files_with(&["dna"]);
    if files.is_empty() {
        return skip("dropping_derived_blocks_keeps_the_molecule_intact");
    }

    let mut saved = 0usize;
    let mut total = 0usize;
    let mut failures = Vec::new();

    for path in &files {
        let Ok(raw) = std::fs::read(path) else {
            continue;
        };
        let Ok(doc) = snapgene::parse(&raw) else {
            continue;
        };
        let slim = snapgene::write(&doc, true);
        total += raw.len();
        saved += raw.len().saturating_sub(slim.len());

        match snapgene::parse(&slim) {
            Err(e) => failures.push(format!(
                "{}: slim file will not reparse: {e}",
                path.display()
            )),
            Ok(re) => {
                if re.molecule.seq != doc.molecule.seq {
                    failures.push(format!("{}: sequence changed", path.display()));
                }
                if re.molecule.topology != doc.molecule.topology {
                    failures.push(format!("{}: topology changed", path.display()));
                }
                if re.molecule.features.len() != doc.molecule.features.len() {
                    failures.push(format!("{}: feature count changed", path.display()));
                }
                if re.derived_bytes() != 0 {
                    failures.push(format!("{}: derived blocks survived", path.display()));
                }
            }
        }
    }

    eprintln!(
        "dropping derived caches saves {:.1} MB of {:.1} MB ({:.0}%)",
        saved as f64 / 1e6,
        total as f64 / 1e6,
        100.0 * saved as f64 / total.max(1) as f64
    );
    assert!(
        failures.is_empty(),
        "failures:\n  {}",
        failures.join("\n  ")
    );
}

#[test]
fn dna_survives_conversion_to_genbank_and_back() {
    let files = files_with(&["dna"]);
    if files.is_empty() {
        return skip("dna_survives_conversion_to_genbank_and_back");
    }

    let mut ok = 0usize;
    let mut failures = Vec::new();

    for path in &files {
        let Ok(raw) = std::fs::read(path) else {
            continue;
        };
        let Ok(doc) = snapgene::parse(&raw) else {
            continue;
        };
        let src = &doc.molecule;
        let title = path.file_name().unwrap().to_string_lossy().to_string();
        let gb = genbank::write(src, &title, (26, 6, 2026));
        let back = genbank::parse(&gb);

        let mut problems = Vec::new();
        if back.seq != src.seq {
            problems.push(format!("sequence: {} vs {} bp", back.len(), src.len()));
        }
        if back.topology != src.topology {
            problems.push("topology".into());
        }
        // GenBank has no separate primer object: each binding site becomes a
        // primer_bind feature, so the expected count includes them.
        //
        // This filter mirrors the writer's own rule and is a derived
        // expectation, not a claim about what ought to be written. It used to
        // read `s.end >= s.start`, which encoded the writer's old habit of
        // skipping any site that crosses the origin — a shape `validate()`
        // calls perfectly legal on a circle and `Molecule::rotate` produces
        // routinely. Those are now written as a `join`, exactly as a feature
        // segment at the same coordinates always was, so the count follows.
        let n = src.span();
        let sites = src
            .primers
            .iter()
            .flat_map(|p| &p.sites)
            .filter(|s| {
                if s.start < 1 {
                    return false; // base 0 does not exist in GenBank
                }
                if s.end < s.start {
                    // A wrap, written only when there is an origin to split it
                    // against.
                    return n >= s.start;
                }
                s.end <= n
            })
            .count();
        let want = src.features.len() + sites;
        if back.features.len() != want {
            problems.push(format!(
                "features: {} vs {} ({} + {} primer sites)",
                back.features.len(),
                want,
                src.features.len(),
                sites
            ));
        }
        // Coordinates and colours of the original features, in order.
        for (i, f) in src.features.iter().enumerate() {
            let Some(g) = back.features.get(i) else { break };
            if (g.start(), g.end()) != (f.start(), f.end()) {
                problems.push(format!(
                    "feature {i} '{}' moved: {}..{} -> {}..{}",
                    f.name,
                    f.start(),
                    f.end(),
                    g.start(),
                    g.end()
                ));
                break;
            }
            if f.color().is_some() && g.color() != f.color() {
                problems.push(format!("feature {i} '{}' lost its colour", f.name));
                break;
            }
        }

        if problems.is_empty() {
            ok += 1;
        } else {
            failures.push(format!("{}: {}", path.display(), problems.join("; ")));
        }
    }

    eprintln!(
        "dna -> GenBank -> parse: {ok}/{} files faithful",
        files.len()
    );
    assert!(
        failures.is_empty(),
        "conversion losses:\n  {}",
        failures.join("\n  ")
    );
}

#[test]
fn genbank_corpus_parses() {
    let files = files_with(&["gb", "gbk", "genbank"]);
    if files.is_empty() {
        return skip("genbank_corpus_parses");
    }

    let mut ok = 0usize;
    let mut declared_no_bases = 0usize;
    let mut annotation_tracks = 0usize;
    let mut empty = 0usize;
    let mut features = 0usize;
    let mut bases = 0usize;
    let mut failures = Vec::new();

    for path in &files {
        let Ok(raw) = std::fs::read(path) else {
            continue;
        };
        let text = String::from_utf8_lossy(&raw);
        if detect(&raw) != Some(Format::GenBank) {
            failures.push(format!("{}: not detected as GenBank", path.display()));
            continue;
        }
        let mol = genbank::parse(&text);
        if mol.sequence_absent() {
            // ORIGIN present but empty; length declared on the LOCUS line.
            declared_no_bases += 1;
        } else if mol.is_annotation_track() {
            // A standalone annotation export: no ORIGIN block, no bp field.
            annotation_tracks += 1;
        } else if mol.seq.is_empty() {
            // An annotation export with nothing in it. Real: one file in this
            // corpus is 148 bytes of LOCUS + FEATURES + "//". Parsing nothing
            // out of a file containing nothing is the correct answer, so this
            // is counted, not failed.
            empty += 1;
        }
        // Every feature must lie within the span it annotates.
        if let Some(bad) = mol
            .features
            .iter()
            .find(|f| f.end() > mol.annotation_span().max(1))
        {
            failures.push(format!(
                "{}: feature '{}' ends at {} beyond the {} bp molecule",
                path.display(),
                bad.name,
                bad.end(),
                mol.annotation_span()
            ));
            continue;
        }
        features += mol.features.len();
        bases += mol.seq.len();
        ok += 1;
    }

    eprintln!(
        "GenBank: {ok}/{} files, {features} features, {:.1} Mb of sequence\n  \
         {declared_no_bases} declared a length but shipped no bases, \
         {annotation_tracks} are standalone annotation tracks, {empty} are empty",
        files.len(),
        bases as f64 / 1e6
    );
    assert!(
        failures.is_empty(),
        "GenBank failures:\n  {}",
        failures.join("\n  ")
    );
}

#[test]
fn every_file_is_identified_from_content() {
    let files = files_with(&[
        "dna", "gb", "gbk", "fa", "fasta", "ab1", "scf", "ztr", "seq",
    ]);
    if files.is_empty() {
        return skip("every_file_is_identified_from_content");
    }

    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    let mut mismatched = Vec::new();
    // The proposition in this test's name, which its body did not check.
    //
    // Everything below line 370 used to be `eprintln!` — output the runner
    // hides unless `--nocapture` is passed — so the function was structurally
    // incapable of failing: no `assert!`, no `panic!`, no `?`. A regression
    // that made `detect` return `None` for every real file would tally them all
    // under "unrecognised" and the test would still exit green, e.g. narrowing
    // the `min(8192)` text window in `detect`, which every sub-60-byte unit
    // fixture survives and no real GenBank file does.
    //
    // Scoped to extensions that have an expected format: `.seq` is in the list
    // above and correctly detects as `None`, because a `.seq` really could be
    // anything. The extension/content *mismatch* list stays a report, and
    // deliberately so — that disagreement is the reason detection reads content
    // in the first place.
    let mut unidentified: Vec<String> = Vec::new();
    let mut empty = 0usize;

    for path in &files {
        let Ok(raw) = std::fs::read(path) else {
            continue;
        };
        let got = detect(&raw);
        let key = got
            .map(|f| f.name().to_string())
            .unwrap_or_else(|| "unrecognised".into());
        *counts.entry(key).or_default() += 1;

        // The interesting case: extension and content disagree.
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let expected = match ext.as_str() {
            "dna" => Some(Format::SnapGene),
            "gb" | "gbk" | "genbank" => Some(Format::GenBank),
            "fa" | "fasta" => Some(Format::Fasta),
            "ab1" => Some(Format::Abif),
            "scf" => Some(Format::Scf),
            "ztr" => Some(Format::Ztr),
            _ => None,
        };
        match (expected, got) {
            (Some(exp), Some(g)) if exp != g => {
                mismatched.push(format!(
                    "{}: .{ext} but content is {}",
                    path.display(),
                    g.name()
                ));
            }
            // The case the `if let` dropped on the floor: we know what this
            // file is supposed to be and `detect` recognised nothing at all.
            //
            // A file with no bytes is exempt, and that is not a loophole. There
            // is nothing in an empty file to identify, and answering "FASTA"
            // from zero bytes would be inventing the one thing this test exists
            // to check is read rather than assumed. The corpus has two: QIIME
            // writes a zero-length `*_failures.fasta` when nothing failed.
            // They are counted so the exemption cannot quietly grow.
            (Some(_), None) if raw.is_empty() => empty += 1,
            (Some(exp), None) => {
                unidentified.push(format!(
                    "{}: .{ext} should be {} and was not identified from content",
                    path.display(),
                    exp.name()
                ));
            }
            _ => {}
        }
    }

    for (k, v) in &counts {
        eprintln!("  {v:>5}  {k}");
    }
    if empty > 0 {
        eprintln!("  {empty:>5}  empty, so there was nothing to identify");
    }
    if !mismatched.is_empty() {
        // Not a failure: this is the reason detection reads content at all.
        eprintln!(
            "\n  {} file(s) whose extension misrepresents their content:",
            mismatched.len()
        );
        for m in mismatched.iter().take(12) {
            eprintln!("    {m}");
        }
    }
    assert!(
        unidentified.is_empty(),
        "{} file(s) whose format we know and could not identify:\n  {}",
        unidentified.len(),
        unidentified
            .iter()
            .take(12)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

/// Rotating a real plasmid must not change what molecule it is.
///
/// This is the assertion `docs/PLAN.md` §7.12.1 asks for — "every assertion
/// about a molecule is an assertion about its `cdseguid`" — applied to the one
/// operation that is guaranteed to be a no-op on a circle. It ties three
/// separate pieces of machinery together: the reader, `Molecule::rotate`, and
/// the checksum. A bug in any of them shows up here.
#[test]
fn rotating_a_plasmid_preserves_its_identity() {
    let files = files_with(&["dna"]);
    if files.is_empty() {
        return skip("rotating_a_plasmid_preserves_its_identity");
    }

    let mut checked = 0usize;
    let mut rotations = 0usize;
    let mut skipped_ambiguous = 0usize;
    let mut failures = Vec::new();

    for path in &files {
        let Ok(raw) = std::fs::read(path) else {
            continue;
        };
        let Ok(doc) = snapgene::parse(&raw) else {
            continue;
        };
        let mol = doc.molecule;
        if !mol.topology.is_circular() || mol.seq.is_empty() || mol.len() > 300_000 {
            continue;
        }
        // SEGUID is defined over unambiguous DNA; anything else is skipped
        // rather than silently coerced.
        let seq: String = String::from_utf8_lossy(&mol.seq).to_uppercase();
        if !seq.chars().all(|c| matches!(c, 'A' | 'C' | 'G' | 'T')) {
            skipped_ambiguous += 1;
            continue;
        }
        let rc = String::from_utf8_lossy(&pl_core::reverse_complement(seq.as_bytes())).into_owned();
        let Ok(expected) = pl_core::cdseguid(&seq, &rc) else {
            skipped_ambiguous += 1;
            continue;
        };
        checked += 1;

        // A handful of rotations, including ones that land mid-feature.
        let n = mol.len();
        for frac in [1u64, 7, 3, 2] {
            let origin = (n / frac).max(1);
            let mut rotated = mol.clone();
            if !rotated.rotate(origin) {
                failures.push(format!("{}: rotate({origin}) refused", path.display()));
                continue;
            }
            rotations += 1;

            if rotated.len() != n {
                failures.push(format!("{}: rotation changed the length", path.display()));
                continue;
            }
            let rseq: String = String::from_utf8_lossy(&rotated.seq).to_uppercase();
            let rrc =
                String::from_utf8_lossy(&pl_core::reverse_complement(rseq.as_bytes())).into_owned();
            match pl_core::cdseguid(&rseq, &rrc) {
                Ok(got) if got == expected => {}
                Ok(got) => failures.push(format!(
                    "{}: rotating to {origin} changed the checksum\n     was {expected}\n     now {got}",
                    path.display()
                )),
                Err(e) => failures.push(format!("{}: {e}", path.display())),
            }

            // Annotations must travel with the bases they describe.
            if rotated.features.len() != mol.features.len() {
                failures.push(format!("{}: rotation lost features", path.display()));
            }
            for f in &rotated.features {
                if f.end() > n {
                    failures.push(format!(
                        "{}: feature '{}' ends at {} past the {n} bp molecule",
                        path.display(),
                        f.name,
                        f.end()
                    ));
                    break;
                }
            }
        }
    }

    eprintln!(
        "rotation identity: {checked} plasmids x {} rotations = {rotations} checks, \
         {skipped_ambiguous} skipped for ambiguous bases",
        if checked > 0 {
            rotations / checked.max(1)
        } else {
            0
        }
    );
    assert!(
        failures.is_empty(),
        "rotation changed a molecule:\n  {}",
        failures.join("\n  ")
    );
}

/// Edit a real plasmid, then undo everything, and it must be the same molecule.
///
/// This is the op log's central claim tested against files nobody wrote for it.
/// It ties together the reader, the edit operations, feature coordinate
/// shifting, lazy replay and the checksum: if any one of them loses information,
/// the `cdseguid` will not come back.
///
/// The failure it is really guarding against is silent — a sequence that
/// survives an edit-and-undo while its annotations end up pointing at the wrong
/// bases. Comparing the checksum alone would miss that, so features are checked
/// too.
#[test]
fn editing_and_undoing_a_real_plasmid_restores_it_exactly() {
    let files = files_with(&["dna"]);
    if files.is_empty() {
        return skip("editing_and_undoing_a_real_plasmid_restores_it_exactly");
    }

    let mut checked = 0usize;
    let mut edits = 0usize;
    let mut failures = Vec::new();

    for path in &files {
        let Ok(raw) = std::fs::read(path) else {
            continue;
        };
        let Ok(doc) = snapgene::parse(&raw) else {
            continue;
        };
        let mol = doc.molecule;
        if mol.seq.is_empty() || mol.len() > 300_000 || mol.features.is_empty() {
            continue;
        }
        let seq: String = String::from_utf8_lossy(&mol.seq).to_uppercase();
        if !seq.chars().all(|c| matches!(c, 'A' | 'C' | 'G' | 'T')) {
            continue;
        }
        let checksum = |m: &pl_core::Molecule| -> Option<String> {
            let s: String = String::from_utf8_lossy(&m.seq).to_uppercase();
            let rc =
                String::from_utf8_lossy(&pl_core::reverse_complement(s.as_bytes())).into_owned();
            if m.topology.is_circular() {
                pl_core::cdseguid(&s, &rc).ok()
            } else {
                pl_core::ldseguid(&s, &rc).ok()
            }
        };
        let Some(before) = checksum(&mol) else {
            continue;
        };
        let features_before: Vec<(String, u64, u64)> = mol
            .features
            .iter()
            .map(|f| (f.name.clone(), f.start(), f.end()))
            .collect();
        checked += 1;

        let n = mol.len();
        let mut log = pl_core::OpLog::new(mol.clone());

        // A plausible session: insert a linker, delete somewhere else, add a
        // feature, and — if it is circular — move the origin.
        let mut program = vec![
            pl_core::OpKind::InsertAt {
                at: n / 3,
                seq: "GAATTCGGATCC".into(),
            },
            pl_core::OpKind::DeleteRange {
                start: n / 2,
                len: 25,
            },
            pl_core::OpKind::SetFeature {
                index: None,
                feature: Box::new(pl_core::Feature::new("inserted linker", "misc_feature")),
            },
        ];
        if mol.topology.is_circular() {
            program.push(pl_core::OpKind::Rotate { origin: n / 4 });
        }

        let mut applied = 0usize;
        for kind in program {
            // SetFeature with no segments is legal but pointless; give it one.
            let kind = match kind {
                pl_core::OpKind::SetFeature { index, mut feature } => {
                    feature.segments.push(pl_core::Segment::new(10, 30));
                    pl_core::OpKind::SetFeature { index, feature }
                }
                other => other,
            };
            if log.apply(kind, "corpus-test").is_ok() {
                applied += 1;
                edits += 1;
            }
        }
        if applied == 0 {
            continue;
        }

        // The edits must have actually changed the molecule, or the test is
        // asserting nothing.
        if checksum(log.current()) == Some(before.clone()) {
            failures.push(format!(
                "{}: {applied} edits left the checksum unchanged; the test is vacuous",
                path.display()
            ));
            continue;
        }

        while log.undo().is_ok() {}

        match checksum(log.current()) {
            Some(after) if after == before => {}
            Some(after) => failures.push(format!(
                "{}: undoing {applied} edits did not restore it\n     was {before}\n     now {after}",
                path.display()
            )),
            None => failures.push(format!("{}: unreadable after undo", path.display())),
        }

        let features_after: Vec<(String, u64, u64)> = log
            .current()
            .features
            .iter()
            .map(|f| (f.name.clone(), f.start(), f.end()))
            .collect();
        if features_after != features_before {
            failures.push(format!(
                "{}: {} features before, {} after, and the coordinates differ",
                path.display(),
                features_before.len(),
                features_after.len()
            ));
        }
    }

    eprintln!("op log: {checked} plasmids, {edits} edits applied and undone");
    assert!(
        failures.is_empty(),
        "edit/undo lost information:\n  {}",
        failures.join("\n  ")
    );
}

/// Does the wild actually contain the coordinates our model permits but should
/// not have?
///
/// `docs/PLAN.md` §5.3.1 (as amended) accepts that `{start, end}` can express
/// an interval that describes nothing — inverted, zero-based, past the end.
/// The question that decides how strict readers should be is empirical: do real
/// files contain them?
///
/// This reports rather than asserting a count, because the answer is a property
/// of other people's software and will drift. It fails only if a file we wrote
/// ourselves is invalid, which would be our bug.
#[test]
fn survey_real_files_for_coordinates_that_describe_nothing() {
    let files = files_with(&["dna", "gb", "gbk", "genbank"]);
    if files.is_empty() {
        return skip("survey_real_files_for_coordinates_that_describe_nothing");
    }

    let mut checked = 0usize;
    let mut clean = 0usize;
    let mut by_kind: std::collections::BTreeMap<&str, usize> = Default::default();
    let mut examples: Vec<String> = Vec::new();
    let mut round_trip_failures = Vec::new();

    for path in &files {
        let Ok(raw) = std::fs::read(path) else {
            continue;
        };
        let Ok((mol, _fmt)) = pl_fileio::load(&raw) else {
            continue;
        };
        checked += 1;
        let problems = mol.validate();
        if problems.is_empty() {
            clean += 1;
        } else {
            for p in &problems {
                let k = match p {
                    pl_core::Invalid::Inverted { .. } => "inverted",
                    pl_core::Invalid::ZeroStart { .. } => "zero start",
                    pl_core::Invalid::PastEnd { .. } => "past the end",
                    pl_core::Invalid::FeatureWithoutSegments { .. } => "no segments",
                    pl_core::Invalid::LengthMismatch { .. } => "declared length disagrees",
                };
                *by_kind.entry(k).or_default() += 1;
            }
            if examples.len() < 6 {
                examples.push(format!(
                    "{}: {}",
                    path.file_name().unwrap().to_string_lossy(),
                    problems[0]
                ));
            }
        }

        // Whatever the input contained, anything *we* write must be sound.
        let title = path.file_name().unwrap().to_string_lossy().to_string();
        let gb = genbank::write(&mol, &title, (26, 6, 2026));
        let ours = genbank::parse(&gb);
        let ours_problems = ours.validate();
        // Only new problems are ours; a bad coordinate on the way in is
        // allowed to survive the trip.
        if ours_problems.len() > problems.len() {
            round_trip_failures.push(format!(
                "{}: we introduced {} problem(s): {}",
                path.display(),
                ours_problems.len() - problems.len(),
                ours_problems
                    .iter()
                    .map(|p| p.to_string())
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
    }

    eprintln!("coordinate survey: {clean}/{checked} files fully sound");
    if by_kind.is_empty() {
        eprintln!("  no invalid coordinates found in the wild");
    } else {
        for (k, n) in &by_kind {
            eprintln!("  {n:>6}  {k}");
        }
        for e in &examples {
            eprintln!("    e.g. {e}");
        }
    }
    assert!(
        round_trip_failures.is_empty(),
        "our own writer produced invalid coordinates:\n  {}",
        round_trip_failures.join("\n  ")
    );
}

#[test]
fn digest_invariants_hold_on_real_plasmids() {
    let files = files_with(&["dna"]);
    if files.is_empty() {
        return skip("digest_invariants_hold_on_real_plasmids");
    }

    let mut sites = 0usize;
    let mut checked = 0usize;
    let mut failures = Vec::new();

    for path in &files {
        let Ok(raw) = std::fs::read(path) else {
            continue;
        };
        let Ok(doc) = snapgene::parse(&raw) else {
            continue;
        };
        let mol = &doc.molecule;
        // Keep the test quick; the big genomes are covered by the unit tests.
        if mol.seq.is_empty() || mol.len() > 300_000 {
            continue;
        }
        checked += 1;

        for d in pl_enzymes::digest_all(mol) {
            sites += d.count();
            // Positions must lie inside the molecule and be strictly sorted.
            if d.positions.iter().any(|&p| p == 0 || p > mol.len()) {
                failures.push(format!(
                    "{}: {} cut outside the molecule",
                    path.display(),
                    d.enzyme.name
                ));
                continue;
            }
            if d.positions.windows(2).any(|w| w[0] >= w[1]) {
                failures.push(format!(
                    "{}: {} positions not sorted/unique",
                    path.display(),
                    d.enzyme.name
                ));
                continue;
            }
            // Fragments must account for every base exactly once.
            let frags = d.fragments(mol.len(), mol.topology);
            if d.count() > 0 {
                let sum: u64 = frags.iter().sum();
                let expect_count = match (mol.topology, d.count()) {
                    (Topology::Circular, 1) => 1,
                    (Topology::Circular, k) => k,
                    (Topology::Linear, k) => k + 1,
                };
                if sum != mol.len() {
                    failures.push(format!(
                        "{}: {} fragments sum to {sum}, molecule is {} bp",
                        path.display(),
                        d.enzyme.name,
                        mol.len()
                    ));
                }
                if frags.len() != expect_count {
                    failures.push(format!(
                        "{}: {} gave {} fragments, expected {expect_count}",
                        path.display(),
                        d.enzyme.name,
                        frags.len()
                    ));
                }
            }
        }
    }

    eprintln!("digest invariants: {checked} molecules, {sites} cut sites");
    assert!(
        failures.is_empty(),
        "digest problems:\n  {}",
        failures.join("\n  ")
    );
}

/// One direct child of a `<Notes>` block, straight out of the payload.
struct Block6Elem {
    name: String,
    /// Trimmed text before the first grandchild, which is the contract
    /// `snapgene::parse_notes` and `reference/python/snapdna.py` both keep.
    text: String,
    attrs: Vec<(String, String)>,
}

/// The direct children of a document's `<Notes>` block.
///
/// Read straight out of the block payload with `xml::scan` rather than out of
/// `Molecule::notes`, which is the entire point: the model is the thing under
/// test, so it cannot also be the reference. Nested descendants are deliberately
/// not returned — `Note` is flat and says so, and `Document
/// ::unrepresentable_notes` is where those are accounted for.
///
/// The text is returned as well as the name and the attributes, and that is not
/// symmetry for its own sake. It did not used to be, and the omission put the
/// larger half of every note back inside the cancellation this whole comparison
/// exists to escape: making `parse_notes` discard every note's value on read —
/// UUIDs, descriptions, creation dates, all of it — left this test green on all
/// 33 real files, because `a.notes == b.notes` (both empty) and the loop below
/// only ever asked whether the element and its attributes were present.
fn block_six(doc: &snapgene::Document) -> Vec<Block6Elem> {
    let Some(b) = doc.blocks.iter().find(|b| b.kind == snapgene::block::NOTES) else {
        return Vec::new();
    };
    let payload = String::from_utf8_lossy(&b.payload);
    let mut out: Vec<Block6Elem> = Vec::new();
    let mut depth = 0usize;
    // Text is collected only until the current element sprouts a grandchild.
    // Fusing the runs on either side of one would invent a string the file does
    // not contain — the reader refuses to, and a reference that did it anyway
    // would report the refusal as a difference.
    let mut collecting = false;
    for ev in xml::scan(&payload) {
        match ev {
            xml::Event::Open {
                name,
                attrs,
                self_closing,
            } => {
                if depth == 1 {
                    out.push(Block6Elem {
                        name: name.clone(),
                        text: String::new(),
                        attrs,
                    });
                    collecting = !self_closing;
                } else if depth >= 2 {
                    collecting = false;
                }
                if !self_closing {
                    depth += 1;
                }
            }
            xml::Event::Close { .. } => depth = depth.saturating_sub(1),
            xml::Event::Text(t) => {
                if depth == 2 && collecting {
                    if let Some(e) = out.last_mut() {
                        e.text.push_str(&t);
                    }
                }
            }
        }
    }
    for e in &mut out {
        e.text = e.text.trim().to_string();
    }
    out
}

/// Every qualifier in a document's block 10, read straight out of the payload.
///
/// One `Vec` per `<Feature>` element, in file order, so the result indexes the
/// same way `Molecule::features` does.
///
/// This is block 10's independent side, and block 10 did not have one. The
/// comparison in the test below runs `x.segments != y.segments` and
/// `a.features != b.features` with *both* sides produced by
/// `snapgene::parse_features`, so anything that reader drops is dropped
/// identically on both and cancels — exactly the trap this test's own doc
/// comment describes for block 6, which was given an independent side and block
/// 10 was not. It cost a whole vintage of real files every qualifier they
/// carry: SnapGene spells them `<Q>`/`<V>` in files at export version 11 and
/// above and `<Qualifier>`/`<QualifierValue …Val>` at 10/5, only the short form
/// was matched, and `/codon_start`, `/transl_table`, `/locus_tag` and the entire
/// protein `/translation` were lost on read with nothing reporting it and this
/// test green.
///
/// So the recognition rule here is deliberately **shape-based rather than a copy
/// of the reader's tag list**: any direct child of `<Feature>` that is not a
/// `<Segment>` and carries a `name` attribute is a qualifier, whatever it is
/// called, and its value is whichever typed attribute its own child carries. A
/// reference that shared the reader's list of element names would share its
/// blind spot and certify the loss.
fn block_ten_qualifiers(doc: &snapgene::Document) -> Vec<Vec<(String, Option<String>)>> {
    let Some(b) = doc
        .blocks
        .iter()
        .find(|b| b.kind == snapgene::block::FEATURES)
    else {
        return Vec::new();
    };
    let payload = String::from_utf8_lossy(&b.payload);
    let mut out: Vec<Vec<(String, Option<String>)>> = Vec::new();
    let mut depth = 0usize;
    let mut pending: Option<(String, Option<String>)> = None;
    for ev in xml::scan(&payload) {
        match ev {
            xml::Event::Open {
                name,
                attrs,
                self_closing,
            } => {
                if depth == 1 && name == "Feature" {
                    out.push(Vec::new());
                } else if depth == 2 && name != "Segment" {
                    if let Some(k) = attrs
                        .iter()
                        .find(|(k, _)| k == "name")
                        .map(|(_, v)| v.clone())
                    {
                        if self_closing {
                            if let Some(f) = out.last_mut() {
                                f.push((k, None));
                            }
                        } else {
                            pending = Some((k, None));
                        }
                    }
                } else if depth == 3 {
                    if let Some(p) = pending.as_mut() {
                        if let Some((_, v)) = attrs.iter().find(|(k, _)| {
                            matches!(
                                k.as_str(),
                                "text" | "textVal" | "int" | "intVal" | "predef" | "predefVal"
                            )
                        }) {
                            p.1 = Some(v.clone());
                        }
                    }
                }
                if !self_closing {
                    depth += 1;
                }
            }
            xml::Event::Close { .. } => {
                depth = depth.saturating_sub(1);
                if depth == 2 {
                    if let (Some(p), Some(f)) = (pending.take(), out.last_mut()) {
                        f.push(p);
                    }
                }
            }
            xml::Event::Text(_) => {}
        }
    }
    out
}

/// Synthesise a `.dna` from the molecule alone, on every real file.
///
/// `snapgene::write` re-emits the blocks it read, so a byte-exact round-trip
/// proves the *container* survives and says nothing about whether the
/// annotations were understood. `from_molecule` throws the original blocks away
/// and rebuilds the file from the parsed model, so anything the reader did not
/// understand or the writer cannot express is simply gone — and this compares
/// what comes back.
///
/// The distinction matters here more than usual: the memory of how the format
/// was worked out records that byte-exact round-tripping on 41 files proved
/// nothing about coordinate *interpretation*, because an off-by-one on read
/// cancels on write. A model→file→model comparison cannot cancel **an error the
/// writer makes**, which is what it was built for.
///
/// **It cancels a loss the *reader* makes, and that is not a hypothetical.**
/// `a.notes != b.notes` was added here and gave notes no cover whatever: both
/// sides come out of the same `parse_notes`, so a `<Created UTC="22:0:0">` whose
/// attribute was discarded on the way in was discarded identically on both
/// sides, the two `Vec`s compared equal, and the recorded time of day was gone
/// from the rebuilt file with this test green. The same held for the `<Empty/>`
/// child the reader dropped and for every nested subtree. Anything read
/// destructively is invisible to every comparison in this loop, however many
/// fields it grows — so notes are additionally checked **against the original
/// block 6 bytes**, below, which is the only form that has an independent side.
///
/// **"Everything" means every field of the molecule**, which it did not: it
/// compared `seq`, `topology`, `methylation` and the features, and said nothing
/// about `primers`, `notes`, `double_stranded` or `description`. A test named
/// for a total claim that checks four fields out of eight is how block 5 came to
/// be dropped by the writer on 12 of these 41 files while this stayed green.
#[test]
fn a_synthesised_dna_preserves_everything_the_model_holds() {
    let files = files_with(&["dna"]);
    if files.is_empty() {
        eprintln!("skipping: set PL_CORPUS");
        return;
    }
    let (mut checked, mut features, mut quals) = (0usize, 0usize, 0usize);
    let (mut primers, mut sites) = (0usize, 0usize);
    let (mut notes_elems, mut notes_attrs, mut nested_reported) = (0usize, 0usize, 0usize);
    // Qualifiers checked against the original block 10 bytes, which is a
    // different and stronger claim than the `quals` above: that one counts
    // qualifiers the model held on both sides of a rebuild.
    let mut quals_in_file = 0usize;
    let mut problems: Vec<String> = Vec::new();

    for f in &files {
        let Ok(data) = std::fs::read(f) else { continue };
        let Ok(orig) = snapgene::parse(&data) else {
            continue;
        };

        let rebuilt = match snapgene::parse(&snapgene::from_molecule(&orig.molecule)) {
            Ok(d) => d,
            Err(e) => {
                problems.push(format!(
                    "{}: cannot re-read what we wrote: {e:?}",
                    f.display()
                ));
                continue;
            }
        };
        checked += 1;
        let (a, b) = (&orig.molecule, &rebuilt.molecule);

        if a.seq != b.seq {
            problems.push(format!("{}: sequence changed", f.display()));
        }
        if a.topology != b.topology {
            problems.push(format!(
                "{}: topology {:?} became {:?}",
                f.display(),
                a.topology,
                b.topology
            ));
        }
        if a.methylation != b.methylation {
            problems.push(format!("{}: methylation changed", f.display()));
        }
        // Primers, notes, strandedness and description were never compared, so
        // the test that advertises itself as the one that "cannot cancel" an
        // on-read/on-write error gave no cover at all to block 5: the writer
        // emitted blocks 9, 0, 10 and 6 and never a primer block, so every
        // primer parsed from a real file was dropped on rebuild and this test
        // still printed "synthesised 41 .dna file(s)" and passed. 12 of the 41
        // corpus files carry a block 5, so this comparison would have failed on
        // 12 of them the day it was written.
        primers += a.primers.len();
        sites += a.primers.iter().map(|p| p.sites.len()).sum::<usize>();
        if a.primers != b.primers {
            problems.push(format!(
                "{}: {} primer(s) with {} binding site(s) became {} with {}",
                f.display(),
                a.primers.len(),
                a.primers.iter().map(|p| p.sites.len()).sum::<usize>(),
                b.primers.len(),
                b.primers.iter().map(|p| p.sites.len()).sum::<usize>()
            ));
        }
        if a.notes != b.notes {
            problems.push(format!(
                "{}: notes {:?} became {:?}",
                f.display(),
                a.notes,
                b.notes
            ));
        }
        // ...and against the file, because the comparison above cannot see a
        // loss that happened before `a` existed. See this test's doc comment.
        //
        // Structure, not bytes: the payloads legitimately differ in whitespace
        // between tags, in `<Empty/>` versus `<Empty></Empty>`, and in entity
        // normalisation — one real file's `<Comments>` contains the asymmetric
        // `&lt;br>`, which unescape-then-escape renders as `&lt;br&gt;`, the
        // same text spelled differently. Comparing bytes here would fail for
        // reasons that are not losses and teach the next reader to delete the
        // check.
        //
        // Positionally, not by name lookup. A `find` by name answers with the
        // first match for every repeat, so a file with two `<Comments>` had one
        // of them checked twice and the other not at all, and an element the
        // writer *added* was invisible from either side. Order is part of what
        // is being preserved here — nothing in block 6 is schema-constrained, so
        // file order is the only order there is.
        let orig_b6 = block_six(&orig);
        let rebuilt_b6 = block_six(&rebuilt);
        if orig_b6.len() != rebuilt_b6.len() {
            problems.push(format!(
                "{}: block 6 has {} element(s) and we wrote {}: {:?} vs {:?}",
                f.display(),
                orig_b6.len(),
                rebuilt_b6.len(),
                orig_b6.iter().map(|e| &e.name).collect::<Vec<_>>(),
                rebuilt_b6.iter().map(|e| &e.name).collect::<Vec<_>>(),
            ));
        }
        for (i, e) in orig_b6.iter().enumerate() {
            let Some(out) = rebuilt_b6.get(i) else {
                problems.push(format!(
                    "{}: block 6 element <{}> is in the file and not in what we wrote",
                    f.display(),
                    e.name
                ));
                notes_elems += 1;
                continue;
            };
            if out.name != e.name {
                problems.push(format!(
                    "{}: block 6 element {i} is <{}> in the file and <{}> in what we wrote",
                    f.display(),
                    e.name,
                    out.name
                ));
            }
            // The text, which is the larger half of most notes and was not
            // compared at all. Trimmed on both sides and unescaped by the same
            // scanner, so `&lt;br>` versus `&lt;br&gt;` — one real file has it —
            // compares equal, as the paragraph above requires.
            if out.text != e.text {
                problems.push(format!(
                    "{}: block 6 <{}> held {:?} and we wrote {:?}",
                    f.display(),
                    e.name,
                    e.text,
                    out.text
                ));
            }
            for (an, av) in &e.attrs {
                if !out.attrs.iter().any(|(bn, bv)| bn == an && bv == av) {
                    problems.push(format!(
                        "{}: block 6 <{} {an}=\"{av}\"> did not survive; we wrote {:?}",
                        f.display(),
                        e.name,
                        out.attrs
                    ));
                }
            }
            notes_attrs += e.attrs.len();
            notes_elems += 1;
        }
        if !orig.unrepresentable_notes.is_empty() {
            nested_reported += orig.unrepresentable_notes.len();
        }
        // Block 10 against the file, for the same reason and with the same
        // shape: the feature comparison above has both sides out of one reader
        // and cancels anything that reader drops. See `block_ten_qualifiers`.
        for (i, want) in block_ten_qualifiers(&orig).iter().enumerate() {
            let Some(feat) = a.features.get(i) else {
                problems.push(format!(
                    "{}: block 10 holds feature {i} and the model does not",
                    f.display()
                ));
                continue;
            };
            for q in want {
                if !feat.qualifiers.contains(q) {
                    problems.push(format!(
                        "{}: feature {:?} lost qualifier {:?} = {:?}, which is in block 10; \
                         the model holds {:?}",
                        f.display(),
                        feat.name,
                        q.0,
                        q.1,
                        feat.qualifiers
                    ));
                }
            }
            quals_in_file += want.len();
        }
        if a.double_stranded != b.double_stranded {
            problems.push(format!(
                "{}: double_stranded {:?} became {:?}",
                f.display(),
                a.double_stranded,
                b.double_stranded
            ));
        }
        if a.description != b.description {
            problems.push(format!(
                "{}: description {:?} became {:?}",
                f.display(),
                a.description,
                b.description
            ));
        }
        if a.features.len() != b.features.len() {
            problems.push(format!(
                "{}: {} features became {}",
                f.display(),
                a.features.len(),
                b.features.len()
            ));
            continue;
        }
        for (x, y) in a.features.iter().zip(&b.features) {
            features += 1;
            quals += x.qualifiers.len();
            if x.name != y.name || x.kind != y.kind || x.strand != y.strand {
                problems.push(format!("{}: feature {} changed", f.display(), x.name));
            }
            if x.segments != y.segments {
                problems.push(format!(
                    "{}: {} segments {:?} became {:?}",
                    f.display(),
                    x.name,
                    x.segments,
                    y.segments
                ));
            }
            // Valueless qualifiers are the specific thing that used to be lost:
            // `/pseudo` is not `/replace=""`, and dropping it turns a
            // pseudogene into an ordinary protein-coding gene.
            if x.qualifiers != y.qualifiers {
                problems.push(format!("{}: {} qualifiers changed", f.display(), x.name));
            }
        }
    }

    eprintln!(
        "synthesised {checked} .dna file(s) from the model alone:          {features} features, \
         {quals} qualifiers, {primers} primers, {sites} binding sites preserved"
    );
    eprintln!(
        "                                                                  {notes_elems} note \
         elements and {notes_attrs} note attributes checked against the original block 6; \
         {nested_reported} nested element(s) reported as unrepresentable"
    );
    eprintln!(
        "                                                                  {quals_in_file} \
         qualifier(s) checked against the original block 10"
    );
    assert!(checked > 0, "no .dna files parsed");
    assert!(
        problems.is_empty(),
        "{} problem(s):
{}",
        problems.len(),
        problems.iter().take(10).cloned().collect::<Vec<_>>().join(
            "
"
        )
    );
}
