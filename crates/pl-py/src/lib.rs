//! Python bindings.
//!
//! # Why these exist
//!
//! Biopython and pydna are where this kind of work is actually done, and the
//! answer to "should I use Polylinker or Biopython" should not be "rewrite your
//! pipeline". The bindings expose the parts that are hard to get right and are
//! cross-validated against those very libraries — restriction digestion across
//! an origin, SEGUID v2, the 27 genetic codes, nearest-neighbour Tm — so they
//! can be used from a script that otherwise stays exactly as it is.
//!
//! # Errors are exceptions, not sentinel values
//!
//! Every fallible call raises. A Tm that could not be computed comes back as a
//! `ValueError` naming the offending base, not as `None` or `0.0` — a caller
//! writing `if tm > 60` sees `0.0` as a cold oligo rather than as a failure,
//! and that is the failure mode these bindings exist to avoid rather than
//! introduce.
//!
//! # This is the boundary, not the logic
//!
//! Everything here forwards to a correctness crate. Nothing is decided in this
//! file, so a Python caller and a `pl` command cannot disagree.
//!
//! Forwarding the *call* is not enough for that to hold, because a field
//! dropped on the way back out is a decision too. `open_reading_frames` used to
//! return neither `laps` nor `wrapped`, and on a 1,000 bp circle carrying a
//! 3,000-base ORF `pl orfs` printed "crosses origin" while the tuple this file
//! documented said it did not. So a result carries every field its correctness
//! crate had to add in order to be unambiguous, rather than a summary of them.
//! (`enzymes()` is a catalogue rather than a result of a computation, and is
//! deliberately just the name and the site.)

use pyo3::exceptions::{PyKeyError, PyValueError};
use pyo3::prelude::*;

/// Reverse complement, preserving case and IUPAC ambiguity codes.
#[pyfunction]
fn reverse_complement(seq: &str) -> String {
    String::from_utf8_lossy(&pl_core::reverse_complement(seq.as_bytes())).into_owned()
}

/// SEGUID v2 for a linear single strand.
#[pyfunction]
fn lsseguid(seq: &str) -> PyResult<String> {
    pl_core::lsseguid(seq).map_err(|e| PyValueError::new_err(format!("{e:?}")))
}

/// SEGUID v2 for a circular double strand.
///
/// Invariant to the choice of origin and to which strand is written first, so
/// the same physical plasmid gives the same checksum whoever exported it.
#[pyfunction]
#[pyo3(signature = (watson, crick=None))]
fn cdseguid(watson: &str, crick: Option<&str>) -> PyResult<String> {
    let c = match crick {
        Some(c) => c.to_string(),
        None => {
            String::from_utf8_lossy(&pl_core::reverse_complement(watson.as_bytes())).into_owned()
        }
    };
    pl_core::cdseguid(watson, &c).map_err(|e| PyValueError::new_err(format!("{e:?}")))
}

/// Nearest-neighbour melting temperature, in Celsius.
///
/// Raises rather than returning a sentinel: `0.0` reads as a cold oligo to
/// `if tm > 60`, and a caller must not be able to mistake a failure for a
/// measurement.
#[pyfunction]
#[pyo3(signature = (oligo, na_mM=50.0, oligo_nM=50.0))]
#[allow(non_snake_case)]
fn melting_temperature(oligo: &str, na_mM: f64, oligo_nM: f64) -> PyResult<f64> {
    let m = pl_thermo::Method {
        na_molar: na_mM / 1000.0,
        oligo_molar: oligo_nM / 1e9,
        ..Default::default()
    };
    pl_thermo::tm(oligo.as_bytes(), &m)
        .map(|t| t.tm)
        .map_err(|e| PyValueError::new_err(format!("{e:?}")))
}

/// Where an enzyme cuts, 1-based, on both strands, across the origin.
///
/// Positions follow Biopython's Restriction convention exactly — the base
/// immediately 3' of the top-strand nick — so a comparison is an equality check
/// rather than an argument about off-by-one.
#[pyfunction]
#[pyo3(signature = (seq, enzyme, circular=false))]
fn cut_positions(seq: &str, enzyme: &str, circular: bool) -> PyResult<Vec<u64>> {
    let e = pl_enzymes::by_name(enzyme)
        .ok_or_else(|| PyKeyError::new_err(format!("no enzyme named {enzyme:?}")))?;
    let topology = if circular {
        pl_core::Topology::Circular
    } else {
        pl_core::Topology::Linear
    };
    Ok(pl_enzymes::cut_positions(seq.as_bytes(), topology, e))
}

