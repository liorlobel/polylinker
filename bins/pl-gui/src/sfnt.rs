//! Just enough of the sfnt container to ask a vendored face whether any
//! OpenType layout rule the shaper turns on can fire on a run of printable
//! ASCII.
//!
//! WHY THIS EXISTS. The sequence grid rests on `x(base) = x0 + col * advance`.
//! A ligature is a GSUB *substitution*: it replaces two glyphs with one at
//! shaping time and leaves every `hmtx` entry untouched, so the column-to-base
//! map drifts and a click lands on the wrong base. egui 0.35 cannot turn
//! ligatures off — `epaint::text_layout::shape_text` calls
//! `shaper.shape(buffer, &[])` with an empty user-feature list, and neither
//! `TextOptions` nor `FontTweak` carries one — so the only defence is a face
//! whose default-on rules cannot reach the alphabet the grid paints. This
//! module answers that question from the bytes committed to the repository,
//! with no egui, no `Context` and no shaper in the path, which is what keeps it
//! meaningful if the shaper is ever replaced.
//!
//! WHY IT IS HAND-ROLLED. Table walking against a TTF-parser dependency the
//! house rules would rather not take on for one assertion.
//!
//! WHY IT IS NOT "the font contains no LigatureSubst lookup", which is the
//! obvious form and is wrong. IBM Plex Mono 2.005 ships two GSUB LookupType 4
//! (LigatureSubst) lookups: 34 rules behind `frac` and 13 behind `ccmp`. A
//! lookup-type test would go red on the healthy face we ship.
//!
//! WHY IT IS ALSO NOT "the font advertises no default-on feature", which was
//! the previous form and was ALSO wrong, in the dangerous direction. That test
//! only passed because [`SHAPER_DEFAULTS`] listed six of harfrust's fourteen
//! globally-enabled tags. Plex Mono advertises three of the missing eight —
//! `ccmp` and `locl` in GSUB, `mark` in GPOS — so the honest form of the tag
//! test is RED on the shipped face, and a ligature parked behind `ccmp` (which
//! is where composition and decomposition rules normally live) walked straight
//! past the guard. The question therefore has to be about REACHABILITY: not
//! which features are advertised, but whether any rule behind them can be
//! spelled from the characters `SeqEdit::row_text` can emit.
//!
//! HOW REACHABILITY IS DECIDED, and why it is sound in the safe direction. A
//! lookup can fire on an ASCII-only run only if EVERY position its rules
//! require can be filled by an ASCII glyph. So one coverage table that holds no
//! ASCII glyph anywhere in the context is enough to rule the lookup out — which
//! is exactly how all four of Plex Mono's `ccmp`, `locl` and `mark` lookups are
//! ruled out: each needs a combining mark this application cannot emit. Where a
//! subtable format is not worth decoding in full (class-based contexts, pair
//! positioning) this OVER-reports rather than under-reports, and where a format
//! is not understood at all it returns `Err`. Both directions fail loud.

use std::collections::BTreeSet;

