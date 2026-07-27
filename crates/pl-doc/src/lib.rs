//! Offline documentation, and the methods paragraph for a paper.
//!
//! # Why the methods text is generated and not written
//!
//! A methods section says what was computed, how, and with which parameters.
//! Prose in a manual drifts from the code the first time a default changes, and
//! the drift is invisible: the sentence still reads correctly and is no longer
//! true. So every number in the text below is **interpolated from the constant
//! the code actually uses**. Change the default oligo concentration and the
//! paragraph changes with it, or the build fails.
//!
//! That is also why this crate depends on the ones it describes rather than
//! restating them.
//!
//! # Every entry states what it cannot do
//!
//! A methods paragraph that only lists strengths is not a methods paragraph.
//! Each [`Topic`] carries a `limits` line, and the limits here are real ones:
//! the feature database ships nothing reviewed, the gel model is not a
//! measurement, Golden Gate fidelity is not computed because the matrices are
//! not ours to ship.
//!
//! # Offline
//!
//! Compiled in. A tool that asks nothing of anyone cannot have documentation
//! that needs a network, and a lab machine on an isolated network is a normal
//! place for this to run.

/// One documented operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Topic {
    /// What `pl methods <name>` answers to.
    pub name: &'static str,
    pub title: &'static str,
}

pub const TOPICS: &[Topic] = &[
    Topic {
        name: "tm",
        title: "Melting temperature",
    },
    Topic {
        name: "digest",
        title: "Restriction digestion",
    },
    Topic {
        name: "gel",
        title: "Agarose gel simulation",
    },
    Topic {
        name: "orfs",
        title: "Open reading frames and translation",
    },
    Topic {
        name: "sanger",
        title: "Sanger read comparison",
    },
    Topic {
        name: "annotate",
        title: "Feature annotation",
    },
    Topic {
        name: "checksum",
        title: "Sequence checksums",
    },
    Topic {
        name: "goldengate",
        title: "Golden Gate overhang sets",
    },
    Topic {
        name: "primers",
        title: "Primer binding sites",
    },
];

pub fn topic(name: &str) -> Option<Topic> {
    TOPICS
        .iter()
        .find(|t| t.name.eq_ignore_ascii_case(name))
        .copied()
}

