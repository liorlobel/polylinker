//! End-to-end: the real database against real plasmids.
//!
//! Gated on `PL_CORPUS`, like the other corpus suites, so contributors without a
//! pile of `.dna` files are not blocked.
//!
//! # What agreement with SnapGene means here, and what it does not
//!
//! `docs/PLAN.md` §7.7 is explicit: files annotated by SnapGene are a **soft**
//! reference. Their annotations are SnapGene's output, so agreement measures
//! *compatibility*, never truth. This suite therefore reports agreement as a
//! number and asserts only on things that are true independently of anyone's
//! curation — that a called feature's coordinates really do contain the sequence
//! claimed, that a CDS call really is in frame, that nothing is called twice.
//!
//! The distinction matters legally as well as scientifically. Reading their
//! annotations to *measure* us is fine; using them to *decide what to include*
//! would make the database derivative of the thing it exists to replace.

use std::path::{Path, PathBuf};

use pl_core::translate;
use pl_features::annotate::{Annotator, Config};
use pl_features::Db;

fn corpus() -> Option<PathBuf> {
    std::env::var("PL_CORPUS")
        .ok()
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
}

fn load_db() -> Option<Db> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../features");
    let f = std::fs::read_to_string(root.join("features.tsv")).ok()?;
    let p = std::fs::read_to_string(root.join("provenance.tsv")).ok()?;
    // Read from disk, not `Db::builtin()`, so this suite measures the tables in
    // the working tree rather than the ones compiled into the test binary. A
    // missing sign-off table is the safe degenerate case — everything reads as
    // `proposed` — so it is defaulted rather than made a hard requirement here.
    let s = std::fs::read_to_string(root.join("SIGNOFF.tsv")).unwrap_or_default();
    let (db, errors) = Db::parse(&f, &p, &s);
    assert!(
        errors.is_empty(),
        "the shipped database does not satisfy its own schema:\n  {}",
        errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("\n  ")
    );
    Some(db)
}

fn dna_files(root: &Path, limit: usize) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| {
                let x = x.to_string_lossy().to_lowercase();
                x == "dna" || x == "gb" || x == "gbk"
            }) {
                out.push(p);
                if out.len() >= limit {
                    return out;
                }
            }
        }
    }
    out
}

#[test]
fn the_shipped_database_satisfies_its_own_schema() {
    let Some(db) = load_db() else {
        eprintln!("skipping: features/features.tsv not built");
        return;
    };
    assert!(!db.records.is_empty());
    let dups = db.duplicates();
    assert!(dups.is_empty(), "{dups:?}");

    // Every record must be able to say where its sequence came from, and under
    // what licence. This is the promise the database is named for.
    //
    // WHICH column holds "its sequence" now depends on the row: a peptide-only
    // synthetic part has residues and no bases, so demanding `reference_nt`
    // provenance for it would demand a source for a field that is deliberately
    // empty. The promise is unchanged; only the column it is asked of moves.
    for r in &db.records {
        let field = if r.is_peptide_only() {
            "reference_aa"
        } else {
            "reference_nt"
        };
        let prov = db.provenance_of(&r.id);
        assert!(
            prov.iter()
                .any(|p| p.field == field && !p.licence.is_empty()),
            "{} has no licensed provenance for its {field}",
            r.id
        );
    }
    eprintln!(
        "database {} — {} records, licences in play: {:?}",
        db.version,
        db.records.len(),
        db.licences()
    );
    eprintln!("census: {:?}", db.census());
}

#[test]
fn every_record_carrying_both_sequences_translates_from_one_to_the_other() {
    // The build script checks this; so does this test, independently, because
    // a database whose nucleotides and protein disagree would annotate the same
    // feature at two different places and look like a matcher bug.
    //
    // Renamed from `every_coding_record_translates_to_the_protein_it_claims`,
    // because since 2026-07-28 that is no longer what it checks: a peptide-only
    // synthetic part carries a protein and no nucleotides, so there is nothing
    // to translate *from*. Skipping on the absent nucleotides rather than on
    // the absent protein is the whole fix — `TABLE11.translate(&[])` returns
    // empty without panicking, so the old skip let the run reach
    // `assert_eq!("", "DYKDDDDK")` and fail by assertion on the first tag.
    //
    // The `checked > 0` floor below is what keeps that placement honest: a bug
    // that emptied every `reference_nt` would otherwise turn this into a test
    // that skips everything and passes.
    let Some(db) = load_db() else { return };
    let mut checked = 0;
    let mut peptide_only = 0;
    for r in &db.records {
        let Some(aa) = r.reference_aa.as_ref() else {
            continue;
        };
        if r.reference_nt.is_empty() {
            assert!(
                r.is_peptide_only() && r.class == pl_features::Class::SyntheticPart,
                "{}: no nucleotides, and not a peptide-only synthetic part either",
                r.id
            );
            peptide_only += 1;
            continue;
        }
        let mut got = translate::TABLE11.translate(&r.reference_nt);
        while got.last() == Some(&b'*') {
            got.pop();
        }
        // An alternative initiation codon reads as Met when it initiates.
        if translate::TABLE11.is_start(&r.reference_nt[..3.min(r.reference_nt.len())])
            && !got.is_empty()
        {
            got[0] = b'M';
        }
        assert_eq!(
            String::from_utf8_lossy(&got),
            String::from_utf8_lossy(aa),
            "{} ({}): nucleotides do not translate to the stored protein",
            r.id,
            r.name
        );
        checked += 1;
    }
    assert!(checked > 0);
    eprintln!(
        "{checked} record(s) translate exactly; {peptide_only} peptide-only synthetic \
         part(s) have no nucleotides to translate from"
    );
}