/// The features harfrust enables when the user-feature list is empty.
///
/// Read at source in the pinned harfrust 0.7.0, `src/hb/ot_shape.rs`, in
/// `hb_ot_shape_planner_t::collect_features`. `COMMON_FEATURES` (lines 84-92)
/// is `abvm blwm ccmp locl mark mkmk rlig`; `HORIZONTAL_FEATURES` (lines 94-102)
/// is `calt clig curs dist kern liga rclt`. Both arrays are added
/// unconditionally for horizontal text at lines 149-156, so all fourteen apply
/// to every string this application draws.
///
/// NOT in this list, deliberately, and each omission is a claim that can be
/// re-checked: `frac`, `numr` and `dnom` are added at lines 124-126 through
/// `add_feature` rather than `enable_feature`, i.e. WITHOUT `F_GLOBAL`, so they
/// are masked on only where the automatic-fraction pass sets them — and that
/// pass keys on U+2044 FRACTION SLASH, not on ASCII `/`. `vert` is
/// vertical-only. `rvrn`, `rand`, `trak`, `Harf`, `Buzz` and the direction tags
/// are machinery, not typographic features a text face advertises.
///
/// Named with the version and the file so the next reader can re-check the
/// premise rather than inheriting it. This list was WRONG for one review cycle
/// — it held six tags and its comment attributed `liga` to the wrong array —
/// and the guard was blind to `ccmp`, `locl`, `dist` and `curs` for exactly as
/// long. If harfrust's defaults change, this is what has to change with them.
pub const SHAPER_DEFAULTS: [[u8; 4]; 14] = [
    *b"abvm", *b"blwm", *b"ccmp", *b"locl", *b"mark", *b"mkmk", *b"rlig", // COMMON
    *b"calt", *b"clig", *b"curs", *b"dist", *b"kern", *b"liga", *b"rclt", // HORIZONTAL
];

/// The default-on tags a monospace text face must not advertise AT ALL, as
/// opposed to the ones it may advertise if nothing behind them reaches ASCII.
///
/// The split is not stylistic. These seven exist to change the shape or the
/// position of ordinary text — `liga`, `clig`, `calt`, `rlig` and `rclt`
/// substitute, `kern` and `dist` move x — so a text face that advertises one at
/// all is advertising it FOR the alphabet, and asking whether the rules happen
/// to reach ASCII is asking the wrong question about a face that should have
/// been rejected on sight. `ccmp`, `locl`, `mark`, `mkmk`, `abvm`, `blwm` and
/// `curs` are the opposite: they are how a face supports scripts this
/// application does not paint, every well-made face carries them, and rejecting
/// them outright would reject IBM Plex Mono.
pub const NEVER_IN_A_MONOSPACE_TEXT_FACE: [[u8; 4]; 7] = [
    *b"liga", *b"clig", *b"calt", *b"rlig", *b"rclt", *b"kern", *b"dist",
];

/// The characters the sequence grid can put on screen: `SeqEdit::row_text`
/// pushes `b as char` for every `is_ascii_graphic` byte and `?` otherwise, so
/// U+0021..=U+007E is the whole alphabet a column-to-base map has to survive.
const PAINTABLE: std::ops::RangeInclusive<u32> = 0x21..=0x7E;

/// The four tables every real TrueType face has, so a blob that is not one
/// cannot reach the feature question and answer it vacuously.
///
/// This is the fail-closed half. Without it a renamed JPEG, a WOFF or a TTC
/// would report "no GSUB, therefore no ligatures" and pass — a guard that
/// cannot fail, which is the defect this whole area exists to avoid.
const REQUIRED: [[u8; 4]; 4] = [*b"head", *b"cmap", *b"hmtx", *b"maxp"];

fn be16(b: &[u8], at: usize) -> Result<u16, String> {
    b.get(at..at + 2)
        .map(|s| u16::from_be_bytes([s[0], s[1]]))
        .ok_or_else(|| format!("truncated at byte {at}, reading a u16"))
}

fn be32(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at + 4)
        .map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or_else(|| format!("truncated at byte {at}, reading a u32"))
}

/// A four-byte tag as the characters it is, for an assertion message.
fn tag(t: &[u8; 4]) -> String {
    String::from_utf8_lossy(t).to_string()
}

