//! GenBank flat file: the way out of any proprietary container.
//!
//! Plain text, read by ApE, UGENE, Benchling, Biopython and SnapGene itself.
//!
//! Feature colours are written in two conventions at once, because the tools
//! disagree and the cost of writing both is three lines:
//! `/ApEinfo_fwdcolor` + `/ApEinfo_revcolor` (ApE, UGENE, SnapGene) and
//! `/note="color: #rrggbb"` (Benchling and several web viewers).

use pl_core::{BindingSite, Feature, Molecule, Primer, Segment, Strand, Topology};

const MONTHS: [&str; 12] = [
    "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
];

/// Parse the first record of a GenBank file.
///
/// Multi-record files are common (a genome per contig); this returns the first
/// and callers that care should use [`parse_all`].
pub fn parse(text: &str) -> Molecule {
    parse_all(text).into_iter().next().unwrap_or_default()
}

pub fn parse_all(text: &str) -> Vec<Molecule> {
    parse_all_reporting(text).0
}

/// Does every LOCUS line in this text state a topology?
///
/// `parse_record` returns `Topology::Linear` both for a file that says `linear`
/// and for one that says nothing, because `Topology` has only two states.
/// Those are different facts: the first is the file's claim, the second is our
/// default. A caller deciding whether to scan for origin-straddling sites needs
/// to know which it has.
///
/// Conservative on purpose — **all** records must declare. A file whose first
/// record says `circular` and whose second says nothing is not a file that
/// declared its topology, and answering `true` would let the second record
/// inherit the first one's provenance.
///
/// Returns `false` for text with no LOCUS line at all: nothing declared it.
pub fn declares_topology(text: &str) -> bool {
    // See `lib::strip_bom`. `load_all` strips it, but this and
    // `parse_all_reporting` are public and are called directly, and a U+FEFF
    // makes the very first `starts_with("LOCUS")` false — which is exactly the
    // record whose topology is being asked about.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut seen = false;
    for locus in text.lines().filter(|l| l.starts_with("LOCUS")) {
        seen = true;
        // Same tokenised rule as `parse_record`, and skipping the name token
        // for the same reason: `pCircularise` is a name, not a topology.
        let declared = locus
            .split_whitespace()
            .skip(2)
            .any(|t| t.eq_ignore_ascii_case("circular") || t.eq_ignore_ascii_case("linear"));
        if !declared {
            return false;
        }
    }
    seen
}

/// Every record, plus any location form we could not represent.
///
/// The warnings are the point: an exotic location that simply vanished left a
/// feature quietly claiming a span it does not have. See [`parse_location`].
pub fn parse_all_reporting(text: &str) -> (Vec<Molecule>, Vec<String>) {
    // See `lib::strip_bom`: a leading U+FEFF makes every `starts_with("LOCUS")`
    // below false, which costs the name, the declared length, the strandedness
    // and the topology — and, for a file with no trailing `//`, the whole
    // record, because the guard at the bottom of this function uses the same
    // token test.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut out = Vec::new();
    let mut warnings = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    let take = |lines: &Vec<&str>, out: &mut Vec<Molecule>, w: &mut Vec<String>| {
        let (m, bad) = parse_record(lines);
        out.push(m);
        w.extend(bad);
    };
    for line in text.lines() {
        if line.starts_with("//") {
            if !current.is_empty() {
                take(&current, &mut out, &mut warnings);
                current.clear();
            }
            continue;
        }
        // A second LOCUS ends the chunk too, and that is not belt-and-braces:
        // `//` is the only thing that used to end one, so a file whose internal
        // terminator is missing — hand-edited, or written by a tool that omits
        // it — became a single chunk and `parse_record`'s ORIGIN loop, which has
        // no stop condition, ate the next record's own header as bases. Two
        // 12 bp records came back as one molecule of
        // `acgtacgtacgtLOCUSlacZbpDNAcircularSYN-JAN-ORIGINttttggggcccc`: 60
        // fabricated bases from 24 real ones, `records == 1` so `truncated()`
        // was false, the second record gone, and `pl convert --to fasta` wrote
        // the invented string out at exit 0.
        //
        // `LOCUS` is the one keyword that can begin a record, it is required to
        // sit at column 1, and every line inside a record that could be confused
        // with it — a FEATURES row, an ORIGIN row, a wrapped DEFINITION — is
        // indented. So this cannot split a well-formed record, and it is what
        // makes the ORIGIN loop's chunk exactly one record.
        if line.starts_with("LOCUS") && current.iter().any(|l| l.starts_with("LOCUS")) {
            take(&current, &mut out, &mut warnings);
            current.clear();
        }
        current.push(line);
    }
    if current.iter().any(|l| l.starts_with("LOCUS")) {
        take(&current, &mut out, &mut warnings);
    }
    (out, warnings)
}

/// A feature being accumulated across lines: (key, location, qualifiers).
///
/// GenBank spreads one feature over many lines, and both the location and any
/// quoted qualifier can wrap, so parsing has to hold a partial feature open.
type Pending = (String, String, Vec<(String, Option<String>)>);

fn parse_record(lines: &[&str]) -> (Molecule, Vec<String>) {
    let mut mol = Molecule::default();
    // Location forms we cannot represent, reported rather than dropped.
    let mut unparsable: Vec<String> = Vec::new();
    let mut declared: Option<u64> = None;

    if let Some(locus) = lines.iter().find(|l| l.starts_with("LOCUS")) {
        // Read by token, never by substring over the whole line.
        //
        // `locus.contains("circular")` matched the *name* as readily as the
        // topology field, so `pCircularise` parsed as circular even when its
        // field said `linear`, and `pcDNA3-ss-mCherry` asserted
        // `double_stranded: Some(false)` about an ordinary plasmid. Both are
        // self-inflicted: `locus_name` keeps `-`, and `write` puts the document
        // title in the name column, so our own export of a linear molecule
        // called `pCircularise-v2.dna` read back circular. `Topology` has no
        // unknown state, and a wrong circular flag changes computed digests.
        //
        // Real LOCUS lines are ragged (`LOCUS       WT       74 bp`, names
        // containing `|`), so every rule below degrades to a default rather
        // than guessing.
        let toks: Vec<&str> = locus.split_whitespace().collect();
        mol.name = toks.get(1).copied().unwrap_or_default().to_string();

        // Token index 1 is the name and is skipped: only a standalone
        // `circular`/`linear` token decides topology.
        //
        // Absence of both is *not* linear, only undeclared — which this returns
        // as `Linear` because `Topology` has no third state. See
        // [`declares_topology`] and `LoadReport::topology_declared`, which is
        // how a caller tells the two apart.
        mol.topology = if toks
            .iter()
            .skip(2)
            .any(|t| t.eq_ignore_ascii_case("circular"))
        {
            Topology::Circular
        } else {
            Topology::Linear
        };

        // "<n> bp" or "<n> aa"
        let unit_at = toks
            .iter()
            .position(|t| *t == "bp" || *t == "aa")
            .filter(|i| *i >= 2);
        if let Some(i) = unit_at {
            if toks[i - 1].chars().all(|c| c.is_ascii_digit()) {
                declared = toks[i - 1].parse().ok();
            }
        }

        // Strandedness lives on the molecule-type token that follows the unit,
        // as an `ss-`/`ds-`/`ms-` prefix. Absent means unknown, not single.
        mol.double_stranded = unit_at.and_then(|i| toks.get(i + 1)).and_then(|t| {
            let lower = t.to_ascii_lowercase();
            if lower.starts_with("ds-") {
                Some(true)
            } else if lower.starts_with("ss-") {
                Some(false)
            } else {
                None
            }
        });
    }

    // DEFINITION wraps, and the continuation lines are part of the value.
    //
    // A single-line `find` kept only the first physical line, and GenBank wraps
    // DEFINITION near column 79, so this fired on ordinary NCBI records:
    // `DEFINITION  Escherichia coli str. K-12 substr. MG1655, complete` +
    // `            genome.` became "...MG1655, complete", and `write` then put a
    // full stop after it so the truncation read as a finished sentence. The word
    // that went missing is also the one a library search would match on, because
    // `pl-scan` indexes this string. The FEATURES parser below already
    // reassembles wrapped qualifier values and wrapped locations, so the header
    // was the odd one out rather than a considered tradeoff.
    if let Some(i) = lines.iter().position(|l| l.starts_with("DEFINITION")) {
        let mut def = lines[i][10..].trim().to_string();
        for line in &lines[i + 1..] {
            // A GenBank continuation line leaves columns 1-10 blank; anything
            // with text there is the next keyword and ends the DEFINITION.
            // `get` rather than a slice: a multi-byte character straddling byte
            // 10 would panic, and this runs inside wasm where a panic kills the
            // module rather than one call.
            let Some(head) = line.get(..10) else { break };
            if !head.trim().is_empty() {
                break;
            }
            let cont = line[10..].trim();
            if cont.is_empty() {
                break;
            }
            def.push(' ');
            def.push_str(cont);
        }
        mol.description = def.trim().trim_end_matches('.').to_string();
    }

    // --- ORIGIN ---
    if let Some(oi) = lines.iter().position(|l| l.starts_with("ORIGIN")) {
        let mut seq = Vec::new();
        // No stop condition, deliberately: `lines` is exactly one record,
        // because `parse_all_reporting` ends a chunk at `//` **or** at the next
        // `LOCUS`, so at most one LOCUS line can be in here and it is above this
        // point. There used to be a `line.starts_with("//")` break, which could
        // never fire — the chunker uses the identical predicate and `continue`s
        // without pushing the terminator — and an unreachable guard is what made
        // this loop look bounded when it was not.
        for line in &lines[oi + 1..] {
            // Case preserved: lowercase marks soft-masked / low-coverage bases.
            seq.extend(
                line.bytes()
                    .filter(|b| !b.is_ascii_whitespace() && !b.is_ascii_digit()),
            );
        }
        mol.seq = seq;
    }
    mol.declared_len = declared;

    // --- FEATURES ---
    if let Some(fi) = lines.iter().position(|l| l.starts_with("FEATURES")) {
        let mut pending: Option<Pending> = None;
        let mut open_qual: Option<usize> = None;
        let mut loc_open = false;

        let flush = |p: Option<Pending>, out: &mut Vec<Feature>, unparsable: &mut Vec<String>| {
            let Some((key, loc, quals)) = p else { return };
            // `source` is whole-molecule metadata every file carries; showing
            // it would draw a full-length bar across the map.
            if key == "source" {
                return;
            }
            let (segments, strand, bad) = parse_location(&loc);
            unparsable.extend(bad.into_iter().map(|b| format!("{key}: {b}")));
            if segments.is_empty() {
                return;
            }
            let name = ["label", "gene", "product", "locus_tag", "note"]
                .iter()
                .find_map(|k| {
                    quals
                        .iter()
                        .find(|(qk, _)| qk == k)
                        // A valueless qualifier cannot name anything.
                        .and_then(|(_, v)| v.clone())
                })
                .unwrap_or_else(|| key.clone());
            let color = quals
                .iter()
                .find(|(k, _)| k == "ApEinfo_fwdcolor" || k == "ApEinfo_revcolor")
                .and_then(|(_, v)| v.clone())
                .or_else(|| {
                    quals
                        .iter()
                        .find(|(k, v)| k == "note" && v.as_deref().is_some_and(|s| s.contains('#')))
                        .and_then(|(_, v)| v.as_deref())
                        .and_then(|v| v.split('#').nth(1))
                        .and_then(|h| {
                            // Take hex digits by character, never by byte: a
                            // multibyte char straddling byte 6 would panic, and
                            // this runs inside wasm where a panic kills the
                            // module rather than one call.
                            let hex: String = h
                                .chars()
                                .take(6)
                                .take_while(|c| c.is_ascii_hexdigit())
                                .collect();
                            (hex.len() == 6).then(|| format!("#{hex}"))
                        })
                });
            let segments = segments
                .into_iter()
                .map(|mut s| {
                    s.color = color.clone();
                    s
                })
                .collect();
            out.push(Feature {
                name,
                kind: key,
                strand,
                segments,
                qualifiers: quals,
            });
        };

        for line in &lines[fi + 1..] {
            if line.starts_with("ORIGIN") || line.starts_with("CONTIG") || line.starts_with("BASE")
            {
                break;
            }
            if line.len() < 6 || !line.is_char_boundary(5) {
                continue;
            }
            let indent_5 = &line[..5];
            if !indent_5.trim().is_empty() {
                continue; // a new top-level keyword
            }
            let body = line[5..].trim_end();
            let is_new_feature = !body.starts_with(' ') && !body.trim().is_empty();

            if is_new_feature {
                flush(pending.take(), &mut mol.features, &mut unparsable);
                let mut parts = body.trim().splitn(2, char::is_whitespace);
                let key = parts.next().unwrap_or_default().to_string();
                let loc = parts.next().unwrap_or_default().trim().to_string();
                loc_open = unbalanced(&loc);
                open_qual = None;
                pending = Some((key, loc, Vec::new()));
                continue;
            }

            let Some((_, loc, quals)) = pending.as_mut() else {
                continue;
            };
            let t = body.trim();

            if loc_open && !t.starts_with('/') {
                loc.push_str(t);
                loc_open = unbalanced(loc);
                continue;
            }

            // A line only starts a new qualifier when no quoted value is open.
            //
            // Testing `starts_with('/')` first meant a continuation line that
            // happened to begin with `/` — extremely common in COG and product
            // descriptions, 168 times across 66 of this project's corpus files
            // — was read as a brand new qualifier. The real value truncated,
            // a junk key was fabricated from the prose, and every later
            // continuation line was dropped. It is also self-inflicted:
            // `qualifier_lines` wraps on spaces, so any name containing " / "
            // failed to survive our own write-then-read.
            if open_qual.is_none() && t.starts_with('/') {
                let rest = &t[1..];
                loc_open = false;
                let (k, raw) = match rest.split_once('=') {
                    Some((k, v)) => (k.to_string(), Some(v.to_string())),
                    // No '=' at all: a valueless qualifier such as /pseudo.
                    None => (rest.to_string(), None),
                };
                match raw {
                    None => {
                        quals.push((k, None));
                        open_qual = None;
                    }
                    Some(v) => {
                        let (text, closed) = open_value(&v);
                        quals.push((k, Some(text)));
                        open_qual = if closed { None } else { Some(quals.len() - 1) };
                    }
                }
            } else if let Some(idx) = open_qual {
                // Continuation of a quoted qualifier such as /translation.
                let (piece, closed) = continue_value(t);
                let sep = if quals[idx].0 == "translation" {
                    ""
                } else {
                    " "
                };
                let slot = quals[idx].1.get_or_insert_with(String::new);
                slot.push_str(sep);
                slot.push_str(&piece);
                if closed {
                    open_qual = None;
                }
            }
        }
        flush(pending.take(), &mut mol.features, &mut unparsable);
    }

    // Features are left in file order on purpose. A reader reports what the
    // file says; sorting is a presentation choice and belongs to whatever is
    // drawing the map. Sorting here also silently breaks round-trip fidelity,
    // because the writer emits in model order.
    promote_primers(&mut mol);
    (mol, unparsable)
}

/// Move the `primer_bind` features this writer produced back into
/// [`Molecule::primers`].
///
/// # Why this exists
///
/// GenBank has no primer object. The writer spends one `primer_bind` feature
/// per BINDING SITE, carrying the primer's name in `/label` and its oligo, with
/// the site's melting temperature, in a `/note`. Until 2026-09-03 nothing ever
/// read that back, so a `.dna` opened, saved as GenBank and reopened had its
/// primers as ordinary features: the Primers tab was empty, `pl info` counted
/// zero primers, and the oligo survived only as prose in a note. The round trip
/// was stable -- writing that molecule again produced the same bytes -- which is
/// exactly why nothing ever went red.
///
/// # What is promoted, and what deliberately is not
///
/// ONLY the exact fixed form this writer emits, and the strictness is the whole
/// design. A `primer_bind` from another program, or one a user made in the
/// feature editor, is a FEATURE and must stay one -- reinterpreting it would
/// silently remove it from the features list on open. So a candidate must have:
///
///   * kind `primer_bind`;
///   * exactly two qualifiers, `label` then `note`, in that order, both with
///     values -- our writer emits precisely these two and nothing else, so a
///     third qualifier means the feature came from somewhere else;
///   * a note of `primer <SEQ>` or `primer <SEQ>; Tm: <number> C`;
///   * a `<SEQ>` that is non-empty and every byte of which is an IUPAC
///     nucleotide code. A note of `primer for colony PCR` fails on the space
///     inside what would be the oligo, which is the case worth being sure of.
///
/// The empty-oligo case is refused explicitly rather than by accident: a primer
/// with an empty `seq` writes `/note="primer "`, and the trailing space survives
/// the round trip, so without the non-empty test that note would promote to a
/// primer with no oligo.
///
/// # A promoted feature LEAVES `mol.features`
///
/// It has to. Kept in both places, the next write would emit it twice -- once
/// from the FEATURES loop and once from the primer loop -- so every save would
/// add one `primer_bind` per site, and a file would grow every time it was
/// opened and saved.
///
/// # Sites of one primer are merged
///
/// The writer spends one feature per site, so a two-site primer arrives as two
/// features that agree on name and oligo. They are merged back into one primer
/// with two sites, in file order. The consequence, stated because it is a real
/// asymmetry rather than a rounding error: two DISTINCT primers that share a
/// name and an oligo go out as separate model objects and come back as one.
/// Nothing in the corpus is known to do that, and it was not measured.
fn promote_primers(mol: &mut Molecule) {
    let mut promoted: Vec<Primer> = Vec::new();
    let mut kept: Vec<Feature> = Vec::with_capacity(mol.features.len());

    for f in std::mem::take(&mut mol.features) {
        let Some((name, seq, tm)) = written_primer(&f) else {
            kept.push(f);
            continue;
        };
        // One segment is what `location_parts` produces for a plain site and
        // two for one crossing the origin; either way the site is the span from
        // the first segment's start to the last segment's end, which is how the
        // writer took it apart.
        let (Some(first), Some(last)) = (f.segments.first(), f.segments.last()) else {
            kept.push(f);
            continue;
        };
        let site = BindingSite {
            start: first.start,
            end: last.end,
            strand: f.strand,
            tm,
        };
        match promoted.iter_mut().find(|p| p.name == name && p.seq == seq) {
            Some(p) => p.sites.push(site),
            None => promoted.push(Primer {
                name,
                seq,
                // GenBank never carried it; see the writer's note on the same
                // subject.
                description: String::new(),
                sites: vec![site],
            }),
        }
    }

    mol.features = kept;
    // Appended, not assigned: a `.dna` read never reaches here, and a GenBank
    // read starts with an empty list, but neither is a reason for this function
    // to be the thing that decides the list was empty.
    mol.primers.extend(promoted);
}

