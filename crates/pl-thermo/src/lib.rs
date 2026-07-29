//! Melting temperature, by nearest-neighbour thermodynamics.
//!
//! # What this reports, and what it refuses to
//!
//! **A single, physically defined Tm**, and nothing else. Annealing-temperature
//! advice lives in [`anneal`], separately and per polymerase, because baking a
//! buffer correction into a reported Tm means never being able to explain a
//! discrepancy afterwards. `docs/PLAN.md` §7.2 is explicit about this and it is
//! worth restating: report Tm, advise Ta.
//!
//! **No promise of decimal parity with SnapGene.** SnapGene documents only "a
//! nearest-neighbor thermodynamic algorithm with up-to-date parameters"; you
//! cannot match to the decimal what is not specified. What is offered instead
//! is [`Method::describe`] — the parameter set, the salt model and the
//! concentrations behind every number — so a difference reads as a documented
//! modelling choice rather than a bug.
//!
//! # Where the numbers came from
//!
//! Extracted programmatically from **Biopython's `Bio.SeqUtils.MeltingTemp`**
//! (`DNA_NN3` and `DNA_NN4`), which `docs/PLAN.md` §7.2 names as a licence-clean
//! source — and explicitly **not** from Primer3's `oligotm.c`, which is
//! GPL-2.0 and would relicense the distribution. The ~10 ΔH/ΔS values per table
//! are published measurements and uncopyrightable facts; only an implementation
//! carries a licence.
//!
//! They were *derived, not recalled*. This project's own record says recalled
//! constants are the unreliable part, and a wrong stacking parameter is
//! invisible: it shifts every Tm by a degree and nothing looks broken.
//!
//! `docs/PLAN.md` said the 1998→2004 revision "changes only the AA/TT stack".
//! It does not: the three initiation terms change too, which is why both tables
//! are stored whole rather than as a patch.
//!
//! # The model
//!
//! ```text
//! Tm = ΔH / (ΔS + R·ln(C_T / x)) − 273.15      R = 1.987 cal/(K·mol)
//! ```
//!
//! with `x = 1` for a self-complementary oligo and `x = 4` otherwise, ΔH in
//! cal/mol and ΔS in cal/(K·mol). `docs/PLAN.md` §7.2 states that pair the
//! wrong way round; see [`tm`].

use pl_core::iupac::reverse_complement;

/// Gas constant, cal/(K·mol).
pub const R: f64 = 1.987;

/// A nearest-neighbour parameter set.
///
/// `stacks` holds the ten independent dinucleotide steps; the other six of the
/// sixteen are the same duplex read from the other strand and are looked up by
/// reverse-complementing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NnTable {
    /// `(step, ΔH kcal/mol, ΔS cal/(K·mol))`.
    pub stacks: [(&'static str, f64, f64); 10],
    /// Applied once, whatever the ends are.
    pub init: (f64, f64),
    /// Per terminal A or T.
    pub init_at: (f64, f64),
    /// Per terminal G or C.
    pub init_gc: (f64, f64),
    /// Applied when the duplex is self-complementary.
    pub sym: (f64, f64),
}

/// SantaLucia (1998) unified parameters — the default.
///
/// The most-cited set, and the one most tools report.
pub const SANTALUCIA_1998: NnTable = NnTable {
    stacks: [
        ("AA", -7.9, -22.2),
        ("AT", -7.2, -20.4),
        ("TA", -7.2, -21.3),
        ("CA", -8.5, -22.7),
        ("GT", -8.4, -22.4),
        ("CT", -7.8, -21.0),
        ("GA", -8.2, -22.2),
        ("CG", -10.6, -27.2),
        ("GC", -9.8, -24.4),
        ("GG", -8.0, -19.9),
    ],
    init: (0.0, 0.0),
    init_at: (2.3, 4.1),
    init_gc: (0.1, -2.8),
    sym: (0.0, -1.4),
};

/// SantaLucia & Hicks (2004).
///
/// Differs from 1998 in the AA/TT stack **and in all three initiation terms**.
pub const SANTALUCIA_2004: NnTable = NnTable {
    stacks: [
        ("AA", -7.6, -21.3),
        ("AT", -7.2, -20.4),
        ("TA", -7.2, -21.3),
        ("CA", -8.5, -22.7),
        ("GT", -8.4, -22.4),
        ("CT", -7.8, -21.0),
        ("GA", -8.2, -22.2),
        ("CG", -10.6, -27.2),
        ("GC", -9.8, -24.4),
        ("GG", -8.0, -19.9),
    ],
    init: (0.2, -5.7),
    init_at: (2.2, 6.9),
    init_gc: (0.0, 0.0),
    sym: (0.0, -1.4),
};

impl NnTable {
    /// ΔH, ΔS for one dinucleotide step, or `None` if either base is not
    /// A, C, G or T.
    ///
    /// A step not in the table is looked up as its reverse complement, which is
    /// the same duplex read the other way — that is why ten entries cover
    /// sixteen steps.
    pub fn step(&self, pair: &[u8]) -> Option<(f64, f64)> {
        let up: Vec<u8> = pair.iter().map(|b| b.to_ascii_uppercase()).collect();
        if !up.iter().all(|b| matches!(b, b'A' | b'C' | b'G' | b'T')) {
            return None;
        }
        let key = String::from_utf8(up.clone()).ok()?;
        if let Some(&(_, h, s)) = self.stacks.iter().find(|(k, _, _)| *k == key) {
            return Some((h, s));
        }
        let rc = String::from_utf8(reverse_complement(&up)).ok()?;
        self.stacks
            .iter()
            .find(|(k, _, _)| *k == rc)
            .map(|&(_, h, s)| (h, s))
    }

    /// As [`NnTable::step`], without allocating.
    ///
    /// `step` builds a `Vec`, a `String` and a reverse complement for every
    /// lookup. That is nothing for one Tm and is not nothing for a primer
    /// search: `pl-design` makes millions of ΔG lookups per run, and this was
    /// measured at 100 seconds of a 104-second design over a 1 kb region.
    ///
    /// A second lookup is exactly the kind of duplication that drifts, so
    /// `step_and_step_fast_agree_on_every_input` asserts equivalence over all
    /// 16 dinucleotides, both cases, and the ambiguous and short inputs.
    /// Private, so nothing outside this module can pick the wrong one.
    fn step_fast(&self, a: u8, b: u8) -> Option<(f64, f64)> {
        fn comp(b: u8) -> Option<u8> {
            match b {
                b'A' => Some(b'T'),
                b'C' => Some(b'G'),
                b'G' => Some(b'C'),
                b'T' => Some(b'A'),
                _ => None,
            }
        }
        let (a, b) = (a.to_ascii_uppercase(), b.to_ascii_uppercase());
        let (ra, rb) = (comp(b)?, comp(a)?);
        for &(k, h, s) in &self.stacks {
            let k = k.as_bytes();
            if k[0] == a && k[1] == b {
                return Some((h, s));
            }
        }
        for &(k, h, s) in &self.stacks {
            let k = k.as_bytes();
            if k[0] == ra && k[1] == rb {
                return Some((h, s));
            }
        }
        None
    }
}

/// How salt is accounted for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaltCorrection {
    /// No correction: the parameters as published, at 1 M Na⁺.
    None,
    /// SantaLucia (1998): an *entropy* correction,
    /// `ΔS' = ΔS + 0.368·(N−1)·ln[Na⁺]`.
    ///
    /// The default, and the one `docs/PLAN.md` §7.2 specifies. Applied to ΔS
    /// rather than to the resulting Tm, which is why it cannot simply be added
    /// to a temperature afterwards.
    SantaLucia1998,
    /// Schildkraut & Lifson (1965): `Tm + 16.6·log10([Na⁺])`, added to the
    /// temperature. Older, still widely quoted, kept because papers report it.
    SchildkrautLifson,
}