/// The bytes of one sfnt table, or `None` if this face does not have it.
///
/// Errors — as opposed to returning `None` — whenever the container itself is
/// not a TrueType face, so "the table is absent" is only ever reported about a
/// font.
fn table<'a>(font: &'a [u8], want: &[u8; 4]) -> Result<Option<&'a [u8]>, String> {
    let version = be32(font, 0)?;
    // 0x00010000 is TrueType outlines; 'OTTO' is CFF. 'ttcf' (a collection),
    // 'wOFF'/'wOF2' and anything else are refused here rather than parsed
    // half-way: an index into the wrong container yields plausible-looking
    // offsets and a silently wrong answer.
    if version != 0x0001_0000 && version != u32::from_be_bytes(*b"OTTO") {
        return Err(format!(
            "not a TrueType or OpenType face: sfntVersion is {version:#010x}"
        ));
    }
    let n = be16(font, 4)? as usize;
    let mut found = None;
    let mut have = [false; REQUIRED.len()];
    for i in 0..n {
        let rec = 12 + i * 16;
        let t: [u8; 4] = font
            .get(rec..rec + 4)
            .ok_or_else(|| format!("table directory ends inside record {i} of {n}"))?
            .try_into()
            .expect("a 4-byte slice is a [u8; 4]");
        let off = be32(font, rec + 8)? as usize;
        let len = be32(font, rec + 12)? as usize;
        for (k, r) in REQUIRED.iter().enumerate() {
            have[k] |= t == *r;
        }
        if t == *want {
            found = Some(font.get(off..off + len).ok_or_else(|| {
                format!(
                    "{} claims bytes {off}..{} of a {}-byte file",
                    tag(&t),
                    off + len,
                    font.len()
                )
            })?);
        }
    }
    for (k, r) in REQUIRED.iter().enumerate() {
        if !have[k] {
            return Err(format!("no {} table, so this is not a usable face", tag(r)));
        }
    }
    Ok(found)
}

/// Every feature tag `which` ("GSUB" or "GPOS") advertises, in file order.
///
/// `Ok(None)` means the face is sound and simply has no such table — Hack
/// 3.003 has no GPOS at all, and a face with no GSUB genuinely cannot
/// substitute anything. `Err` means the question could not be answered, and
/// callers must treat that as a failure and not as an absence.
pub fn feature_tags(font: &[u8], which: &[u8; 4]) -> Result<Option<Vec<[u8; 4]>>, String> {
    Ok(features(font, which)?.map(|f| f.into_iter().map(|(t, _)| t).collect()))
}

/// One FeatureList record: the tag it advertises and the lookups it runs.
type FeatureRecord = ([u8; 4], Vec<u16>);

/// Each FeatureList record as its tag and the lookup indices it runs.
fn features(font: &[u8], which: &[u8; 4]) -> Result<Option<Vec<FeatureRecord>>, String> {
    let Some(t) = table(font, which)? else {
        return Ok(None);
    };
    let name = tag(which);
    let major = be16(t, 0)?;
    if major != 1 {
        return Err(format!("{name} version {major} is not one this reads"));
    }
    // GSUB/GPOS header: major, minor, scriptListOffset, featureListOffset,
    // lookupListOffset. The FeatureList is a count followed by (tag, Offset16)
    // records; each Offset16 leads to a Feature table of featureParams,
    // lookupIndexCount, lookupListIndices[].
    let list = be16(t, 6)? as usize;
    let count = be16(t, list)? as usize;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let at = list + 2 + i * 6;
        let ft: [u8; 4] = t
            .get(at..at + 4)
            .ok_or_else(|| format!("{name} FeatureList ends inside record {i} of {count}"))?
            .try_into()
            .expect("a 4-byte slice is a [u8; 4]");
        let feat = list + be16(t, at + 4)? as usize;
        // A zero offset is how the synthetic fixtures say "tag only, no rules";
        // real faces always point at a Feature table.
        let mut idx = Vec::new();
        if be16(t, at + 4)? != 0 {
            let n = be16(t, feat + 2)? as usize;
            for k in 0..n {
                idx.push(be16(t, feat + 4 + k * 2)?);
            }
        }
        out.push((ft, idx));
    }
    Ok(Some(out))
}

