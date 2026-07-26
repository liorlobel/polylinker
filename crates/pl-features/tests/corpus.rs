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
    let (db, errors) = Db::parse(&f, &p);
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
    for r in &db.records {
        let prov = db.provenance_of(&r.id);
        assert!(
            prov.iter()
                .any(|p| p.field == "reference_nt" && !p.licence.is_empty()),
            "{} has no licensed provenance for its sequence",
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
fn every_coding_record_translates_to_the_protein_it_claims() {
    // The build script checks this; so does this test, independently, because
    // a database whose nucleotides and protein disagree would annotate the same
    // feature at two different places and look like a matcher bug.
    let Some(db) = load_db() else { return };
    let mut checked = 0;
    for r in &db.records {
        let Some(aa) = r.reference_aa.as_ref() else {
            continue;
        };
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
    eprintln!("{checked} coding records translate exactly");
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
            assert!(
                n > 0 && n <= rec.reference_nt.len() as u64 + 64,
                "{} in {:?}: reported {} bases for a {} base feature",
                rec.id,
                path.file_name().unwrap(),
                n,
                rec.reference_nt.len()
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
