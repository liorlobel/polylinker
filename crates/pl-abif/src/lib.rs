//! ABIF: Applied Biosystems chromatograms, the `.ab1` a sequencing facility
//! sends back.
//!
//! # Two sequences, and which one you are shown
//!
//! An ABIF file can carry the same thing twice, and the tag names are the
//! opposite way round from the obvious guess:
//!
//! - **`PBAS2` is the basecaller's call** — what the machine read.
//! - **`PBAS1` is the sequence after a human edited it.**
//!
//! Measured on 374 real traces from a working lab drive, **the two differ in
//! 58% of files**. Picking one silently is therefore not a detail: it decides
//! whether the user sees the machine's opinion or their colleague's correction,
//! and in more than half of all files those are different sequences.
//!
//! [`Trace::sequence`] is the basecaller's, which is what Biopython and most
//! tools report — and [`Trace::edited`] says when a human's differs, with
//! [`Trace::edited_sequence`] carrying it. Reporting one and hiding the other
//! is the failure this project keeps refusing.
//!
//! # `.ab1` is not a guarantee
//!
//! Of 394 files named `.ab1` in that same corpus, **20 are not ABIF**: 4 are
//! SCF and 16 are ZTR. That is 5%, and it is why `pl-fileio` decides format
//! from content and never from the extension. This module parses ABIF and says
//! so plainly when handed something else, rather than reading a header out of
//! whatever bytes happen to be there.

/// The four-byte magic that opens every ABIF file.
pub const MAGIC: &[u8; 4] = b"ABIF";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Not an ABIF file. `.0` is what the first four bytes actually were, for
    /// a message that names the format the user really has.
    NotAbif([u8; 4]),
    Truncated {
        need: usize,
        got: usize,
    },
    /// The directory points outside the file.
    BadDirectory(String),
    /// No base calls at all — an `.fsa` fragment-analysis file, typically,
    /// which is a real thing to be handed and is not a chromatogram.
    NoBaseCalls,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotAbif(m) => {
                let named = match m {
                    b".scf" => " (this is SCF)",
                    [0xAE, b'Z', b'T', b'R'] => " (this is ZTR)",
                    _ => "",
                };
                write!(
                    f,
                    "not an ABIF chromatogram: starts {:?}{named}",
                    String::from_utf8_lossy(m)
                )
            }
            Error::Truncated { need, got } => {
                write!(f, "truncated: needs {need} bytes, has {got}")
            }
            Error::BadDirectory(e) => write!(f, "{e}"),
            Error::NoBaseCalls => write!(
                f,
                "no base calls in this file; it is probably fragment analysis \
                 (.fsa) rather than a sequencing read"
            ),
        }
    }
}

/// One directory entry: a tag, and where its data lives.
#[derive(Debug, Clone)]
pub struct Tag {
    pub name: [u8; 4],
    pub number: i32,
    pub element_type: i16,
    pub element_size: i16,
    pub elements: i32,
    pub data: Vec<u8>,
}

impl Tag {
    pub fn key(&self) -> String {
        format!("{}{}", String::from_utf8_lossy(&self.name), self.number)
    }
}

/// A parsed chromatogram.
#[derive(Debug, Clone, Default)]
pub struct Trace {
    /// The basecaller's sequence (`PBAS2`), or `PBAS1` when that is all there
    /// is.
    pub sequence: Vec<u8>,
    /// The human-edited sequence (`PBAS1`), when it differs from the above.
    pub edited_sequence: Option<Vec<u8>>,
    /// Per-base quality, 0-255, from `PCON2`. Empty when the file carries none.
    pub quality: Vec<u8>,
    /// Where each called base sits along the trace, from `PLOC2`.
    pub peaks: Vec<u16>,
    /// The four analysed channels (`DATA9`..`DATA12`), in `base_order`.
    pub channels: [Vec<u16>; 4],
    /// Which base each channel represents, from `FWO_`. Conventionally `GATC`.
    pub base_order: [u8; 4],
    pub sample_name: String,
    pub machine: String,
    pub run_start: String,
    pub abif_version: u16,
}