/// The features in `which` that the shaper will actually apply: the
/// intersection of what the face advertises with [`SHAPER_DEFAULTS`].
///
/// Sorted and deduplicated, because a FeatureList registers one record per
/// script and language system and Plex Mono's 136 records cover 23 distinct
/// tags — a caller comparing against a list wants the tags, not the records.
///
/// This is the ADVERTISED set, and on its own it is not a verdict: IBM Plex
/// Mono legitimately advertises `ccmp`, `locl` and `mark`. See
/// [`ascii_reachable_default_on`] for the question that has an answer.
pub fn default_on_features(font: &[u8], which: &[u8; 4]) -> Result<Vec<[u8; 4]>, String> {
    let mut on: Vec<[u8; 4]> = features(font, which)?
        .unwrap_or_default()
        .into_iter()
        .map(|(t, _)| t)
        .filter(|t| SHAPER_DEFAULTS.contains(t))
        .collect();
    on.sort_unstable();
    on.dedup();
    Ok(on)
}

/// The default-on features of `which` that have at least one lookup which could
/// fire on a run of printable ASCII.
///
/// THIS IS THE VERDICT. Empty means no rule the shaper turns on can touch the
/// alphabet the sequence grid paints, so `x = x0 + col * advance` holds. A
/// non-empty answer names the tags to go and look at.
///
/// Errors rather than guessing whenever the container, the cmap, a lookup type
/// or a subtable format is not one it understands, so silence is only ever
/// reported about a file it actually read.
pub fn ascii_reachable_default_on(font: &[u8], which: &[u8; 4]) -> Result<Vec<[u8; 4]>, String> {
    let Some(feats) = features(font, which)? else {
        return Ok(Vec::new());
    };
    let t = table(font, which)?.expect("features() already proved this table is here");
    let ascii = paintable_glyphs(font)?;
    let name = tag(which);

    let mut hit = Vec::new();
    for (ft, lookups) in feats {
        if !SHAPER_DEFAULTS.contains(&ft) || hit.contains(&ft) {
            continue;
        }
        for li in lookups {
            if lookup_reaches_ascii(t, which, li, &ascii)
                .map_err(|e| format!("{name} feature {}, lookup {li}: {e}", tag(&ft)))?
            {
                hit.push(ft);
                break;
            }
        }
    }
    hit.sort_unstable();
    Ok(hit)
}

// ---------------------------------------------------------------------------
// cmap: which glyphs the application can actually put on screen
// ---------------------------------------------------------------------------

/// The glyph ids of U+0021..=U+007E, from the face's own cmap.
///
/// Without this the reachability walk has nothing to compare a coverage table
/// against: coverage is in glyph ids and the alphabet is in codepoints, and the
/// mapping between them is per-face.
fn paintable_glyphs(font: &[u8]) -> Result<BTreeSet<u16>, String> {
    let cmap = table(font, b"cmap")?.ok_or("no cmap table")?;
    let n = be16(cmap, 2)? as usize;
    // Prefer a Unicode full-repertoire subtable, then a BMP one. Both Plex
    // faces carry (0,3) and (3,1) format 4 and a legacy (1,0) format 6, which
    // this deliberately will not read: a Macintosh Roman byte table is not a
    // Unicode mapping and using it would silently answer a different question.
    let mut best: Option<(u8, &[u8])> = None;
    for i in 0..n {
        let rec = 4 + i * 8;
        let plat = be16(cmap, rec)?;
        let enc = be16(cmap, rec + 2)?;
        let off = be32(cmap, rec + 4)? as usize;
        let sub = cmap.get(off..).ok_or_else(|| {
            format!(
                "cmap subtable {i} starts at {off}, past the {}-byte table",
                cmap.len()
            )
        })?;
        let unicode = matches!((plat, enc), (0, _) | (3, 1) | (3, 10));
        if !unicode {
            continue;
        }
        let rank = match be16(sub, 0)? {
            12 => 2,
            4 => 1,
            _ => continue,
        };
        if best.is_none_or(|(r, _)| rank > r) {
            best = Some((rank, sub));
        }
    }
    let (rank, sub) = best.ok_or(
        "no Unicode cmap subtable in format 4 or 12; this reader cannot say which \
         glyphs the paintable characters map to, and guessing would make every \
         answer below meaningless",
    )?;
    if rank == 2 {
        cmap12(sub)
    } else {
        cmap4(sub)
    }
}