/// `Some((name, oligo, tm))` if this feature is one this writer produced for a
/// primer binding site. See [`promote_primers`] for why the test is this exact.
fn written_primer(f: &Feature) -> Option<(String, String, Option<f64>)> {
    if f.kind != "primer_bind" || f.qualifiers.len() != 2 {
        return None;
    }
    let (k0, v0) = (&f.qualifiers[0].0, f.qualifiers[0].1.as_deref()?);
    let (k1, v1) = (&f.qualifiers[1].0, f.qualifiers[1].1.as_deref()?);
    if k0 != "label" || k1 != "note" {
        return None;
    }
    let rest = v1.strip_prefix("primer ")?;
    // `; Tm: <number> C` or nothing at all. Anything else is not ours.
    let (oligo, tm) = match rest.split_once("; Tm: ") {
        Some((oligo, tail)) => {
            let number = tail.strip_suffix(" C")?;
            (oligo, Some(number.parse::<f64>().ok()?))
        }
        None => (rest, None),
    };
    if oligo.is_empty() || oligo.bytes().any(|b| pl_core::iupac::code_mask(b) == 0) {
        return None;
    }
    Some((v0.to_string(), oligo.to_string(), tm))
}

fn unbalanced(s: &str) -> bool {
    s.matches('(').count() > s.matches(')').count()
}

/// Read the text of a quoted qualifier value, collapsing `""` to `"`.
///
/// Returns the decoded text and whether the closing quote was reached on this
/// line. An unquoted value (`/codon_start=1`) is taken whole and is always
/// closed.
///
/// The old code used `trim_matches('"')`, which never collapsed the escape, so
/// a value containing a literal quote doubled its quote count on **every**
/// save/load cycle: 2 → 4 → 8 → 16. Worse, when an escaped `""` fell on a line
/// wrap, `ends_with('"')` read it as the close, the qualifier was marked
/// finished, and every remaining continuation line was silently discarded.
fn open_value(v: &str) -> (String, bool) {
    match v.strip_prefix('"') {
        None => (v.to_string(), true),
        Some(body) => decode_quoted(body),
    }
}

/// The same, for a continuation line of an already-open value.
fn continue_value(t: &str) -> (String, bool) {
    decode_quoted(t)
}

/// Walk a quoted run, treating `""` as one literal `"` and a lone `"` as the
/// terminator.
fn decode_quoted(s: &str) -> (String, bool) {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'"' {
            if i + 1 < b.len() && b[i + 1] == b'"' {
                out.push('"');
                i += 2;
                continue;
            }
            // A single quote closes the value; GenBank puts nothing after it.
            return (out, true);
        }
        // Copy whole characters so multi-byte text is not split.
        let ch = s[i..].chars().next().expect("index is on a char boundary");
        out.push(ch);
        i += ch.len_utf8();
    }
    (out, false)
}

/// Parse a GenBank location into 1-based inclusive segments plus a strand.
/// Parse a GenBank location into 1-based inclusive segments plus a strand.
///
/// Returns any part it could not represent, rather than discarding it. The
/// crate's posture is liberal-on-read, so an exotic location must not be fatal;
/// but it must not be invisible either, and it certainly must not be *invented*.
///
/// Both halves matter, and shipping either alone makes things worse:
///
/// - Without the strictness, `bond(5,10)` split on the comma into `bond(5` and
///   `10)`; the first failed to parse and vanished, and the second became a
///   **fabricated** `10..10` segment that `validate()` was happy with.
///   `order(bond(30,115),bond(64,80))` — the form NCBI writes in GenPept —
///   yielded `[115..115, 80..80]`, two annotations pointing at nothing.
/// - Without the report, `1^2`, `J00194.1:200..300` and every other form we
///   cannot express simply disappear, and `join(1..100,J00194.1:200..300)`
///   leaves a feature quietly claiming to be 100 bp when it is not.
fn parse_location(loc: &str) -> (Vec<Segment>, Strand, Vec<String>) {
    let mut s = loc.trim();
    let mut strand = Strand::Forward;
    // Which of the two reverse spellings this is. They order their exons
    // differently and the segment order below depends on knowing which arrived
    // — see the `segs.reverse()` at the bottom.
    let mut outer_complement = false;
    if let Some(inner) = s.strip_prefix("complement(") {
        strand = Strand::Reverse;
        outer_complement = true;
        s = inner.strip_suffix(')').unwrap_or(inner);
    }
    // `order` is read as a join, because `Feature` has no operator to hold, and
    // `join_parts` then writes `join(...)` back out. That is a reinterpretation,
    // not a parse: INSDC's `order` asserts the elements occur in this order and
    // deliberately does *not* assert that they are joined, which is why a
    // submitter reaches for it — X92946 carries
    // `gene complement(order(14253..14810,14820..14824))` beside a
    // `/note="-1 translational frameshift"`, precisely because the two pieces
    // are not spliced. Saving that file turned the file's own claim into
    // `complement(join(...))` with nothing said, while the mixed-strand branch
    // below reports the identical class of change. Tracked here and reported at
    // the bottom, once, and only if a segment actually survives.
    let mut order_read_as_join = false;
    for p in ["join(", "order("] {
        if let Some(inner) = s.strip_prefix(p) {
            s = inner.strip_suffix(')').unwrap_or(inner);
            order_read_as_join = p == "order(";
            break;
        }
    }
    let mut segs = Vec::new();
    let mut unparsable = Vec::new();
    // Whether any *representable* part named each strand, so a join that names
    // both can be reported rather than flattened. Counted only for parts that
    // actually yield a segment: `bond(5,10)` must not vote on strandedness.
    let mut saw_forward_part = false;
    let mut saw_reverse_part = false;
    for raw in s.split(',') {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        // Only a *balanced* `complement(...)` wrapper is unwrapped. Blanket
        // `trim_end_matches(')')` was what made `bond(5,10)` dangerous: split on
        // the comma it gives `bond(5` and `10)`, and stripping the stray paren
        // turned the second into a perfectly numeric `10` — a fabricated
        // `10..10` segment that `validate()` accepted.
        let (inner, part_is_reverse) = match raw
            .strip_prefix("complement(")
            .and_then(|r| r.strip_suffix(')'))
        {
            Some(i) => (i, true),
            None => (raw, false),
        };
        let part = inner.replace(['<', '>'], "");

        // A location part is a number or an `a..b` range and nothing else. Any
        // other character — `^` for a site between bases, `:` for a remote
        // accession, a letter or paren from a `bond(`/`gap(` operator — means
        // this is a form we do not model.
        let numeric = |t: &str| !t.is_empty() && t.chars().all(|c| c.is_ascii_digit());
        let ok = match part.split_once("..") {
            Some((a, b)) => numeric(a.trim()) && numeric(b.trim()),
            None => numeric(part.trim()),
        };
        if !ok {
            unparsable.push(raw.to_string());
            continue;
        }

        let (a, b) = match part.split_once("..") {
            Some((a, b)) => (a, b),
            None => (part.as_str(), part.as_str()),
        };
        if let (Ok(start), Ok(end)) = (a.trim().parse::<u64>(), b.trim().parse::<u64>()) {
            if end >= start && start > 0 {
                segs.push(Segment::new(start, end));
                if part_is_reverse {
                    saw_reverse_part = true;
                } else {
                    saw_forward_part = true;
                }
            } else {
                unparsable.push(raw.to_string());
            }
        } else {
            // All-digit but past `u64::MAX` (a hostile 20-digit coordinate).
            // `numeric()` accepted it as a number, so without this branch the
            // segment would neither parse nor be reported — vanishing silently,
            // the one outcome this function's contract exists to prevent.
            unparsable.push(raw.to_string());
        }
    }

    // Gated on a surviving segment, and structurally so rather than by
    // inspection: if every part was rejected on its own merits — the GenPept
    // `order(bond(30,115),bond(64,80))`, which yields nothing and four reports —
    // then nothing was re-expressed as a join and a line here would be false.
    // Reporting at the `strip_prefix` above instead would have fired on it and
    // broken `an_unrepresentable_location_is_reported_not_invented`.
    if order_read_as_join && !segs.is_empty() {
        unparsable.push(format!(
            "{}: order() read as join(), which asserts the parts are joined; a save writes \
             join() and the file no longer says what it said",
            loc.trim()
        ));
    }

    // A complement() nested inside join() flips the whole feature for our model.
    if saw_reverse_part {
        strand = Strand::Reverse;
    }
    if saw_forward_part && saw_reverse_part {
        // A mixed-strand join — `join(1..100,complement(500..600))`, which the
        // spec permits and trans-spliced and organelle annotations really use.
        //
        // `Segment` carries no strand and `Feature` carries exactly one, so
        // this is not a parse slip we could tighten up: it is a form the model
        // cannot hold. Whatever strand we choose, some part of the file's own
        // claim is contradicted, and a save then rewrites the location as
        // `complement(join(1..100,500..600))` — so the *file* now says
        // something it did not say before.
        //
        // The coordinates are kept, because losing an exon is worse than
        // mislabelling its strand, and the reinterpretation is reported through
        // the same channel as every other form we cannot express. Doing it
        // silently is how a map arrow ends up pointing at the wrong template
        // with nothing anywhere saying so.
        unparsable.push(format!(
            "{}: mixed-strand join, every part placed on the {} strand",
            loc.trim(),
            if strand.is_reverse() { "minus" } else { "plus" }
        ));
    }

    // `join(complement(a),complement(b))` and `complement(join(a,b))` are not
    // the same feature, and this model has room for only one of them.
    //
    // `Feature::segments` is stored in join order and a Reverse feature is read
    // back to front — `bins/pl-gui/src/aa.rs` reverses the parts before
    // translating, checked there against pKoV's stored `/translation` for SacB,
    // and `crates/pl-draw` and `crates/pl-wasm` read the same order. So
    // `complement(join(a,b))` splices rc(b) then rc(a), while the per-part
    // spelling splices rc(a) then rc(b): in that spelling file order IS
    // transcription order, and the parts have to be stored reversed to mean the
    // same thing.
    //
    // Without this, both spellings were stored in file order and `join_parts`
    // re-emitted them as `complement(join(...))` — so an INSDC-legal location
    // came back naming a different spliced product, with an empty report and
    // exit 0. Measured on a 60 bp record carrying
    // `join(complement(1..12),complement(31..42))`: the input splices
    // ATGAAACGCGGT+TGCTGGTGCTAA and the exported file splices them the other way
    // round, so a start codon ends up in the middle of the protein. pl's own
    // amino-acid track read the input that way too, before any save.
    //
    // Reversing here rather than reporting it as unrepresentable, because it IS
    // representable: `complement(join(31..42,1..12))` is exactly the input's
    // meaning, which is what the writer now emits. Nothing is reinterpreted, so
    // unlike the `order()` and mixed-strand branches above there is nothing to
    // report.
    //
    // Both conditions are load-bearing:
    //
    // - `!outer_complement` — `complement(join(complement(a),complement(b)))` is
    //   a double negation that this reader already flattens to one Reverse
    //   feature. Reordering on top of that would be a second guess about a form
    //   no emitter writes.
    // - `!saw_forward_part` — a mixed-strand join is already reported above as a
    //   reinterpretation; reordering it as well would change the file's claim
    //   twice over, and its exon order is not derivable from a spelling that
    //   contradicts itself.
    if saw_reverse_part && !saw_forward_part && !outer_complement {
        segs.reverse();
    }
    (segs, strand, unparsable)
}

// ---------------------------------------------------------------------------
// writing
// ---------------------------------------------------------------------------

/// Sanitise a name for the LOCUS line: no spaces, conventionally short.
pub fn locus_name(title: &str) -> String {
    let stem = title.rsplit_once('.').map(|(a, _)| a).unwrap_or(title);
    let cleaned: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches(['_', '.', '-']);
    // A name of "..." or "___" is not a name. Fall back rather than emit a
    // LOCUS line no parser will like.
    let usable = trimmed.chars().any(|c| c.is_ascii_alphanumeric());
    let name = if usable { trimmed } else { "sequence" };
    // The LOCUS name occupies columns 13-28: sixteen characters. Overrunning it
    // shifts every field after it, and the LOCUS line is the one line in the
    // format that strict parsers read positionally.
    name.chars().take(16).collect()
}

/// Render one interval as GenBank location parts, or `None` when the format
/// cannot express it at all.
///
/// `span` is the molecule length, needed only to split an interval that crosses
/// the origin; pass 0 when there is nothing to split against.
///
/// # The origin split belongs here
///
/// The model writes an origin-spanning interval as `end < start`, which
/// `Molecule::subseq`, the annotator and the SVG renderer all understand.
/// GenBank has no such form: `12..3` is not a location, and our own reader
/// silently dropped it — one feature in, zero out, and the molecule still
/// reported valid. So the wrap is expanded into `join(12..16,1..3)` at the
/// format boundary, which is exactly where `docs/PLAN.md` §5.3.1 says
/// coordinate conversions belong.
///
/// # And what cannot be split has to be said out loud
///
/// Three shapes have no legal GenBank form and used to be written literally
/// anyway, which is the same one-in-zero-out loss wearing a different hat:
///
/// - `start > span` with `end < start`. A `.dna` may carry
///   `<Segment range="150-50"/>` on a 100 bp molecule — `snapgene.rs` takes the
///   range at face value on purpose — and there is no origin to split against,
///   so this used to emit ` misc_feature 150..50`. Reading that back,
///   `parse_location` rejects it and `flush` drops the whole feature, and on a
///   circle `validate()` reports nothing either side of the trip.
/// - `start == 0`. GenBank numbers bases from 1, so `0..50` is rejected on
///   re-read in exactly the same way.
/// - `end == 0` with `end < start`. This one reached the file, because it took
///   the origin-crossing branch and came out as the second part of a join:
///   `<Segment range="5-0"/>` on a 16 bp molecule wrote
///   ` misc_feature join(5..16,1..0)` at exit 0 with nothing on stderr. `1..0`
///   names no base — GenBank locations are 1-based inclusive, so the low
///   bound cannot exceed the high one — and Biopython does not reject it but
///   "fixes" it, yielding a feature over the whole molecule. A wrong
///   annotation that looks deliberate is worse than a dropped one, which is
///   why this returns `None` and is reported rather than being clamped to
///   something plausible.
///
/// Returning `None` puts the caller in a position to report it.
fn location_parts(start: u64, end: u64, span: u64) -> Option<Vec<String>> {
    if start < 1 {
        return None;
    }
    if end < start {
        // `end == 0` names no base, so there is no second part to write and no
        // wrap to describe. It has to be refused here rather than in the caller
        // because this is the only branch that can emit `1..{end}`.
        if span >= start && end >= 1 {
            // Crosses the origin: two ranges, in reading order.
            return Some(vec![format!("{start}..{span}"), format!("1..{end}")]);
        }
        return None;
    }
    if span > 0 && end > span {
        // The feature runs past the last base of the molecule. GenBank has no
        // faithful form for that — a `{start}..{end}` under a shorter LOCUS line
        // is a location longer than the sequence it sits over, which Biopython
        // will "fix" or mis-extract — so report it through the report's
        // `absent` half rather than write it, the same guard the primer writer
        // already applies.
        return None;
    }
    Some(vec![format!("{start}..{end}")])
}

/// Wrap location parts in `join(...)` and `complement(...)` as needed.
fn join_parts(parts: &[String], reverse: bool) -> String {
    let joined = if parts.len() > 1 {
        format!("join({})", parts.join(","))
    } else {
        parts.concat()
    };
    if reverse {
        format!("complement({joined})")
    } else {
        joined
    }
}

/// Render a feature's location, reporting any segment GenBank cannot hold.
///
/// `None` means not one segment survived, so there is no location to write and
/// the caller must skip the feature — loudly.
fn format_location(f: &Feature, span: u64, absent: &mut Vec<String>) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    for s in &f.segments {
        match location_parts(s.start, s.end, span) {
            Some(p) => parts.extend(p),
            None => absent.push(format!(
                "feature {:?}: segment {}..{} has no GenBank form on a {span} bp molecule and was not written",
                f.name, s.start, s.end
            )),
        }
    }
    if parts.is_empty() {
        return None;
    }
    Some(join_parts(&parts, f.strand.is_reverse()))
}

/// Is this the `/note="color: #rrggbb"` line that [`write`] generates itself?
///
/// `ApEinfo_fwdcolor` and `ApEinfo_revcolor` were already skipped on the way
/// out for exactly this reason and `note` was simply missed. The reader stores
/// qualifiers verbatim, so every save/load cycle wrote the stored copy *and* a
/// freshly generated one: one colour note after the first export, five after
/// five, in the file and in `Feature::qualifiers` alike. Nothing corrupted —
/// the reader prefers the `ApEinfo` pair, so the colour never drifted — the
/// file just grew for ever on input the user thought was idempotent, starting
/// with any file that already carries an ApE colour note.
///
/// Matched on shape, not on the current colour: a stale note naming a colour
/// the feature no longer has is superseded by the one being written, and only
/// a note that is *nothing but* a colour is dropped. `color: #1a2b3c and then
/// some prose` is a note somebody wrote and survives.
fn is_generated_colour_note(key: &str, value: Option<&str>) -> bool {
    if key != "note" {
        return false;
    }
    let Some(rest) = value.and_then(|v| v.trim().strip_prefix("color:")) else {
        return false;
    };
    let hex = rest.trim();
    hex.len() == 7 && hex.starts_with('#') && hex[1..].chars().all(|c| c.is_ascii_hexdigit())
}

fn qualifier_lines(key: &str, value: &str, out: &mut String) {
    qualifier_lines_opt(key, Some(value), out)
}