/// A tag fused to a real GTG-started marker from this project's own table.
///
/// Not gated on `PL_CORPUS`: the substrate is the shipped database, so it runs
/// everywhere, and it is the shipped rows that carry the property. Five of the
/// 38 CDS rows begin `GTG` — `TetA`, `AprR`, `HygR`, `lacI` and lambda `int` —
/// and while `Config::code` defaulted to table 1, `find_orfs` reported no ORF
/// over any of them, so an N-terminal FLAG on any of the five was dropped with
/// no output of any kind. The synthetic sweep in `annotate.rs` states the same
/// property against whatever start set the configured code has; this one states
/// it against real markers, because "five of our own rows" is the fact that
/// makes it matter.
#[test]
fn a_tag_fused_to_a_real_gtg_started_marker_is_found() {
    let Some(db) = load_db() else {
        eprintln!("skipping: features/features.tsv not built");
        return;
    };
    let Some(flag) = db.records.iter().find(|r| r.id == "PLF:3000") else {
        eprintln!("skipping: PLF:3000 (FLAG tag) is not in the built table");
        return;
    };
    let aa = flag.reference_aa.clone().expect("FLAG carries a peptide");

    // Back-translate FLAG with the first codon in NCBI's TCAG order for each
    // residue. Generated here, never recalled — and which synonymous encoding
    // it is does not matter, because a peptide-only row is found by translation
    // and by nothing else.
    let code = Config::default().code;
    let mut tag = Vec::new();
    for &residue in &aa {
        let c = (0..64usize)
            .map(|i| [b"TCAG"[i / 16], b"TCAG"[(i / 4) % 4], b"TCAG"[i % 4]])
            .find(|c| code.codon(c) == residue)
            .unwrap_or_else(|| panic!("no codon encodes {}", residue as char));
        tag.extend_from_slice(&c);
    }

    let ann = Annotator::new(&db, Config::default());
    let mut gtg = 0usize;
    for r in &db.records {
        if r.class != pl_features::Class::Cds || r.reference_nt.len() < 300 {
            continue;
        }
        if &r.reference_nt[..3] != b"GTG" {
            continue;
        }
        gtg += 1;
        // The tag spliced in frame immediately after the initiator: an
        // N-terminal fusion, which is where His/FLAG/Strep tags usually go and
        // so where this bug bit hardest.
        let mut seq = r.reference_nt[..3].to_vec();
        seq.extend_from_slice(&tag);
        seq.extend_from_slice(&r.reference_nt[3..]);
        let mol = pl_core::Molecule {
            seq,
            topology: pl_core::Topology::Linear,
            ..Default::default()
        };
        let found: Vec<_> = ann
            .annotate(&mol)
            .into_iter()
            .filter(|a| db.records[a.record].id == "PLF:3000")
            .collect();
        assert_eq!(
            found.len(),
            1,
            "{} ({}) starts GTG and an N-terminal FLAG on it was not reported",
            r.id,
            r.name
        );
        assert_eq!(found[0].start, 4, "{}: the tag follows the initiator", r.id);
        assert!(
            found[0].fusion_orf.is_some(),
            "{}: reported with no ORF evidence",
            r.id
        );
    }
    assert!(
        gtg >= 3,
        "this test needs GTG-started CDS rows to have anything to say; found {gtg}"
    );
    eprintln!("{gtg} GTG-started marker(s) carry an N-terminal fusion correctly");
}