/// cmap format 4, the segmented BMP mapping every desktop face carries.
fn cmap4(sub: &[u8]) -> Result<BTreeSet<u16>, String> {
    let seg2 = be16(sub, 6)? as usize;
    let segs = seg2 / 2;
    let ends = 14;
    let starts = ends + seg2 + 2;
    let deltas = starts + seg2;
    let ranges = deltas + seg2;
    let mut out = BTreeSet::new();
    for cp in PAINTABLE {
        let cp = cp as u16;
        for s in 0..segs {
            if be16(sub, ends + s * 2)? < cp {
                continue;
            }
            if be16(sub, starts + s * 2)? > cp {
                break;
            }
            let delta = be16(sub, deltas + s * 2)?;
            let ro = be16(sub, ranges + s * 2)?;
            let g = if ro == 0 {
                cp.wrapping_add(delta)
            } else {
                // idRangeOffset is a byte offset from ITS OWN slot, which is
                // what makes this table format famous.
                let at =
                    ranges + s * 2 + ro as usize + 2 * (cp - be16(sub, starts + s * 2)?) as usize;
                match be16(sub, at)? {
                    0 => 0,
                    g => g.wrapping_add(delta),
                }
            };
            if g != 0 {
                out.insert(g);
            }
            break;
        }
    }
    Ok(out)
}