/// Everything that decides a number, so it can be printed next to one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Method {
    pub table: NnTable,
    pub table_name: &'static str,
    pub salt: SaltCorrection,
    /// Total strand concentration, molar. Default 50 nM.
    pub oligo_molar: f64,
    /// Monovalent cation, molar. Default 50 mM.
    pub na_molar: f64,
}

impl Default for Method {
    fn default() -> Self {
        Method {
            table: SANTALUCIA_1998,
            table_name: "SantaLucia 1998",
            salt: SaltCorrection::SantaLucia1998,
            oligo_molar: 50e-9,
            na_molar: 50e-3,
        }
    }
}

impl Method {
    pub fn santalucia_2004() -> Self {
        Method {
            table: SANTALUCIA_2004,
            table_name: "SantaLucia & Hicks 2004",
            ..Default::default()
        }
    }

    /// One line naming everything behind the number.
    ///
    /// Not decoration. Without it a Tm that differs from another tool's by a
    /// degree is indistinguishable from a bug, and this project cannot promise
    /// parity with a tool that does not publish its model.
    pub fn describe(&self) -> String {
        let salt = match self.salt {
            SaltCorrection::None => "no salt correction",
            SaltCorrection::SantaLucia1998 => "SantaLucia 1998 salt correction",
            SaltCorrection::SchildkrautLifson => "Schildkraut-Lifson 1965 salt correction",
        };
        format!(
            "{} nearest-neighbour, {salt}, {:.0} nM oligo, {:.0} mM Na+",
            self.table_name,
            self.oligo_molar * 1e9,
            self.na_molar * 1e3
        )
    }
}

/// Why a Tm could not be computed.
///
/// `Eq` is deliberately absent: [`TmError::SaltUndefined`] carries the
/// concentration it was handed, and one of the values that reaches it is NaN.
#[derive(Debug, Clone, PartialEq)]
pub enum TmError {
    /// Fewer than two bases: there is no stack to sum.
    TooShort,
    /// A base that is not A, C, G or T, at a 0-based position.
    ///
    /// Refused rather than skipped. An ambiguity code has no stacking
    /// parameters, and quietly dropping it would report the Tm of a *different,
    /// shorter* oligo — a number that looks entirely reasonable and is about
    /// something else.
    NotUnambiguous(usize, u8),
    /// The denominator vanished, which no real oligo produces.
    Undefined,
    /// A salt correction was asked for at a sodium concentration where it is
    /// not defined. `.0` is that concentration, molar.
    ///
    /// Both corrections take a logarithm of `[Na⁺]`, which zero, a negative and
    /// NaN all fail to have. Skipping the correction instead — which this did
    /// until 2026-07-28 — does not fall back to *nothing*, it falls back to the
    /// published parameters' own 1 M Na⁺ condition, and then reports that
    /// number under a [`Method::describe`] line still naming the correction.
    /// For ACGTACGTACGTACGTACGT that is 68.5 °C against 54.0 °C at the 50 mM
    /// default: 14.5 degrees, beside the words "SantaLucia 1998 salt
    /// correction, 0 mM Na+".
    SaltUndefined(f64),
}