/// Write one qualifier. `None` emits the bare `/key` form.
///
/// # A newline inside a value
///
/// GenBank has no spelling for one. Column 1-10 blank is a continuation and the
/// reader joins continuations with a space, so a line break can only ever come
/// back as a space. What it must NOT do is reach the file raw, and it used to:
/// this function split on `' '` alone, so `/note="line one\nline two"` was
/// emitted with the break intact and column 1 of the next line holding `l`. The
/// reader then read that as a new record and everything after it was lost —
/// measured, `/note="line one\nline two"` followed by `/codon_start="1"` came
/// back as `note = "line one /codon_start="` with `codon_start` GONE, and the
/// report was empty. The break is now normalised to a space here and
/// [`write_reporting`] says so — through `reduced`, since the qualifier is in
/// the file and only that one character changed; the qualifier that follows
/// survives.
fn qualifier_lines_opt(key: &str, value: Option<&str>, out: &mut String) {
    const PAD: &str = "                     "; // 21 spaces
    let Some(value) = value else {
        // `/pseudo` and friends. These used to be skipped entirely by a
        // `v.is_empty()` test at the call site, which silently turned every
        // pseudogene in an exported file into an ordinary protein-coding gene.
        out.push_str(PAD);
        out.push('/');
        out.push_str(key);
        out.push('\n');
        return;
    };
    let flat = flatten_value(value);
    let raw = format!("/{}=\"{}\"", key, flat.replace('"', "\"\""));
    let mut line = String::from(PAD);
    for word in raw.split(' ') {
        if line.len() + word.len() + 1 > 79 && line.trim() != "" {
            out.push_str(&line);
            out.push('\n');
            line = String::from(PAD);
        }
        if line != PAD {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.trim().is_empty() {
        out.push_str(&line);
        out.push('\n');
    }
}

/// Every control character in a qualifier value replaced by a space.
///
/// `\r\n` collapses to ONE space rather than two, so a value pasted from a
/// Windows editor does not gain a run of blanks. Anything else below U+0020 —
/// a stray tab, a form feed out of a lab notebook export — goes the same way:
/// the GenBank line format is column-significant and none of them has a
/// meaning inside a quoted value.
fn flatten_value(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    let mut last_was_break = false;
    for c in v.chars() {
        if (c as u32) < 0x20 {
            if !(last_was_break && (c == '\n' || c == '\r')) {
                out.push(' ');
            }
            last_was_break = c == '\n' || c == '\r';
        } else {
            out.push(c);
            last_was_break = false;
        }
    }
    out
}

/// Flatten a string that is about to be interpolated into a GenBank *line*,
/// and say so when it had to be changed.
///
/// [`flatten_value`] already did this for every qualifier value; the header
/// lines were interpolated raw, and they are the ones where it matters most.
/// GenBank reads column 1 as a keyword, so a line break inside `DEFINITION`
/// puts the text after it at column 1: a `.dna` whose `<Description>` reads
/// `Constitutive mNeonGreen vector.` / `ORIGIN of replication swapped for p15A`
/// — three ordinary lines a person typed into SnapGene's description box —
/// exported to a GenBank whose third line is `ORIGIN` at column 1. Our own
/// reader then took that as the start of the bases, and a 20 bp plasmid
/// re-read as 226 bp of `ACCESSION.VERSION.KEYWORDS.SOURCEsynthetic...`, at
/// exit 0 with an empty report. `mol.description` is assigned straight from
/// the note by `snapgene.rs` and `parse_notes` keeps interior control
/// characters, so no hostile encoding is needed to reach this.
///
/// The repair is here in the writer and not in the reader, for the same reason
/// [`fasta::write_record`](crate::fasta::write_record) sanitises its header
/// fields rather than asking the parser to refuse the input: `Molecule` is
/// built by `pl-py`, by the GUI's editor and by `pl-clone` as well as by the
/// parsers in this crate, and only the writer sees every one of them. Refusing
/// to *load* such a file would also cost the user the twenty bases that are
/// perfectly fine.
///
/// Reported rather than silently flattened, because the `.dna` writer keeps
/// the same string exactly (`xml::escape` turns a break into `&#10;`), so a
/// `.gb` and a `.dna` of one document disagree and nothing else would say
/// which one changed.
fn header_text(raw: &str, field: &str, reduced: &mut Vec<String>) -> String {
    if raw.chars().any(|c| (c as u32) < 0x20) {
        reduced.push(format!(
            "{field} contains a control character, which GenBank cannot hold — column 1 of a \
             line is a keyword — so it was written as a space (.dna keeps it exactly)"
        ));
    }
    flatten_value(raw)
}

/// Does this free-text field carry any text at all, once its markup is off?
///
/// SnapGene's description boxes are rich-text controls and they serialise as
/// HTML, so an UNTOUCHED box does not arrive here as an empty string. Block 5
/// of the 8117 bp lab plasmid this was measured against on 2026-09-04 writes
/// `description="<html><body></body></html>"` on 4 of its 9 primers, writes no
/// `description` attribute at all on 2 more, and carries real prose --
/// "Chloramphenicol resistance gene, reverse primer" -- on the last 3.
/// `!p.description.is_empty()` counts the first group, so 4 of the 7 `reduced`
/// lines 0.13.3 printed for that file reported the loss of a string with no
/// text in it, from a box nobody had typed into. A notice that fires on every
/// file is a notice nobody reads, and this one fired on the majority of the
/// primers in the first real file it met.
///
/// The test is "is any character outside a tag something other than
/// whitespace", not a match against that one literal, because the same empty
/// box is also spelled `<html><body><br></body></html>` and
/// `<html><body><p></p></body></html>`, and because `&nbsp;` reaches the model
/// verbatim: [`xml::unescape`](crate::xml::unescape) expands the five XML
/// entities and numeric references and nothing else, so `&#160;` becomes
/// U+00A0 -- whitespace, which `trim` removes -- while `&nbsp;` stays six
/// characters of text. It is replaced here rather than in the reader, which
/// must keep the bytes the file held.
///
/// Deliberately biased towards SPEAKING UP. A `<` that opens no tag -- `5' <-
/// 3'` is prose somebody could type into that box -- swallows everything after
/// it, and the text BEFORE it still counts, so the field is still reported.
/// The failure this cannot cause is a silent loss; the failure it can cause is
/// one extra line about a description whose entire content was `<...>`, a
/// shape no observed file has.
///
/// **Not a general HTML-to-text renderer, and it does not strip markup from
/// anything that is written.** It decides whether a field is EMPTY, and that
/// is all it decides. A description that does carry text reaches the file
/// exactly as the model holds it, markup and all -- see [`write_reporting`]'s
/// DEFINITION line, which uses this to choose between the description and the
/// molecule's name and then writes whichever it chose verbatim. Nothing in
/// this workspace renders a primer description at all: `pl info` prints it in
/// neither its text nor its `--json` branch, `bins/pl-gui` has no widget for
/// it, and the browser prototype puts it in a summary object nothing draws.
fn carries_text(raw: &str) -> bool {
    let mut in_tag = false;
    let mut text = String::new();
    for c in raw.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => text.push(c),
            _ => {}
        }
    }
    !text.replace("&nbsp;", " ").trim().is_empty()
}

/// Sanitise a feature key, and say so when it had to be changed.
///
/// A feature key is one token in columns 6-20 and the whole feature table's
/// structure rests on it. It arrives from the `.dna`'s `type=` attribute,
/// which is file-controlled and validated nowhere on the way in, so
/// `type="CDS&#10;ORIGIN"` used to emit `     CDS` followed by
/// `ORIGIN      1..12` at column 1 — a forged keyword that swallowed the rest
/// of the feature table as bases.
///
/// Illegal characters are mapped to `_` rather than the feature being dropped:
/// this is the same trade [`locus_name`] makes for the name column, and losing
/// an annotation is worse than renaming its key. The legal set is INSDC's —
/// alphanumerics, `_`, `-` and `'` — which keeps real keys such as `3'UTR`,
/// `-10_signal` and `misc_feature` untouched. A key with no alphanumeric left
/// is not a key at all, so it falls back the way `locus_name` does.
fn feature_key(kind: &str, name: &str, reduced: &mut Vec<String>) -> String {
    // Truncated by character, not by byte: a key that is not ASCII must not
    // panic here.
    let cut: String = kind.chars().take(15).collect();
    let cleaned: String = cut
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '\'' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let key = if cleaned.chars().any(|c| c.is_ascii_alphanumeric()) {
        cleaned
    } else {
        "misc_feature".to_string()
    };
    // TRUNCATION IS ITS OWN REPORT, and it was the one loss this function made
    // in silence. The comparison below is `key != cut` — against the string
    // ALREADY cut to fifteen characters — so a `type=` that lost nothing but
    // its tail passed it. `bins/pl-gui/src/main.rs` named that hole in
    // `plan_genbank`'s doc and could do nothing with it: the only channel was
    // the one the browser build refuses on, and refusing to export a plasmid
    // because one feature key is sixteen characters long is not a trade worth
    // making. The feature is in the file, under a shorter key, which is
    // exactly what `reduced` is for.
    if cut.chars().count() < kind.chars().count() {
        reduced.push(format!(
            "feature {name:?}: feature key {kind:?} is longer than the fifteen columns GenBank \
             gives it; it was written as {key:?}"
        ));
    }
    if key != cut {
        reduced.push(format!(
            "feature {name:?}: {kind:?} is not a GenBank feature key; it was written as {key:?}"
        ));
    }
    key
}

/// The LOCUS line, in the columns the specification names.
///
/// ```text
///   1-5   LOCUS          45-47  ss-/ds-/ms- or blank
///  13-28  name           48-53  molecule type
///  30-40  length         56-63  linear | circular
///  42-43  bp | aa        65-67  division
///                        69-79  DD-MMM-YYYY
/// ```
///
/// The previous line was 75 characters with every field left of where it
/// belongs -- length ending at 36 rather than 40, `bp` at 38, topology at 52 --
/// and Biopython 1.87 emitted `BiopythonParserWarning: Attempting to parse
/// malformed locus line` on **100% of our exports** and on none of the 303 real
/// corpus files. Biopython recovers through a lenient fallback, so this was a
/// conformance defect rather than corruption; a parser reading the columns as
/// specified got `"ular SYN"` for topology.
///
/// The width of the length field matters too: `{:>7}` overflowed at 10 Mbp and
/// shifted every field after it, which an annotation-only molecule can reach
/// carrying no sequence data at all.
fn locus_line(mol: &Molecule, name: &str, n: u64, date: &str) -> String {
    // Unknown strandedness is written blank rather than guessed. `ss-` on a
    // plasmid is a claim, and this is exactly the field where it is believed.
    let strandedness = match mol.double_stranded {
        Some(true) => "ds-",
        Some(false) => "ss-",
        None => "   ",
    };
    let topology = if mol.topology.is_circular() {
        "circular"
    } else {
        "linear"
    };
    format!(
        "LOCUS       {name:<16} {n:>11} bp {strandedness}{:<6}  {topology:<8} {:<3} {date}",
        "DNA", "SYN"
    )
}

/// What a GenBank write cost, split by whether the work reached the file.
///
/// One vector was one severity, and the two are not the same thing. `absent`
/// is an annotation that is NOT in the file — a feature no segment of which
/// has a GenBank location, a primer with no binding site to hang it on. Open
/// the export and the work is gone. `reduced` is an annotation that IS in the
/// file, spelled smaller: a `/note` whose line break became a space, a feature
/// key cut to the fifteen columns GenBank gives it, a primer's free-text
/// description that the format has nowhere to put. Open the export and the
/// annotation is there; something about it is not.
///
/// # Why this is a type and not a second `Vec` argument
///
/// The consumers want different halves of it, and until 2026-09-04 they had no
/// way to ask:
///
/// - `crates/pl-wasm` turns a non-empty report into a REFUSAL to write the
///   file at all, because a browser download has one buffer and one return
///   code and cannot hand over both the bytes and the hedge. That is right for
///   `absent` and much too strong for `reduced`: for a few hours on 2026-09-03
///   a primer's dropped description was pushed into the one vector there was,
///   and the browser stopped exporting any molecule whose primer carried a
///   note. The revert kept the export working and left the loss silent, which
///   is the other half of the same bug.
/// - the desktop GUI computes `faithful` from the report and clears the
///   unsaved-changes dot with it. It consults BOTH: a reduced annotation is
///   still work that is on screen and not in the file the way the screen has
///   it, so a save that reduces has not saved the document.
/// - `pl convert` prints both, in two sentences, because stderr is the one
///   surface here with room to say two different things.
///
/// A `(String, Vec<String>, Vec<String>)` would have carried the same
/// information and let one call site swap the halves silently; the fields are
/// named so that cannot happen.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct WriteReport {
    /// Work the file does not contain.
    pub absent: Vec<String>,
    /// Work the file contains in a diminished form.
    pub reduced: Vec<String>,
}

impl WriteReport {
    /// Nothing was lost either way — the file carries the whole document.
    ///
    /// This is what `faithful` is, and it is deliberately the only aggregate
    /// offered. A caller that wants a COUNT, a JOINED STRING or a single
    /// iterator has to reach for `absent` or `reduced` by name, because the
    /// two get different sentences on every surface that reports them, and a
    /// convenience that flattened them is how they became one channel in the
    /// first place.
    pub fn is_empty(&self) -> bool {
        self.absent.is_empty() && self.reduced.is_empty()
    }
}

/// Render a molecule as GenBank. `date` is `(day, month_index_0_based, year)`;
/// passing it in keeps this function pure and its output reproducible.
///
/// This drops the report. Prefer [`write_reporting`] anywhere the caller can
/// tell the user what the format could not carry — an annotation GenBank has no
/// form for leaves no trace in the file it is missing from.
pub fn write(mol: &Molecule, title: &str, date: (u32, usize, i32)) -> String {
    write_reporting(mol, title, date).0
}