/// The methods text for a topic: what was done, and what it does not establish.
///
/// Written in the past tense and the passive, which is what a methods section
/// wants, so it can be pasted and edited rather than rewritten.
pub fn methods(t: Topic) -> String {
    let v = env!("CARGO_PKG_VERSION");
    match t.name {
        "tm" => {
            let m = pl_thermo::Method::default();
            format!(
                "Melting temperatures were calculated with a nearest-neighbour model \
                 ({}), using Polylinker {v}. Parameters: {}. Tm is reported for the \
                 annealing footprint only; any 5' tail on a primer is excluded, since \
                 it does not pair with the template on the first cycle.\n\n\
                 Limits: the model assumes two-state hybridisation of a perfectly \
                 matched duplex in a well-mixed solution, and no correction is applied \
                 for Mg2+, dNTPs, DMSO or betaine, all of which shift Tm in a real PCR.",
                m.table_name,
                m.describe()
            )
        }
        "digest" => format!(
            "Restriction sites were located by exact matching of IUPAC recognition \
             sequences on both strands, using Polylinker {v}. Cut positions follow the \
             convention of Biopython's Restriction module: the 1-based position of the \
             base immediately 3' of the nick on the top strand. Circular molecules were \
             searched across the origin. Fragment lengths from a multiple digest were \
             computed from the union of the cut positions, with coincident cuts counted \
             once.\n\n\
             Limits: {} enzymes are included, transcribed from primary literature and \
             catalogue chemistry rather than from any vendor database, so this is not a \
             substitute for REBASE. Star activity, blocked sites and incomplete \
             digestion are not modelled. Methylation sensitivity is reported separately \
             and is not applied automatically.",
            pl_enzymes::ENZYMES.len()
        ),
        "gel" => {
            let c = pl_gel::Conditions::default();
            format!(
                "Agarose gel migration was simulated with Polylinker {v}. Band positions \
                 were interpolated with a monotone piecewise cubic (Fritsch & Carlson, \
                 SIAM J. Numer. Anal. 17:238, 1980), which cannot invert the order of two \
                 fragments. Default conditions: {:.1}% agarose, {:.0} mm run, {:.1} mm \
                 band width; fragments predicted to fall within one band width of each \
                 other were reported as a single band.\n\n\
                 Limits: unless a measured ladder was supplied, positions come from a \
                 model in which migration is linear in log10(length) across the \
                 published resolving range for that agarose percentage. This is adequate \
                 for deciding whether two fragments will separate and is **not** a basis \
                 for sizing an unknown band. Fragments outside the resolving range are \
                 not placed at all.",
                c.agarose_percent, c.run_mm, c.band_mm
            )
        }
        "orfs" => {
            let n = pl_core::translate::all_tables().count();
            let readthrough = pl_core::translate::all_tables()
                .filter(|c| !c.is_stop(b"TGA"))
                .count();
            format!(
                "Open reading frames were identified in all six frames with Polylinker \
                 {v}, using the NCBI genetic code specified for each molecule. An ORF \
                 was defined as a run beginning at an initiation codon permitted by that \
                 code and ending at the first in-frame termination codon; runs without \
                 an initiation codon were not reported. On circular molecules the search \
                 was synchronised to a termination codon before scanning, so results do \
                 not depend on where the sequence was linearised.\n\n\
                 Limits: all {n} NCBI codes are available and the code must be chosen \
                 correctly — {readthrough} of them do not treat TGA as a termination \
                 codon, so the wrong table silently reads through a stop or ends a \
                 protein early. In codes 27, 28 and 31 termination is context-dependent \
                 and an ORF boundary at one of those codons is a guess. Reading-frame \
                 analysis alone is not evidence that a sequence is translated.",
            )
        }
        "sanger" => {
            let p = pl_sanger::Params::default();
            format!(
                "Sanger reads were compared to their reference with Polylinker {v}. Both \
                 orientations were aligned and the higher-scoring one retained. \
                 Alignment was semi-global with affine gaps (match {}, mismatch {}, gap \
                 open {}, gap extend {}), the read being aligned in full while unmatched \
                 reference at either end was not penalised. Differences at Phred {} or \
                 above were treated as substantive; those below were reported separately \
                 and not counted. The reliable extent of each read was determined by \
                 Mott trimming.\n\n\
                 Limits: this compares one read to one reference and does not call \
                 variants. Agreement of independent reads, and the interpretation of a \
                 difference seen in only one, are judgements the software does not make. \
                 Differences in a read with no quality values are reported as \
                 undetermined rather than dismissed.",
                p.scoring.match_score,
                p.scoring.mismatch,
                p.scoring.gap_open,
                p.scoring.gap_extend,
                p.min_quality
            )
        }
        "annotate" => {
            let (db, _) = pl_features::Db::builtin();
            let reviewed = db.reviewed().records.len();
            format!(
                "Features were annotated with Polylinker {v} against its own curated \
                 library (release {}), by k-mer seeding followed by infix alignment, \
                 with protein-level matching in six frames so that recoded genes \
                 resolve. Default thresholds: {:.0}% identity, {:.0}% minimum coverage \
                 of the database feature.\n\n\
                 Limits: the library contains {} record(s), of which **{reviewed} have \
                 been reviewed by a named curator**. Unreviewed records were assembled \
                 by machine from public sources and are not shipped by default; any \
                 annotation derived from them is a suggestion to check against the cited \
                 accession, not an identification. A feature boundary is a claim, and \
                 each record states how its boundary was arrived at.",
                db.version,
                pl_features::annotate::Config::default().min_identity * 100.0,
                pl_features::annotate::Config::default().min_coverage * 100.0,
                db.records.len()
            )
        }
        "checksum" => format!(
            "Sequence identity was recorded with SEGUID v2 checksums (Babnigg & \
             Giometti, Proteomics 6:4514, 2006; v2 specification by Pereira et al., \
             2024) as implemented in Polylinker {v}. Circular double-stranded molecules \
             use cdseguid, which is invariant to the choice of origin and to which \
             strand is written first, so the same physical plasmid gives the same \
             checksum whoever exported it.\n\n\
             Limits: a checksum establishes that two sequences are identical, not that \
             either is correct. It covers bases only — annotations, topology metadata \
             and history are outside it, so two files with the same checksum may differ \
             in everything except the DNA."
        ),
        "goldengate" => format!(
            "Type IIS overhang sets were checked with Polylinker {v} for repeated \
             overhangs, palindromes, single-mismatch neighbours and cross-pairing \
             between the sense and antisense of different junctions.\n\n\
             Limits: **no fidelity percentage is reported.** Quantitative ligation \
             fidelity requires the experimentally measured overhang-ligation matrices \
             of Potapov et al. (ACS Synth. Biol. 7:2665, 2018; Nucleic Acids Res. \
             46:e79, 2018), which are not distributed with this software. The checks \
             above are structural and catch the common failures; they do not rank two \
             sets that both pass."
        ),
        "primers" => {
            let p = pl_primer::Params::default();
            format!(
                "Primer binding sites were located with Polylinker {v} using a \
                 3'-anchored seed of {} exact bases, extended toward the 5' end. The \
                 annealing footprint and any 5' tail are reported separately, and the \
                 melting temperature is computed from the footprint alone.\n\n\
                 Limits: this reports where a primer can anneal, not whether a product \
                 forms. Amplification efficiency, secondary structure, primer-dimer \
                 formation and 3'-end mismatch tolerance are not modelled.",
                p.seed_len
            )
        }
        _ => String::new(),
    }
}