impl std::fmt::Display for TmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TmError::TooShort => write!(f, "a melting temperature needs at least two bases"),
            TmError::NotUnambiguous(i, b) => write!(
                f,
                "base {} is {:?}, which has no stacking parameters; \
                 a Tm over the rest would be a different oligo's",
                i + 1,
                *b as char
            ),
            TmError::Undefined => write!(f, "the entropy term cancelled; no Tm is defined"),
            TmError::SaltUndefined(na) => write!(
                f,
                "the salt correction needs a positive sodium concentration and \
                 was given {} mM; a Tm computed without it would be the 1 M \
                 Na+ number reported under the corrected model's name",
                na * 1e3
            ),
        }
    }
}

/// A melting temperature and the thermodynamics behind it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tm {
    /// Degrees Celsius.
    pub tm: f64,
    /// kcal/mol.
    pub dh: f64,
    /// cal/(K·mol), **after** any entropy salt correction.
    pub ds: f64,
    /// Was the oligo its own reverse complement?
    pub self_complementary: bool,
    pub gc_percent: f64,
}

/// Is a sequence its own reverse complement?
pub fn is_self_complementary(seq: &[u8]) -> bool {
    let up: Vec<u8> = seq.iter().map(|b| b.to_ascii_uppercase()).collect();
    reverse_complement(&up) == up
}

/// Melting temperature of a duplex formed by `seq` and its complement.
pub fn tm(seq: &[u8], m: &Method) -> Result<Tm, TmError> {
    if seq.len() < 2 {
        return Err(TmError::TooShort);
    }
    let up: Vec<u8> = seq.iter().map(|b| b.to_ascii_uppercase()).collect();
    if let Some((i, &b)) = up
        .iter()
        .enumerate()
        .find(|(_, b)| !matches!(b, b'A' | b'C' | b'G' | b'T'))
    {
        return Err(TmError::NotUnambiguous(i, b));
    }

    let (mut dh, mut ds) = m.table.init;
    for end in [up[0], up[up.len() - 1]] {
        let (h, s) = if matches!(end, b'A' | b'T') {
            m.table.init_at
        } else {
            m.table.init_gc
        };
        dh += h;
        ds += s;
    }
    for w in up.windows(2) {
        let (h, s) = m.table.step(w).expect("checked unambiguous above");
        dh += h;
        ds += s;
    }

    let selfcomp = is_self_complementary(&up);
    if selfcomp {
        dh += m.table.sym.0;
        ds += m.table.sym.1;
    }

    // A salt correction that cannot be computed is refused, not skipped.
    //
    // The guard here used to be `&& m.na_molar > 0.0`, which reads as caution
    // and is not: it does not disable the correction, it substitutes a
    // *different model* — the parameters as published, at 1 M Na+ — and hands
    // the answer back under `describe()`'s unchanged "SantaLucia 1998 salt
    // correction, 0 mM Na+". `pl tm --na 0 ACGTACGTACGTACGTACGT` reported
    // 68.5 C where 50 mM gives 54.0 C. `--na nan` parses, fails `NaN > 0.0`,
    // and printed the same 68.5 C beside "NaN mM Na+". A negative --na is the
    // clearest of the three: without the guard, `ln` of a negative makes the
    // denominator non-finite and the check below already returns `Undefined`,
    // so the guard converted a working refusal into a plausible wrong number.
    //
    // Written as a positive test rather than `x <= 0.0`, because NaN fails
    // every comparison and would slip past that one. `is_finite` also refuses
    // an infinite concentration, which `--na inf` parses to and which has no
    // more of a logarithm than the rest.
    let na_usable = m.na_molar.is_finite() && m.na_molar > 0.0;
    if m.salt != SaltCorrection::None && !na_usable {
        return Err(TmError::SaltUndefined(m.na_molar));
    }

    // Salt, on the entropy where the model puts it.
    let mut ds_corrected = ds;
    if m.salt == SaltCorrection::SantaLucia1998 {
        ds_corrected += 0.368 * (up.len() as f64 - 1.0) * m.na_molar.ln();
    }

    // C_T/4 for an ordinary duplex, C_T for a self-complementary one.
    //
    // **This is the direction `docs/PLAN.md` §7.2 had backwards**, and copying
    // it from there put every palindrome's Tm out by about 8 degrees and every
    // ordinary oligo's by about 4. SantaLucia: for A + B a AB with the two
    // strands at C_T/2 each, the effective concentration entering the
    // equilibrium is C_T/4; a palindrome anneals to a copy of itself, so there
    // is no factor to divide by. Biopython encodes the same thing as
    // `k = dnac1 - dnac2/2`, which is C_T/4 when the strands are equal, and
    // `k = dnac1` when self-complementary.
    //
    // Caught by the differential against Biopython, not by any test written
    // here -- which is the entire argument for having one.
    let x = if selfcomp { 1.0 } else { 4.0 };
    let denom = ds_corrected + R * (m.oligo_molar / x).ln();
    if denom == 0.0 || !denom.is_finite() {
        return Err(TmError::Undefined);
    }
    let mut celsius = (dh * 1000.0) / denom - 273.15;

    if m.salt == SaltCorrection::SchildkrautLifson {
        // Guarded above, together with SantaLucia1998: log10 of zero or a
        // negative is no more defined than ln of one.
        celsius += 16.6 * m.na_molar.log10();
    }

    let gc = up.iter().filter(|b| matches!(b, b'G' | b'C')).count() as f64;
    Ok(Tm {
        tm: celsius,
        dh,
        ds: ds_corrected,
        self_complementary: selfcomp,
        gc_percent: 100.0 * gc / up.len() as f64,
    })
}