/// Fragment lengths, descending, from a digest with one or more enzymes.
///
/// Several enzymes means one tube: their cut positions are merged. Running each
/// separately and concatenating gives fragments that are too long, which is
/// usually the whole reason for doing a double digest.
#[pyfunction]
#[pyo3(signature = (seq, enzymes, circular=false))]
fn digest(seq: &str, enzymes: Vec<String>, circular: bool) -> PyResult<Vec<u64>> {
    let topology = if circular {
        pl_core::Topology::Circular
    } else {
        pl_core::Topology::Linear
    };
    let mut cuts = Vec::new();
    for name in &enzymes {
        let e = pl_enzymes::by_name(name)
            .ok_or_else(|| PyKeyError::new_err(format!("no enzyme named {name:?}")))?;
        cuts.extend(pl_enzymes::cut_positions(seq.as_bytes(), topology, e));
    }
    Ok(pl_enzymes::fragments_from_cuts(
        &cuts,
        seq.len() as u64,
        topology,
    ))
}

/// Every enzyme this build knows, with its site.
#[pyfunction]
fn enzymes() -> Vec<(String, String)> {
    pl_enzymes::ENZYMES
        .iter()
        .map(|e| (e.name.to_string(), e.site.to_string()))
        .collect()
}

/// Translate in frame 0 with an NCBI genetic code.
///
/// The code number is required and not defaulted, because 13 of the 27 do not
/// treat TGA as a stop and a silent default is how a mitochondrial construct
/// gets mistranslated.
#[pyfunction]
fn translate(seq: &str, table: u8) -> PyResult<String> {
    let code = pl_core::translate::table(table)
        .ok_or_else(|| PyKeyError::new_err(format!("no NCBI code {table}")))?;
    Ok(String::from_utf8_lossy(&code.translate(seq.as_bytes())).into_owned())
}

/// The NCBI codes this build carries: `(id, name, tga_is_a_stop)`.
#[pyfunction]
fn genetic_codes() -> Vec<(u8, String, bool)> {
    pl_core::translate::all_tables()
        .map(|c| (c.id, c.name().to_string(), c.is_stop(b"TGA")))
        .collect()
}

/// One ORF as Python sees it: start, end, laps, strand, length, start codon,
/// complete, wrapped.
///
/// A tuple rather than a class because it crosses the boundary once and a
/// caller unpacks it; naming the shape here keeps the signature readable.
///
/// `laps` and `wrapped` are here because they were once left out and a pair of
/// coordinates on a circle cannot stand in for either. The 19 bp circle
/// `CGTAATGCCTTTCCCTAAC` carries a 33-base, 10 aa ORF that came back as
/// `(5, 18, "+", 10, "ATG", True)`: `seq[4:18]` is fourteen bases — four codons
/// of a ten-residue protein — and `start < end`, so nothing in the tuple said
/// anything was missing.
type PyOrf = (u64, u64, u32, String, usize, String, bool, bool);

/// Open reading frames, as
/// `(start, end, laps, strand, aa_len, start_codon, complete, wrapped)`.
///
/// Coordinates are 1-based inclusive on the plus strand, and on a circular
/// molecule both are reduced modulo the length because that is all a position
/// on a circle can hold.
///
/// `wrapped`, not `end < start`, is the test for crossing the origin.
/// `end < start` does imply a crossing, but the converse is false, so a caller
/// who tests the coordinates disagrees with `pl orfs` on exactly the ORFs that
/// go furthest round.
///
/// `start..end` is likewise not the extent whenever `laps` is non-zero, which
/// happens when the length is not a multiple of three and one turn of a frame
/// walks more than a single physical lap. The length in bases is
///
/// ```text
/// aa_len * 3 + (3 if complete else 0)
///     == inclusive span of start..end round the circle + laps * len(seq)
/// ```
#[pyfunction]
#[pyo3(signature = (seq, table=11, circular=false, min_aa=30))]
fn open_reading_frames(
    seq: &str,
    table: u8,
    circular: bool,
    min_aa: usize,
) -> PyResult<Vec<PyOrf>> {
    let code = pl_core::translate::table(table)
        .ok_or_else(|| PyKeyError::new_err(format!("no NCBI code {table}")))?;
    let p = pl_core::orf::Params {
        min_aa,
        ..Default::default()
    };
    Ok(pl_core::orf::find_orfs(seq.as_bytes(), code, circular, &p)
        .into_iter()
        .map(|o| {
            (
                o.start,
                o.end,
                o.laps,
                if o.strand == pl_core::Strand::Reverse {
                    "-"
                } else {
                    "+"
                }
                .to_string(),
                o.aa_len,
                String::from_utf8_lossy(&o.start_codon).into_owned(),
                o.complete,
                o.wrapped,
            )
        })
        .collect())
}

