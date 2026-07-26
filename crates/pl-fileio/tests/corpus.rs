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
use pl_fileio::{detect, genbank, snapgene, Format};

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
        let n = src.span();
        let sites = src
            .primers
            .iter()
            .flat_map(|p| &p.sites)
            .filter(|s| s.start >= 1 && s.end <= n && s.end >= s.start)
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
        if let (Some(exp), Some(g)) = (expected, got) {
            if exp != g {
                mismatched.push(format!(
                    "{}: .{ext} but content is {}",
                    path.display(),
                    g.name()
                ));
            }
        }
    }

    for (k, v) in &counts {
        eprintln!("  {v:>5}  {k}");
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