/// 37 °C in kelvin — the reference temperature for every ΔG in this crate.
///
/// 37 rather than a PCR temperature because that is the condition the
/// published parameters are tabulated at, and quoting ΔG at a temperature the
/// table was not measured at would be inventing a number.
pub const T37_KELVIN: f64 = 310.15;

/// Sum ΔH and ΔS over a sequence's nearest-neighbour steps.
///
/// The stacks only: no initiation, no terminal AT/GC penalty, no symmetry
/// term. `tm` adds those; the two ΔG functions below differ in exactly this.
fn stack_sums(seq: &[u8], t: &NnTable) -> Result<(f64, f64), TmError> {
    if seq.len() < 2 {
        return Err(TmError::TooShort);
    }
    // Allocation-free, deliberately: this is the inner loop of a primer search,
    // and an uppercase copy per call plus three more inside `step` is what made
    // one design run take a hundred seconds. `step_fast` is pinned equivalent
    // to `step`.
    if let Some((i, &b)) = seq
        .iter()
        .enumerate()
        .find(|(_, b)| !matches!(b.to_ascii_uppercase(), b'A' | b'C' | b'G' | b'T'))
    {
        return Err(TmError::NotUnambiguous(i, b.to_ascii_uppercase()));
    }
    let (mut dh, mut ds) = (0.0, 0.0);
    for w in seq.windows(2) {
        let (h, s) = t.step_fast(w[0], w[1]).expect("checked unambiguous above");
        dh += h;
        ds += s;
    }
    Ok((dh, ds))
}

/// ΔG°37 of the stacks alone, kcal/mol, at the table's own 1 M Na⁺.
///
/// `ΔG = ΔH − T·ΔS/1000`, with the stored ΔH and ΔS. **No new table is
/// needed**: the ΔG°37 column SantaLucia publishes is this arithmetic on the
/// two columns already here, and computing it rather than storing a third set
/// of numbers means a transcription error cannot make the two disagree.
///
/// This is the *stack-sum* convention, and it is the one every ΔG reported by
/// `pl-design` uses — including the Rychlik 3'-end-stability criterion, which
/// is defined over a terminal pentamer treated as bare stacks. It is not the
/// free energy of forming a duplex: [`dg37`] is, and on the 2004 table the two
/// differ by up to 2.03 kcal/mol, which is larger than several of the
/// thresholds measured against it. Naming them apart is why they are two
/// functions rather than one with a flag.
///
/// **No salt correction.** SantaLucia's correction is defined on duplex
/// entropy at the melting transition; applying it to a ΔG°37 stack sum would
/// be inventing a model. The number is a 1 M Na⁺ number and must be reported
/// as one.
pub fn dg37_stacks(seq: &[u8], t: &NnTable) -> Result<f64, TmError> {
    let (dh, ds) = stack_sums(seq, t)?;
    Ok(dh - T37_KELVIN * ds / 1000.0)
}

/// ΔG°37 of the whole duplex, kcal/mol: stacks plus initiation, the two
/// terminal penalties, and the symmetry term for a palindrome.
///
/// The physically complete number for `seq` annealed to its perfect
/// complement. Also at 1 M Na⁺, for the reason [`dg37_stacks`] gives.
pub fn dg37(seq: &[u8], t: &NnTable) -> Result<f64, TmError> {
    let (mut dh, mut ds) = stack_sums(seq, t)?;
    let up: Vec<u8> = seq.iter().map(|b| b.to_ascii_uppercase()).collect();
    dh += t.init.0;
    ds += t.init.1;
    for end in [up[0], up[up.len() - 1]] {
        let (h, s) = if matches!(end, b'A' | b'T') {
            t.init_at
        } else {
            t.init_gc
        };
        dh += h;
        ds += s;
    }
    if is_self_complementary(&up) {
        dh += t.sym.0;
        ds += t.sym.1;
    }
    Ok(dh - T37_KELVIN * ds / 1000.0)
}

/// A polymerase, and how its vendor says to pick an annealing temperature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Polymerase {
    pub name: &'static str,
    /// Degrees added to the *lower* primer Tm.
    pub offset_low: i32,
    pub offset_high: i32,
    pub note: &'static str,
}

/// Annealing advice, kept apart from Tm on purpose.
///
/// A reported Tm is a physical property of a duplex. An annealing temperature
/// is a protocol recommendation that depends on the enzyme and its buffer.
/// Folding the second into the first is how a tool ends up unable to explain
/// why its number differs from anyone else's.
pub const POLYMERASES: &[Polymerase] = &[
    Polymerase {
        name: "Phusion",
        offset_low: 3,
        offset_high: 3,
        note: "Ta = Tm + 3 for primers over 20 nt; use the lower Tm",
    },
    Polymerase {
        name: "Q5",
        offset_low: 0,
        offset_high: 0,
        note: "NEB advise their own calculator; Ta near the lower Tm",
    },
    Polymerase {
        name: "Phire",
        offset_low: 3,
        offset_high: 3,
        note: "as Phusion",
    },
    Polymerase {
        name: "Taq",
        offset_low: -5,
        offset_high: -5,
        note: "Ta = Tm - 5, the classic rule",
    },
];

