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
//! an annotation is a suggestion to check against the record's cited accession
//! and not an identification, the gel model is not a measurement, Golden Gate
//! fidelity is not computed because the matrices are not ours to ship.
//!
//! The first of those named an empty sign-off table until 2026-08-09. It was
//! written on the morning of 2026-07-28 and was false by that evening, when the
//! tables were signed; the `annotate` arm below had been reading the live count
//! out of `pl_features::Db::reviewed` the whole time, which is why the drift
//! was in the prose alone and is the rule for this crate — name the shape of a
//! limit in the header, and let the paragraph read the number.
//! `nothing_here_calls_the_shipped_database_unreviewed` now holds this
//! paragraph to it.
//!
//! # Offline
//!
//! Compiled in. Every word `pl methods` and the editor's Help window can show
//! is in the binary, so the documentation is there on a machine with no route
//! out — and a lab machine on an isolated network is a normal place for this to
//! run.
//!
//! This paragraph used to begin "a tool that asks nothing of anyone cannot have
//! documentation that needs a network". The conclusion is unchanged and the
//! premise stopped being true on 2026-08-06, when `pl update` and an
//! off-by-default check in the editor arrived: Polylinker will now ask one
//! server one question, when a person asks it to. This crate is not part of
//! that — it has no dependency outside the workspace and does no I/O — and the
//! reason the text is compiled in never depended on the premise anyway. Help
//! that needs a network is help that is missing exactly when the network is.

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
        name: "cloning",
        title: "Restriction, Gibson and Golden Gate cloning",
    },
    Topic {
        name: "primers",
        title: "Primer binding sites",
    },
    Topic {
        name: "design",
        title: "Primer design",
    },
    Topic {
        name: "map",
        title: "Plasmid and construct maps",
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
///
/// # It is handed a topic and nothing else, so every number here is a default
///
/// There is no argument for what a particular run used, and `pl methods` parses
/// no flags, so this function can only interpolate `*::default()`. Saying so is
/// not pedantry: `pl tm --na 1000 --oligo 500 GTAAAACGACGGCCAGT` reports 67.4 C,
/// the same oligo at the defaults reports 49.2 C, and `pl methods tm` used to
/// print "Parameters: ... 50 nM oligo, 50 mM Na+" in the past tense beside
/// either of them. Every parameter below is therefore labelled as a default,
/// the way the gel and annotate paragraphs always did.
///
/// Hedging a number is not enough for a claim about *what was computed*, since
/// a reader cannot tell that such a sentence needs editing. Three of those were
/// false outright — the paragraph asserted an off-target scan that
/// `pl design --no-specificity` never runs, a start-codon requirement that
/// `pl orfs --any-start` removes, and an exact seed that `pl primers
/// --seed-mismatch` relaxes — so they are stated conditionally now, naming both
/// settings rather than assuming one. A caller that knows what a run used
/// should print it from the run, not from here.
pub fn methods(t: Topic) -> String {
    let v = env!("CARGO_PKG_VERSION");
    match t.name {
        "tm" => {
            let m = pl_thermo::Method::default();
            format!(
                "Melting temperatures were calculated with a nearest-neighbour model, \
                 using Polylinker {v}. Default parameters, unless overridden: {}. Tm is \
                 reported for the annealing footprint only; any 5' tail on a primer is \
                 excluded, since it does not pair with the template on the first \
                 cycle.\n\n\
                 Limits: the model assumes two-state hybridisation of a perfectly \
                 matched duplex in a well-mixed solution, and no correction is applied \
                 for Mg2+, dNTPs, DMSO or betaine, all of which shift Tm in a real PCR. \
                 The nearest-neighbour table, the salt correction, the oligo \
                 concentration and the [Na+] are all selectable and none of them is \
                 known here, so the line above describes the defaults and not \
                 necessarily the run: changing the salt or the oligo moves Tm by \
                 degrees, and changing the table changes the citation.",
                m.describe()
            )
        }
        // THE LIMITS ONLY, and deliberately: what a particular cloning DID —
        // which enzymes, which fragments, which junctions, how many features
        // travelled — is known to the panel that planned it and is printed from
        // there. This function is handed a topic and nothing else, so anything
        // specific it claimed would be a claim about a run it cannot see. See
        // the note on this function.
        "cloning" => format!(
            "Constructs were planned in silico with Polylinker {v}. Restriction \
             fragments were generated by exact matching of IUPAC recognition sequences \
             on both strands and joined where their single-stranded ends are \
             complementary; homology assemblies were joined at terminal overlaps; \
             Golden Gate overhang sets were checked for repeats, palindromes, \
             cross-pairing and single-mismatch neighbours, each in both orientations. \
             Sequence identity of a circular product is by cdSEGUID and of a linear \
             product by ldSEGUID, so a construct reached by two routes is reported \
             once.\n\n\
             Limits: this is a plan and not a result. Nothing here establishes that a \
             reaction worked. Ligation and assembly efficiency, transformation, \
             insert-to-vector ratio, star activity, blocked or methylated sites, \
             incomplete digestion, plasmid toxicity and host recombination are all \
             outside what a sequence can show, and every one of them decides whether \
             the clone in the tube is the clone on screen. No fidelity percentage is \
             claimed for a Golden Gate set: the structural checks above find the faults \
             that can be found without data, and the measured pairwise ligation rates \
             are not shipped. A feature was carried into a construct only where its \
             whole span sat inside one fragment that could be placed unambiguously in \
             its parent; anything else was dropped and counted rather than placed at a \
             coordinate that merely looks right."
        ),
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
                 {v}, using the NCBI genetic code specified for each molecule. By \
                 default an ORF was defined as a run beginning at an initiation codon \
                 permitted by that code and ending at the first in-frame termination \
                 codon, and runs without an initiation codon were not reported; the \
                 stop-to-stop option removes that requirement and reports runs that \
                 begin at no initiation codon at all, so a run made with it has to say \
                 so, because the two settings do not report the same ORFs. On circular \
                 molecules the search was synchronised to a termination codon before \
                 scanning, so results do not depend on where the sequence was \
                 linearised. Residues were written in the convention a CDS record \
                 uses: the first codon of a reading is written M wherever the code \
                 permits initiation there whatever that codon spells, and a \
                 termination codon is written as an asterisk.\n\n\
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
                 Alignment was semi-global with affine gaps, at the default scores \
                 (match {}, mismatch {}, gap open {}, gap extend {}), the read being \
                 aligned in full while unmatched reference at either end was not \
                 penalised. Differences at or above the quality threshold -- Phred {} \
                 unless it was overridden -- were treated as substantive; those below \
                 were reported separately and not counted. The reliable extent of each \
                 read was determined by Mott trimming.\n\n\
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
            // Counted live rather than written down, like `reviewed` above and
            // for the same reason: a methods paragraph that hard-codes a number
            // becomes a false statement the first time the library grows.
            let peptides = db.records.iter().filter(|r| r.is_peptide_only()).count();
            // Read off the default rather than written down, for the same
            // reason: this table decides which ORFs exist, and therefore which
            // tags are reported at all. Naming the wrong one in a methods
            // paragraph would misdescribe the result, not just the settings.
            let code = pl_features::annotate::Config::default().code.id;
            let code_name = pl_features::annotate::Config::default().code.name();
            // What the library has no rows for at all, read off the table by
            // `Db::absent_common_kinds` and not written down here. A methods
            // paragraph that names the review status of every record but not
            // the whole categories the database has never held is describing
            // the wrong limit: a reader of the paper has no way to know that
            // "no origin of replication was annotated" was decided by the
            // database's contents rather than by the plasmid's.
            //
            // Asked of the REVIEWED table, not the whole one. That choice was
            // invisible until 2026-08-10, because the two held the same rows;
            // they no longer do. Twelve promoter, terminator and poly(A) rows
            // arrived `proposed`, so the whole table has those classes and the
            // default annotation run still does not search them — and reporting
            // the whole table's gaps here would have quietly stopped mentioning
            // promoters in a paragraph describing a run that never looked for
            // one. The desktop app's proposals panel asks the same question of
            // whichever table it just searched, which is the right question
            // there; this paragraph describes the default and says so.
            let kinds = db.reviewed().absent_common_kinds();
            let gaps = match kinds.split_last() {
                None => String::new(),
                Some((last, rest)) => format!(
                    " The reviewed library holds no {} record of any kind, so features \
                     of those classes are not searched for by default and cannot be \
                     reported.",
                    if rest.is_empty() {
                        (*last).to_string()
                    } else {
                        format!("{} or {last}", rest.join(", "))
                    }
                ),
            };
            format!(
                "Features were annotated with Polylinker {v} against its own curated \
                 library (release {}), by k-mer seeding followed by infix alignment, \
                 with protein-level matching in six frames so that recoded genes \
                 resolve. Default thresholds: {:.0}% identity, {:.0}% minimum coverage \
                 of the database feature.\n\n\
                 {peptides} of the library's records are designed peptide parts -- \
                 epitope tags, protease sites and linkers -- which carry a residue \
                 string and no nucleotides, because such a peptide has many synonymous \
                 encodings and no single one of them is the sequence. Those are matched \
                 by translation only, at zero mismatches over the whole peptide, and are \
                 reported only when the hit lies in frame inside an open reading frame \
                 of the query with at least 20 residues of that frame outside the tag. A \
                 tag that fails that test is not reported at all, so a tag on a construct \
                 with no detectable reading frame -- an empty tagging vector, or a \
                 fragment with no start codon -- will be absent rather than flagged. \
                 Which codons may open a reading frame is the genetic code's business, \
                 so it is also the code's business whether a given tag is reported: the \
                 default here is NCBI translation table {code} ({code_name}), and a \
                 construct whose gene initiates at a codon that table does not accept \
                 carries no detectable frame and therefore no tag.\n\n\
                 Limits: the library contains {} record(s), of which **{reviewed} have \
                 been reviewed by a named curator**. Unreviewed records were assembled \
                 by machine from public sources and are not shipped by default; any \
                 annotation derived from them is a suggestion to check against the cited \
                 accession, not an identification. A feature boundary is a claim, and \
                 each record states how its boundary was arrived at.{gaps} The absence \
                 of a feature from this output is therefore not evidence of its absence \
                 from the molecule.",
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
                 3'-anchored seed of {} bases -- exact by default, or allowing one \
                 mismatch, never at the 3' end, where that option was given -- extended \
                 toward the 5' end through isolated mismatches, two adjacent mismatches \
                 ending the footprint. The annealing footprint and any 5' tail are \
                 reported separately, and the melting temperature is computed from the \
                 footprint alone, with default parameters unless overridden: {}.\n\n\
                 Limits: this reports where a primer can anneal, not whether a product \
                 forms. Amplification efficiency, secondary structure, primer-dimer \
                 formation and 3'-end mismatch tolerance are not modelled. No melting \
                 temperature is reported for a footprint carrying a mismatch, because \
                 the nearest-neighbour model describes a perfectly matched duplex and \
                 has no internal-mismatch parameters; the conditions above are this \
                 model's scale and not a PCR buffer's, which sits about 5 C higher.",
                p.seed_len,
                p.tm_method.describe()
            )
        }
        "design" => {
            let c = pl_design::Constraints::default();
            // Backslash continuations and an explicit `\n\n`, like every sibling
            // arm. The paragraph break here was once a bare newline in the
            // source, which put 17 spaces of source indentation in front of
            // "Limits:" -- invisible through `pl methods`, which rewraps, but
            // the MCP tool and the PyO3 binding hand the string over raw, and a
            // blank line followed by four spaces is a CommonMark indented code
            // block: the two **...** hedges below rendered as literal asterisks
            // in monospace. Note the `\n\n\` and not a bare `\`: a backslash
            // before an empty line eats the paragraph break entirely, and
            // `pl methods` paginates by splitting on "\n\n".
            format!(
                "PCR primer pairs were designed with Polylinker {v}. Candidate oligos \
                 were enumerated over the selected region, filtered independently on \
                 melting temperature, composition and secondary structure, and then \
                 paired. Default constraints, unless overridden: {}. Melting \
                 temperatures were computed for the annealed footprint only, with a \
                 nearest-neighbour model ({} by default); a 5' tail carrying a \
                 restriction site does not pair with the template on the first cycle \
                 and is excluded, and the tailed oligo's own melting temperature is \
                 reported separately as the one that applies from the third cycle \
                 onward. The secondary-structure screen was applied to that same \
                 annealed footprint; the whole ordered oligo's hairpin and dimer free \
                 energies are reported beside the screened ones and were not gated \
                 on. {}. Unless the off-target scan was turned off, each candidate \
                 was additionally required to anneal at exactly one site on the \
                 supplied molecule, located with a 3'-anchored seed of {} exact bases \
                 extended toward the 5' end. Surviving pairs were ranked by a weighted \
                 sum of normalised deviations ({}), an approach following Rozen & \
                 Skaletsky (Methods Mol Biol 132:365, 2000) and Untergasser et al. \
                 (Nucleic Acids Res 40:e115, 2012); the weights are this software's own \
                 and no equivalence to any other designer is claimed.\n\n\
                 Limits: **where the off-target scan ran, specificity was checked \
                 against the supplied molecule and nothing else, and where it was \
                 turned off nothing was checked at all.** The run's own report states \
                 which of the two happened and this paragraph cannot, so it has to be \
                 read off the report. A primer unique in a plasmid is routinely not \
                 unique in a host genome, and no genome-wide search was performed \
                 either way. Secondary \
                 structure was screened as perfect ungapped helices only: internal \
                 loops, bulges, dangling ends, terminal mismatches, coaxial stacking \
                 and G-quadruplexes are not modelled, and hairpin loop initiation is \
                 not applied, so the reported free energies are a screen and not a \
                 fold. No correction is made for Mg2+ or dNTPs, so melting \
                 temperatures are on a monovalent scale -- {:.0} mM Na+ unless a \
                 different concentration was given -- and an ordinary PCR buffer sits \
                 about 5 C higher. **For RT-PCR the design cannot exclude \
                 genomic DNA**: this software is scoped to bacterial templates, \
                 bacterial genes have no introns, and there is therefore no exon-exon \
                 junction for a primer to span -- a no-RT control is the only thing \
                 that distinguishes cDNA from contaminating genomic DNA. No in-frame \
                 mode is offered and tail length is never adjusted to preserve a \
                 reading frame. Where a restriction site was added as a 5' tail, the \
                 finished amplicon was scanned on both strands for further occurrences \
                 of that site and any pair carrying one was rejected; cleavage \
                 efficiency close to a fragment terminus was NOT modelled, and the \
                 amplicon was treated as unmethylated, which it is -- it is synthesised \
                 in vitro from dNTPs. Methylation of the destination vector was \
                 considered only where a vector was supplied.",
                c.describe(),
                c.tm_method.table_name,
                c.describe_dg(),
                c.off_seed,
                c.weights.describe(),
                c.tm_method.na_molar * 1e3,
            )
        }
        // The figure, and the two things about it a reader of a paper needs
        // told: which shape a molecule was drawn in, and that a map may name
        // fewer things than the molecule has.
        //
        // Added when `pl-draw` grew its second figure. Until then the only
        // honest version of this paragraph would have been "linear molecules
        // were drawn as rings with a notch", which nobody would have pasted
        // into a methods section — and its absence is exactly why the defect
        // survived: there was no page for it to be wrong on.
        "map" => {
            let o = pl_draw::Options::default();
            format!(
                "Maps were drawn with Polylinker {v}. A CIRCULAR molecule is drawn as a \
                 ring and a LINEAR one as a horizontal track; the shape follows the \
                 molecule's own topology unless it was overridden. A circular molecule \
                 drawn on a track has been cut open between its last base and its first, \
                 and the figure says so in its caption. Features are drawn as boxes on a \
                 band the backbone runs through, with an arrowhead at the end the \
                 feature reads towards and no arrowhead at all where the file did not \
                 say which strand it is on; a feature spanning the origin of a circular \
                 molecule is drawn as the two spans it occupies. Restriction sites are \
                 marked with a tick and labelled with the enzyme and its coordinate. \
                 Defaults, unless overridden: a {w:.0} x {h:.0} unit canvas with \
                 {fs:.0} unit type and an {rw:.0} unit feature band, a ruler, and \
                 features covering less than {deg:.2}% of the molecule drawn as marks \
                 across the band rather than as boxes. A unit is a point where the \
                 figure is written at its own size; asking for a physical width scales \
                 the whole drawing, type included. The same geometry is written to SVG, \
                 PDF, EPS and PNG, and one build of the program gives byte-identical \
                 output for identical input. The vector formats round their coordinates \
                 on the way out, which is intended to hold that between machines as \
                 well; the raster is not promised to.\
                 \n\n\
                 Limits: a map is a drawing of a file and establishes nothing about a \
                 molecule in a tube. Where a canvas cannot hold every label, labels are \
                 dropped rather than overprinted, and WHICH ones were dropped is \
                 reported by the exporter rather than shown on the figure -- a map \
                 missing three names looks exactly like a molecule with three fewer \
                 features, so the count beside the export is part of the figure and not \
                 decoration. A name too long for the space is shortened with an \
                 ellipsis; the whole name survives in the SVG's title element and as a \
                 comment in the EPS, but a PDF and a PNG carry no copy of it at all, \
                 so there it survives only in what the exporter reports. Overlapping \
                 features overprint in one band and \
                 are not separated into lanes, so a dense region shows fewer distinct \
                 boxes than it has features. On a linear figure the canvas height is a \
                 budget for the label rows rather than the size of the drawing, and a \
                 figure needing more rows than the budget allows will drop labels; the \
                 caption, the feature band and the ruler are always drawn, so a figure \
                 may come back taller than the height it was given.",
                w = o.width,
                h = o.height,
                fs = o.font_size,
                rw = o.ring_width,
                deg = o.min_feature_degrees / 360.0 * 100.0,
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
            // Split by topology, because only one of the two invariances holds
            // for both: rotating a LINEAR duplex makes a different molecule, so
            // only `cdseguid` can promise origin-independence. The old wording
            // here — "the same for the same molecule however it was rotated,
            // exported or which strand was written first" — claimed it for
            // everything. `pl-mcp`'s tool description and `methods` below were
            // both already correct; this string is what `pl methods` with no
            // argument prints and what the GUI Help window shows above the
            // methods text, so it was the copy the user actually read.
            "A checksum that is the same for the same molecule however it was \
                       exported and whichever strand was written first. A circular \
                       molecule's is also the same however it was rotated; a linear \
                       one's is not, because rotating a linear duplex makes a \
                       different molecule."
        }
        "goldengate" => {
            "Structural problems in a Type IIS overhang set. No fidelity \
                         percentage — see the limits."
        }
        "cloning" => {
            "How a construct was put together, and what a plan does not \
                      establish about a reaction."
        }
        "primers" => {
            "Where a primer anneals, with the footprint and the tail kept \
                      apart."
        }
        "design" => {
            // Backslash continuations, like every sibling arm. Written as one
            // long literal this carried two 22-space runs from the source
            // indentation; `pl methods` rewraps and hid them, but a consumer
            // that does not wrap would print the gaps.
            "Pick a PCR primer pair for a region, checked for a second binding \
                     site on the molecule you have open. A restriction site goes on \
                     as a 5' tail and stays out of the Tm."
        }
        "map" => {
            // Both shapes named, and the cut said out loud. A track and a track
            // are the same picture, so "linear" alone would leave a reader with
            // no way to tell a linearised plasmid from a molecule that is a
            // line -- which is the most consequential thing a map can get wrong.
            "A ring for a circular molecule, a horizontal track for a linear \
                  one, as SVG, PDF, EPS or PNG. A plasmid drawn as a track has \
                  been cut open, and the figure says where."
        }
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rotation invariance may never be claimed without naming the topology it
    /// holds for.
    ///
    /// PROVEN TO FAIL at 713bd3b: `help(checksum)` read "A checksum that is the
    /// same for the same molecule however it was rotated, exported or which
    /// strand was written first" — unconditional, and false for half the
    /// molecules this program opens. `cdseguid` is origin-invariant; `ldseguid`
    /// is not and cannot be, because rotating a linear duplex makes a different
    /// molecule. `bins/pl-mcp/src/main.rs` had this same sentence corrected,
    /// with a comment saying exactly that, and `methods(checksum)` was written
    /// correctly from the start — only `help` kept the old form, and `help` is
    /// what `pl methods` with no argument prints for every topic and what the
    /// GUI Help window puts *above* the methods text. So a user with a linear
    /// construct got a different `ldseguid=` after a rotation the tool had told
    /// them could not change it.
    ///
    /// Asserted over every topic rather than over `checksum` alone: the trap is
    /// the unqualified word, not the one arm that fell into it.
    #[test]
    fn no_topic_claims_rotation_invariance_without_naming_the_topology() {
        for t in TOPICS {
            for (surface, text) in [("help", help(*t).to_string()), ("methods", methods(*t))] {
                if !text.contains("rotat") {
                    continue;
                }
                assert!(
                    text.contains("ircular"),
                    "{}'s {surface} text claims rotation invariance without saying \
                     it holds for circular molecules only: {text:?}",
                    t.name
                );
            }
        }
    }

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
    fn no_paragraph_states_a_setting_as_though_it_knew_what_the_run_used() {
        // `methods` gets a Topic and nothing else, and `pl methods` parses no
        // flags, so every value here is a default. The paragraph is meant to be
        // pasted into a paper beside a number, and it used to assert the
        // defaults in the past tense: `pl tm --na 1000 --oligo 500` reports
        // 67.4 C where the defaults give 49.2 C, and `pl methods tm` printed
        // "Parameters: ... 50 nM oligo, 50 mM Na+" beside it. Worse, three
        // sentences described *what was computed* and were flatly false under a
        // documented flag. Each assertion below names one of them.
        let m = methods(topic("tm").unwrap());
        let d = pl_thermo::Method::default();
        assert!(
            m.contains(&format!(
                "Default parameters, unless overridden: {}",
                d.describe()
            )),
            "the tm parameters are labelled as defaults, not asserted of the run: {m}"
        );
        assert!(
            !m.contains(&format!("Parameters: {}", d.describe())),
            "the unhedged past-tense parameter list is gone: {m}"
        );

        // `pl orfs --any-start` reports stop-to-stop runs; the sentence said
        // they were not reported at all.
        let m = methods(topic("orfs").unwrap());
        assert!(
            !m.contains("codon; runs without an initiation codon were not reported."),
            "the start-codon requirement is not asserted unconditionally: {m}"
        );
        assert!(
            m.contains("stop-to-stop"),
            "and the option that removes it is named: {m}"
        );
        // The residue convention, added 2026-08-07 when the desktop app grew a
        // way to take the letters away and a reader could put them in a figure.
        // Asserted against `translate_cds` itself and not only quoted, because
        // this is a claim about what the letters ARE: `GTG` initiates under
        // table 11 and does not under table 1, so the same three bases carry
        // the whole sentence.
        assert!(
            m.contains("written M wherever the code permits initiation there"),
            "the paragraph does not state the initiator convention: {m}"
        );
        let t11 = pl_core::translate::table(11).expect("table 11");
        let t1 = pl_core::translate::table(1).expect("table 1");
        assert_eq!(t11.translate_cds(b"GTGAAATAA"), b"MK*".to_vec());
        assert_eq!(t1.translate_cds(b"GTGAAATAA"), b"VK*".to_vec());

        // `pl primers --seed-mismatch` allows one mismatch in the seed.
        let m = methods(topic("primers").unwrap());
        let p = pl_primer::Params::default();
        assert!(
            !m.contains(&format!("seed of {} exact bases,", p.seed_len)),
            "the seed is not asserted to be exact: {m}"
        );
        assert!(
            m.contains("mismatch, never at the 3' end"),
            "and the option that relaxes it is named: {m}"
        );

        // `pl sanger --min-quality` moves the threshold.
        let m = methods(topic("sanger").unwrap());
        let q = pl_sanger::Params::default().min_quality;
        assert!(
            m.contains(&format!("Phred {q} unless it was overridden")),
            "the quality threshold is labelled as a default: {m}"
        );

        // The one that would put a false claim in a paper rather than a wrong
        // number: `pl design --no-specificity` runs no off-target scan, and the
        // flag's own help says "skip the off-target scan, and say so".
        let m = methods(topic("design").unwrap());
        assert!(
            !m.contains(
                "**specificity was checked against the supplied molecule and nothing else.**"
            ),
            "the off-target scan is not asserted to have run: {m}"
        );
        assert!(
            m.contains("Unless the off-target scan was turned off, each candidate"),
            "the specificity requirement is conditional: {m}"
        );
        assert!(
            m.contains("turned off nothing was checked at all"),
            "and the limits state what a skipped scan leaves behind: {m}"
        );
        let na = pl_design::Constraints::default().tm_method.na_molar * 1e3;
        assert!(
            m.contains(&format!(
                "{na:.0} mM Na+ unless a different concentration was given"
            )),
            "the salt scale is labelled as a default, since --na moves it: {m}"
        );
    }

    /// A temperature in a methods paragraph is meaningless without its scale.
    ///
    /// The rule was already applied to the "tm" arm, which prints
    /// `Method::describe` in full, and to "design", which names the table and
    /// the sodium. It was MISSING from "primers", which said "the melting
    /// temperature is computed from the footprint alone" and then named no
    /// table, no salt correction and no concentration at all.
    ///
    /// That is the 50 mM trap in the one place it is hardest to withdraw. These
    /// paragraphs exist to be pasted into a paper — the arm above them carries a
    /// "Copy this paragraph" button — and the same 20 nt footprint reads 53.9 C
    /// on this model's 50 mM Na+ scale and about five degrees higher in an
    /// ordinary PCR buffer. A reader given the number and not the scale can
    /// neither reproduce it nor compare it with another tool's, and, unlike a
    /// wrong number, a missing condition does not look like anything.
    ///
    /// Every topic is swept rather than the three that report a Tm today, so an
    /// arm added later has to carry its conditions to pass.
    ///
    /// PROVEN TO FAIL by dropping `p.tm_method.describe()` from the "primers"
    /// arm: `primers: reports a melting temperature and names no
    /// nearest-neighbour table`.
    #[test]
    fn every_paragraph_reporting_a_melting_temperature_names_its_conditions() {
        let d = pl_thermo::Method::default();
        let na = format!("{:.0} mM Na+", d.na_molar * 1e3);
        // Selected by what the paragraph SAYS, not by a list of topic names
        // here: a second list is how the sweep comes to miss the arm that was
        // added after it.
        let reports_a_tm =
            |m: &str| m.to_lowercase().contains("melting temperature") || m.contains(" Tm ");
        for t in TOPICS {
            let m = methods(*t);
            if !reports_a_tm(&m) {
                continue;
            }
            assert!(
                m.contains(d.table_name),
                "{}: reports a melting temperature and names no nearest-neighbour table, so \
                 the number cannot be reproduced or compared with another tool's -- {m}",
                t.name
            );
            assert!(
                m.contains(&na),
                "{}: reports a melting temperature and names no sodium concentration, and \
                 this model's 50 mM scale is not the PCR buffer the reader is standing in \
                 front of -- {m}",
                t.name
            );
        }
        // THE CONTROL. Every assertion above is inside a `continue`, so a
        // predicate that matched nothing would make this pass while checking
        // nothing at all -- the shape of check this project keeps finding.
        let seen = TOPICS
            .iter()
            .filter(|t| reports_a_tm(&methods(**t)))
            .count();
        assert_eq!(
            seen, 3,
            "expected the tm, primers and design paragraphs to report a temperature; matched \
             {seen}, so the sweep is measuring something other than what it says"
        );
    }

    #[test]
    fn no_paragraph_carries_source_indentation_into_the_text() {
        // The design arm broke its literal at the paragraph before "Limits:"
        // without a `\` continuation, so 17 spaces of source indentation ended
        // up in the returned String. `pl methods` rewraps and hid it, but the
        // MCP tool and the PyO3 binding return the string raw, and a blank line
        // followed by four spaces is a CommonMark indented code block -- which
        // is where the two **...** hedges in that arm lost their emphasis and
        // became literal asterisks. Three spaces is the cheapest signal: every
        // other topic already has none.
        for t in TOPICS {
            let m = methods(*t);
            assert!(
                !m.contains("   "),
                "{} carries source indentation: {m}",
                t.name
            );
            assert!(!help(*t).contains("   "), "{} help: {}", t.name, help(*t));
        }
    }

    #[test]
    fn the_annotation_methods_report_how_many_records_are_actually_reviewed() {
        // The number that matters most and is easiest to leave stale: whatever
        // it is, the paragraph has to read it out of the tables rather than
        // describe from memory a library nobody has checked. This comment said
        // "It is zero today" until 2026-08-09 — written the morning before the
        // tables were signed, which is the same drift, inside the rationale of
        // the check against it.
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

    /// The methods paragraph names the whole CLASSES the library has no rows
    /// for, not only the review status of the rows it does have.
    ///
    /// A reader of a paper cannot tell "no origin of replication was
    /// annotated" from "no origin of replication is annotatable", and the
    /// second is the true one today. The limits paragraph naming every record's
    /// review status and not this was describing the smaller of the two limits.
    ///
    /// Read from `Db::absent_common_kinds` on both sides, deliberately: the
    /// point is that the sentence tracks the table, so a test hard-coding
    /// "promoter" would go stale in exactly the way the sentence must not.
    ///
    /// PROVEN TO FAIL by dropping `{gaps}` from the format string:
    ///
    /// ```text
    /// the methods do not say the library has no `promoter` rows at all: ...
    /// ```
    #[test]
    fn the_annotation_methods_name_what_the_library_has_no_rows_for() {
        let (db, _) = pl_features::Db::builtin();
        let missing = db.reviewed().absent_common_kinds();
        assert!(
            !missing.is_empty(),
            "the premise: today's table really is missing something common"
        );
        let m = methods(topic("annotate").unwrap());
        for kind in &missing {
            assert!(
                m.contains(kind),
                "the methods do not say the library has no `{kind}` rows at all: {m}"
            );
        }
        assert!(
            m.contains("not evidence of its absence"),
            "the paragraph does not tell a reader what a missing feature means: {m}"
        );
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

    /// Does `text` make `claim` in its own voice, rather than quoting it?
    ///
    /// An occurrence whose nearest preceding non-space character opens a
    /// quotation — `"`, `“` or a backtick — is somebody being quoted, and a
    /// correction that says what a sentence *used* to say has to be able to
    /// print the old sentence. The same six lines are in
    /// `crates/pl-scan/src/lib.rs` and `bins/pl-mcp/src/main.rs`, which guard
    /// the same class of stale claim in their own files; sharing them would
    /// mean a new workspace member existing only to hold one predicate.
    fn asserts(text: &str, claim: &str) -> bool {
        text.match_indices(claim).any(|(i, _)| {
            !matches!(
                text[..i].chars().rev().find(|c| !c.is_whitespace()),
                Some('"') | Some('\u{201c}') | Some('`')
            )
        })
    }

    /// Nothing in this file may describe the shipped database as unreviewed
    /// while [`pl_features::Db::reviewed`] returns records.
    ///
    /// PROVEN TO FAIL at c44757b, in two places at once — the crate header's
    /// list of limits and the rationale of the very test that guards the
    /// generated number:
    ///
    /// ```text
    /// crates/pl-doc/src/lib.rs says "nothing reviewed" while 89 of 89 records
    /// are signed off
    /// ```
    ///
    /// Both sentences were written on the morning of 2026-07-28 and were false
    /// by that evening, when `c8436d5` signed the tables. Neither was covered:
    /// `pl-features`' README check reads `README.md` and `features/README.md`
    /// and searches for the literal "0 reviewed", which this file's wording
    /// does not contain and this file is not in the list for. The whole file is
    /// scanned, not just the header, because the stale claim had spread from
    /// one to the other.
    #[test]
    fn nothing_here_calls_the_shipped_database_unreviewed() {
        const SELF: &str = include_str!("lib.rs");
        let (db, errors) = pl_features::Db::builtin();
        assert!(errors.is_empty(), "{errors:?}");
        let signed = db.reviewed().records.len();
        assert!(
            signed > 0,
            "the premise: if the shipped tables really are unsigned again, this \
             test and the sentences it constrains both describe the new state"
        );
        for claim in [
            "nothing reviewed",
            "none reviewed",
            "0 reviewed",
            "no reviewed records",
            "entirely unreviewed",
            "It is zero today",
        ] {
            assert!(
                !asserts(SELF, claim),
                "this file says {claim:?} while {signed} of {} records carry a \
                 curator sign-off",
                db.records.len()
            );
        }
    }

    /// The map methods paragraph is written into somebody's paper, so it may
    /// not promise across platforms what `pl-draw`'s raster declines.
    ///
    /// PROVEN TO FAIL at c44757b:
    ///
    /// ```text
    /// the map methods promise identical bytes "on every platform" while
    /// crates/pl-draw/src/raster.rs declines that claim for the PNG
    /// ```
    ///
    /// The paragraph named all four writers — "SVG, PDF, EPS and PNG" — and
    /// then made one promise for the lot. `crates/pl-draw/src/lib.rs` carried
    /// the same overclaim in its own header; that one is a reader's problem,
    /// and this one is a reviewer's.
    #[test]
    fn the_map_methods_do_not_promise_bytes_across_platforms() {
        const RASTER: &str = include_str!("../../pl-draw/src/raster.rs");
        assert!(
            RASTER.contains("does not claim byte-identical output across platforms"),
            "pl-draw's raster no longer declines the cross-platform claim; \
             re-read the sentence this test constrains"
        );
        let m = methods(topic("map").unwrap());
        assert!(
            !m.contains("on every platform"),
            "the map methods promise bytes across platforms: {m}"
        );
        assert!(
            m.contains("byte-identical"),
            "and the identity that does hold is still stated: {m}"
        );
    }
}