/// The methods paragraph for an operation, with its limits.
#[pyfunction]
fn methods(topic: &str) -> PyResult<String> {
    pl_doc::topic(topic)
        .map(pl_doc::methods)
        .ok_or_else(|| PyKeyError::new_err(format!("no topic {topic:?}")))
}

#[pymodule]
fn polylinker(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add_function(wrap_pyfunction!(reverse_complement, m)?)?;
    m.add_function(wrap_pyfunction!(lsseguid, m)?)?;
    m.add_function(wrap_pyfunction!(cdseguid, m)?)?;
    m.add_function(wrap_pyfunction!(melting_temperature, m)?)?;
    m.add_function(wrap_pyfunction!(cut_positions, m)?)?;
    m.add_function(wrap_pyfunction!(digest, m)?)?;
    m.add_function(wrap_pyfunction!(enzymes, m)?)?;
    m.add_function(wrap_pyfunction!(translate, m)?)?;
    m.add_function(wrap_pyfunction!(genetic_codes, m)?)?;
    m.add_function(wrap_pyfunction!(open_reading_frames, m)?)?;
    m.add_function(wrap_pyfunction!(methods, m)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 19 bp circle whose ORF is longer than the molecule it sits on.
    ///
    /// 19 % 3 == 1, so the three reading frames merge into one and a single
    /// turn of that frame walks up to 3n bases. This carries a 33-base, 10 aa
    /// ORF that `pl_core::orf` reports as `start: 5, end: 18, laps: 1`.
    const LAPPING_CIRCLE: &str = "CGTAATGCCTTTCCCTAAC";

    /// What a Python caller receives for an ORF that laps the circle.
    ///
    /// Asserted through the interpreter, on the real `#[pyfunction]` wrapper,
    /// rather than by destructuring the tuple in Rust: a Rust pattern against
    /// the old six-field shape would not *compile*, and a test that fails to
    /// compile says nothing about what Python sees. Every assertion below is a
    /// runtime one, and against the six-field tuple the first of them fails.
    #[test]
    fn an_orf_that_laps_the_circle_says_so_to_python() {
        let n = LAPPING_CIRCLE.len() as u64;
        assert_eq!(n % 3, 1, "the merged-frame case is the whole point");

        Python::initialize();
        Python::attach(|py| {
            let f = wrap_pyfunction!(open_reading_frames, py).expect("the binding wraps");
            let orfs = f
                .call1((LAPPING_CIRCLE, 1u8, true, 1usize))
                .expect("table 1, circular, min_aa 1");

            // Checked before anything is unpacked, because on a six-field
            // tuple there is no index to read `laps` or `wrapped` from and the
            // rest of this test would be indexing off the end.
            for t in orfs.try_iter().expect("a sequence of ORFs came back") {
                let t = t.expect("an ORF tuple");
                assert_eq!(
                    t.len().expect("a tuple has a length"),
                    8,
                    "an ORF tuple must carry laps and wrapped, got {}",
                    t.repr().expect("repr")
                );
            }

            let mut hits = 0;
            for t in orfs.try_iter().expect("a sequence of ORFs came back") {
                let t = t.expect("an ORF tuple");
                let get = |i| t.get_item(i).expect("field in range");
                let start: u64 = get(0).extract().expect("start is an int");
                let strand: String = get(3).extract().expect("strand is a str");
                if start != 5 || strand != "+" {
                    continue;
                }
                hits += 1;
                let end: u64 = get(1).extract().expect("end is an int");
                let laps: u32 = get(2).extract().expect("laps is an int");
                let aa_len: u64 = get(4).extract().expect("aa_len is an int");
                let complete: bool = get(6).extract().expect("complete is a bool");
                let wrapped: bool = get(7).extract().expect("wrapped is a bool");

                assert_eq!(
                    (end, laps, aa_len, complete, wrapped),
                    (18, 1, 10, true, true)
                );
                // The coordinates read forwards, so the `end < start` rule
                // `open_reading_frames` used to document as *the* origin test
                // answers "does not cross" for an ORF that does. `pl orfs`
                // prints "crosses origin" on this same input, which is the
                // disagreement between the two front doors that the module
                // docs say cannot happen.
                assert!(
                    start < end,
                    "the case exists because the range reads forwards"
                );
                assert!(wrapped, "wrapped, not end < start, is the origin test");

                // And the coordinates alone are 14 of the ORF's 33 bases.
                let inclusive = end - start + 1;
                assert_eq!(inclusive, 14, "four codons of a ten-residue protein");
                assert_eq!(
                    inclusive + laps as u64 * n,
                    aa_len * 3 + 3,
                    "start..end plus laps must account for every base"
                );
            }
            assert_eq!(hits, 1, "the forward ORF at 5 is reported exactly once");
        });
    }
}