/// Suggested annealing temperature for a primer pair, or one primer.
///
/// Always from the **lower** of the two Tms: the weaker primer is the one that
/// fails to anneal.
pub fn anneal(tm_a: f64, tm_b: Option<f64>, p: &Polymerase) -> (f64, f64) {
    let low = match tm_b {
        Some(b) => tm_a.min(b),
        None => tm_a,
    };
    (low + p.offset_low as f64, low + p.offset_high as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64, tol: f64, what: &str) {
        assert!((a - b).abs() <= tol, "{what}: {a} vs {b}");
    }

    #[test]
    fn the_two_tables_differ_in_four_places_not_one() {
        // docs/PLAN.md said the 1998 -> 2004 revision changes only the AA/TT
        // stack. It changes the three initiation terms as well, which is why
        // both tables are stored whole.
        let a = SANTALUCIA_1998;
        let b = SANTALUCIA_2004;
        let stack_diffs = a
            .stacks
            .iter()
            .zip(b.stacks.iter())
            .filter(|(x, y)| x != y)
            .count();
        assert_eq!(stack_diffs, 1, "only AA/TT differs among the stacks");
        assert_ne!(a.init, b.init);
        assert_ne!(a.init_at, b.init_at);
        assert_ne!(a.init_gc, b.init_gc);
        assert_eq!(a.sym, b.sym);
    }

    #[test]
    fn a_step_missing_from_the_table_is_found_as_its_reverse_complement() {
        // Ten entries cover sixteen steps because a duplex read from the other
        // strand is the same duplex.
        let t = SANTALUCIA_1998;
        assert_eq!(t.step(b"AA"), t.step(b"TT"));
        assert_eq!(t.step(b"CA"), t.step(b"TG"));
        assert_eq!(t.step(b"GT"), t.step(b"AC"));
        assert_eq!(t.step(b"CG"), t.step(b"CG"));
        // Every one of the sixteen steps resolves.
        for a in b"ACGT" {
            for b in b"ACGT" {
                assert!(t.step(&[*a, *b]).is_some(), "{}{}", *a as char, *b as char);
            }
        }
        // Lowercase is folded; ambiguity is not a step.
        assert_eq!(t.step(b"aa"), t.step(b"AA"));
        assert_eq!(t.step(b"AN"), None);
        assert_eq!(t.step(b"A-"), None);
    }

    #[test]
    fn an_ambiguous_base_is_refused_rather_than_skipped() {
        // Dropping it would report the Tm of a shorter, different oligo -- a
        // perfectly reasonable-looking number about something else.
        let m = Method::default();
        assert_eq!(
            tm(b"ACGTNACGT", &m).unwrap_err(),
            TmError::NotUnambiguous(4, b'N')
        );
        assert!(tm(b"A", &m).is_err());
        assert_eq!(tm(b"", &m).unwrap_err(), TmError::TooShort);
        // The message names the position, 1-based, and the base.
        let msg = tm(b"ACGTRACGT", &m).unwrap_err().to_string();
        assert!(msg.contains("base 5"), "{msg}");
        assert!(msg.contains("'R'"), "{msg}");
    }

    #[test]
    fn a_palindrome_is_recognised_and_costs_a_symmetry_term() {
        assert!(is_self_complementary(b"GAATTC"));
        assert!(is_self_complementary(b"gaattc"));
        assert!(!is_self_complementary(b"GAATTG"));
        // AAAACCCCGGGGTTTT is its own reverse complement, which has caught this
        // project out before.
        assert!(is_self_complementary(b"AAAACCCCGGGGTTTT"));

        let m = Method::default();
        let p = tm(b"GAATTC", &m).unwrap();
        assert!(p.self_complementary);
        let q = tm(b"GAATTG", &m).unwrap();
        assert!(!q.self_complementary);
    }

    #[test]
    fn gc_content_and_length_move_tm_the_way_they_must() {
        let m = Method::default();
        let at = tm(b"ATATATATATATATATATAT", &m).unwrap();
        let gc = tm(b"GCGCGCGCGCGCGCGCGCGC", &m).unwrap();
        assert!(gc.tm > at.tm + 20.0, "GC-rich must melt far higher");
        approx(at.gc_percent, 0.0, 1e-9, "AT gc");
        approx(gc.gc_percent, 100.0, 1e-9, "GC gc");

        let short = tm(b"ACGTACGTAC", &m).unwrap();
        let long = tm(b"ACGTACGTACACGTACGTAC", &m).unwrap();
        assert!(long.tm > short.tm, "a longer duplex melts higher");
    }

    #[test]
    fn more_salt_raises_tm_and_the_correction_is_on_entropy() {
        let low = Method {
            na_molar: 10e-3,
            ..Default::default()
        };
        let high = Method {
            na_molar: 200e-3,
            ..Default::default()
        };
        let a = tm(b"ACGTACGTACGTACGTACGT", &low).unwrap();
        let b = tm(b"ACGTACGTACGTACGTACGT", &high).unwrap();
        assert!(b.tm > a.tm, "more salt stabilises the duplex");
        // The correction lands on entropy, not on the temperature: the two runs
        // must report different ΔS and identical ΔH.
        approx(a.dh, b.dh, 1e-9, "enthalpy is salt-independent");
        assert_ne!(a.ds, b.ds);
    }

    #[test]
    fn concentration_moves_tm_the_right_way() {
        let dilute = Method {
            oligo_molar: 1e-9,
            ..Default::default()
        };
        let strong = Method {
            oligo_molar: 1e-6,
            ..Default::default()
        };
        let a = tm(b"ACGTACGTACGTACGTACGT", &dilute).unwrap();
        let b = tm(b"ACGTACGTACGTACGTACGT", &strong).unwrap();
        assert!(b.tm > a.tm, "more oligo melts higher");
    }

    #[test]
    fn annealing_advice_uses_the_weaker_primer_and_is_not_the_tm() {
        let taq = POLYMERASES.iter().find(|p| p.name == "Taq").unwrap();
        let (lo, hi) = anneal(60.0, Some(65.0), taq);
        assert_eq!((lo, hi), (55.0, 55.0), "from the lower Tm, minus five");
        let phusion = POLYMERASES.iter().find(|p| p.name == "Phusion").unwrap();
        assert_eq!(anneal(60.0, Some(65.0), phusion), (63.0, 63.0));
        // One primer is allowed.
        assert_eq!(anneal(60.0, None, taq), (55.0, 55.0));
    }

    #[test]
    fn the_method_says_what_produced_the_number() {
        // Without this a Tm differing from another tool's by a degree is
        // indistinguishable from a bug, and no parity is promised.
        let d = Method::default().describe();
        assert!(d.contains("SantaLucia 1998"), "{d}");
        assert!(d.contains("50 nM"), "{d}");
        assert!(d.contains("50 mM"), "{d}");
        let d = Method::santalucia_2004().describe();
        assert!(d.contains("2004"), "{d}");
    }

    #[test]
    fn a_salt_correction_at_zero_or_negative_or_nan_sodium_is_refused() {
        // The guard here used to be `&& m.na_molar > 0.0`, which does not
        // disable the correction: it reverts to the published parameters' own
        // 1 M Na+ condition and reports that under `describe()`'s unchanged
        // "SantaLucia 1998 salt correction". `pl tm --na 0` on this oligo
        // printed 68.5 C where the 50 mM default gives 54.0 C, beside the
        // words "0 mM Na+" -- a method line that does not describe the number
        // next to it. Nothing upstream range-checks --na, and Rust's f64
        // FromStr accepts "nan" and "inf".
        let seq = b"ACGTACGTACGTACGTACGT";
        // "inf" parses too, and has no more of a logarithm than the rest.
        for na in [0.0, -50e-3, f64::NAN, f64::INFINITY] {
            for salt in [
                SaltCorrection::SantaLucia1998,
                SaltCorrection::SchildkrautLifson,
            ] {
                let m = Method {
                    salt,
                    na_molar: na,
                    ..Default::default()
                };
                match tm(seq, &m) {
                    Err(TmError::SaltUndefined(got)) => {
                        assert_eq!(got.is_nan(), na.is_nan());
                        if !na.is_nan() {
                            assert_eq!(got, na, "the message must name what it was given");
                        }
                    }
                    other => panic!("{salt:?} at {na} molar Na+ gave {other:?}"),
                }
            }
        }
        // And the refusal says which quantity is at fault.
        let m = Method {
            na_molar: 0.0,
            ..Default::default()
        };
        let msg = tm(seq, &m).unwrap_err().to_string();
        assert!(msg.contains("sodium"), "{msg}");
        assert!(msg.contains("0 mM"), "{msg}");
    }

    #[test]
    fn asking_for_no_salt_correction_is_not_the_same_as_a_bad_concentration() {
        // The control, and the reason the refusal is conditioned on the
        // correction rather than on the concentration alone. `SaltCorrection::
        // None` is a real, honestly-labelled model -- the parameters as
        // published, at 1 M Na+ -- and `na_molar` is simply not read, so a
        // nonsense value there must not stop it.
        let seq = b"ACGTACGTACGTACGTACGT";
        let none = Method {
            salt: SaltCorrection::None,
            na_molar: 0.0,
            ..Default::default()
        };
        let a = tm(seq, &none).expect("no correction needs no concentration");
        let also_none = Method {
            na_molar: f64::NAN,
            ..none
        };
        let b = tm(seq, &also_none).unwrap();
        approx(a.tm, b.tm, 1e-12, "na is not read when unused");
        assert!(
            none.describe().contains("no salt correction"),
            "{}",
            none.describe()
        );

        // And an ordinary concentration is still corrected, in the direction
        // it must be: the uncorrected 1 M number is the higher one.
        let fifty = Method::default();
        let c = tm(seq, &fifty).unwrap();
        assert!(
            a.tm > c.tm + 10.0,
            "1 M Na+ ({}) must sit far above 50 mM ({})",
            a.tm,
            c.tm
        );
    }

    /// PROVEN TO FAIL: with the reverse-complement fallback dropped from
    /// `step_fast`, `AC` resolves in one lookup and not the other.
    #[test]
    fn step_and_step_fast_agree_on_every_input() {
        // Two lookups is exactly the kind of duplication that drifts, and the
        // drift would be invisible: a wrong stack shifts every downstream
        // number by a degree and nothing looks broken. So the fast one is
        // checked against the slow one exhaustively rather than trusted.
        for t in [SANTALUCIA_1998, SANTALUCIA_2004] {
            for a in b"ACGTacgt" {
                for b in b"ACGTacgt" {
                    assert_eq!(
                        t.step(&[*a, *b]),
                        t.step_fast(*a, *b),
                        "{}{}",
                        *a as char,
                        *b as char
                    );
                }
            }
            // And the refusals agree too.
            for (a, b) in [(b'A', b'N'), (b'N', b'A'), (b'-', b'-'), (b'A', b'U')] {
                assert_eq!(t.step(&[a, b]), t.step_fast(a, b));
                assert_eq!(t.step_fast(a, b), None);
            }
        }
    }

    /// PROVEN TO FAIL: with the GC stack's stored entropy mistyped by 1.0
    /// cal/(K.mol) - one keystroke - this reports `GC: -1.92 vs -2.24`. Nothing
    /// else in the crate would have noticed; every Tm would simply be wrong.
    #[test]
    fn the_stored_enthalpies_and_entropies_reproduce_the_published_dg37_column() {
        // A free, high-value check on the transcription. SantaLucia & Hicks
        // (2004) print a ΔG°37 column beside ΔH and ΔS; it is not stored here,
        // it is derived, so if either stored column were mistyped this
        // reproduction would fail. A wrong stacking parameter is otherwise
        // invisible — it shifts every downstream number and nothing looks
        // broken, which is the failure this crate's module doc opens with.
        //
        // Published values, transcribed from the review's Table 1 (rounded to
        // 2 dp there, so the tolerance is 0.02).
        let published: &[(&str, f64)] = &[
            ("AA", -1.00),
            ("AT", -0.88),
            ("TA", -0.58),
            ("CA", -1.45),
            ("GT", -1.44),
            ("CT", -1.28),
            ("GA", -1.30),
            ("CG", -2.17),
            ("GC", -2.24),
            ("GG", -1.84),
        ];
        for (step, want) in published {
            let got = dg37_stacks(step.as_bytes(), &SANTALUCIA_2004).unwrap();
            approx(got, *want, 0.02, step);
        }
        // And ΔG°37 really is a different quantity from ΔH: the two orderings
        // are not the same, so this is not a check that trivially passes.
        let (h_at, _) = SANTALUCIA_2004.step(b"AT").unwrap();
        let (h_ta, _) = SANTALUCIA_2004.step(b"TA").unwrap();
        assert_eq!(h_at, h_ta, "AT and TA share an enthalpy");
        assert!(
            dg37_stacks(b"AT", &SANTALUCIA_2004).unwrap()
                < dg37_stacks(b"TA", &SANTALUCIA_2004).unwrap() - 0.2,
            "and are told apart only by entropy"
        );
    }

    #[test]
    fn the_two_dg_conventions_differ_by_exactly_the_initiation_terms() {
        // They are two functions rather than one with a flag because the
        // difference is large enough to move a threshold: 2.03 kcal/mol on the
        // 2004 table for a GC-ended oligo, against a 3'-end-stability limit of
        // -7.5.
        let seq = b"CGCGC";
        let stacks = dg37_stacks(seq, &SANTALUCIA_2004).unwrap();
        let duplex = dg37(seq, &SANTALUCIA_2004).unwrap();
        let init = SANTALUCIA_2004.init.0 - T37_KELVIN * SANTALUCIA_2004.init.1 / 1000.0;
        let end_gc = SANTALUCIA_2004.init_gc.0 - T37_KELVIN * SANTALUCIA_2004.init_gc.1 / 1000.0;
        approx(duplex - stacks, init + 2.0 * end_gc, 1e-9, "initiation");
        assert!(
            duplex - stacks > 1.9,
            "the gap must be big enough to matter: {}",
            duplex - stacks
        );
    }

    /// PROVEN TO FAIL: with `T37_KELVIN` moved, the achievable range moves with
    /// it and the most stable pentamer is no longer CGCGC. The assertion is
    /// about this crate's own scale, so it goes red whenever that scale moves -
    /// which is the point.
    #[test]
    fn no_pentamer_on_this_scale_reaches_minus_nine_kcal_per_mole() {
        // The literature 3'-end-stability threshold is -9 kcal/mol, and it
        // originates on **Breslauer's** 1986 parameters, which are
        // systematically more negative than SantaLucia's. Imported unchanged
        // onto this crate's tables it is arithmetically incapable of firing:
        // reaching -9 over four stacks needs a mean of -2.25 and the single
        // most stable stack here, GC, is -2.23.
        //
        // A production threshold that cannot fire is worse than no threshold,
        // because the report then carries a line saying the check was applied
        // and passed. This test exists so that anyone "correcting" pl-design's
        // -7.5 back to the literature value breaks the build here, where the
        // reason is written down.
        let bases = b"ACGT";
        let mut best = 0.0f64;
        let mut best_seq = Vec::new();
        let mut count = 0usize;
        for a in bases {
            for b in bases {
                for c in bases {
                    for d in bases {
                        for e in bases {
                            let p = [*a, *b, *c, *d, *e];
                            let g = dg37_stacks(&p, &SANTALUCIA_2004).unwrap();
                            count += 1;
                            if g < best {
                                best = g;
                                best_seq = p.to_vec();
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(count, 1024);
        assert_eq!(best_seq, b"CGCGC".to_vec(), "the most stable pentamer");
        assert!(
            best > -9.0,
            "the most stable pentamer reaches only {best}, so -9 kcal/mol cannot fire"
        );
        approx(best, -8.79, 0.01, "CGCGC");
    }

    #[test]
    fn a_dg_over_an_ambiguity_code_is_refused_like_a_tm() {
        assert!(matches!(
            dg37_stacks(b"ACNGT", &SANTALUCIA_2004),
            Err(TmError::NotUnambiguous(2, b'N'))
        ));
        assert_eq!(dg37(b"A", &SANTALUCIA_2004).unwrap_err(), TmError::TooShort);
    }

    #[test]
    fn case_is_folded_and_does_not_change_the_answer() {
        let m = Method::default();
        let a = tm(b"acgtacgtacgtacgtacgt", &m).unwrap();
        let b = tm(b"ACGTACGTACGTACGTACGT", &m).unwrap();
        approx(a.tm, b.tm, 1e-12, "case");
    }
}

/// A JSON line per oligo, for the differential test against Biopython.
///
/// Lives in the crate rather than in the CLI so the oracle drives exactly the
/// code the product uses, with no formatting layer in between to disagree.
pub fn tm_report_line(seq: &str, m: &Method) -> String {
    match tm(seq.as_bytes(), m) {
        Ok(t) => format!(
            "{{\"seq\": \"{seq}\", \"tm\": {:.6}, \"dh\": {:.6}, \"ds\": {:.6}, \"selfcomp\": {}}}",
            t.tm, t.dh, t.ds, t.self_complementary
        ),
        Err(e) => format!("{{\"seq\": \"{seq}\", \"error\": \"{e}\"}}"),
    }
}

#[cfg(test)]
mod convention_tests {
    use super::*;

    /// The direction `docs/PLAN.md` had backwards, pinned.
    ///
    /// Ordinary duplex: `C_T/4`. Self-complementary: `C_T`. Written the other
    /// way round, every palindrome came out about 8 °C low and every ordinary
    /// oligo about 4 °C high — numbers that look entirely plausible. Nothing
    /// hand-written here caught it; the differential against Biopython did.
    #[test]
    fn the_effective_concentration_is_ct_over_four_except_for_palindromes() {
        let m = Method::default();

        // Compute the denominator both ways and check which the code used.
        // NOT self-complementary -- and picking one is harder than it looks.
        // `ACGTACGTACGTACGTACGT` is a palindrome: `ACGT` is its own reverse
        // complement, so any number of repeats of it is too. That was the
        // first fixture here and it made this test assert the opposite of
        // what it says.
        let seq = b"GTAAAACGACGGCCAGTGAATT";
        assert!(
            !is_self_complementary(seq),
            "the fixture must not be a palindrome"
        );
        let t = tm(seq, &m).unwrap();
        let want_quarter = (t.dh * 1000.0) / (t.ds + R * (m.oligo_molar / 4.0).ln()) - 273.15;
        let want_whole = (t.dh * 1000.0) / (t.ds + R * m.oligo_molar.ln()) - 273.15;
        assert!(
            (t.tm - want_quarter).abs() < 1e-9,
            "an ordinary duplex must use C_T/4: got {}, C_T/4 gives {want_quarter}, C_T gives {want_whole}",
            t.tm
        );

        let pal = b"AAAACCCCGGGGTTTT"; // its own reverse complement
        let p = tm(pal, &m).unwrap();
        assert!(p.self_complementary);
        let want_whole = (p.dh * 1000.0) / (p.ds + R * m.oligo_molar.ln()) - 273.15;
        let want_quarter = (p.dh * 1000.0) / (p.ds + R * (m.oligo_molar / 4.0).ln()) - 273.15;
        assert!(
            (p.tm - want_whole).abs() < 1e-9,
            "a palindrome must use C_T: got {}, C_T gives {want_whole}, C_T/4 gives {want_quarter}",
            p.tm
        );
        // The two conventions really are different, so this is not a
        // distinction without a difference. The gap is R·ln(4) on the
        // denominator, which lands around 3 °C for an oligo this size -- large
        // enough to change a PCR and small enough to look like a rounding
        // choice, which is why it went unnoticed until an oracle looked.
        assert!(
            (want_whole - want_quarter).abs() > 1.5,
            "the two conventions differ by only {:.2} C",
            (want_whole - want_quarter).abs()
        );
    }

    /// Ordinary primers land in the band ordinary primers land in.
    ///
    /// A deliberately wide band, because the exact value depends on the salt
    /// and strand concentrations and a *narrow* range recalled from memory is
    /// this project's documented way of encoding a wrong expectation as a test.
    /// The numeric authority is the differential against Biopython; this only
    /// catches an answer that is not a primer temperature at all.
    #[test]
    fn ordinary_primers_melt_in_the_range_primers_melt_in() {
        let m = Method::default();
        for (name, seq) in [
            ("M13 forward", "GTAAAACGACGGCCAGTGAATT"),
            ("M13 reverse", "CAGGAAACAGCTATGACCATG"),
            ("T7", "TAATACGACTCACTATAGGG"),
            // SP6 is 18 nt and AT-rich; it genuinely sits near 39 °C at these
            // concentrations, which is why the band below is wide rather than
            // tightened around the others.
            ("SP6", "ATTTAGGTGACACTATAG"),
        ] {
            let t = tm(seq.as_bytes(), &m).unwrap();
            assert!(
                (30.0..=75.0).contains(&t.tm),
                "{name}: {:.1} C is not a primer temperature at all",
                t.tm
            );
        }
    }
}