impl Trace {
    /// Did a human change the basecaller's answer?
    pub fn edited(&self) -> bool {
        self.edited_sequence.is_some()
    }
    /// How many bases the human changed, when the two are the same length.
    ///
    /// `None` when they differ in length, because a positional diff would be
    /// meaningless and a number that looks like an edit count but is not is
    /// worse than no number.
    pub fn edit_distance(&self) -> Option<usize> {
        let e = self.edited_sequence.as_ref()?;
        if e.len() != self.sequence.len() {
            return None;
        }
        Some(
            e.iter()
                .zip(&self.sequence)
                .filter(|(a, b)| !a.eq_ignore_ascii_case(b))
                .count(),
        )
    }
    /// Bases the basecaller could not call.
    pub fn ambiguous(&self) -> usize {
        self.sequence
            .iter()
            .filter(|b| !matches!(b.to_ascii_uppercase(), b'A' | b'C' | b'G' | b'T'))
            .count()
    }
    /// Mean quality over the read, or `None` when the file carries none.
    pub fn mean_quality(&self) -> Option<f64> {
        if self.quality.is_empty() {
            return None;
        }
        Some(self.quality.iter().map(|&q| q as f64).sum::<f64>() / self.quality.len() as f64)
    }
}

fn be_u16(b: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([b[at], b[at + 1]])
}
fn be_i32(b: &[u8], at: usize) -> i32 {
    i32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// Every directory entry in an ABIF file.
pub fn tags(data: &[u8]) -> Result<(u16, Vec<Tag>), Error> {
    if data.len() < 34 {
        return Err(Error::Truncated {
            need: 34,
            got: data.len(),
        });
    }
    if &data[..4] != MAGIC {
        let mut m = [0u8; 4];
        m.copy_from_slice(&data[..4]);
        return Err(Error::NotAbif(m));
    }
    let version = be_u16(data, 4);
    // The header's own 28-byte directory entry describes the directory itself.
    // Field offsets *within* an entry: name 0, number 4, elementtype 8,
    // elementsize 10, numelements 12, datasize 16, dataoffset 20, datahandle 24
    // — so the entry count is at 6+12 and not at 6+16, which is the byte count.
    // Reading the latter gives a number that is 28x too large and a directory
    // that appears to run past the end of every file.
    let count = be_i32(data, 6 + 12) as usize;
    let offset = be_i32(data, 6 + 20) as usize;

    let end = offset
        .checked_add(
            count
                .checked_mul(28)
                .ok_or_else(|| Error::BadDirectory("the directory length overflows".into()))?,
        )
        .ok_or_else(|| Error::BadDirectory("the directory runs past the file".into()))?;
    if end > data.len() {
        return Err(Error::BadDirectory(format!(
            "the directory claims {count} entries ending at byte {end}, \
             but the file is {} bytes",
            data.len()
        )));
    }

    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let o = offset + i * 28;
        let mut name = [0u8; 4];
        name.copy_from_slice(&data[o..o + 4]);
        let number = be_i32(data, o + 4);
        let element_type = be_u16(data, o + 8) as i16;
        let element_size = be_u16(data, o + 10) as i16;
        let elements = be_i32(data, o + 12);
        let size = be_i32(data, o + 16).max(0) as usize;
        let where_ = be_i32(data, o + 20);

        // Four bytes or fewer are stored **in the offset field itself**, not at
        // it. Reading them as a pointer gives a plausible offset into the file
        // and four bytes of unrelated data -- and for the short tags that
        // matters most (`FWO_`, run dates) it would be wrong every time.
        let bytes: Vec<u8> = if size <= 4 {
            where_.to_be_bytes()[..size].to_vec()
        } else {
            let start = where_.max(0) as usize;
            match data.get(start..start + size) {
                Some(s) => s.to_vec(),
                None => continue, // a tag pointing outside the file is dropped
            }
        };
        out.push(Tag {
            name,
            number,
            element_type,
            element_size,
            elements,
            data: bytes,
        });
    }
    Ok((version, out))
}

fn find<'a>(tags: &'a [Tag], key: &str) -> Option<&'a Tag> {
    tags.iter().find(|t| t.key() == key)
}

fn as_string(t: Option<&Tag>) -> String {
    // A pString stores its length in the first byte; a cString is
    // NUL-terminated. Both turn up, so the length byte is used when it agrees
    // with what is there and the whole payload otherwise.
    let Some(t) = t else { return String::new() };
    let d = &t.data;
    if d.is_empty() {
        return String::new();
    }
    let body = if d[0] as usize == d.len() - 1 {
        &d[1..]
    } else {
        d.split(|&b| b == 0).next().unwrap_or(d)
    };
    String::from_utf8_lossy(body).trim().to_string()
}

fn as_u16s(t: Option<&Tag>) -> Vec<u16> {
    let Some(t) = t else { return Vec::new() };
    t.data.chunks_exact(2).map(|c| be_u16(c, 0)).collect()
}