/// A short description of what a topic's operation does, for in-app help.
pub fn help(t: Topic) -> &'static str {
    match t.name {
        "tm" => {
            "Nearest-neighbour melting temperature for an oligo. The footprint \
                 only — a 5' tail is not part of the duplex on the first cycle."
        }
        "digest" => {
            "Where restriction enzymes cut, on both strands, across the origin \
                     of a circular molecule."
        }
        "gel" => {
            "What a digest will look like on a gel, and — the useful part — which \
                  fragments will not separate."
        }
        "orfs" => {
            "Open reading frames in six frames, honouring the molecule's genetic \
                   code. 13 of the 27 NCBI codes do not stop at TGA."
        }
        "sanger" => {
            "A read against its reference: where it sits, which strand it was \
                     read from, and what differs, weighted by base quality."
        }
        "annotate" => {
            "Known features found in a plasmid. Nothing unreviewed is \
                       reported unless you ask for it."
        }
        "checksum" => {
            "A checksum that is the same for the same molecule however it was \
                       rotated, exported or which strand was written first."
        }
        "goldengate" => {
            "Structural problems in a Type IIS overhang set. No fidelity \
                         percentage — see the limits."
        }
        "primers" => {
            "Where a primer anneals, with the footprint and the tail kept \
                      apart."
        }
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_topic_has_methods_text_and_help() {
        for t in TOPICS {
            let m = methods(*t);
            assert!(m.len() > 200, "{} has {} chars of methods", t.name, m.len());
            assert!(!help(*t).is_empty(), "{} has no help", t.name);
            assert_eq!(topic(t.name), Some(*t));
            assert_eq!(topic(&t.name.to_uppercase()), Some(*t));
        }
        assert_eq!(topic("no such thing"), None);
    }

    #[test]
    fn every_methods_paragraph_states_its_limits() {
        // A methods section that lists only strengths is not a methods section.
        // This is the property the whole module exists to guarantee, so it is
        // asserted for all of them rather than trusted.
        for t in TOPICS {
            let m = methods(*t);
            assert!(
                m.contains("Limits:"),
                "{} does not say what it cannot establish",
                t.name
            );
            let after = m.split("Limits:").nth(1).unwrap_or("");
            assert!(after.len() > 80, "{}'s limits are a token gesture", t.name);
        }
    }

    #[test]
    fn the_numbers_come_from_the_code_and_not_from_prose() {
        // The reason this is generated. A default that changes must change the
        // paragraph, or the paragraph becomes a sentence that still reads
        // correctly and is no longer true.
        let m = methods(topic("tm").unwrap());
        let d = pl_thermo::Method::default();
        assert!(m.contains(&d.describe()), "{m}");
        assert!(m.contains(d.table_name));

        let s = methods(topic("sanger").unwrap());
        let p = pl_sanger::Params::default();
        assert!(
            s.contains(&format!("Phred {}", p.min_quality)),
            "the quality threshold is interpolated: {s}"
        );
        assert!(s.contains(&format!("gap open {}", p.scoring.gap_open)));

        let d = methods(topic("digest").unwrap());
        assert!(
            d.contains(&pl_enzymes::ENZYMES.len().to_string()),
            "the enzyme count is interpolated: {d}"
        );
    }

    #[test]
    fn the_annotation_methods_report_how_many_records_are_actually_reviewed() {
        // The number that matters most and is easiest to leave stale. It is
        // zero today, and the paragraph has to say so rather than describe a
        // library nobody has checked as though it were curated.
        let (db, _) = pl_features::Db::builtin();
        let m = methods(topic("annotate").unwrap());
        assert!(
            m.contains(&format!(
                "{} have been reviewed",
                db.reviewed().records.len()
            )),
            "{m}"
        );
        assert!(m.contains(&db.version), "the release is stamped: {m}");
    }

    #[test]
    fn the_orf_methods_count_the_tables_that_read_through_tga() {
        let n = pl_core::translate::all_tables()
            .filter(|c| !c.is_stop(b"TGA"))
            .count();
        let m = methods(topic("orfs").unwrap());
        assert_eq!(n, 13, "measured, not recalled");
        assert!(m.contains(&format!("{n} of them do not treat TGA")), "{m}");
    }

    #[test]
    fn the_golden_gate_methods_refuse_to_imply_a_fidelity_number() {
        // The same refusal the report itself makes: a percentage looks equally
        // authoritative whether or not the data behind it was shipped.
        let m = methods(topic("goldengate").unwrap());
        assert!(m.contains("no fidelity percentage"), "{m}");
        assert!(m.contains("Potapov"), "and says what would be needed: {m}");
        assert!(!m.contains('%'), "no percentage appears at all: {m}");
    }

    #[test]
    fn the_gel_methods_say_it_is_not_a_measurement() {
        let m = methods(topic("gel").unwrap());
        assert!(m.contains("not** a basis for sizing"), "{m}");
        assert!(m.contains("Fritsch"), "the interpolation is cited: {m}");
    }
}