/// Render a molecule as GenBank, and say what the format could not hold.
///
/// The report is empty for the overwhelming majority of molecules. It is not
/// empty for the ones that matter: a feature segment or a primer binding site
/// with no legal GenBank location, which is the class that used to be skipped
/// by a bare `continue` with the function returning a `String` and so no
/// channel to say anything at all.
///
/// It has two halves because there are two severities — see [`WriteReport`],
/// which carries the argument. In short: `absent` is work that is not in the
/// file, `reduced` is work that is in the file with something taken off it,
/// and a caller that treats the second like the first refuses to export
/// perfectly usable plasmids.
pub fn write_reporting(
    mol: &Molecule,
    title: &str,
    date: (u32, usize, i32),
) -> (String, WriteReport) {
    let mut report = WriteReport::default();
    let name = locus_name(title);
    let n = mol.span();
    let (d, m, y) = date;
    let date_str = format!("{:02}-{}-{}", d, MONTHS[m.min(11)], y);

    let mut out = String::new();
    out.push_str(&locus_line(mol, &name, n, &date_str));
    out.push('\n');
    // `carries_text`, not `is_empty`, for the same reason the primer loop
    // below uses it -- and here the cost of the weaker test was not a spurious
    // report but visible junk in the file. A `<Description>` holding SnapGene's
    // untouched rich-text box is not empty, so it won two ways: it DISPLACED
    // the molecule's name, which is what this fallback exists to supply, and
    // then it was written out as `DEFINITION  <html><body></body></html>.`
    // with an entirely empty report, because `header_text` reports control
    // characters and nothing else.
    //
    // Constructed rather than observed, and the distinction is worth keeping:
    // the 32 block-6 payloads surveyed in docs/DNA-FORMAT.md all hold plain
    // text in `<Description>`, and the plasmid measured on 2026-09-04 has no
    // `<Description>` at all. The HTML habit is established in block 5's
    // primer descriptions, which is a different element; this is the same
    // program writing the same kind of field, so the two agree about what
    // empty means rather than waiting for a file to prove it.
    //
    // Markup is NOT stripped from a description that does carry text. That
    // would rewrite the DEFINITION line of real files on the strength of a
    // shape nothing here has seen.
    let def = if carries_text(&mol.description) {
        mol.description.as_str()
    } else {
        name.as_str()
    };
    let def = header_text(def, "DEFINITION", &mut report.reduced);
    out.push_str(&format!("DEFINITION  {def}.\n"));
    out.push_str("ACCESSION   .\nVERSION     .\nKEYWORDS    .\n");
    out.push_str("SOURCE      synthetic DNA construct\n  ORGANISM  synthetic DNA construct\n");
    out.push_str("COMMENT     Converted by Polylinker.\n");
    if let Some(uuid) = mol.note("UUID") {
        // Same treatment as DEFINITION and for the same reason: this is a
        // continuation of COMMENT, so a break in it reaches column 1 too.
        let uuid = header_text(
            uuid,
            "the COMMENT source document UUID",
            &mut report.reduced,
        );
        out.push_str(&format!("            Source document UUID: {uuid}\n"));
    }
    out.push_str("FEATURES             Location/Qualifiers\n");
    // `source` is the one location in this writer that does not go through
    // `location_parts`, and it was hard-coded as `1..{n}`. On a molecule with no
    // bases — a standalone annotation track with no ORIGIN block and no `bp`
    // field, or a FASTA record with a header and nothing under it — that is
    // `source 1..0`, which is not a legal INSDC base range at all;
    // `location_parts(1, 0, 0)` returns `None` for exactly that shape, so the
    // writer's own policy function refuses what this line printed, and it
    // printed it with exit 0 and an empty report. Our own reader cannot see it,
    // because `flush` discards `source` before `parse_location` runs.
    //
    // A record with no bases has no source to describe, so the feature and its
    // two qualifiers are omitted rather than written with an invented range.
    if n >= 1 {
        out.push_str(&format!("     source          1..{n}\n"));
        qualifier_lines("organism", "synthetic DNA construct", &mut out);
        qualifier_lines("mol_type", "other DNA", &mut out);
    }

    for f in &mol.features {
        let kind = if f.kind.is_empty() {
            "misc_feature"
        } else {
            &f.kind
        };
        let key = feature_key(kind, &f.name, &mut report.reduced);
        let Some(loc) = format_location(f, n, &mut report.absent) else {
            // Every segment was unwritable and each one has already been named
            // above. Writing the feature key with an empty location would
            // produce a line no parser can read, so the feature is skipped —
            // and the skip is said out loud, which is the whole difference
            // between this and the `continue` it replaces.
            report.absent.push(format!(
                "feature {:?} was not written: no segment had a GenBank form",
                f.name
            ));
            continue;
        };
        out.push_str(&format!("     {key:<15} {loc}\n"));
        qualifier_lines("label", &f.name, &mut out);
        // Whether the colour block below will generate its own `/note="color:
        // ..."`, in which case a stored one is a duplicate rather than content.
        let writes_colour = f.color().is_some();
        for (k, v) in &f.qualifiers {
            // A key must be a legal GenBank qualifier name. The reader used to
            // manufacture keys out of prose when it mistook a continuation line
            // for a new qualifier; that is fixed, but validating here keeps a
            // malformed input from becoming malformed output.
            if k == "label"
                || k.starts_with("ApEinfo")
                || (writes_colour && is_generated_colour_note(k, v.as_deref()))
                || k.is_empty()
                || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                continue;
            }
            // Reported, because `flatten_value` is a lossy rescue and not a
            // round trip: the value reaches the file whole and the qualifier
            // after it survives, but the line break itself is a space from here
            // on. Silence would make a `.gb` and a `.dna` of the same document
            // disagree with nothing anywhere to say which one changed.
            if v.as_deref()
                .is_some_and(|s| s.chars().any(|c| (c as u32) < 0x20))
            {
                report.reduced.push(format!(
                    "feature {:?}: qualifier /{k} contains a line break, which GenBank cannot \
                     express inside a value; it was written as a space (.dna keeps it exactly)",
                    f.name
                ));
            }
            qualifier_lines_opt(k, v.as_deref(), &mut out);
        }
        if let Some(c) = f.color() {
            qualifier_lines("ApEinfo_fwdcolor", c, &mut out);
            qualifier_lines("ApEinfo_revcolor", c, &mut out);
            qualifier_lines("note", &format!("color: {c}"), &mut out);
        }
    }

    for p in &mol.primers {
        // TWO LOSSES THAT USED TO BE SILENT, reported here rather than inside
        // the site loop below, because both are properties of the PRIMER and
        // neither depends on a site being written.
        //
        // The mechanism matters more than the two cases. The report is what
        // `faithful` is computed from -- `bins/pl-gui/src/main.rs` sets
        // `faithful = report.is_empty()` and, when it is true, both clears the
        // unsaved-changes dot and retargets the document at the file just
        // written. So anything dropped here without a line in the report is
        // dropped from a document the editor then calls saved. Until
        // 2026-09-03 this loop pushed a line only for a site past the end and
        // a site with no GenBank form; a primer could lose its description, or
        // vanish whole, and the save still counted as faithful BY OMISSION.
        //
        // Keyed on `p.sites.is_empty()` and not on "no site was written": a
        // primer whose only site was rejected above is already reported by
        // that branch, and counting it twice would change the exact
        // `report.absent.len() == 1` that
        // `a_primer_binding_site_past_the_end_is_reported_not_silently_skipped`
        // asserts.
        if p.sites.is_empty() {
            report.absent.push(format!(
                "primer {:?}: has no binding site, and GenBank has no way to carry \
                 a primer that is not bound to a position, so the oligo {:?} was \
                 not written at all",
                p.name, p.seq
            ));
        }
        // Whether this primer reached the file at all, which decides whether
        // its description is a REDUCTION or part of a loss already reported.
        let mut wrote_a_site = false;
        for s in &p.sites {
            // A site past the end of the molecule is skipped rather than
            // written, because a `primer_bind` at 5000..5100 on a 2686 bp
            // record claims annealing to bases the file does not contain. It is
            // reported, which is the part that was missing.
            if s.end >= s.start && s.end > n {
                report.absent.push(format!(
                    "primer {:?}: binding site {}..{} lies past the end of a {n} bp molecule and was not written",
                    p.name, s.start, s.end
                ));
                continue;
            }
            // A site that crosses the origin arrives here as `end < start`,
            // exactly as a feature segment does, and it is *valid*: `validate`
            // calls a wrap legal on a circle, and `Molecule::rotate` produces
            // one for any primer straddling the new origin — a 2686 bp circle
            // carrying M13F at 100..116, rotated to origin 110, gives 2677..7.
            // That used to hit `s.end < s.start` and vanish without a word,
            // while a feature segment at the identical coordinates was written
            // as `join(2677..2686,1..7)` two loops above. Features and primers
            // now agree about what is expressible.
            let Some(parts) = location_parts(s.start, s.end, n) else {
                report.absent.push(format!(
                    "primer {:?}: binding site {}..{} has no GenBank form on a {n} bp molecule and was not written",
                    p.name, s.start, s.end
                ));
                continue;
            };
            let loc = join_parts(&parts, s.strand.is_reverse());
            out.push_str(&format!("     {:<15} {}\n", "primer_bind", loc));
            qualifier_lines("label", &p.name, &mut out);
            let note = match s.tm {
                Some(tm) => format!("primer {}; Tm: {} C", p.seq, tm),
                None => format!("primer {}", p.seq),
            };
            qualifier_lines("note", &note, &mut out);
            wrote_a_site = true;
        }
        // THE DESCRIPTION, which used to go without a word.
        //
        // `snapgene.rs` writes `description="..."` on the `<Primer>` element
        // and GenBank has nowhere to put it: the two qualifiers this loop
        // emits are `/label` (the name) and `/note` (the oligo and its Tm),
        // and a second `/note` would be read back as part of the primer's
        // note by our own reader. So the text a person typed about what the
        // primer is FOR is dropped, and a `.dna` converted to `.gb` lost it
        // at exit 0 with an empty report.
        //
        // For a few hours on 2026-09-03 this was pushed into the one vector
        // there was, and it was reverted the same day because that vector is
        // what `crates/pl-wasm` refuses on: the browser build stopped
        // exporting any molecule whose primer merely carried a note. The
        // revert kept the export working and left the loss silent. `reduced`
        // is the channel that was missing -- the primer IS in the file, with
        // its name, its oligo and its position; only the prose is gone.
        //
        // Gated on `wrote_a_site`: when nothing was written for this primer
        // the description is not a reduction of anything, it is part of the
        // primer that the branch above has already reported as absent whole.
        //
        // AND gated on the description carrying TEXT, since 2026-09-04. The
        // predicate here was `!p.description.is_empty()` in 0.13.3, and it was
        // wrong on the first real file it met: 4 of the 9 primers in the
        // 8117 bp plasmid measured that day hold SnapGene's untouched
        // rich-text box, `<html><body></body></html>`, so `pl convert --to gb`
        // announced seven reductions of which four had lost nothing. It was
        // worse than one noisy line each: `bins/pl` prints the first three
        // entries of this vector, the four empty ones sort first in block 5,
        // and so the three REAL losses on that file were pushed out of the
        // report entirely by the four that were not losses at all.
        //
        // See `carries_text` for why the check is structural rather than a
        // comparison against that one literal.
        if wrote_a_site && carries_text(&p.description) {
            report.reduced.push(format!(
                "primer {:?}: its description {:?} has no GenBank form -- the primer is in the \
                 file with its name, oligo and position, and the description is not (.dna keeps \
                 it exactly)",
                p.name, p.description
            ));
        }
    }

    out.push_str("ORIGIN\n");
    // Decoded ONCE, then grouped — never grouped and then decoded.
    //
    // Slicing the raw bytes into 10-byte groups first and calling
    // `from_utf8_lossy` on each group meant no decode ever saw a multi-byte
    // character that straddled a group or line boundary: it saw a lone lead
    // byte at the end of one group and a lone continuation byte at the start of
    // the next, and turned each into its own U+FFFD. `acgtacgta` + µ (U+00B5,
    // C2 B5) + `cgtacgtac` is 20 bytes with C2 at index 9; it came back out as
    // 18 ASCII bases plus two replacement characters, which re-parses to 24
    // bytes. One base in, two mojibake characters out, and both the content and
    // the length of the exported sequence changed with nothing said. Decoding
    // the whole sequence first cannot split a character, because there is no
    // boundary left to split on.
    //
    // Counting the base index in characters rather than bytes follows from the
    // same decision, and is identical for the ASCII every real file holds.
    let decoded = String::from_utf8_lossy(&mol.seq);

    // Whitespace and digits cannot come back out of an ORIGIN block, so they
    // must not go into one.
    //
    // `parse_record`'s base loop filters exactly `!is_ascii_whitespace() &&
    // !is_ascii_digit()`, because the block's own line numbers and group
    // spacing are made of them. So those two classes are precisely the
    // characters that do not survive a round trip, and a line break is the
    // dangerous one: written raw it ends the line, and whatever follows sits
    // at column 1 where `LOCUS` opens a second record. Measured on a 129 bp
    // `.dna` whose block 0 payload held a break and a LOCUS line — the
    // SnapGene reader assigns that payload to `Molecule::seq` verbatim — the
    // export re-read as two records, the first holding 10 bases. 119 bases
    // gone, exit 0, empty report.
    //
    // Written as `n` rather than dropped: `n` is IUPAC for an unknown base and
    // claims nothing, and dropping would change the length the LOCUS line
    // above already declares from `mol.span()` — every feature location in the
    // file was computed against that same number, so a shorter ORIGIN would
    // move every annotation relative to the bases it names. Once no whitespace
    // is left, every emitted line begins with its own index, which is what
    // makes it impossible for sequence content to reach column 1 at all.
    //
    // Reported for the same reason the qualifier path reports `flatten_value`:
    // the `.dna` writer keeps these bytes exactly, so the two exports of one
    // document differ and only this line says which one changed.
    let mut substituted = 0usize;
    let decoded: String = decoded
        .chars()
        .map(|c| {
            if c.is_ascii_whitespace() || c.is_ascii_digit() {
                substituted += 1;
                'n'
            } else {
                c
            }
        })
        .collect();
    if substituted > 0 {
        report.reduced.push(format!(
            "ORIGIN: {substituted} character(s) of the sequence are whitespace or digits, which \
             a GenBank ORIGIN block cannot carry; each was written as `n` (.dna keeps them exactly)"
        ));
    }
    let mut chars = decoded.chars().peekable();
    let mut pos = 0usize;
    while chars.peek().is_some() {
        let mut line = format!("{:>9}", pos + 1);
        for _ in 0..6 {
            let group: String = chars.by_ref().take(10).collect();
            if group.is_empty() {
                break;
            }
            pos += group.chars().count();
            line.push(' ');
            line.push_str(&group);
        }
        out.push_str(&line);
        out.push('\n');
    }
    out.push_str("//\n");
    (out, report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_core::{BindingSite, Primer};

    /// A minimal record wrapping one feature's qualifier block.
    fn with_quals(quals: &str) -> Molecule {
        let src = format!(
            "LOCUS       test                      12 bp    DNA     linear   SYN 27-JUL-2026\n\
             FEATURES             Location/Qualifiers\n\
             \x20    CDS             1..12\n{quals}\
             ORIGIN\n        1 acgtacgtacgt\n//\n"
        );
        parse(&src)
    }

    #[test]
    fn a_continuation_line_starting_with_a_slash_is_not_a_new_qualifier() {
        // 168 occurrences across 66 of this project's corpus files. The value
        // truncated, the continuation became a fabricated qualifier key, and
        // every later continuation line was dropped.
        let m = with_quals(
            "                     /product=\"Energy production\n\
             \x20                    /conversion; Region: UbiH\n\
             \x20                    /and more text\"\n\
             \x20                    /codon_start=1\n",
        );
        assert_eq!(m.features.len(), 1);
        let f = &m.features[0];
        assert_eq!(
            f.qualifier("product"),
            Some("Energy production /conversion; Region: UbiH /and more text")
        );
        // No key was invented out of the prose...
        assert!(!f.has_qualifier("conversion; Region: UbiH"));
        assert!(!f.has_qualifier("and"));
        // ...and the qualifier that genuinely followed still arrived.
        assert_eq!(f.qualifier("codon_start"), Some("1"));
        assert_eq!(f.qualifiers.len(), 2, "{:?}", f.qualifiers);
    }

    /// PROVEN TO FAIL against this file as it stood: `qualifier_lines_opt` split
    /// on `' '` alone and never looked at `'\n'`, so the break reached the file
    /// raw and column 1 of the next line held the value's own text. Measured,
    /// `/note="line one\nline two"` followed by `/codon_start="1"` came back as
    ///
    /// ```text
    /// [("label", Some("x")), ("note", Some("line one /codon_start="))]
    /// ```
    ///
    /// — the note truncated and mangled, `codon_start` GONE, `unwritable` empty
    /// and exit clean. The `.dna` writer keeps the value byte for byte, so the
    /// two formats disagreed about the same document with nothing anywhere to
    /// say so.
    ///
    /// The feature editor is the first surface in the program that can put a
    /// newline in a value — its qualifier box is the only `TextEdit::multiline`
    /// in the GUI — so one Enter, or one pasted note, reached this.
    #[test]
    fn a_line_break_in_a_value_does_not_eat_the_qualifier_after_it() {
        let mut m = Molecule {
            declared_len: Some(12),
            ..Default::default()
        };
        m.seq = "acgtacgtacgt".into();
        let mut f = Feature::new("x", "CDS");
        f.segments.push(Segment::new(1, 12));
        f.set_qualifier("note", "line one\nline two");
        f.set_qualifier("codon_start", "1");
        m.features.push(f);

        let (text, unwritable) = write_reporting(&m, "t", (1, 0, 2026));
        // Non-vacuity: without the newline there is nothing here to lose.
        assert!(
            m.features[0].qualifier("note").unwrap().contains('\n'),
            "the fixture has no line break to lose"
        );
        // Every line of a FEATURES block either starts a feature (columns 6-20)
        // or is a continuation (columns 1-21 blank). A raw newline broke that.
        for line in text.lines().skip_while(|l| !l.starts_with("FEATURES")) {
            if line.starts_with("ORIGIN") || line.starts_with("//") {
                break;
            }
            assert!(
                line.starts_with("     ") || line.starts_with("FEATURES"),
                "a value's own text reached column 1: {line:?}"
            );
        }

        let back = parse(&text);
        let g = &back.features[0];
        assert_eq!(
            g.qualifier("codon_start"),
            Some("1"),
            "the qualifier AFTER the break survived: {:?}",
            g.qualifiers
        );
        assert_eq!(
            g.qualifier("note"),
            Some("line one line two"),
            "and the value itself is whole, with the break as a space"
        );
        // Reported, because that space is a real loss and the `.dna` writer
        // does not make it.
        // Through `reduced` and not `absent`: the qualifier reached the file
        // and the reader read it back, with one character normalised. Pinned
        // on the severity as well as the text, because sorting it the other
        // way is what made the browser build refuse whole plasmids.
        assert!(
            unwritable.reduced.iter().any(|u| u.contains("line break")),
            "{unwritable:?}"
        );
        assert!(
            unwritable.absent.is_empty(),
            "nothing is missing from this file: {unwritable:?}"
        );
    }

    #[test]
    fn an_escaped_quote_survives_repeated_round_trips() {
        // `trim_matches('"')` never collapsed `""`, so the quote count doubled
        // on every save/load: 2 -> 4 -> 8 -> 16.
        let m = with_quals("                     /note=\"a \"\"quoted\"\" word\"\n");
        assert_eq!(m.features[0].qualifier("note"), Some(r#"a "quoted" word"#));

        let mut cur = m;
        for cycle in 0..4 {
            let text = write(&cur, "test", (27, 6, 2026));
            cur = parse(&text);
            assert_eq!(
                cur.features[0].qualifier("note"),
                Some(r#"a "quoted" word"#),
                "quote handling drifted on cycle {cycle}"
            );
        }
    }

    #[test]
    fn an_escaped_quote_at_a_line_wrap_does_not_end_the_value() {
        // The nastier half: when `""` landed on a wrap boundary the old
        // `ends_with('"')` read it as the close, and every remaining
        // continuation line was silently discarded.
        let m = with_quals(
            "                     /note=\"first part ends with an escaped \"\"\n\
             \x20                    second line must not be lost\"\n",
        );
        let note = m.features[0].qualifier("note").unwrap();
        assert!(note.contains("second line must not be lost"), "{note:?}");
        assert!(
            note.contains('"'),
            "the escaped quote should be literal: {note:?}"
        );
    }

    #[test]
    fn a_valueless_qualifier_survives_a_round_trip() {
        // 11,561 `/pseudo` in this project's corpus were being deleted on
        // write, silently turning every pseudogene into a protein-coding gene.
        let m = with_quals(
            "                     /pseudo\n\
             \x20                    /ribosomal_slippage\n\
             \x20                    /gene=\"thrA\"\n",
        );
        let f = &m.features[0];
        assert!(f.has_qualifier("pseudo"));
        assert_eq!(f.qualifier("pseudo"), None, "it has no value, and says so");
        assert!(f.has_qualifier("ribosomal_slippage"));
        assert_eq!(f.qualifier("gene"), Some("thrA"));

        let text = write(&m, "test", (27, 6, 2026));
        assert!(
            text.lines().any(|l| l.trim() == "/pseudo"),
            "/pseudo must be written bare, not dropped:\n{text}"
        );
        let again = parse(&text);
        assert!(again.features[0].has_qualifier("pseudo"));
        assert!(again.features[0].has_qualifier("ribosomal_slippage"));
        assert_eq!(again.features[0].qualifier("gene"), Some("thrA"));
    }

    #[test]
    fn an_empty_value_is_not_the_same_as_no_value() {
        let m = with_quals("                     /pseudo\n                     /replace=\"\"\n");
        let f = &m.features[0];
        assert_eq!(f.qualifier("pseudo"), None);
        assert_eq!(f.qualifier("replace"), Some(""));
        let text = write(&m, "test", (27, 6, 2026));
        assert!(text.lines().any(|l| l.trim() == "/pseudo"));
        assert!(text.lines().any(|l| l.trim() == r#"/replace="""#), "{text}");
    }

    #[test]
    fn a_name_containing_the_word_circular_does_not_make_the_molecule_circular() {
        // `locus.contains("circular")` matched the *name* as readily as the
        // topology field. This is self-inflicted: `locus_name` keeps `-` and
        // `write` puts the document title in the name column, so our own export
        // of a linear molecule called `pCircularise-v2.dna` read back circular
        // -- and topology feeds `pl_enzymes::fragments`, so it changes computed
        // digest results.
        let m = parse(
            "LOCUS       pCircularise-v2      100 bp    DNA     linear   SYN 27-JUL-2026
             ORIGIN
        1 acgt
//
",
        );
        assert_eq!(m.topology, Topology::Linear, "the name is not the topology");

        // ...and the real field still decides.
        let c = parse(
            "LOCUS       pLinearThing         100 bp    DNA     circular SYN 27-JUL-2026
             ORIGIN
        1 acgt
//
",
        );
        assert_eq!(c.topology, Topology::Circular);
    }

    #[test]
    fn a_name_containing_ss_does_not_assert_single_strandedness() {
        // `pcDNA3-ss-mCherry` claimed `double_stranded: Some(false)` about an
        // ordinary plasmid. Unknown must stay unknown.
        let m = parse(
            "LOCUS       pcDNA3-ss-mCherry    100 bp    DNA     linear   SYN 27-JUL-2026
//
",
        );
        assert_eq!(m.double_stranded, None, "absent means unknown, not single");

        let d = parse(
            "LOCUS       plain                100 bp ds-DNA     linear   SYN 27-JUL-2026
//
",
        );
        assert_eq!(d.double_stranded, Some(true));
        let ss = parse(
            "LOCUS       plain                100 bp ss-RNA      linear   SYN 27-JUL-2026
//
",
        );
        assert_eq!(ss.double_stranded, Some(false));
    }

    #[test]
    fn a_ragged_locus_line_degrades_rather_than_guessing() {
        // Real corpus lines look like this; none may panic or invent a value.
        for src in [
            "LOCUS       WT       74 bp
//
",
            "LOCUS       a|b|c    12 bp    DNA
//
",
            "LOCUS
//
",
            "LOCUS       only-a-name
//
",
        ] {
            let m = parse(src);
            assert_eq!(m.topology, Topology::Linear);
            assert_eq!(m.double_stranded, None);
        }
        assert_eq!(
            parse(
                "LOCUS       WT       74 bp
//
"
            )
            .declared_len,
            Some(74)
        );
    }

    #[test]
    fn the_locus_line_lands_in_the_columns_the_spec_names() {
        // 79 characters, and every field where a positional reader expects it.
        // The old line was 75 with everything shifted left, and Biopython
        // warned on 100% of our exports and 0 of 303 real files.
        let m = Molecule {
            seq: b"acgtacgtacgt".to_vec(),
            topology: Topology::Circular,
            double_stranded: Some(true),
            ..Default::default()
        };
        let line = write(&m, "pTest.dna", (27, 6, 2026))
            .lines()
            .next()
            .unwrap()
            .to_string();
        assert_eq!(line.len(), 79, "{line:?}");
        assert_eq!(&line[0..5], "LOCUS");
        assert_eq!(line[12..28].trim(), "pTest");
        assert_eq!(line[29..40].trim(), "12");
        assert_eq!(&line[41..43], "bp");
        assert_eq!(&line[44..47], "ds-");
        assert_eq!(line[47..53].trim(), "DNA");
        assert_eq!(&line[55..63], "circular");
        assert_eq!(&line[64..67], "SYN");
        assert_eq!(&line[68..79], "27-JUL-2026");

        // A linear molecule of unknown strandedness leaves that field blank
        // rather than claiming anything.
        let m2 = Molecule {
            seq: b"acgt".to_vec(),
            ..Default::default()
        };
        let l2 = write(&m2, "x.gb", (1, 0, 2026))
            .lines()
            .next()
            .unwrap()
            .to_string();
        assert_eq!(l2.len(), 79);
        assert_eq!(&l2[44..47], "   ", "unknown strandedness must be blank");
        assert_eq!(&l2[55..63], "linear  ");
    }

    #[test]
    fn a_very_long_molecule_does_not_shift_the_locus_fields() {
        // `{:>7}` overflowed at 10 Mbp and pushed every later field right.
        // Reachable with no sequence data at all, via a declared length.
        let m = Molecule {
            declared_len: Some(2_500_000_000),
            ..Default::default()
        };
        let line = write(&m, "big.gb", (1, 0, 2026))
            .lines()
            .next()
            .unwrap()
            .to_string();
        assert_eq!(line.len(), 79, "{line:?}");
        assert_eq!(line[29..40].trim(), "2500000000");
        assert_eq!(&line[41..43], "bp");
    }

    #[test]
    fn our_own_output_round_trips_topology_and_strandedness() {
        for (topology, ds) in [
            (Topology::Circular, Some(true)),
            (Topology::Linear, Some(false)),
            (Topology::Linear, None),
            (Topology::Circular, None),
        ] {
            let m = Molecule {
                seq: b"acgtacgt".to_vec(),
                topology,
                double_stranded: ds,
                ..Default::default()
            };
            let again = parse(&write(&m, "pCircularise-v2.dna", (27, 6, 2026)));
            assert_eq!(again.topology, topology, "topology drifted");
            assert_eq!(again.double_stranded, ds, "strandedness drifted");
        }
    }

    #[test]
    fn a_feature_crossing_the_origin_survives_a_round_trip() {
        // The model writes a wrap as `end < start`. GenBank has no such form,
        // and our own reader silently dropped `12..3` — one feature in, zero
        // out, molecule still reported valid. It must be split into a join at
        // the format boundary.
        let mut m = Molecule {
            seq: b"AAAACCCCGGGGTTTT".to_vec(),
            topology: Topology::Circular,
            ..Default::default()
        };
        let mut f = Feature::new("wraps", "misc_feature");
        f.segments.push(Segment::new(13, 3));
        m.features.push(f);

        let text = write(&m, "test", (27, 6, 2026));
        assert!(
            text.contains("join(13..16,1..3)"),
            "the wrap must become a join:
{text}"
        );

        let again = parse(&text);
        assert_eq!(again.features.len(), 1, "the feature was dropped");
        let segs = &again.features[0].segments;
        assert_eq!(segs.len(), 2);
        assert_eq!((segs[0].start, segs[0].end), (13, 16));
        assert_eq!((segs[1].start, segs[1].end), (1, 3));
        // The same seven bases, either way round.
        assert_eq!(segs.iter().map(|s| s.len()).sum::<u64>(), 7);
    }

    #[test]
    fn a_primer_binding_site_crossing_the_origin_is_written_as_a_join() {
        // `Molecule::rotate` produces exactly this shape, and `validate()`
        // calls it legal: on a circle `end < start` is not a mistake, it is an
        // annotation running across the origin. A 2686 bp plasmid carrying
        // M13F at 100..116, rotated to origin 110, gives a site of 2677..7 —
        // and `write` hit `s.end < s.start`, skipped the site with a bare
        // `continue`, and returned a `String` with no channel to mention it.
        // The exported file simply had no primer_bind line for M13F, while a
        // feature segment at the identical coordinates was written as
        // join(2677..2686,1..7) the whole time.
        let mut mol = Molecule {
            seq: b"a".repeat(2686),
            topology: Topology::Circular,
            ..Default::default()
        };
        mol.primers.push(Primer {
            name: "M13F".into(),
            seq: "GTAAAACGACGGCCAGT".into(),
            description: String::new(),
            sites: vec![BindingSite {
                start: 2677,
                end: 7,
                strand: Strand::Forward,
                tm: Some(55.3),
            }],
        });

        let (text, report) = write_reporting(&mol, "p.dna", (27, 6, 2026));
        assert!(
            text.contains("join(2677..2686,1..7)"),
            "the wrapping site must become a join:\n{text}"
        );
        assert!(
            report.is_empty(),
            "nothing was lost, so nothing to report: {report:?}"
        );

        // THIS HALF USED TO LOOK FOR A FEATURE, and until 2026-09-03 it found
        // one: the reader kept every `primer_bind` as an ordinary feature, so
        // the assertions below read `back.features` and the primer itself was
        // gone. `promote_primers` now puts it back where it started, so the
        // wrap is asserted on the site that made it rather than on the feature
        // it was spelled as. The join is still checked above, on the text.
        let back = parse(&text);
        assert!(
            !back.features.iter().any(|f| f.kind == "primer_bind"),
            "a promoted primer must not also remain a feature: {:?}",
            back.features
        );
        assert_eq!(back.primers.len(), 1, "{:?}", back.primers);
        let p = &back.primers[0];
        assert_eq!(p.name, "M13F");
        assert_eq!(p.seq, "GTAAAACGACGGCCAGT");
        assert_eq!(p.sites.len(), 1, "{:?}", p.sites);
        // The wrap survives as the wrap: start past end, on a circle.
        assert_eq!((p.sites[0].start, p.sites[0].end), (2677, 7));

        // A reverse-strand wrap keeps its complement wrapper.
        mol.primers[0].sites[0].strand = Strand::Reverse;
        let (rev, _) = write_reporting(&mol, "p.dna", (27, 6, 2026));
        assert!(rev.contains("complement(join(2677..2686,1..7))"), "{rev}");
    }

    #[test]
    fn an_ordinary_primer_binding_site_is_written_exactly_as_before() {
        // Control for the origin split: the common case must not have moved.
        let mut mol = Molecule {
            seq: b"a".repeat(2686),
            topology: Topology::Circular,
            ..Default::default()
        };
        mol.primers.push(Primer {
            name: "M13F".into(),
            seq: "GTAAAACGACGGCCAGT".into(),
            description: String::new(),
            sites: vec![
                BindingSite {
                    start: 100,
                    end: 116,
                    strand: Strand::Forward,
                    tm: Some(55.3),
                },
                BindingSite {
                    start: 900,
                    end: 916,
                    strand: Strand::Reverse,
                    tm: None,
                },
            ],
        });
        let (text, report) = write_reporting(&mol, "p.dna", (27, 6, 2026));
        assert!(report.is_empty(), "{report:?}");
        assert!(text.contains("primer_bind     100..116"), "{text}");
        assert!(
            text.contains("primer_bind     complement(900..916)"),
            "{text}"
        );
        assert!(text.contains("/note=\"primer GTAAAACGACGGCCAGT; Tm: 55.3 C\""));
    }

    #[test]
    fn a_primer_binding_site_past_the_end_is_reported_not_silently_skipped() {
        // The other limb of the same guard. A primer_bind at 5000..5100 on a
        // 2686 bp record would claim annealing to bases the file does not
        // contain, so it is still not written -- but the drop is now said out
        // loud instead of being a bare `continue` behind a bare `String`.
        let mut mol = Molecule {
            seq: b"a".repeat(2686),
            topology: Topology::Circular,
            ..Default::default()
        };
        mol.primers.push(Primer {
            name: "ghost".into(),
            seq: "ACGT".into(),
            description: String::new(),
            sites: vec![BindingSite {
                start: 5000,
                end: 5100,
                strand: Strand::Forward,
                tm: None,
            }],
        });
        let (text, report) = write_reporting(&mol, "p.dna", (27, 6, 2026));
        assert!(!text.contains("primer_bind"), "{text}");
        assert_eq!(report.absent.len(), 1, "{report:?}");
        assert!(report.absent[0].contains("ghost"), "{report:?}");
        assert!(report.absent[0].contains("past the end"), "{report:?}");
        assert!(report.reduced.is_empty(), "{report:?}");
    }

    /// A primer survives a GenBank round trip as a PRIMER, not as prose.
    ///
    /// PROVEN TO FAIL on 2026-09-03: `back.primers` was `[]` and the oligo
    /// existed only inside `back.features[0].qualifiers[1]`, as the text
    /// `"primer GTAAAACGACGGCCAGT; Tm: 55.3 C"`. A `.dna` opened, saved as
    /// GenBank and reopened lost its Primers tab, and `pl info` reported zero
    /// primers for a file that plainly had two.
    #[test]
    fn a_primer_round_trips_through_genbank_as_a_primer() {
        let mut mol = Molecule {
            seq: b"a".repeat(500),
            topology: Topology::Circular,
            ..Default::default()
        };
        mol.primers.push(Primer {
            name: "M13F".into(),
            seq: "GTAAAACGACGGCCAGT".into(),
            description: String::new(),
            sites: vec![
                BindingSite {
                    start: 100,
                    end: 116,
                    strand: Strand::Forward,
                    tm: Some(55.3),
                },
                BindingSite {
                    start: 300,
                    end: 316,
                    strand: Strand::Reverse,
                    tm: None,
                },
            ],
        });
        let (text, report) = write_reporting(&mol, "p.dna", (27, 6, 2026));
        assert!(report.is_empty(), "{report:?}");

        let back = parse(&text);
        assert_eq!(back.primers.len(), 1, "{:?}", back.primers);
        let p = &back.primers[0];
        assert_eq!(p.name, "M13F");
        assert_eq!(p.seq, "GTAAAACGACGGCCAGT");
        // TWO SITES, MERGED BACK ONTO ONE PRIMER. The writer spends one
        // feature per site; both carry the same label and oligo, so they are
        // one primer again.
        assert_eq!(p.sites.len(), 2, "{:?}", p.sites);
        assert_eq!((p.sites[0].start, p.sites[0].end), (100, 116));
        assert_eq!(p.sites[0].tm, Some(55.3));
        assert_eq!(p.sites[1].strand, Strand::Reverse);
        assert_eq!(p.sites[1].tm, None);
        // And it is not ALSO a feature.
        assert!(back.features.is_empty(), "{:?}", back.features);

        // THE FILE DOES NOT GROW. Writing the reparsed molecule gives the same
        // bytes; a promoted feature left in `features` would be emitted twice.
        let (again, _) = write_reporting(&back, "p.dna", (27, 6, 2026));
        assert_eq!(again, text, "a round trip changed the bytes");
    }

    /// The whole chain a user walks: a `.dna` opened, saved as GenBank, and
    /// opened again still has its primers.
    ///
    /// The other round-trip test starts from a `Molecule` in memory. This one
    /// starts from `.dna` BYTES, which is the thing a user actually double
    /// clicks, and goes through four pieces rather than two:
    /// `snapgene::from_molecule` -> `snapgene::parse` -> `genbank::write` ->
    /// `genbank::parse`. It exists because the only test that covered this
    /// chain on real files is corpus-gated (`tests/corpus.rs`, which skips
    /// without `PL_CORPUS`), so on any machine without a plasmid collection --
    /// every CI runner this project has -- nothing exercised it end to end.
    ///
    /// PROVEN TO FAIL on 2026-09-03, before `promote_primers`: the last step
    /// returned a molecule with `primers: []` and two `primer_bind` entries in
    /// `features`, so the Primers tab of a reopened `.gb` was empty and its
    /// oligo survived only as the text of a note.
    #[test]
    fn a_dna_saved_as_genbank_and_reopened_still_has_its_primers() {
        let mut mol = Molecule {
            seq: b"acgt".repeat(250),
            topology: Topology::Circular,
            ..Default::default()
        };
        mol.primers.push(Primer {
            name: "F_colony".into(),
            seq: "GTAAAACGACGGCCAGT".into(),
            description: String::new(),
            sites: vec![BindingSite {
                start: 40,
                end: 56,
                strand: Strand::Forward,
                tm: Some(58.1),
            }],
        });

        // .dna out, .dna back in -- the format the primer object is native to.
        let dna = crate::snapgene::from_molecule(&mol);
        let doc = crate::snapgene::parse(&dna).expect("the .dna we just wrote must parse");
        assert_eq!(
            doc.molecule.primers.len(),
            1,
            "the .dna leg lost the primer"
        );

        // GenBank out, GenBank back in -- the format that has no primer object.
        let (text, report) = write_reporting(&doc.molecule, "p.dna", (27, 6, 2026));
        assert!(report.is_empty(), "nothing should be lost here: {report:?}");
        let back = parse(&text);

        assert_eq!(back.primers.len(), 1, "{:?}", back.primers);
        assert_eq!(back.primers[0].name, "F_colony");
        assert_eq!(back.primers[0].seq, "GTAAAACGACGGCCAGT");
        assert_eq!(back.primers[0].sites.len(), 1);
        assert_eq!(
            (back.primers[0].sites[0].start, back.primers[0].sites[0].end),
            (40, 56)
        );
        assert_eq!(back.primers[0].sites[0].tm, Some(58.1));
        assert!(
            !back.features.iter().any(|f| f.kind == "primer_bind"),
            "the primer is a primer, not also a feature: {:?}",
            back.features
        );
    }

    /// Only the form this writer emits is promoted. Everything else stays a
    /// feature, because reinterpreting it would delete it from the file's own
    /// feature list on open.
    ///
    /// PROVEN TO FAIL against a predicate that matched on the `primer_bind`
    /// key alone, or on a note merely starting `primer `: each case below then
    /// disappeared from `features` and became a primer with a nonsense oligo.
    #[test]
    fn a_primer_bind_that_is_not_ours_stays_a_feature() {
        let head = "LOCUS       t   40 bp    DNA     circular UNA 27-JUN-2026\nFEATURES             Location/Qualifiers\n";
        let tail = "ORIGIN\n        1 acgtacgtac gtacgtacgt acgtacgtac gtacgtacgt\n//\n";
        // Each of these must survive as a feature, for the reason named.
        let cases: [(&str, &str); 6] = [
            (
                "a third qualifier means it came from somewhere else",
                "     primer_bind     1..10\n                     /label=\"x\"\n                     /note=\"primer ACGT\"\n                     /gene=\"y\"\n",
            ),
            (
                "prose, not an oligo: the space inside it is the tell",
                "     primer_bind     1..10\n                     /label=\"x\"\n                     /note=\"primer for colony PCR\"\n",
            ),
            (
                "not an oligo: Q is not an IUPAC nucleotide code",
                "     primer_bind     1..10\n                     /label=\"x\"\n                     /note=\"primer ACGQ\"\n",
            ),
            (
                "an empty oligo promotes to a primer that is not one",
                "     primer_bind     1..10\n                     /label=\"x\"\n                     /note=\"primer \"\n",
            ),
            (
                "the qualifiers are the right two in the wrong order",
                "     primer_bind     1..10\n                     /note=\"primer ACGT\"\n                     /label=\"x\"\n",
            ),
            (
                "a malformed Tm is not our writer's output",
                "     primer_bind     1..10\n                     /label=\"x\"\n                     /note=\"primer ACGT; Tm: warm C\"\n",
            ),
        ];
        for (why, feat) in cases {
            let mol = parse(&format!("{head}{feat}{tail}"));
            assert!(
                mol.primers.is_empty(),
                "promoted a primer_bind it should not have -- {why}: {:?}",
                mol.primers
            );
            assert_eq!(
                mol.features.len(),
                1,
                "the feature was lost -- {why}: {:?}",
                mol.features
            );
        }
    }

    /// A primer with no binding site vanishes from a GenBank file, and says so.
    ///
    /// PROVEN TO FAIL on 2026-09-03: `report` was `[]` and
    /// `text.contains("NoSites")` was `false`. The primer -- its name, its
    /// oligo and its description -- disappeared without a word, and because
    /// `faithful` is `report.is_empty()`, the editor then cleared the
    /// unsaved-changes dot and retargeted the document at the file that had
    /// just lost it. `.dna` block 5 can hold a `<Primer/>` with no
    /// `<BindingSite>`, so this is reachable from a real file rather than only
    /// from a constructed one.
    #[test]
    fn a_primer_with_no_binding_site_is_reported_rather_than_vanishing() {
        let mut mol = Molecule {
            seq: b"a".repeat(500),
            topology: Topology::Circular,
            ..Default::default()
        };
        mol.primers.push(Primer {
            name: "NoSites".into(),
            seq: "GTAAAACGACGGCCAGT".into(),
            description: String::new(),
            sites: Vec::new(),
        });
        let (text, report) = write_reporting(&mol, "p.dna", (27, 6, 2026));
        assert!(!text.contains("NoSites"), "{text}");
        assert_eq!(report.absent.len(), 1, "{report:?}");
        assert!(report.absent[0].contains("NoSites"), "{report:?}");
        assert!(report.absent[0].contains("no binding site"), "{report:?}");
        // The oligo is named too, because the report is the only place it
        // survives at all.
        assert!(report.absent[0].contains("GTAAAACGACGGCCAGT"), "{report:?}");
        // And not ALSO in `reduced`: the description is empty here, but even a
        // primer that had one would not be reported twice -- see
        // `a_primers_description_is_reported_as_a_reduction_not_as_a_refusal`.
        assert!(report.reduced.is_empty(), "{report:?}");
    }

    /// A primer's description is lost by GenBank, and the loss is REDUCED.
    ///
    /// PROVEN TO FAIL on 2026-09-04 by commenting out the `wrote_a_site &&
    /// !p.description.is_empty()` push: `report.reduced` was `[]`. A `.dna`
    /// carrying `<Primer description="anneals in the linker, use at 58 C"/>`
    /// converted to GenBank at exit 0 with an empty report, and the text a
    /// person had typed about what the primer was FOR existed nowhere in the
    /// output.
    ///
    /// The severity is the point of the test and not a detail of it. This
    /// exact line lived in `absent` for a few hours on 2026-09-03 and had to
    /// be reverted, because `crates/pl-wasm` refuses to write a file at all
    /// when `absent` is non-empty: the browser build stopped exporting any
    /// molecule whose primer merely carried a note. The third assertion below
    /// is what makes that impossible to reintroduce.
    #[test]
    fn a_primers_description_is_reported_as_a_reduction_not_as_a_refusal() {
        let mut mol = Molecule {
            seq: b"a".repeat(500),
            topology: Topology::Circular,
            ..Default::default()
        };
        mol.primers.push(Primer {
            name: "M13F".into(),
            seq: "GTAAAACGACGGCCAGT".into(),
            description: "anneals in the linker, use at 58 C".into(),
            sites: vec![BindingSite {
                start: 100,
                end: 116,
                strand: Strand::Forward,
                tm: Some(55.3),
            }],
        });
        let (text, report) = write_reporting(&mol, "p.dna", (27, 6, 2026));

        // The primer IS in the file: name, oligo and position all survive, and
        // it comes back out of the reader as a primer.
        let back = parse(&text);
        assert_eq!(back.primers.len(), 1, "{text}");
        assert_eq!(back.primers[0].name, "M13F");
        assert_eq!(back.primers[0].seq, "GTAAAACGACGGCCAGT");
        assert_eq!(back.primers[0].sites.len(), 1, "{text}");
        // And the description is not.
        assert_eq!(back.primers[0].description, "", "{text}");

        assert_eq!(report.reduced.len(), 1, "{report:?}");
        assert!(report.reduced[0].contains("M13F"), "{report:?}");
        assert!(
            report.reduced[0].contains("anneals in the linker"),
            "the text itself has to be named -- the report is now the only \
             place it exists: {report:?}"
        );
        assert!(
            report.absent.is_empty(),
            "a description is a reduction, and putting it here is what stopped \
             the browser build exporting: {report:?}"
        );

        // THE CONTROL, so this cannot become "report every primer": the same
        // primer with nothing in its description costs nothing at all.
        let mut quietmol = mol.clone();
        quietmol.primers[0].description = String::new();
        let (_, quiet) = write_reporting(&quietmol, "p.dna", (27, 6, 2026));
        assert!(quiet.is_empty(), "{quiet:?}");

        // AND THE OTHER CONTROL: a primer that never reached the file is
        // reported once, as absent, and its description is not a second
        // finding on top of it.
        let mut gone = mol.clone();
        gone.primers[0].sites.clear();
        let (_, report) = write_reporting(&gone, "p.dna", (27, 6, 2026));
        assert_eq!(report.absent.len(), 1, "{report:?}");
        assert!(
            report.reduced.is_empty(),
            "the whole primer is missing; the description is not a separate \
             finding: {report:?}"
        );
    }

    /// An EMPTY description costs nothing, however SnapGene spells "empty".
    ///
    /// PROVEN TO FAIL on 2026-09-04 by putting the shipped 0.13.3 predicate
    /// back -- `wrote_a_site && !p.description.is_empty()`, everything else
    /// unchanged: `report.reduced.len()` was 2, and the extra line read
    /// `primer "Untouched": its description "<html><body></body></html>" has
    /// no GenBank form`.
    ///
    /// Corpus-free on purpose, so it runs on every CI leg: the input is the
    /// SHAPE measured in block 5 of a real `.dna`, not the file itself.
    ///
    /// THE CONTROL IS IN THE SAME REPORT. One primer's description is the
    /// untouched box and must cost nothing; the other's is prose a person
    /// typed and must still cost a line. A "fix" that degenerated into never
    /// reporting a description passes the first assertion and fails the
    /// second, and the two cannot be separated by editing one of them out.
    #[test]
    fn a_description_with_no_text_in_it_is_not_reported_as_a_loss() {
        let site = || BindingSite {
            start: 100,
            end: 116,
            strand: Strand::Forward,
            tm: None,
        };
        let mut mol = Molecule {
            seq: b"a".repeat(500),
            topology: Topology::Circular,
            ..Default::default()
        };
        // SnapGene's untouched description box, as written on 4 of the 9
        // primers of the plasmid measured on 2026-09-04.
        mol.primers.push(Primer {
            name: "Untouched".into(),
            seq: "GTAAAACGACGGCCAGT".into(),
            description: "<html><body></body></html>".into(),
            sites: vec![site()],
        });
        // The control, from the same file: 3 of those 9 carry prose.
        mol.primers.push(Primer {
            name: "CAT-R".into(),
            seq: "GCAACTGACTGAAATGCCTC".into(),
            description: "Chloramphenicol resistance gene, reverse primer".into(),
            sites: vec![site()],
        });

        let (_, report) = write_reporting(&mol, "p.dna", (4, 8, 2026));
        assert!(report.absent.is_empty(), "{report:?}");
        assert_eq!(report.reduced.len(), 1, "{report:?}");
        assert!(
            report.reduced[0].contains("CAT-R"),
            "a description a person typed is still a reduction: {report:?}"
        );
        assert!(
            !report.reduced[0].contains("Untouched"),
            "an untouched rich-text box is not a loss: {report:?}"
        );

        // The other spellings of the same empty box, each on its own so a
        // failure names the one that leaked. `&nbsp;` is deliberate: it is an
        // HTML entity, `xml::unescape` does not expand it, and it arrives in
        // the model as six characters that are not whitespace.
        for empty in [
            "<html><body></body></html>",
            "<html><body><br></body></html>",
            "<html><body><p></p></body></html>",
            "<html><body>&nbsp;</body></html>",
            "<html><body>\n</body></html>",
            "   ",
            "",
        ] {
            let mut one = mol.clone();
            one.primers.truncate(1);
            one.primers[0].description = empty.to_string();
            let (_, r) = write_reporting(&one, "p.dna", (4, 8, 2026));
            assert!(r.is_empty(), "{empty:?} was reported as a loss: {r:?}");
        }

        // ...and the shapes that DO carry text: bare prose, prose wrapped in
        // the same markup, and a bare `<` that is prose rather than a tag.
        for text in [
            "Chloramphenicol resistance gene, reverse primer",
            "<html><body><p>anneals in the linker, use at 58 C</p></body></html>",
            "5' <- 3'",
        ] {
            let mut one = mol.clone();
            one.primers.truncate(1);
            one.primers[0].description = text.to_string();
            let (_, r) = write_reporting(&one, "p.dna", (4, 8, 2026));
            assert_eq!(r.reduced.len(), 1, "{text:?} went unreported: {r:?}");
        }
    }

    /// A description with no text in it does not become the DEFINITION line.
    ///
    /// PROVEN TO FAIL on 2026-09-04 with `is_empty` in place of `carries_text`
    /// at that call site: the record read `DEFINITION  <html><body></body></html>.`
    /// and `report` was completely empty -- `header_text` reports control
    /// characters and has nothing to say about markup.
    ///
    /// Two losses in one line, which is why the fallback and not just the
    /// report is at issue: the junk is IN the file, and it is there INSTEAD of
    /// the molecule's name, which is what this fallback exists to supply.
    #[test]
    fn a_description_with_no_text_in_it_does_not_displace_the_locus_name() {
        let mut mol = Molecule {
            seq: b"acgtacgtacgt".to_vec(),
            description: "<html><body></body></html>".into(),
            ..Default::default()
        };
        let (text, report) = write_reporting(&mol, "pTest.dna", (4, 8, 2026));
        assert!(
            text.contains("DEFINITION  pTest."),
            "the name is what a record with no description is called:\n{text}"
        );
        assert!(!text.contains("<html>"), "markup reached the file:\n{text}");
        assert!(report.is_empty(), "nothing was lost: {report:?}");

        // THE CONTROL: a description somebody wrote still wins over the name,
        // and is written exactly as the model holds it.
        mol.description = "Cloning vector pUC19c, complete sequence".into();
        let (text, report) = write_reporting(&mol, "pTest.dna", (4, 8, 2026));
        assert!(
            text.contains("DEFINITION  Cloning vector pUC19c, complete sequence."),
            "{text}"
        );
        assert!(report.is_empty(), "{report:?}");
    }

    #[test]
    fn a_wrap_with_no_origin_to_split_against_is_reported_not_written_as_an_illegal_range() {
        // `<Segment range="150-50"/>` on a 100 bp molecule is a shape the
        // SnapGene reader carries at face value on purpose. `span >= s.start`
        // is false, so the origin split was skipped and ` misc_feature 150..50`
        // went into the file -- not a legal GenBank base range. Read back,
        // `parse_location` rejects it and `flush` drops the whole feature: one
        // feature in, zero out, and on a circle `validate()` reports nothing
        // either side of the trip. That is precisely the silent loss the origin
        // split was written to end.
        let mut m = Molecule {
            seq: b"a".repeat(100),
            topology: Topology::Circular,
            ..Default::default()
        };
        let mut f = Feature::new("ghost", "misc_feature");
        f.segments.push(Segment::new(150, 50));
        m.features.push(f);

        let (text, report) = write_reporting(&m, "t", (27, 6, 2026));
        assert!(
            !text.contains("150..50"),
            "an illegal base range reached the file:\n{text}"
        );
        assert!(
            parse(&text).features.is_empty(),
            "one feature in, zero out -- and it must say so"
        );
        assert_eq!(report.absent.len(), 2, "{report:?}");
        assert!(
            report.absent.iter().any(|r| r.contains("150..50")),
            "{report:?}"
        );
        assert!(
            report.absent.iter().any(|r| r.contains("ghost")),
            "{report:?}"
        );

        // Control: the same wrap on a molecule long enough to split against is
        // still expanded, and reports nothing.
        let mut ok = Molecule {
            seq: b"a".repeat(200),
            topology: Topology::Circular,
            ..Default::default()
        };
        let mut f2 = Feature::new("real", "misc_feature");
        f2.segments.push(Segment::new(150, 50));
        ok.features.push(f2);
        let (t2, r2) = write_reporting(&ok, "t", (27, 6, 2026));
        assert!(t2.contains("join(150..200,1..50)"), "{t2}");
        assert!(r2.is_empty(), "{r2:?}");
    }

    #[test]
    fn a_mixed_strand_join_is_reported_rather_than_flattened_in_silence() {
        // `join(1..100,complement(500..600))` is legal GenBank and real
        // trans-spliced and organelle annotations use it. `Segment` carries no
        // strand and `Feature` carries exactly one, so both parts land on
        // whichever strand we pick: the 1..100 exon the file explicitly puts on
        // the plus strand is reassigned, the map arrow points at the wrong
        // template, and a save rewrites the location as
        // complement(join(1..100,500..600)) -- the file now says something it
        // never said. The naive `s.contains("complement(")` did all of that and
        // put nothing in the report the doc comment designates for it.
        let (segs, strand, bad) = parse_location("join(1..100,complement(500..600))");
        assert_eq!(segs.len(), 2, "the coordinates must not be lost: {segs:?}");
        assert_eq!(strand, Strand::Reverse);
        assert_eq!(
            bad.len(),
            1,
            "the reinterpretation went unreported: {bad:?}"
        );
        assert!(bad[0].contains("mixed-strand"), "{bad:?}");

        // ...and it reaches the caller through the channel every other
        // unrepresentable form uses.
        let src = [
            "LOCUS       test                      12 bp    DNA     linear   SYN 27-JUL-2026",
            "FEATURES             Location/Qualifiers",
            "     CDS             join(1..100,complement(500..600))",
            "ORIGIN",
            "//",
        ]
        .join("\n");
        let (mols, warnings) = parse_all_reporting(&src);
        assert_eq!(mols[0].features.len(), 1, "the feature itself still loads");
        assert!(
            warnings.iter().any(|w| w.contains("mixed-strand")),
            "{warnings:?}"
        );
    }

    #[test]
    fn a_join_that_names_only_one_strand_is_not_reported_as_mixed() {
        // Control: the all-reverse case the flattening rule was written for,
        // and its two neighbours, are unchanged and say nothing.
        for loc in [
            "join(complement(1..3),complement(7..9))",
            "complement(join(1..3,7..9))",
            "join(1..3,7..9)",
        ] {
            let (segs, _, bad) = parse_location(loc);
            assert_eq!(segs.len(), 2, "{loc}");
            assert!(bad.is_empty(), "{loc} reported {bad:?}");
        }
        assert_eq!(
            parse_location("join(complement(1..3),complement(7..9))").1,
            Strand::Reverse
        );
        assert_eq!(
            parse_location("complement(join(1..3,7..9))").1,
            Strand::Reverse
        );
        assert_eq!(parse_location("join(1..3,7..9)").1, Strand::Forward);
        // An unparsable part must not vote on strandedness either.
        let (_, strand, bad) = parse_location("join(1..3,complement(bond(5,10)))");
        assert_eq!(
            strand,
            Strand::Forward,
            "a form we cannot read is not a strand"
        );
        assert!(!bad.is_empty());
        assert!(!bad.iter().any(|b| b.contains("mixed-strand")), "{bad:?}");
    }

    #[test]
    fn an_unrepresentable_location_is_reported_not_invented() {
        // `bond(5,10)` split on the comma into `bond(5` and `10)`. The first
        // failed to parse and vanished; the second became a FABRICATED 10..10
        // segment that `validate()` was perfectly happy with. And
        // `order(bond(30,115),bond(64,80))` -- the form NCBI writes in GenPept
        // -- produced [115..115, 80..80]: two annotations pointing at nothing.
        let (segs, _, bad) = parse_location("bond(5,10)");
        assert!(segs.is_empty(), "nothing may be invented here: {segs:?}");
        assert_eq!(bad.len(), 2, "{bad:?}");

        let (segs, _, bad) = parse_location("order(bond(30,115),bond(64,80))");
        assert!(segs.is_empty(), "{segs:?}");
        assert_eq!(bad.len(), 4);

        // Forms we simply cannot express must be reported, not dropped.
        for loc in ["1^2", "J00194.1:200..300", "gap(unk100)"] {
            let (segs, _, bad) = parse_location(loc);
            assert!(segs.is_empty(), "{loc}: {segs:?}");
            assert!(!bad.is_empty(), "{loc} vanished silently");
        }

        // A join that mixes a representable part with an unrepresentable one
        // keeps the good half AND says the other was skipped -- otherwise the
        // feature quietly claims to be 100 bp when it is not.
        let (segs, _, bad) = parse_location("join(1..100,J00194.1:200..300)");
        assert_eq!(segs, vec![Segment::new(1, 100)]);
        assert_eq!(bad.len(), 1);
    }

    #[test]
    fn a_wrap_that_ends_at_base_zero_is_reported_rather_than_written_as_one_dot_zero() {
        // `<Segment range="5-0"/>` reaches this writer intact: `snapgene.rs`
        // takes a range at face value on purpose, and `pl convert` never calls
        // `Molecule::validate()`. `end < start` sent it down the origin-crossing
        // branch, which wrote the second part as `1..{end}` — `1..0`, a range
        // whose low bound exceeds its high one and which therefore names no
        // base. It went out at exit 0 with an empty report, and Biopython does
        // not reject it: it "fixes" it into a feature spanning the whole
        // molecule, so a coordinate naming nothing became an annotation over
        // everything.
        assert_eq!(location_parts(5, 0, 16), None);
        // The neighbours that must keep working, or the guard has eaten the
        // feature it was meant to protect.
        assert_eq!(
            location_parts(5, 2, 16),
            Some(vec!["5..16".to_string(), "1..2".to_string()]),
            "an ordinary wrap still splits at the origin"
        );
        assert_eq!(location_parts(5, 16, 16), Some(vec!["5..16".to_string()]));

        // End to end: it is refused AND named, not silently dropped.
        let m = Molecule {
            seq: b"ACGTACGTACGTACGT".to_vec(),
            topology: Topology::Circular,
            features: vec![Feature {
                name: "wrap".into(),
                kind: "misc_feature".into(),
                strand: Strand::Forward,
                segments: vec![Segment::new(5, 0)],
                qualifiers: Vec::new(),
            }],
            ..Default::default()
        };
        let (text, report) = write_reporting(&m, "w.gb", (1, 0, 2026));
        assert!(!text.contains("1..0"), "wrote an illegal range:\n{text}");
        assert!(
            report.absent.iter().any(|r| r.contains("5..0")),
            "the loss has to be named, or it is just a quieter loss: {report:?}"
        );
    }

    #[test]
    fn order_read_as_join_is_reported_because_the_file_stops_saying_what_it_said() {
        // INSDC `order` asserts the parts occur in this order and explicitly
        // does NOT assert they are joined; `join` asserts they are. `Feature`
        // carries no operator, so `order` is read as a join and `join_parts`
        // writes `join(...)` back — the same class of change the mixed-strand
        // branch two lines below already reports, and it was silent. X92946
        // carries `gene complement(order(14253..14810,14820..14824))` next to a
        // `/note="-1 translational frameshift"`, i.e. the submitter used `order`
        // precisely because the pieces are not spliced.
        let (segs, strand, bad) = parse_location("order(1..10,20..30)");
        assert_eq!(segs, vec![Segment::new(1, 10), Segment::new(20, 30)]);
        assert_eq!(strand, Strand::Forward);
        assert_eq!(bad.len(), 1, "{bad:?}");
        assert!(bad[0].contains("order()"), "{bad:?}");

        // The real one, complement-wrapped.
        let (_, strand, bad) = parse_location("complement(order(14253..14810,14820..14824))");
        assert_eq!(strand, Strand::Reverse);
        assert_eq!(bad.len(), 1, "{bad:?}");

        // A single-element `order` is written back as a bare range, which is the
        // same reinterpretation with the join spelling removed, so it counts.
        let (segs, _, bad) = parse_location("order(1..10)");
        assert_eq!(segs, vec![Segment::new(1, 10)]);
        assert_eq!(bad.len(), 1, "{bad:?}");

        // But an `order` whose every part was rejected on its own merits was not
        // re-expressed as anything, so it must NOT gain a line: the GenPept
        // form still reports exactly its four unrepresentable parts.
        let (segs, _, bad) = parse_location("order(bond(30,115),bond(64,80))");
        assert!(segs.is_empty());
        assert_eq!(bad.len(), 4, "{bad:?}");

        // And `join` itself is not a reinterpretation and stays quiet.
        let (_, _, bad) = parse_location("join(1..10,20..30)");
        assert!(bad.is_empty(), "{bad:?}");
    }

    #[test]
    fn ordinary_locations_still_parse_and_report_nothing() {
        for loc in [
            "1..10",
            "complement(5..8)",
            "join(1..3,7..9)",
            "<1..>10",
            "42",
            "complement(join(1..3,7..9))",
        ] {
            let (segs, _, bad) = parse_location(loc);
            assert!(!segs.is_empty(), "{loc} produced no segments");
            assert!(bad.is_empty(), "{loc} reported {bad:?}");
        }
    }

    #[test]
    fn the_reader_surfaces_skipped_locations_through_load() {
        let src = [
            "LOCUS       test                      12 bp    DNA     linear   SYN 27-JUL-2026",
            "FEATURES             Location/Qualifiers",
            "     misc_feature    bond(5,10)",
            "     CDS             1..12",
            "ORIGIN",
            "        1 acgtacgtacgt",
            "//",
        ]
        .join(
            "
",
        );
        let (mols, warnings) = parse_all_reporting(&src);
        assert_eq!(mols.len(), 1);
        // The good feature survives; the impossible one is named, not invented.
        assert_eq!(mols[0].features.len(), 1);
        assert_eq!(mols[0].features[0].kind, "CDS");
        assert!(
            !warnings.is_empty(),
            "the skipped location was not reported"
        );
        assert!(warnings[0].contains("misc_feature"), "{warnings:?}");
    }

    #[test]
    fn location_forms_all_parse() {
        assert_eq!(parse_location("1..10").0, vec![Segment::new(1, 10)]);
        assert_eq!(parse_location("complement(5..8)").1, Strand::Reverse);
        let (segs, strand, _) = parse_location("join(1..3,7..9)");
        assert_eq!(segs.len(), 2);
        assert_eq!(strand, Strand::Forward);
        // fuzzy boundaries
        assert_eq!(parse_location("<1..>10").0, vec![Segment::new(1, 10)]);
        // single base
        assert_eq!(parse_location("42").0, vec![Segment::new(42, 42)]);
        // complement inside a join still flips the feature
        assert_eq!(
            parse_location("join(complement(1..3),complement(7..9))").1,
            Strand::Reverse
        );
    }

    #[test]
    fn round_trip_preserves_the_things_that_matter() {
        let mut mol = Molecule {
            name: "test".into(),
            seq: b"ACGTacgtNN".repeat(10),
            topology: Topology::Circular,
            ..Default::default()
        };
        let mut f = Feature::new("AmpR", "CDS");
        f.strand = Strand::Reverse;
        let mut s = Segment::new(5, 20);
        s.color = Some("#9a5b8c".into());
        f.segments.push(s);
        f.set_qualifier("gene", "bla");
        mol.features.push(f);

        let gb = write(&mol, "test.dna", (26, 6, 2026));
        let back = parse(&gb);

        assert_eq!(back.seq, mol.seq, "sequence and its case survive");
        assert_eq!(back.topology, Topology::Circular);
        assert_eq!(
            back.features.len(),
            1,
            "the source feature must not be counted"
        );
        let g = &back.features[0];
        assert_eq!(g.name, "AmpR");
        assert_eq!(g.strand, Strand::Reverse);
        assert_eq!((g.start(), g.end()), (5, 20));
        assert_eq!(g.color(), Some("#9a5b8c"));
        assert_eq!(g.qualifier("gene"), Some("bla"));
    }

    #[test]
    fn repeated_exports_do_not_accumulate_colour_notes() {
        // `write` generates /note="color: #rrggbb" and the reader stores every
        // qualifier verbatim, so the stored copy was emitted again next time
        // alongside a fresh one: one colour note after the first export, two
        // after the second, five after five, in the file and in
        // `Feature::qualifiers` alike. `ApEinfo_fwdcolor` and
        // `ApEinfo_revcolor` were already skipped for exactly this hazard;
        // `note` was simply missed. Nothing corrupted -- the reader prefers the
        // ApEinfo pair, so the colour never drifted -- the file just grew for
        // ever on an operation the user believes is idempotent.
        let mut mol = Molecule {
            seq: b"acgtacgtacgt".to_vec(),
            ..Default::default()
        };
        let mut f = Feature::new("AmpR", "CDS");
        let mut s = Segment::new(1, 12);
        s.color = Some("#9a5b8c".into());
        f.segments.push(s);
        mol.features.push(f);

        let mut cur = mol;
        for cycle in 1..=5 {
            let text = write(&cur, "t", (27, 6, 2026));
            let emitted = text
                .lines()
                .filter(|l| l.trim() == "/note=\"color: #9a5b8c\"")
                .count();
            assert_eq!(
                emitted, 1,
                "cycle {cycle} wrote {emitted} colour notes:\n{text}"
            );
            cur = parse(&text);
            let stored = cur.features[0]
                .qualifiers
                .iter()
                .filter(|(k, v)| k == "note" && v.as_deref() == Some("color: #9a5b8c"))
                .count();
            assert_eq!(stored, 1, "cycle {cycle} stored {stored} colour notes");
            assert_eq!(
                cur.features[0].color(),
                Some("#9a5b8c"),
                "the colour itself must survive every cycle"
            );
        }
    }

    #[test]
    fn a_note_that_says_more_than_a_colour_is_not_mistaken_for_a_generated_one() {
        // Control for the de-duplication: only a note that is *nothing but* the
        // colour line this writer generates is dropped. Prose somebody typed
        // that happens to start with a colour is content.
        let mut mol = Molecule {
            seq: b"acgtacgtacgt".to_vec(),
            ..Default::default()
        };
        let mut f = Feature::new("AmpR", "CDS");
        let mut s = Segment::new(1, 12);
        s.color = Some("#9a5b8c".into());
        f.segments.push(s);
        f.set_qualifier("note", "color: #9a5b8c chosen by hand");
        f.set_qualifier("note", "beta-lactamase");
        mol.features.push(f);

        let text = write(&mol, "t", (27, 6, 2026));
        assert!(text.contains("chosen by hand"), "{text}");
        assert!(text.contains("beta-lactamase"), "{text}");
        let back = parse(&text);
        assert!(back.features[0]
            .qualifiers
            .iter()
            .any(|(k, v)| k == "note" && v.as_deref() == Some("color: #9a5b8c chosen by hand")));
        assert!(back.features[0]
            .qualifiers
            .iter()
            .any(|(k, v)| k == "note" && v.as_deref() == Some("beta-lactamase")));
    }

    #[test]
    fn a_wrapped_definition_keeps_its_continuation_lines() {
        // GenBank wraps DEFINITION near column 79, so this is the ordinary NCBI
        // record rather than an exotic one. Reading only the first physical
        // line dropped the last word -- and `write` then put a full stop after
        // the stump, so the truncation read as a finished sentence. It is also
        // the string `pl-scan` indexes, so the missing words were unfindable in
        // the library.
        let gb =
            "LOCUS       NC_000913            4641652 bp    DNA     circular BCT 30-MAR-2010\n\
                  DEFINITION  Escherichia coli str. K-12 substr. MG1655, complete\n\
                  \x20           genome.\n\
                  ACCESSION   NC_000913\n\
                  ORIGIN\n//\n";
        let m = parse(gb);
        assert_eq!(
            m.description,
            "Escherichia coli str. K-12 substr. MG1655, complete genome"
        );
        let out = write(&m, "t", (1, 0, 2026));
        assert!(
            out.contains("complete genome."),
            "the round trip lost a word:\n{out}"
        );
    }

    #[test]
    fn a_definition_stops_at_the_next_keyword() {
        // Control: continuation lines are the ones with columns 1-10 blank, so
        // nothing after the DEFINITION block may be swallowed into it.
        let gb = "LOCUS       x                        4 bp    DNA     linear   SYN 01-JAN-2026\n\
                  DEFINITION  a short one.\n\
                  ACCESSION   x\n\
                  KEYWORDS    .\n\
                  ORIGIN\n\
                  \x20       1 acgt\n//\n";
        assert_eq!(parse(gb).description, "a short one");

        // ...including a FEATURES table, whose qualifier lines *are* indented
        // past column 10 and would otherwise be read as continuations.
        let straight_into_features =
            "LOCUS       x                        4 bp    DNA     linear   SYN 01-JAN-2026\n\
             DEFINITION  a short one.\n\
             FEATURES             Location/Qualifiers\n\
             \x20    gene            1..4\n\
             \x20                    /label=\"g\"\n\
             ORIGIN\n\
             \x20       1 acgt\n//\n";
        let m = parse(straight_into_features);
        assert_eq!(m.description, "a short one");
        assert_eq!(m.features.len(), 1);
    }

    #[test]
    fn a_multibyte_base_straddling_a_group_boundary_is_not_split_into_two_replacements() {
        // The ORIGIN writer chunked the raw bytes into 10-byte groups and
        // decoded each group on its own, so no decode ever saw a character
        // crossing a boundary whole: `acgtacgta` + µ (U+00B5 = C2 B5) +
        // `cgtacgtac` is 20 bytes with C2 at index 9, and the two halves became
        // two separate U+FFFD. One character in, two mojibake characters out,
        // and a sequence that re-parsed to 24 bytes instead of 20.
        let mut seq = b"acgtacgta".to_vec();
        seq.extend_from_slice("µ".as_bytes());
        seq.extend_from_slice(b"cgtacgtac");
        assert_eq!(seq.len(), 20, "nine bases, a two-byte char, nine bases");

        let mol = Molecule {
            seq: seq.clone(),
            ..Default::default()
        };
        let text = write(&mol, "t", (1, 0, 2026));
        assert!(
            !text.contains('\u{FFFD}'),
            "the character was split across two lossy decodes:\n{text}"
        );
        assert_eq!(parse(&text).seq, seq, "the exported sequence changed");

        // The 60-base line boundary is the same boundary.
        let mut long = b"a".repeat(59);
        long.extend_from_slice("µ".as_bytes());
        long.extend_from_slice(&b"c".repeat(40));
        let m2 = Molecule {
            seq: long.clone(),
            ..Default::default()
        };
        let t2 = write(&m2, "t", (1, 0, 2026));
        assert!(!t2.contains('\u{FFFD}'), "{t2}");
        assert_eq!(parse(&t2).seq, long);
    }

    #[test]
    fn annotation_only_file_reports_no_bases_but_keeps_the_declared_length() {
        // ORIGIN immediately followed by // -- real, and common for genomes.
        let gb =
            "LOCUS       NC_003210            2944528 bp    DNA     circular BCT 30-MAR-2010\n\
                  FEATURES             Location/Qualifiers\n\
                  \x20    gene            1..100\n\
                  \x20                    /gene=\"dnaA\"\n\
                  ORIGIN      \n//\n";
        let m = parse(gb);
        assert!(
            m.seq.is_empty(),
            "there are genuinely no bases in this file"
        );
        assert_eq!(m.declared_len, Some(2_944_528));
        assert_eq!(m.span(), 2_944_528);
        assert!(m.sequence_absent());
        assert_eq!(m.features.len(), 1);
    }

    #[test]
    fn multi_line_locations_and_translations_are_reassembled() {
        let gb = "LOCUS       x                        30 bp    DNA     linear   SYN 01-JAN-2026\n\
                  FEATURES             Location/Qualifiers\n\
                  \x20    CDS             join(1..6,\n\
                  \x20                    13..18)\n\
                  \x20                    /translation=\"MAD\n\
                  \x20                    EIT\"\n\
                  ORIGIN\n\
                  \x20       1 acgtacgtac gtacgtacgt acgtacgtac\n//\n";
        let m = parse(gb);
        assert_eq!(m.features.len(), 1);
        assert_eq!(
            m.features[0].segments.len(),
            2,
            "wrapped location must rejoin"
        );
        assert_eq!(m.features[0].qualifier("translation"), Some("MADEIT"));
    }

    #[test]
    fn multibyte_text_never_panics_on_truncation() {
        // Both of these used to slice a str by byte offset. A feature key or a
        // colour note containing a multibyte character would panic, which in
        // wasm takes down the whole module.
        let mut mol = Molecule {
            seq: b"acgt".to_vec(),
            ..Default::default()
        };
        let mut f = Feature::new("δ subunit", "δδδδδδδδδδδδδδδδδδδδ");
        f.segments.push(Segment::new(1, 4));
        f.set_qualifier("note", "color: #δβγ");
        mol.features.push(f);

        let gb = write(&mol, "t.dna", (1, 0, 2026)); // must not panic
        let back = parse(&gb);
        assert_eq!(back.features.len(), 1);
        assert_eq!(back.features[0].name, "δ subunit");
        // "#δβγ" is not six hex digits, so no colour is claimed.
        assert_eq!(back.features[0].color(), None);
    }

    #[test]
    fn colour_notes_are_only_believed_when_they_are_real_hex() {
        let gb = "LOCUS       x                         4 bp    DNA     linear   SYN 01-JAN-2026\n\
                  FEATURES             Location/Qualifiers\n\
                  \x20    gene            1..4\n\
                  \x20                    /note=\"color: #1a2b3c\"\n\
                  ORIGIN\n\
                  \x20       1 acgt\n//\n";
        assert_eq!(parse(gb).features[0].color(), Some("#1a2b3c"));

        let short = gb.replace("#1a2b3c", "#12");
        assert_eq!(parse(&short).features[0].color(), None);
    }

    #[test]
    fn standalone_annotation_tracks_are_read() {
        // UGENE and SnapGene both export these: features only, no ORIGIN block,
        // and no bp field on the LOCUS line. Real files, and Biopython refuses
        // them outright.
        let gb =
            "LOCUS       Annotations                                             19-MAR-2018\n\
                  UNIMARK     Annotations\n\
                  FEATURES             Location/Qualifiers\n\
                  \x20    CDS             242..1015\n\
                  \x20                    /ugene_name=\"hypothetical protein\"\n\
                  \x20    CDS             complement(1118..1951)\n\
                  \x20                    /ugene_name=\"PknD\"\n//\n";
        let m = parse(gb);
        assert_eq!(m.features.len(), 2);
        assert!(m.seq.is_empty());
        assert_eq!(m.declared_len, None);
        assert!(m.is_annotation_track());
        assert!(
            !m.sequence_absent(),
            "nothing was declared, so nothing is missing"
        );
        // No length anywhere, so the span has to be inferred for display.
        assert_eq!(m.span(), 0);
        assert_eq!(m.annotation_span(), 1951);
        assert_eq!(m.features[1].strand, Strand::Reverse);
    }

    #[test]
    fn strandedness_is_unknown_unless_the_file_says() {
        let base = "LOCUS       x                        4 bp    {} linear   SYN 01-JAN-2026\nORIGIN\n        1 acgt\n//\n";
        assert_eq!(
            parse(&base.replace("{}", "DNA    ")).double_stranded,
            None,
            "a plain DNA record records nothing, so we must not claim single-stranded"
        );
        assert_eq!(
            parse(&base.replace("{}", "ds-DNA ")).double_stranded,
            Some(true)
        );
        assert_eq!(
            parse(&base.replace("{}", "ss-DNA ")).double_stranded,
            Some(false)
        );
    }

    #[test]
    fn features_are_left_in_file_order() {
        let gb = "LOCUS       x                        99 bp    DNA     linear   SYN 01-JAN-2026\n\
                  FEATURES             Location/Qualifiers\n\
                  \x20    gene            50..60\n\
                  \x20                    /label=\"later\"\n\
                  \x20    gene            10..20\n\
                  \x20                    /label=\"earlier\"\n\
                  ORIGIN\n//\n";
        let m = parse(gb);
        assert_eq!(m.features[0].name, "later", "a reader reports file order");
        assert_eq!(m.features[1].name, "earlier");
    }

    #[test]
    fn multi_record_files_yield_every_record() {
        let one = "LOCUS       a                         4 bp    DNA     linear   SYN 01-JAN-2026\nORIGIN\n        1 acgt\n//\n";
        let two = "LOCUS       b                         8 bp    DNA     circular SYN 01-JAN-2026\nORIGIN\n        1 acgtacgt\n//\n";
        let all = parse_all(&format!("{one}{two}"));
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].seq, b"acgt".to_vec());
        assert_eq!(all[1].topology, Topology::Circular);
    }

    #[test]
    fn a_missing_record_terminator_does_not_turn_the_next_header_into_bases() {
        // `//` was the only thing that ended a chunk, and `parse_record`'s
        // ORIGIN loop had no stop condition — its `line.starts_with("//")` guard
        // could never fire, because the chunker uses the same predicate and
        // never pushes a terminator through. So a file whose internal `//` is
        // missing came back as ONE molecule holding the next record's header as
        // sequence, with `records == 1` so nothing was reported as truncated.
        let nosep = "\
LOCUS       recA                      12 bp    DNA     linear   SYN 01-JAN-2026
ORIGIN
        1 acgtacgtacgt
LOCUS       lacZ                      12 bp    DNA     circular SYN 01-JAN-2026
ORIGIN
        1 ttttggggcccc
//
";
        let all = parse_all(nosep);
        assert_eq!(all.len(), 2, "the second record was swallowed: {all:?}");
        assert_eq!(
            all[0].seq,
            b"acgtacgtacgt".to_vec(),
            "fabricated bases: {:?}",
            String::from_utf8_lossy(&all[0].seq)
        );
        assert_eq!(all[0].name, "recA");
        assert_eq!(all[1].seq, b"ttttggggcccc".to_vec());
        assert_eq!(all[1].name, "lacZ");
        assert_eq!(all[1].topology, Topology::Circular);

        // The ordinary case is untouched: one LOCUS per chunk, `//` ends it,
        // and nothing splits a record whose indented lines merely mention the
        // word — a wrapped DEFINITION, a FEATURES row, an ORIGIN row.
        let ok = "\
LOCUS       a                          4 bp    DNA     linear   SYN 01-JAN-2026
DEFINITION  the word
            LOCUS appears indented here
FEATURES             Location/Qualifiers
     misc_feature    1..4
ORIGIN
        1 acgt
//
";
        let all = parse_all(ok);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].seq, b"acgt".to_vec());
        assert_eq!(all[0].features.len(), 1);
    }

    #[test]
    fn a_record_with_no_bases_writes_no_source_feature() {
        // `source 1..{n}` was hard-coded and is the one location in the writer
        // that does not go through `location_parts`. On a molecule with no bases
        // that is `source 1..0`, which is not a legal INSDC range —
        // `location_parts(1, 0, 0)` returns `None` for that exact shape — and it
        // went out at exit 0 with an empty report. Our own reader cannot catch
        // it either: `flush` drops `source` before `parse_location` ever runs.
        let track = Molecule {
            features: vec![Feature {
                name: "CDS".into(),
                kind: "CDS".into(),
                strand: Strand::Forward,
                segments: vec![Segment::new(242, 1015)],
                qualifiers: Vec::new(),
            }],
            ..Default::default()
        };
        assert_eq!(track.span(), 0);
        let (text, report) = write_reporting(&track, "track.gb", (1, 0, 2026));
        assert!(
            !text.contains("1..0"),
            "wrote an illegal base range:\n{text}"
        );
        assert!(
            !text.contains("     source "),
            "a record with no bases has no source to describe:\n{text}"
        );
        assert!(report.is_empty(), "got {report:?}");

        // Every molecule that has any extent still gets its source line, and it
        // still covers the whole molecule — including an annotation track whose
        // length is declared rather than carried.
        let declared = Molecule {
            declared_len: Some(3000),
            ..Default::default()
        };
        assert!(write(&declared, "x.gb", (1, 0, 2026)).contains("     source          1..3000\n"));
        let bases = Molecule {
            seq: b"acgt".to_vec(),
            ..Default::default()
        };
        assert!(write(&bases, "x.gb", (1, 0, 2026)).contains("     source          1..4\n"));
    }

    #[test]
    fn locus_names_are_made_safe() {
        assert_eq!(locus_name("my plasmid v2.dna"), "my_plasmid_v2");
        assert_eq!(locus_name("....dna"), "sequence");
        assert_eq!(locus_name("___.dna"), "sequence");
        // Sixteen characters, so the LOCUS line's fixed columns still line up.
        assert_eq!(locus_name(&"x".repeat(40)).len(), 16);
        assert_eq!(
            locus_name("pACYC184-Ppho-fab2-6his.dna"),
            "pACYC184-Ppho-fa"
        );
    }

    #[test]
    fn locus_line_keeps_its_columns_even_with_a_long_name() {
        let mol = Molecule {
            seq: b"acgt".to_vec(),
            ..Default::default()
        };
        let line = write(&mol, &format!("{}.dna", "x".repeat(40)), (1, 0, 2026));
        let first = line.lines().next().unwrap();
        // "LOCUS" + 7 spaces = 12 chars, then 16 for the name.
        assert!(first.starts_with("LOCUS       "));
        assert_eq!(&first[12..28], &"x".repeat(16));
        assert!(
            first[28..].starts_with(" "),
            "bp field must follow the name column"
        );
    }

    #[test]
    fn origin_block_is_sixty_bases_in_ten_base_groups() {
        let mol = Molecule {
            seq: b"a".repeat(65),
            ..Default::default()
        };
        let gb = write(&mol, "t", (1, 0, 2026));
        let lines: Vec<&str> = gb
            .lines()
            .skip_while(|l| !l.starts_with("ORIGIN"))
            .collect();
        assert!(lines[1].starts_with("        1 "));
        assert_eq!(lines[1].split_whitespace().count(), 7); // index + 6 groups
        assert!(lines[2].starts_with("       61 "));
    }

    /// A `.dna` note reaches DEFINITION and COMMENT verbatim, and GenBank reads
    /// column 1 as a keyword.
    ///
    /// PROVEN TO FAIL against the unfixed writer, which interpolated
    /// `mol.description` and `note("UUID")` with `format!`:
    ///
    /// ```text
    /// ---- genbank::tests::a_control_character_in_a_header_field_cannot_forge_a_keyword_line stdout ----
    /// assertion `left == right` failed: the header was read as bases:
    ///   left: [65, 67, 67, 69, 83, 83, 73, 79, 78, 46, 86, 69, ...]
    ///  right: [97, 99, 103, 116, 97, 99, 103, 116, ...]
    /// ```
    ///
    /// `left` is `ACCESSION.VERSION...`: the forged `ORIGIN` sat above those
    /// keywords, so the whole header below it came back as bases.
    #[test]
    fn a_control_character_in_a_header_field_cannot_forge_a_keyword_line() {
        // Not a hand-built molecule: `snapgene::parse` assigns
        // `molecule.description` straight from the `<Description>` note and
        // `parse_notes` keeps interior control characters, so an ordinary
        // lab description typed over three lines arrives here intact. The
        // lines it lands on are column-significant — `parse_record` takes the
        // FIRST line starting `ORIGIN` and its base loop has no stop
        // condition, so everything below the forged keyword became bases.
        let mut mol = Molecule {
            description: "Constitutive mNeonGreen vector.\nORIGIN of replication swapped for p15A"
                .into(),
            seq: b"acgtacgtacgtacgtacgt".to_vec(),
            topology: Topology::Circular,
            ..Default::default()
        };
        mol.notes.push(pl_core::Note::new("UUID", "abc\nORIGIN"));

        let (text, report) = write_reporting(&mol, "vector.gb", (1, 0, 2026));
        let back = parse_all(&text);
        assert_eq!(
            back.len(),
            1,
            "the export forged a record boundary:\n{text}"
        );
        assert_eq!(
            back[0].seq, mol.seq,
            "the header was read as bases:\n{text}"
        );
        assert_eq!(
            back[0].description,
            "Constitutive mNeonGreen vector. ORIGIN of replication swapped for p15A",
            "the description did not survive:\n{text}"
        );
        assert_eq!(
            report.reduced.len(),
            2,
            "the writer's own report channel said nothing: {report:?}"
        );
        assert!(
            report.reduced.iter().any(|r| r.contains("DEFINITION")),
            "{report:?}"
        );
        assert!(
            report.reduced.iter().any(|r| r.contains("UUID")),
            "{report:?}"
        );
        // Both fields are IN the file, carrying their text with one character
        // normalised, so neither is `absent` -- and the browser build, which
        // refuses on `absent` alone, still exports this molecule.
        assert!(report.absent.is_empty(), "{report:?}");

        // The ordinary case still reports nothing at all.
        let plain = Molecule {
            description: "Cloning vector pUC19c, complete sequence".into(),
            seq: b"acgtacgtacgtacgtacgt".to_vec(),
            ..Default::default()
        };
        let (_, quiet) = write_reporting(&plain, "p.gb", (1, 0, 2026));
        assert!(quiet.is_empty(), "{quiet:?}");
    }

    /// PROVEN TO FAIL against the unfixed writer, which wrote `f.kind` raw.
    /// The record count survives — the forged keyword is `ORIGIN`, not
    /// `LOCUS` — so what fails is the sequence:
    ///
    /// ```text
    /// ---- genbank::tests::a_feature_key_that_is_not_a_key_does_not_break_the_table stdout ----
    /// assertion `left == right` failed: the feature table became bases:
    /// ...
    ///      source          1..12
    ///                      /organism="synthetic DNA construct"
    ///                      /mol_type="other DNA"
    ///      CDS
    /// ORIGIN      1..12
    ///                      /label="gene of interest"
    /// ORIGIN
    ///         1 acgtacgtac gt
    /// //
    ///
    ///   left: [47, 108, 97, 98, 101, 108, 61, 34, ...]
    ///  right: [97, 99, 103, 116, 97, 99, 103, 116, ...]
    /// ```
    ///
    /// `left` is `/label="gene of interest"ORIGINacgt...`: the feature's own
    /// qualifier line became part of the molecule.
    #[test]
    fn a_feature_key_that_is_not_a_key_does_not_break_the_table() {
        // `f.kind` is the `.dna`'s `type=` attribute, which is file-controlled
        // and never validated on the way in, so `type="CDS&#10;ORIGIN"` used to
        // emit `     CDS` followed by `ORIGIN      1..12` at column 1.
        let mut f = Feature::new("gene of interest", "CDS\nORIGIN");
        f.segments.push(Segment::new(1, 12));
        let mol = Molecule {
            seq: b"acgtacgtacgt".to_vec(),
            features: vec![f],
            ..Default::default()
        };
        let (text, report) = write_reporting(&mol, "x.gb", (1, 0, 2026));
        let back = parse_all(&text);
        assert_eq!(
            back.len(),
            1,
            "the export forged a record boundary:\n{text}"
        );
        assert_eq!(
            back[0].seq, mol.seq,
            "the feature table became bases:\n{text}"
        );
        assert_eq!(back[0].features.len(), 1, "the feature was lost:\n{text}");
        assert_eq!(
            back[0].features[0].segments,
            vec![Segment::new(1, 12)],
            "{text}"
        );
        assert_eq!(back[0].features[0].name, "gene of interest");
        assert!(
            report.reduced.iter().any(|r| r.contains("CDS")),
            "the key was changed in silence: {report:?}"
        );
        assert!(
            report.absent.is_empty(),
            "the feature is in the file: {report:?}"
        );

        // A real key with a character the format allows is untouched, and
        // reports nothing.
        let mut ok = Feature::new("utr", "3'UTR");
        ok.segments.push(Segment::new(1, 12));
        let m2 = Molecule {
            seq: b"acgtacgtacgt".to_vec(),
            features: vec![ok],
            ..Default::default()
        };
        let (t2, quiet) = write_reporting(&m2, "x.gb", (1, 0, 2026));
        assert!(t2.contains("     3'UTR"), "{t2}");
        assert!(quiet.is_empty(), "{quiet:?}");
    }

    /// A feature key too long for its column is cut, and now says so.
    ///
    /// PROVEN TO FAIL on 2026-09-04 by removing the `cut.chars().count() <
    /// kind.chars().count()` push: `report` was empty and `back.kind` was
    /// `"misc_recombinat"` -- fifteen characters of it -- against the
    /// twenty-three that went in. The test above catches a key with an ILLEGAL
    /// character because `feature_key` compares its cleaned string against
    /// `cut`, which is already truncated; a key whose only fault is length
    /// passed that comparison and was rewritten in silence.
    ///
    /// `bins/pl-gui/src/main.rs` recorded this hole in `plan_genbank`'s doc
    /// and left it open on purpose: the only report channel then in existence
    /// was the one `crates/pl-wasm` refuses on, and refusing to export a
    /// plasmid because one feature key is sixteen characters long is not a
    /// trade worth making. `reduced` is the channel that makes it reportable
    /// -- the feature is in the file, under a shorter key.
    #[test]
    fn a_feature_key_longer_than_its_column_is_reported_rather_than_cut_in_silence() {
        let mut f = Feature::new("gene of interest", "misc_recombination_site");
        f.segments.push(Segment::new(1, 12));
        let mol = Molecule {
            seq: b"acgtacgtacgt".to_vec(),
            features: vec![f],
            ..Default::default()
        };
        let (text, report) = write_reporting(&mol, "x.gb", (1, 0, 2026));

        // The feature is in the file, under fifteen characters of its key.
        let back = parse(&text);
        assert_eq!(back.features.len(), 1, "{text}");
        assert_eq!(back.features[0].kind, "misc_recombinat", "{text}");
        assert_eq!(back.features[0].name, "gene of interest");

        assert_eq!(report.reduced.len(), 1, "{report:?}");
        assert!(
            report.reduced[0].contains("misc_recombination_site"),
            "the key that went in has to be named: {report:?}"
        );
        assert!(
            report.absent.is_empty(),
            "the feature is in the file: {report:?}"
        );

        // THE CONTROL: the longest key INSDC actually defines is fifteen
        // characters, so no real file pays for this. `misc_difference` fits
        // its column exactly and costs nothing.
        let mut ok = Feature::new("d", "misc_difference");
        ok.segments.push(Segment::new(1, 12));
        let m2 = Molecule {
            seq: b"acgtacgtacgt".to_vec(),
            features: vec![ok],
            ..Default::default()
        };
        let (t2, quiet) = write_reporting(&m2, "x.gb", (1, 0, 2026));
        assert!(t2.contains("     misc_difference "), "{t2}");
        assert!(quiet.is_empty(), "{quiet:?}");
    }

    /// PROVEN TO FAIL against the unfixed ORIGIN writer:
    ///
    /// ```text
    /// ---- genbank::tests::sequence_bytes_the_origin_block_cannot_carry_are_not_written_raw stdout ----
    /// assertion `left == right` failed: a second record was forged:
    /// ...
    /// ORIGIN
    ///         1 acgtacgtac
    /// LOCUS        EVIL                          6 bp    DN
    ///        61 A     line ar   SYN 0 1-JAN-2026
    /// ggggggggg gggggggggg g
    /// //
    ///
    ///   left: 2
    ///  right: 1
    /// ```
    ///
    /// The base index kept counting through the break, which is why `61`
    /// appears in the middle of the injected line.
    #[test]
    fn sequence_bytes_the_origin_block_cannot_carry_are_not_written_raw() {
        // `snapgene.rs` assigns block 0's payload to `Molecule::seq` verbatim,
        // so a corrupt container puts a line break in the middle of the bases.
        // Written raw it starts a new line at column 1, where `LOCUS` opens a
        // second record: a 129 bp molecule exported to a file whose first
        // record holds 10 bases, at exit 0 with an empty report.
        let mut seq = b"acgtacgtac".to_vec();
        seq.extend_from_slice(
            b"\nLOCUS       EVIL                       6 bp    DNA     linear   SYN 01-JAN-2026\n",
        );
        seq.extend_from_slice(&b"g".repeat(20));
        let mol = Molecule {
            seq: seq.clone(),
            ..Default::default()
        };

        let (text, report) = write_reporting(&mol, "x.gb", (1, 0, 2026));
        let back = parse_all(&text);
        assert_eq!(back.len(), 1, "a second record was forged:\n{text}");
        // Whitespace and digits are what the reader's own ORIGIN filter drops,
        // so they are the bytes that cannot come back; each is written as `n`,
        // which keeps every coordinate in the file pointing at the base it did.
        let expected: Vec<u8> = seq
            .iter()
            .map(|b| {
                if b.is_ascii_whitespace() || b.is_ascii_digit() {
                    b'n'
                } else {
                    *b
                }
            })
            .collect();
        assert_eq!(back[0].seq, expected, "the sequence changed:\n{text}");
        assert_eq!(
            back[0].seq.len(),
            seq.len(),
            "the length no longer matches the LOCUS line"
        );
        assert!(
            report.reduced.iter().any(|r| r.contains("ORIGIN")),
            "the substitution went unreported: {report:?}"
        );

        // An ordinary sequence is written exactly as before and says nothing.
        let plain = Molecule {
            seq: b"acgtacgtacgtacgtacgt".to_vec(),
            ..Default::default()
        };
        let (t2, quiet) = write_reporting(&plain, "x.gb", (1, 0, 2026));
        assert!(quiet.is_empty(), "{quiet:?}");
        assert_eq!(parse(&t2).seq, plain.seq);
    }

    /// PROVEN TO FAIL against the unfixed reader:
    ///
    /// ```text
    /// ---- genbank::tests::a_per_part_complement_join_keeps_its_exons_in_transcription_order stdout ----
    /// assertion `left == right` failed: the exons were read in the wrong order
    ///   left: "tgctggtgctaaatgaaacgcggt"
    ///  right: "atgaaacgcggttgctggtgctaa"
    /// ```
    #[test]
    fn a_per_part_complement_join_keeps_its_exons_in_transcription_order() {
        // `join(complement(a),complement(b))` splices rc(a) then rc(b);
        // `complement(join(a,b))` splices rc(b) then rc(a). They are different
        // products, and the writer only ever emits the second spelling, so the
        // reader has to store the parts in the order that spelling means.
        //
        // The model's convention: `Feature::segments` is in join order and a
        // Reverse feature is read back to front — bins/pl-gui/src/aa.rs:878-886,
        // pinned there by `a_two_segment_cds_reads_its_segments_in_transcription_order`
        // and checked against pKoV's stored /translation for SacB.
        fn spliced(m: &Molecule, f: &Feature) -> String {
            let mut segs: Vec<&Segment> = f.segments.iter().collect();
            if f.strand.is_reverse() {
                segs.reverse();
            }
            let mut out = Vec::new();
            for s in segs {
                let bases = m.subseq(s.start, s.end).expect("segment is in range");
                if f.strand.is_reverse() {
                    out.extend(bases.iter().rev().map(|b| match b {
                        b'a' => b't',
                        b't' => b'a',
                        b'c' => b'g',
                        b'g' => b'c',
                        other => *other,
                    }));
                } else {
                    out.extend(bases);
                }
            }
            String::from_utf8(out).expect("ascii")
        }

        let origin = "accgcgtttcat".to_string()   //  1..12
            + &"a".repeat(18)                     // 13..30
            + "ttagcaccagca"                      // 31..42
            + &"t".repeat(18); // 43..60
        assert_eq!(origin.len(), 60);
        let record = |loc: &str| {
            format!(
                "LOCUS       jc                        60 bp    DNA     linear   SYN 01-JAN-2026\n\
                 FEATURES             Location/Qualifiers\n\
                 \x20    CDS             {loc}\n\
                 \x20                    /gene=\"test\"\n\
                 ORIGIN\n\
                 \x20       1 {origin}\n//\n"
            )
        };

        let src = record("join(complement(1..12),complement(31..42))");
        let (mols, warnings) = parse_all_reporting(&src);
        let m = &mols[0];
        assert_eq!(m.seq.len(), 60, "the test record lost bases");
        assert_eq!(m.features.len(), 1);
        // rc(1..12) then rc(31..42): the order the file names.
        let want = "atgaaacgcggttgctggtgctaa";
        assert_eq!(
            spliced(m, &m.features[0]),
            want,
            "the exons were read in the wrong order"
        );
        // Re-spelling a location the model can hold exactly is not a
        // reinterpretation, so there is nothing to report.
        assert!(warnings.is_empty(), "{warnings:?}");

        // ...and the file this writes says the same thing, in the one spelling
        // the writer has.
        let out = write(m, "jc.gb", (1, 0, 2026));
        assert!(
            out.contains("complement(join(31..42,1..12))"),
            "the written location swapped the exons:\n{out}"
        );
        let round = parse(&out);
        assert_eq!(
            spliced(&round, &round.features[0]),
            want,
            "a save changed the spliced product"
        );

        // The other spelling of the same product is unchanged, both ways.
        let plain = parse(&record("complement(join(31..42,1..12))"));
        assert_eq!(spliced(&plain, &plain.features[0]), want);
        assert_eq!(
            plain.features[0].segments, round.features[0].segments,
            "the two spellings must land on one model"
        );
    }
}