#[test]
fn markers_are_found_in_real_plasmids_at_coordinates_that_hold_them() {
    let (Some(db), Some(root)) = (load_db(), corpus()) else {
        eprintln!("skipping: needs PL_CORPUS and a built database");
        return;
    };
    let ann = Annotator::new(&db, Config::default());
    let files = dna_files(&root, 120);
    assert!(!files.is_empty());

    let mut with_hits = 0usize;
    let mut total = 0usize;
    let mut by_name: std::collections::BTreeMap<String, usize> = Default::default();
    let mut via_protein = 0usize;
    let mut fragments = 0usize;
    let mut scanned = 0usize;

    for path in &files {
        let Ok(raw) = std::fs::read(path) else {
            continue;
        };
        let Ok((mol, _)) = pl_fileio::load(&raw) else {
            continue;
        };
        if mol.seq.len() < 200 {
            continue;
        }
        scanned += 1;
        let hits = ann.annotate(&mol);
        if !hits.is_empty() {
            with_hits += 1;
        }
        for h in &hits {
            total += 1;
            let rec = &db.records[h.record];
            *by_name.entry(rec.name.clone()).or_default() += 1;
            if h.via_protein {
                via_protein += 1;
            }
            if h.is_fragment {
                fragments += 1;
            }

            // The assertion that does not depend on anyone's curation: the
            // coordinates we reported must actually contain what we said.
            let len = mol.seq.len() as u64;
            assert!(
                h.start >= 1 && h.start <= len,
                "{}: start out of range",
                rec.id
            );
            assert!(h.end >= 1 && h.end <= len, "{}: end out of range", rec.id);
            let n = h.len(len);
            // The reference's own length in *bases*, whichever alphabet it is
            // stored in. A peptide-only synthetic part reports 0 nucleotides
            // while covering three bases per residue, so measuring it against
            // `reference_nt.len()` would cap a 38-residue SBP tag at 64 bases
            // and fail on a correct call.
            let db_bases = if rec.reference_nt.is_empty() {
                3 * rec.units() as u64
            } else {
                rec.reference_nt.len() as u64
            };
            assert!(
                n > 0 && n <= db_bases + 64,
                "{} in {:?}: reported {} bases for a {} base feature",
                rec.id,
                path.file_name().unwrap(),
                n,
                db_bases
            );

            // Re-extract and re-check identity from scratch.
            let observed: Vec<u8> = if h.wraps_origin {
                let mut v = mol.seq[(h.start - 1) as usize..].to_vec();
                v.extend_from_slice(&mol.seq[..h.end as usize]);
                v
            } else {
                mol.seq[(h.start - 1) as usize..h.end as usize].to_vec()
            };
            assert_eq!(observed.len() as u64, n);
            let subject = if h.strand == pl_core::Strand::Reverse {
                pl_core::iupac::reverse_complement(&observed)
            } else {
                observed
            };
            let budget = (rec.reference_nt.len() as f64 * 0.10).ceil() as u32;
            if !h.via_protein && !h.is_fragment {
                let re = pl_features::align::infix(&rec.reference_nt, &subject, budget);
                assert!(
                    re.is_some(),
                    "{} in {:?}: the coordinates reported do not re-align to the feature",
                    rec.id,
                    path.file_name().unwrap()
                );
            }
        }
    }

    eprintln!("\nannotated {scanned} molecules from the corpus");
    eprintln!("  {with_hits} had at least one hit; {total} hits total");
    eprintln!("  {via_protein} found by translation, {fragments} fragments");
    for (name, n) in &by_name {
        eprintln!("  {n:>4}  {name}");
    }
    assert!(
        with_hits > 0,
        "a database of the commonest resistance markers found nothing in {scanned} real plasmids — \
         that is a matcher failure, not a coverage result"
    );
}

#[test]
fn agreement_with_existing_annotations_is_measured_not_asserted() {
    // SnapGene's own calls, used strictly as a compatibility yardstick. See the
    // module docstring for why this must never become an assertion about truth.
    let (Some(db), Some(root)) = (load_db(), corpus()) else {
        return;
    };
    let ann = Annotator::new(&db, Config::default());

    let mut agreed = 0usize;
    let mut ours_only = 0usize;
    let mut examples: Vec<String> = Vec::new();

    for path in dna_files(&root, 120) {
        let Ok(raw) = std::fs::read(&path) else {
            continue;
        };
        let Ok((mol, _)) = pl_fileio::load(&raw) else {
            continue;
        };
        if mol.seq.len() < 200 || mol.features.is_empty() {
            continue;
        }
        for h in ann.annotate(&mol) {
            let rec = &db.records[h.record];
            // Does an existing annotation overlap the same span at all?
            let overlap = mol.features.iter().any(|f| {
                f.segments.iter().any(|s| {
                    let (a, b) = (s.start.min(s.end), s.start.max(s.end));
                    h.start <= b && a <= h.end
                })
            });
            if overlap {
                agreed += 1;
            } else {
                ours_only += 1;
                if examples.len() < 5 {
                    examples.push(format!(
                        "{}: {} at {}..{} has no counterpart",
                        path.file_name().unwrap().to_string_lossy(),
                        rec.name,
                        h.start,
                        h.end
                    ));
                }
            }
        }
    }
    let total = agreed + ours_only;
    if total == 0 {
        eprintln!("no overlapping calls to compare");
        return;
    }
    eprintln!(
        "\ncompatibility with existing annotations: {agreed}/{total} of our calls \
         ({:.0}%) land where the file already had a feature",
        100.0 * agreed as f64 / total as f64
    );
    for e in &examples {
        eprintln!("  {e}");
    }
    eprintln!("(a yardstick, not a score: those annotations are SnapGene's output)");
}