/// cmap format 12, the segmented full-repertoire mapping.
fn cmap12(sub: &[u8]) -> Result<BTreeSet<u16>, String> {
    let groups = be32(sub, 12)? as usize;
    let mut out = BTreeSet::new();
    for g in 0..groups {
        let at = 16 + g * 12;
        let lo = be32(sub, at)?;
        let hi = be32(sub, at + 4)?;
        let gid = be32(sub, at + 8)?;
        for cp in PAINTABLE {
            if cp >= lo && cp <= hi {
                let id = gid + (cp - lo);
                if id != 0 && id <= u16::MAX as u32 {
                    out.insert(id as u16);
                }
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// coverage
// ---------------------------------------------------------------------------

/// The glyphs a Coverage table covers, in coverage-index order.
///
/// The order matters: LigatureSubst keys its LigatureSet array by coverage
/// index, so "the third covered glyph" has to be the third one this returns.
fn coverage(t: &[u8], at: usize) -> Result<Vec<u16>, String> {
    let mut out = Vec::new();
    match be16(t, at)? {
        1 => {
            let n = be16(t, at + 2)? as usize;
            for i in 0..n {
                out.push(be16(t, at + 4 + i * 2)?);
            }
        }
        2 => {
            let n = be16(t, at + 2)? as usize;
            for i in 0..n {
                let r = at + 4 + i * 6;
                let (lo, hi, first) = (be16(t, r)?, be16(t, r + 2)?, be16(t, r + 4)?);
                if hi < lo {
                    return Err(format!("coverage range {i} runs {lo}..{hi}, backwards"));
                }
                for (k, g) in (lo..=hi).enumerate() {
                    let idx = first as usize + k;
                    if out.len() <= idx {
                        out.resize(idx + 1, 0);
                    }
                    out[idx] = g;
                }
            }
        }
        f => return Err(format!("coverage format {f} is not one this reads")),
    }
    Ok(out)
}

/// Resolve the Offset16 stored `field` bytes into the subtable at `at`.
///
/// Every coverage reference in GSUB and GPOS is an Offset16 from the start of
/// the subtable that holds it, never an address — reading the offset slot as if
/// it were the table is the mistake this function exists to make impossible.
fn at_offset(t: &[u8], at: usize, field: usize) -> Result<usize, String> {
    Ok(at + be16(t, at + field)? as usize)
}

/// Does the coverage table AT `off` hold any glyph the application can paint?
fn coverage_has_ascii(t: &[u8], off: usize, ascii: &BTreeSet<u16>) -> Result<bool, String> {
    Ok(coverage(t, off)?.iter().any(|g| ascii.contains(g)))
}

/// Does the coverage table referenced `field` bytes into `at` hold any glyph
/// the application can paint?
fn covers_ascii(t: &[u8], at: usize, field: usize, ascii: &BTreeSet<u16>) -> Result<bool, String> {
    coverage_has_ascii(t, at_offset(t, at, field)?, ascii)
}

/// Every coverage offset in an array of `n` Offset16s starting at `at`, and
/// whether all of them hold ASCII.
///
/// "All" rather than "any" is the whole point: a contextual rule needs every
/// position filled, so ONE position no ASCII glyph can fill makes the rule
/// unreachable from a sequence row. This is what clears IBM Plex Mono's five
/// `ccmp` chain contexts, each of which needs a combining mark.
fn every_position_ascii(
    t: &[u8],
    at: usize,
    n: usize,
    base: usize,
    ascii: &BTreeSet<u16>,
) -> Result<bool, String> {
    for i in 0..n {
        let off = base + be16(t, at + i * 2)? as usize;
        if !coverage_has_ascii(t, off, ascii)? {
            return Ok(false);
        }
    }
    Ok(true)
}

// ---------------------------------------------------------------------------
// lookups
// ---------------------------------------------------------------------------

/// Can lookup `index` of `which` fire on a run of printable ASCII?
fn lookup_reaches_ascii(
    t: &[u8],
    which: &[u8; 4],
    index: u16,
    ascii: &BTreeSet<u16>,
) -> Result<bool, String> {
    let list = be16(t, 8)? as usize;
    let n = be16(t, list)? as usize;
    if index as usize >= n {
        return Err(format!("lookup index {index} in a LookupList of {n}"));
    }
    let lk = list + be16(t, list + 2 + index as usize * 2)? as usize;
    let kind = be16(t, lk)?;
    let subs = be16(t, lk + 4)? as usize;
    for i in 0..subs {
        // subtableOffsets are Offset16 from the START of the Lookup table, not
        // from the end of its header.
        let at = lk + be16(t, lk + 6 + i * 2)? as usize;
        if subtable_reaches_ascii(t, which, kind, at, ascii, false)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Can one subtable fire on a run of printable ASCII?
///
/// Over-reports where a format is not decoded in full; errors where a format is
/// not recognised at all. Never under-reports, which is the only direction that
/// would make the guard a lie.
fn subtable_reaches_ascii(
    t: &[u8],
    which: &[u8; 4],
    kind: u16,
    at: usize,
    ascii: &BTreeSet<u16>,
    nested: bool,
) -> Result<bool, String> {
    let gsub = which == b"GSUB";
    let format = be16(t, at)?;
    // Extension positioning/substitution: a wrapper whose only job is a 32-bit
    // offset, because a large font's lookups do not fit in Offset16.
    if (gsub && kind == 7) || (!gsub && kind == 9) {
        if nested {
            return Err("an Extension lookup wrapping another Extension lookup".into());
        }
        let inner = be16(t, at + 2)?;
        let off = at + be32(t, at + 4)? as usize;
        return subtable_reaches_ascii(t, which, inner, off, ascii, true);
    }
    match (gsub, kind) {
        // Single, Multiple, Alternate substitution and Single positioning: one
        // coverage table, and every rule in them is keyed on it alone.
        (true, 1..=3) | (false, 1) => covers_ascii(t, at, 2, ascii),
        // Ligature substitution. The one that actually threatens the grid, so
        // it is decoded properly rather than approximated: a rule is reachable
        // only if its first glyph AND every component is one we can paint.
        (true, 4) => {
            let cov = coverage(t, at_offset(t, at, 2)?)?;
            let sets = be16(t, at + 4)? as usize;
            for (i, first) in cov.iter().enumerate().take(sets) {
                if !ascii.contains(first) {
                    continue;
                }
                let set = at + be16(t, at + 6 + i * 2)? as usize;
                let ligs = be16(t, set)? as usize;
                for l in 0..ligs {
                    let lig = set + be16(t, set + 2 + l * 2)? as usize;
                    let parts = be16(t, lig + 2)? as usize;
                    // componentCount counts the first glyph, which is not
                    // repeated in the array.
                    let mut all = true;
                    for c in 1..parts {
                        all &= ascii.contains(&be16(t, lig + 2 + c * 2)?);
                    }
                    if all {
                        return Ok(true);
                    }
                }
            }
            Ok(false)
        }
        // Sequence context (GSUB 5 / GPOS 7) and chained sequence context
        // (GSUB 6 / GPOS 8). Formats 1 and 2 key their rule sets on a leading
        // coverage table and then match by glyph or by class; this reads the
        // leading coverage only and so over-reports, which is safe. Format 3
        // spells its whole context as coverage arrays and is read in full,
        // which is what lets Plex Mono's `ccmp` chains through.
        (true, 5) | (false, 7) => match format {
            1 | 2 => covers_ascii(t, at, 2, ascii),
            3 => {
                let n = be16(t, at + 2)? as usize;
                every_position_ascii(t, at + 6, n, at, ascii)
            }
            f => Err(format!("sequence context format {f} is not one this reads")),
        },
        (true, 6) | (false, 8) => match format {
            1 | 2 => covers_ascii(t, at, 2, ascii),
            3 => {
                let back = be16(t, at + 2)? as usize;
                let input_at = at + 4 + back * 2;
                let input = be16(t, input_at)? as usize;
                let ahead_at = input_at + 2 + input * 2;
                let ahead = be16(t, ahead_at)? as usize;
                Ok(every_position_ascii(t, at + 4, back, at, ascii)?
                    && every_position_ascii(t, input_at + 2, input, at, ascii)?
                    && every_position_ascii(t, ahead_at + 2, ahead, at, ascii)?)
            }
            f => Err(format!("chained context format {f} is not one this reads")),
        },
        // Reverse chained single substitution: coverage, then backtrack and
        // lookahead coverage arrays.
        (true, 8) => {
            if !covers_ascii(t, at, 2, ascii)? {
                return Ok(false);
            }
            let back = be16(t, at + 4)? as usize;
            let ahead_at = at + 6 + back * 2;
            let ahead = be16(t, ahead_at)? as usize;
            Ok(every_position_ascii(t, at + 6, back, at, ascii)?
                && every_position_ascii(t, ahead_at + 2, ahead, at, ascii)?)
        }
        // Pair and cursive positioning. Both move x. Read as their leading
        // coverage only — over-reporting — because neither appears behind a
        // default-on feature in any face this repository ships, so decoding
        // ValueRecord widths would be untested code guarding nothing.
        (false, 2 | 3) => covers_ascii(t, at, 2, ascii),
        // Mark-to-base, mark-to-ligature and mark-to-mark positioning: two
        // coverage tables, and BOTH have to be fillable. The mark side is what
        // rules all four of Plex Mono's `mark` lookups out — this application
        // cannot emit a combining mark.
        (false, 4..=6) => Ok(covers_ascii(t, at, 2, ascii)? && covers_ascii(t, at, 4, ascii)?),
        _ => Err(format!(
            "{} lookup type {kind} is not one this reads",
            tag(which)
        )),
    }
}

/// A feature tag as the four characters it is, for an assertion message.
pub fn show(tags: &[[u8; 4]]) -> String {
    tags.iter().map(tag).collect::<Vec<_>>().join(" ")
}