/// Parse a chromatogram.
pub fn parse(data: &[u8]) -> Result<Trace, Error> {
    let (version, tags) = tags(data)?;

    let called = find(&tags, "PBAS2").map(|t| t.data.clone());
    let edited = find(&tags, "PBAS1").map(|t| t.data.clone());

    // `PBAS2` is the basecaller's, `PBAS1` the human's. Prefer the machine's
    // and carry the edit; when only one exists, that is the sequence.
    let (sequence, edited_sequence) = match (called, edited) {
        (Some(c), Some(e)) if e != c => (c, Some(e)),
        (Some(c), _) => (c, None),
        (None, Some(e)) => (e, None),
        (None, None) => return Err(Error::NoBaseCalls),
    };
    if sequence.is_empty() {
        return Err(Error::NoBaseCalls);
    }

    let mut base_order = *b"GATC";
    if let Some(t) = find(&tags, "FWO_1") {
        if t.data.len() == 4 {
            base_order.copy_from_slice(&t.data);
        }
    }

    let mut channels: [Vec<u16>; 4] = Default::default();
    for (i, n) in [9, 10, 11, 12].into_iter().enumerate() {
        channels[i] = as_u16s(find(&tags, &format!("DATA{n}")));
    }

    Ok(Trace {
        quality: find(&tags, "PCON2")
            .or_else(|| find(&tags, "PCON1"))
            .map(|t| t.data.clone())
            .unwrap_or_default(),
        peaks: as_u16s(find(&tags, "PLOC2").or_else(|| find(&tags, "PLOC1"))),
        channels,
        base_order,
        sample_name: as_string(find(&tags, "SMPL1")),
        machine: as_string(find(&tags, "MCHN1")),
        run_start: as_string(find(&tags, "RUND1")),
        abif_version: version,
        sequence,
        edited_sequence,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal ABIF file, so the parser can be tested without shipping
    /// a real chromatogram (which would be someone's data and megabytes of it).
    fn build(entries: &[(&[u8; 4], i32, i16, &[u8])]) -> Vec<u8> {
        let header_len = 128;
        let dir_off = header_len;
        let mut dir = Vec::new();
        let mut heap = Vec::new();
        let heap_off = dir_off + entries.len() * 28;
        for (name, num, etype, payload) in entries {
            dir.extend_from_slice(*name);
            dir.extend_from_slice(&num.to_be_bytes());
            dir.extend_from_slice(&etype.to_be_bytes());
            dir.extend_from_slice(&1i16.to_be_bytes());
            dir.extend_from_slice(&(payload.len() as i32).to_be_bytes());
            dir.extend_from_slice(&(payload.len() as i32).to_be_bytes());
            if payload.len() <= 4 {
                let mut inline = [0u8; 4];
                inline[..payload.len()].copy_from_slice(payload);
                dir.extend_from_slice(&inline);
            } else {
                dir.extend_from_slice(&((heap_off + heap.len()) as i32).to_be_bytes());
                heap.extend_from_slice(payload);
            }
            dir.extend_from_slice(&0i32.to_be_bytes());
        }
        let mut out = vec![0u8; header_len];
        out[..4].copy_from_slice(MAGIC);
        out[4..6].copy_from_slice(&101u16.to_be_bytes());
        out[6..10].copy_from_slice(b"tdir");
        out[10..14].copy_from_slice(&1i32.to_be_bytes());
        out[14..16].copy_from_slice(&1023i16.to_be_bytes());
        out[16..18].copy_from_slice(&28i16.to_be_bytes());
        out[18..22].copy_from_slice(&(entries.len() as i32).to_be_bytes());
        out[22..26].copy_from_slice(&((entries.len() * 28) as i32).to_be_bytes());
        out[26..30].copy_from_slice(&(dir_off as i32).to_be_bytes());
        out.extend_from_slice(&dir);
        out.extend_from_slice(&heap);
        out
    }

    #[test]
    fn something_that_is_not_abif_names_the_format_it_actually_is() {
        // 20 of 394 files named `.ab1` on a real lab drive are SCF or ZTR. A
        // reader that says "parse error" sends the user looking for a corrupt
        // file; one that says "this is ZTR" sends them to the right tool.
        let e = parse(b".scf\x00\x00\x00\x00................................").unwrap_err();
        assert!(e.to_string().contains("SCF"), "{e}");
        let e = parse(b"\xaeZTR\x00\x00\x00\x00...............................").unwrap_err();
        assert!(e.to_string().contains("ZTR"), "{e}");
        let e = parse(b"LOCUS  x  4 bp .................................").unwrap_err();
        assert!(e.to_string().contains("not an ABIF"), "{e}");
        assert!(matches!(parse(b"AB"), Err(Error::Truncated { .. })));
    }

    #[test]
    fn the_basecallers_sequence_is_shown_and_the_human_edit_is_reported() {
        // The distinction that decides what the user reads, in 58% of real
        // files. PBAS1 is the *edited* sequence and PBAS2 the machine's, which
        // is the opposite of what the numbering suggests.
        let f = build(&[
            (b"PBAS", 1, 2, b"ACGTACGTAA"),
            (b"PBAS", 2, 2, b"ACGTACGTNN"),
        ]);
        let t = parse(&f).unwrap();
        assert_eq!(t.sequence, b"ACGTACGTNN".to_vec(), "the basecaller's");
        assert_eq!(
            t.edited_sequence,
            Some(b"ACGTACGTAA".to_vec()),
            "the human's, carried rather than hidden"
        );
        assert!(t.edited());
        assert_eq!(t.edit_distance(), Some(2));
        assert_eq!(t.ambiguous(), 2);
    }

    #[test]
    fn an_unedited_file_reports_no_edit_rather_than_an_empty_one() {
        let f = build(&[
            (b"PBAS", 1, 2, b"ACGTACGTAC"),
            (b"PBAS", 2, 2, b"ACGTACGTAC"),
        ]);
        let t = parse(&f).unwrap();
        assert!(!t.edited());
        assert_eq!(t.edited_sequence, None);
        assert_eq!(t.edit_distance(), None);
    }

    #[test]
    fn a_file_with_only_one_sequence_uses_it() {
        for num in [1, 2] {
            let f = build(&[(b"PBAS", num, 2, b"ACGTACGTAC")]);
            let t = parse(&f).unwrap();
            assert_eq!(t.sequence, b"ACGTACGTAC".to_vec());
            assert!(!t.edited());
        }
    }

    #[test]
    fn a_file_with_no_base_calls_says_what_it_probably_is() {
        let f = build(&[(b"DATA", 9, 4, &[0u8; 16])]);
        let e = parse(&f).unwrap_err();
        assert_eq!(e, Error::NoBaseCalls);
        assert!(e.to_string().contains(".fsa"), "{e}");
    }

    #[test]
    fn a_short_tag_is_read_from_the_offset_field_not_through_it() {
        // Four bytes or fewer live *in* the offset field. Following it as a
        // pointer yields a plausible offset and four bytes of unrelated data --
        // and `FWO_` is exactly four bytes, so the channel order would be wrong
        // in every file.
        // The order here is deliberately **not** `GATC`, which is the default
        // this field falls back to. The first version of this test used
        // `GATC`, so following the offset made the tag unreadable, the parser
        // fell back, and the assertion passed against a value that had never
        // been read.
        let f = build(&[(b"PBAS", 2, 2, b"ACGTACGTAC"), (b"FWO_", 1, 2, b"ACGT")]);
        let t = parse(&f).unwrap();
        assert_eq!(&t.base_order, b"ACGT");
        assert_ne!(
            &t.base_order, b"GATC",
            "the fixture must not be the default"
        );
    }

    #[test]
    fn a_directory_pointing_outside_the_file_is_refused_not_followed() {
        let mut f = build(&[(b"PBAS", 2, 2, b"ACGTACGTAC")]);
        f[22..26].copy_from_slice(&1_000_000i32.to_be_bytes());
        f[18..22].copy_from_slice(&40_000i32.to_be_bytes());
        let e = parse(&f).unwrap_err();
        assert!(matches!(e, Error::BadDirectory(_)), "{e:?}");
        assert!(e.to_string().contains("bytes"), "{e}");
    }

    #[test]
    fn quality_and_peaks_come_back_when_present_and_are_absent_otherwise() {
        let f = build(&[
            (b"PBAS", 2, 2, b"ACGTACGTAC"),
            (b"PCON", 2, 2, &[20, 30, 40, 50, 60, 60, 60, 60, 60, 60]),
        ]);
        let t = parse(&f).unwrap();
        assert_eq!(t.quality.len(), 10);
        assert_eq!(t.mean_quality().map(|q| q.round()), Some(50.0));

        let f = build(&[(b"PBAS", 2, 2, b"ACGTACGTAC")]);
        let t = parse(&f).unwrap();
        assert!(t.quality.is_empty());
        assert_eq!(t.mean_quality(), None, "no quality is not zero quality");
    }

    #[test]
    fn an_edit_of_a_different_length_reports_no_distance_rather_than_a_wrong_one() {
        let f = build(&[(b"PBAS", 1, 2, b"ACGTACGT"), (b"PBAS", 2, 2, b"ACGTACGTAC")]);
        let t = parse(&f).unwrap();
        assert!(t.edited());
        assert_eq!(
            t.edit_distance(),
            None,
            "a positional diff between different lengths is meaningless"
        );
    }
}
