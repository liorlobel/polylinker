//! What a designed pair has to actually do.
//!
//! Each test here was run against a deliberately broken build of the code it
//! guards, and the failure recorded in its comment. A check that cannot fail
//! proves nothing, and "the crate did not exist at HEAD, so it failed to
//! compile" proves only that the crate did not exist.

use pl_design::{design, design_molecule, Constraints, DesignError, Mode, Region};

/// A deterministic pseudo-random template.
///
/// An LCG rather than `rand`, because this crate has no external dependencies
/// and because a fixture that changes between runs would make every assertion
/// below a different assertion. Numerical Recipes' constants; the sequence
/// itself carries no meaning, only reproducibility.
fn seq(n: usize, seed: u64) -> Vec<u8> {
    let mut s = seed;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        out.push(b"ACGT"[((s >> 24) & 3) as usize]);
    }
    out
}

fn ecori() -> &'static pl_enzymes::Enzyme {
    pl_enzymes::by_name("EcoRI").unwrap()
}

// ---------------------------------------------------------------------------
// The one that matters: does the pair amplify what was asked for?
// ---------------------------------------------------------------------------

/// A pair that does not amplify the selected region is the whole failure mode.
///
/// Verified by an **independent implementation**: `pl_clone::pcr` finds the
/// annealing sites itself, by exact 3' suffix matching, and builds the product
/// its own way. pl-design's arithmetic (tail + template span + reverse-
/// complemented tail) never touches it. Two implementations of one question is
/// the point.
///
/// PROVEN TO FAIL, at runtime and not merely to compile: with the forward
/// enumeration's window changed from `(s - flank, s)` to `(s, s + flank)` — the
/// reading that makes `flank` a bound on the 3' end rather than the 5' one, and
/// exactly what `Mode::Contain`'s doc warns about — this reports
/// `product 1199..1404 does not contain the selected 1001..1400`. The pair
/// passes every thermodynamic check and the band is the predicted size; what is
/// missing is the first 198 bases of the gene.
///
/// Recorded because a plausible-looking mutation did NOT fail it: changing the
/// reverse enumeration's `lo = hi - len + 1` to `lo = hi - len` merely makes
/// every reverse candidate one base longer, and since the footprint is read
/// back out of the span the product stays self-consistent and `pl_clone::pcr`
/// still agrees. A test can only see the bugs that change the answer.
#[test]
fn a_designed_pair_amplifies_the_region_that_was_asked_for() {
    let template = seq(3_000, 12);
    let region = Region::new(1_001, 1_400);
    let r = design(&template, false, region, &Constraints::default()).expect("a pair exists here");
    assert!(!r.pairs.is_empty());

    for p in &r.pairs {
        // 1. The product contains the whole selection.
        assert!(
            p.product_start <= region.start && p.product_end >= region.end,
            "product {}..{} does not contain the selected {}..{}",
            p.product_start,
            p.product_end,
            region.start,
            region.end
        );

        // 2. pl_clone::pcr, given only the two oligos and the template, makes
        //    the same molecule.
        let dseq = pl_clone::Dseq::new(&String::from_utf8_lossy(&template), false);
        let f = String::from_utf8_lossy(&p.forward.oligo()).to_string();
        let rv = String::from_utf8_lossy(&p.reverse.oligo()).to_string();
        let product = pl_clone::pcr(&f, &rv, &dseq).expect("a designed pair must simulate");
        assert_eq!(
            product.watson.len() as u64,
            p.product_bp,
            "pl-clone made {} bp, pl-design predicted {}",
            product.watson.len(),
            p.product_bp
        );

        // 3. And the amplicon really is the template's bases over that span.
        let want = String::from_utf8_lossy(
            &template[(p.product_start - 1) as usize..p.product_end as usize],
        )
        .to_string();
        assert_eq!(product.watson, want.to_ascii_uppercase());
        assert!(
            product.watson.contains(
                &String::from_utf8_lossy(
                    &template[(region.start - 1) as usize..region.end as usize]
                )
                .to_ascii_uppercase()
            ),
            "the selected bases are not in the product"
        );

        assert_eq!(p.pcr_check, Ok(p.product_bp), "the built-in cross-check");
    }
}

/// The same, across the origin of a plasmid — which is ordinary and is where
/// coordinate arithmetic goes wrong invisibly.
///
/// PROVEN TO FAIL: with `Region::len` changed to
/// `end.max(start) - end.min(start) + 1` — the form that avoids the underflow a
/// bare `end - start + 1` would hit on a wrapping region, and therefore the
/// plausible one — the length assertion fires at once: a 200 bp selection
/// measures 2,801 bases, the complement arc. That is the trap `seqedit.rs`
/// documents, and the number would look entirely reasonable on screen.
#[test]
fn an_origin_crossing_region_is_designed_for_the_arc_that_was_selected() {
    let template = seq(3_000, 77);
    // 100 bases before the origin and 100 after it.
    let region = Region::new(2_901, 100);
    assert!(region.wraps());
    assert_eq!(region.len(3_000), 200);

    let r = design(&template, true, region, &Constraints::default()).expect("a pair exists here");
    let p = &r.pairs[0];
    assert!(
        p.product_bp < 1_000,
        "a 200 bp selection must not give a {} bp product -- that is the complement arc",
        p.product_bp
    );
    // The product wraps too, so its own coordinates read end < start.
    assert!(
        p.product_end < p.product_start,
        "product {}..{}",
        p.product_start,
        p.product_end
    );
    assert_eq!(p.pcr_check, Ok(p.product_bp));
}

// ---------------------------------------------------------------------------
// The tail is not in the Tm
// ---------------------------------------------------------------------------

/// A restriction tail must not reach the reported Tm.
///
/// The design asserts three things at once: the reported Tm equals the
/// tail-free footprint's Tm exactly; it differs from the whole oligo's by a
/// margin the fixture makes obvious; and the whole-oligo number is present but
/// separately named.
///
/// PROVEN TO FAIL: with `to_primer` changed to compute `tm` over
/// `[tail, footprint].concat()` — the natural implementation, and the one
/// reached for when the oligo is already in hand — this reports
/// `reported 70.4 C, footprint alone 55.0 C`. Fifteen degrees, in the direction
/// that runs the anneal too hot.
#[test]
fn the_reported_tm_is_the_footprints_and_not_the_tailed_oligos() {
    let template = seq(3_000, 5);
    let region = Region::new(1_001, 1_400);
    let mut c = Constraints {
        tail_five: Some(pl_design::params::Tailspec {
            enzyme: ecori(),
            spacer: b"GCGGCCGC".to_vec(),
        }),
        ..Default::default()
    };
    // A tail on one side only, so the untailed reverse primer is a control in
    // the same run.
    c.tail_three = None;

    let r = design(&template, false, region, &c).expect("a pair exists here");
    let p = &r.pairs[0];
    let f = &p.forward;
    let tail = f
        .tail
        .as_ref()
        .expect("the forward primer carries the tail");
    assert_eq!(tail.bases, b"GCGGCCGCGAATTC".to_vec());

    let m = c.tm_method;
    let foot_only = pl_thermo::tm(&f.footprint, &m).unwrap().tm;
    let whole = pl_thermo::tm(&f.oligo(), &m).unwrap().tm;
    assert!(
        whole - foot_only > 8.0,
        "the fixture must make the difference obvious: {whole} vs {foot_only}"
    );
    assert!(
        (f.tm - foot_only).abs() < 1e-9,
        "reported {:.1} C, footprint alone {foot_only:.1} C",
        f.tm
    );
    assert!(
        (f.tm - whole).abs() > 8.0,
        "the reported Tm must not be the whole oligo's"
    );

    // The whole-oligo number is reported, and named apart.
    let full = f.tm_full.expect("a tailed primer reports its cycle-3 Tm");
    assert!((full - whole).abs() < 1e-9);
    assert!(p.reverse.tm_full.is_none(), "no tail, no second number");

    // %GC follows the same rule, and for the same reason.
    let foot_gc = pl_thermo::tm(&f.footprint, &m).unwrap().gc_percent;
    assert!((f.gc - foot_gc).abs() < 1e-9);

    // And the coordinates are the footprint's only. The tail pairs with
    // nothing on this molecule, so a span covering it would annotate bases the
    // primer does not match.
    assert_eq!((f.end - f.start + 1) as usize, f.footprint.len());
    assert_eq!(
        &template[(f.start - 1) as usize..f.end as usize].to_ascii_uppercase(),
        &f.footprint
    );
}

// ---------------------------------------------------------------------------
// A site inside the amplicon
// ---------------------------------------------------------------------------

/// An enzyme whose site occurs inside the product cannot be used, and the
/// designer must not offer it.
///
/// The failure this prevents is invisible until a gel: the digest works, the
/// ligation gives colonies, and the insert is short by an internal fragment.
///
/// PROVEN TO FAIL: with the internal-site verdict changed from a rejection to a
/// no-op, `expect_err` fires and five pairs come back, each of whose products
/// EcoRI cuts into three. The sibling test below covers the other half — that
/// the scan has to run over the finished **product** and not the template span.
#[test]
fn an_enzyme_that_cuts_inside_the_amplicon_is_refused() {
    let mut template = seq(3_000, 31);
    // A GAATTC squarely in the middle of the region.
    template[1_200..1_206].copy_from_slice(b"GAATTC");
    let region = Region::new(1_001, 1_400);

    let with_site = Constraints {
        tail_five: Some(pl_design::params::Tailspec {
            enzyme: ecori(),
            spacer: b"TTAAGG".to_vec(),
        }),
        ..Default::default()
    };
    let err = design(&template, false, region, &with_site)
        .expect_err("every pair carries an unusable EcoRI");
    match &err {
        DesignError::NoPair { tally, .. } => {
            assert!(
                tally.get(pl_design::Reason::InternalSite) > 0,
                "the refusal must name the internal site"
            );
        }
        other => panic!("{other}"),
    }
    assert!(err
        .to_string()
        .contains("the added restriction site already occurs in the product"));

    // The control: without a tail there is nothing to refuse, and the same
    // template designs cleanly. Otherwise this test would pass on a designer
    // that simply cannot design against this template at all.
    let clean = design(&template, false, region, &Constraints::default())
        .expect("without a tail the same template is fine");
    assert!(!clean.pairs.is_empty());
}

/// The extra site is in the **tail**, not the template — which is what makes
/// scanning the finished product rather than the template span non-negotiable.
///
/// A user pastes a linker they had lying around as the spacer, and it already
/// contains the site they are adding. The template is clean, so a scan over
/// `template[f.lo..=r.hi]` sees nothing at all; the product carries two copies
/// of the site at the 5' end and the digest lops the first one off.
///
/// PROVEN TO FAIL: with the product built as the template span alone — the
/// obvious shortcut, since both footprints are template substrings -
/// `expect_err` fires and five perfectly good-looking pairs come back.
#[test]
fn a_spacer_that_carries_the_site_itself_is_caught() {
    let template = seq(3_000, 61);
    let region = Region::new(1_001, 1_400);
    // The template has no EcoRI site of its own; only the spacer does.
    assert!(pl_core::iupac::find_all(b"GAATTC", &template, false).is_empty());

    let c = Constraints {
        tail_five: Some(pl_design::params::Tailspec {
            enzyme: ecori(),
            spacer: b"GAATTC".to_vec(),
        }),
        ..Default::default()
    };
    let err =
        design(&template, false, region, &c).expect_err("a spacer carrying the site is not usable");
    match &err {
        DesignError::NoPair { tally, .. } => {
            assert!(tally.get(pl_design::Reason::InternalSite) > 0, "{err}")
        }
        other => panic!("{other}"),
    }

    // The control: the same spacer with one base changed carries no site, and
    // the identical design succeeds. Without it, a designer that refused every
    // tailed design would pass.
    let ok = Constraints {
        tail_five: Some(pl_design::params::Tailspec {
            enzyme: ecori(),
            spacer: b"GAATTG".to_vec(),
        }),
        ..c
    };
    let r = design(&template, false, region, &ok).expect("one base makes it usable");
    assert!(!r.pairs.is_empty());
    assert_eq!(
        r.pairs[0].forward.tail.as_ref().unwrap().bases,
        b"GAATTGGAATTC".to_vec()
    );
}

/// The REVERSE tail, which had no coverage at all until a reviewer said so.
///
/// Every restriction test above uses `tail_five`; `tail_three` appeared in this
/// file only as `c.tail_three = None`. A mutation study on a scratch copy found
/// three separate ways to break the reverse tail that left all 1,011 workspace
/// tests green, including the one this whole feature is about: synthesising the
/// recognition site onto the **3'** end of the reverse primer, where it neither
/// primes nor adds a site.
///
/// PROVEN TO FAIL, each mutation applied on a scratch copy and run against this
/// test:
///
/// 1. `Primer::oligo()` returning `[footprint, tail]` for `Side::Rev` — the
///    site on the wrong end. Fails the `rev.oligo()` assertion.
/// 2. The product built with `&t.bases` instead of
///    `&reverse_complement(&t.bases)` for the reverse tail. Fails on the
///    product's 3' end.
/// 3. `engineered_positions` dropping `t.spacer.len()` from the reverse
///    position. Fails as `expect`: the site the design put there stops being
///    excused, so the design is refused for its own tail.
///
/// BsaI rather than EcoRI on purpose: a palindromic site reads the same in both
/// orientations, so mutation 2 is invisible under one. An orientation bug needs
/// a non-palindromic enzyme to have anything to be wrong about.
#[test]
fn a_reverse_tail_is_built_on_the_reverse_primers_five_prime_end() {
    let bsai = pl_enzymes::by_name("BsaI").expect("BsaI ships");
    assert!(
        !pl_core::iupac::is_palindrome_masks(bsai.site.as_bytes()),
        "the premise: an orientation bug needs a non-palindromic site to show"
    );
    let template = seq(3_000, 77);
    let region = Region::new(1_001, 1_400);
    let spacer = b"TTAA".to_vec();

    let c = Constraints {
        tail_three: Some(pl_design::params::Tailspec {
            enzyme: bsai,
            spacer: spacer.clone(),
        }),
        ..Default::default()
    };
    let r = design(&template, false, region, &c).expect("BsaI is absent from this template");
    assert!(!r.pairs.is_empty());

    for p in &r.pairs {
        let rev = &p.reverse;
        let tail = rev.tail.as_ref().expect("the reverse primer carries it");
        assert!(p.forward.tail.is_none(), "only --add-3 was asked for");

        // 1. spacer, then site, then footprint -- in that order, 5'->3'.
        let mut want = spacer.clone();
        want.extend_from_slice(bsai.site.as_bytes());
        assert_eq!(tail.bases, want, "the tail is the spacer then the site");
        let mut oligo = want.clone();
        oligo.extend_from_slice(&rev.footprint);
        assert_eq!(
            rev.oligo(),
            oligo,
            "tail on the 5' end, footprint on the 3'"
        );

        // 2. The footprint really is the reverse complement of the template at
        //    the reported coordinates, so the tail did not eat into it.
        let span = &template[(rev.start - 1) as usize..rev.end as usize];
        assert_eq!(
            rev.footprint,
            pl_core::iupac::reverse_complement(&span.to_ascii_uppercase()),
            "the reverse footprint is rc(template[start..end])"
        );

        // 3. On the product's top strand the reverse tail appears reverse-
        //    complemented, at the 3' end: rc(site) then rc(spacer).
        let dseq = pl_clone::Dseq::new(&String::from_utf8_lossy(&template), false);
        let f = String::from_utf8_lossy(&p.forward.oligo()).to_string();
        let rv = String::from_utf8_lossy(&rev.oligo()).to_string();
        let product = pl_clone::pcr(&f, &rv, &dseq).expect("an independent simulation");
        let bytes = product.watson.as_bytes().to_ascii_uppercase();
        assert_eq!(
            product.watson.len() as u64,
            p.product_bp,
            "pl-clone agrees on the length"
        );
        let want_end = pl_core::iupac::reverse_complement(&want);
        assert!(
            bytes.ends_with(&want_end),
            "the product's 3' end must read rc(site) then rc(spacer): {}",
            String::from_utf8_lossy(&bytes[bytes.len() - 16..])
        );

        // 4. The engineered site is where the arithmetic says, on the minus
        //    strand, and is NOT reported as an unintended one.
        let want_at = (bytes.len() - spacer.len() - bsai.site.len() + 1) as u64;
        let rc_site = pl_core::iupac::reverse_complement(bsai.site.as_bytes());
        assert_eq!(
            pl_core::iupac::find_all(&rc_site, &bytes, false),
            vec![want_at],
            "exactly one BsaI site, at the engineered position, on the minus strand"
        );
        assert!(
            pl_core::iupac::find_all(bsai.site.as_bytes(), &bytes, false).is_empty(),
            "and none on the plus strand"
        );
        assert!(
            pl_design::tail::internal_sites(&bytes, bsai, &[want_at]).is_empty(),
            "the site it put there is not an unintended one"
        );
    }
}

/// Two enzymes with the same palindromic site are the same primer-dimer, and
/// the warning has to key on the SITE rather than on the name.
///
/// SmaI and XmaI are the one palindromic isoschizomer pair in the 58-enzyme
/// table — the other three, AarI/PaqCI, BsmBI/Esp3I and BspQI/SapI, are Type
/// IIS and correctly not warned about, since their sites are not palindromes.
/// Both write CCCGGG, so `--add-5 SmaI --add-3 XmaI` ships two exactly
/// complementary 5' ends: a designed primer-dimer. The remedy the warning
/// itself gives is what steers a user into that case.
///
/// PROVEN TO FAIL: with the shipped `a.enzyme.name == b.enzyme.name`, the
/// XmaI half asserts 1 and gets 0. The SmaI/SmaI half passes under both, which
/// is exactly why a same-name test alone proves nothing here.
#[test]
fn two_isoschizomers_are_the_same_designed_primer_dimer_as_one_enzyme_twice() {
    let smai = pl_enzymes::by_name("SmaI").expect("SmaI ships");
    let xmai = pl_enzymes::by_name("XmaI").expect("XmaI ships");
    assert_eq!(smai.site, xmai.site, "the premise: isoschizomers");
    assert_ne!(smai.name, xmai.name);
    assert!(pl_core::iupac::is_palindrome_masks(smai.site.as_bytes()));

    let template = seq(3_000, 17);
    let region = Region::new(1_001, 1_400);
    let bamhi = pl_enzymes::by_name("BamHI").expect("BamHI ships");
    // The premise, checked rather than assumed: none of the three sites occurs
    // in the template, so nothing below is refused for an internal site and
    // every count really is about the primer-dimer warning. Seed 41 has a
    // GGATCC at 1113 and silently turned the control into a refusal.
    for e in [smai, xmai, bamhi] {
        assert!(
            pl_core::iupac::find_all(e.site.as_bytes(), &template, false).is_empty(),
            "{} site {} is in the fixture",
            e.name,
            e.site
        );
    }
    let count = |b: &'static pl_enzymes::Enzyme| {
        let c = Constraints {
            tail_five: Some(pl_design::params::Tailspec {
                enzyme: smai,
                spacer: b"TTAA".to_vec(),
            }),
            tail_three: Some(pl_design::params::Tailspec {
                enzyme: b,
                spacer: b"TTAA".to_vec(),
            }),
            ..Default::default()
        };
        let r = design(&template, false, region, &c).expect("these sites are absent here");
        assert!(!r.pairs.is_empty());
        r.pairs[0]
            .warnings
            .iter()
            .filter(|w| w.contains("designed primer-dimer"))
            .count()
    };
    assert_eq!(count(smai), 1, "the same enzyme twice");
    assert_eq!(
        count(xmai),
        1,
        "and two isoschizomers, which is the same DNA"
    );

    // The control: two enzymes whose sites differ must NOT be warned about, or
    // the assertions above would hold for a warning that always fires.
    assert_ne!(smai.site, bamhi.site);
    assert_eq!(count(bamhi), 0);
}

/// The product window bounds the AMPLICON, tails included — the molecule that
/// runs on the gel, and the one MIQE requires to be reported.
///
/// PROVEN TO FAIL: with the window applied to the template span, as shipped —
/// `want_lo = f.lo + product_min - 1` — every reported amplicon overshoots by
/// the tail length and the first assertion fires with `product 173 bp is
/// outside the stated 140-150`. The same arithmetic under `--rt`, whose whole
/// point is a 70-150 bp qPCR amplicon, made a pair capped at 150 come out at
/// 174.
#[test]
fn the_product_window_bounds_the_amplicon_and_not_the_template_span() {
    let template = seq(2_000, 23);
    let region = Region::new(500, 1_200);
    let bamhi = pl_enzymes::by_name("BamHI").expect("BamHI ships");
    let spacer = b"TTGGCA".to_vec();
    let c = Constraints {
        mode: Mode::Within,
        product_min: 140,
        product_max: 150,
        tail_five: Some(pl_design::params::Tailspec {
            enzyme: ecori(),
            spacer: spacer.clone(),
        }),
        tail_three: Some(pl_design::params::Tailspec {
            enzyme: bamhi,
            spacer: spacer.clone(),
        }),
        ..Default::default()
    };
    let tails = (spacer.len() * 2 + ecori().site.len() + bamhi.site.len()) as u64;
    assert_eq!(tails, 24, "the premise: 24 nt of tail on every product");

    let r = design(&template, false, region, &c).expect("a pair exists here");
    assert!(!r.pairs.is_empty());
    for p in &r.pairs {
        assert!(
            (140..=150).contains(&p.product_bp),
            "product {} bp is outside the stated 140-150",
            p.product_bp
        );
        // pl-clone, independently, on the same two oligos.
        let dseq = pl_clone::Dseq::new(&String::from_utf8_lossy(&template), false);
        let f = String::from_utf8_lossy(&p.forward.oligo()).to_string();
        let rv = String::from_utf8_lossy(&p.reverse.oligo()).to_string();
        let sim = pl_clone::pcr(&f, &rv, &dseq).expect("simulates");
        assert_eq!(sim.watson.len() as u64, p.product_bp);
    }
    // The constraint line and the amplicon line cannot disagree, because the
    // line now says which quantity it bounds.
    let text = r.text("fixture");
    assert!(
        text.contains("product 140-150 bp INCLUDING the 24 nt of tail"),
        "{text}"
    );

    // And a window the tails alone cannot fit inside is named arithmetically
    // rather than coming out as "0 pairs were built".
    let impossible = Constraints {
        product_min: 20,
        product_max: 24,
        ..c
    };
    assert_eq!(
        design(&template, false, region, &impossible).unwrap_err(),
        DesignError::TailsExceedProduct {
            tail_bp: 24,
            product_max: 24
        }
    );
}

// ---------------------------------------------------------------------------
// Off-target
// ---------------------------------------------------------------------------

/// A candidate that also anneals elsewhere on the open molecule is rejected.
///
/// The fixture is a template with a **real duplication**, because on random
/// sequence this check never fires: 4^12 against ~6,000 both-strand positions
/// in a 3 kb molecule gives an expected 1.5e-3 spurious hits per candidate. A
/// property test over random sequence would pass whether the code worked or
/// not — which is exactly what `pl-primer`'s own
/// `a_primer_that_binds_twice_reports_both` avoids by concatenating a repeat.
///
/// PROVEN TO FAIL: with `accept_specificity` returning `true` unconditionally,
/// this reports `5 pairs offered from a duplicated block; every one of them
/// primes in two places` — pairs `pl_clone::pcr` would itself refuse with
/// `not specific`.
#[test]
fn a_candidate_that_binds_twice_on_this_template_is_rejected() {
    // 3 kb of random sequence with bases 1,001-1,400 repeated at 2,001-2,400.
    let mut template = seq(3_000, 99);
    let block: Vec<u8> = template[1_000..1_400].to_vec();
    template[2_000..2_400].copy_from_slice(&block);
    let region = Region::new(1_001, 1_400);

    let c = Constraints {
        // Within, so every candidate is drawn from the duplicated block itself
        // and therefore has a second site by construction.
        mode: Mode::Within,
        ..Default::default()
    };
    let refused = design(&template, false, region, &c);
    match refused {
        Err(DesignError::NoCandidate { tally, .. }) => {
            assert!(
                tally.get(pl_design::Reason::OffTarget) > 0,
                "the duplication must be what refused them"
            );
        }
        Err(other) => panic!("refused for the wrong reason: {other}"),
        Ok(r) => panic!(
            "{} pairs offered from a duplicated block; \
             every one of them primes in two places",
            r.pairs.len()
        ),
    }

    // The control, and it is what makes the assertion above mean something:
    // the identical search on the same template with the duplication removed
    // succeeds. Without this, a designer that refused everything would pass.
    let unique = seq(3_000, 99);
    let ok = design(&unique, false, region, &c).expect("the same search without the repeat");
    assert!(!ok.pairs.is_empty());
    assert!(ok.specificity.ran && ok.specificity.used_index);

    // And the scan really is what rejected them: turning it off returns pairs
    // from the duplicated template, with the report saying the check was
    // skipped.
    let unchecked = Constraints {
        specificity: false,
        ..c
    };
    let lax = design(&template, false, region, &unchecked).expect("no scan, no rejection");
    assert!(!lax.pairs.is_empty());
    assert!(!lax.specificity.ran);
    assert!(lax
        .warnings
        .iter()
        .any(|w| w.contains("off-target scan was skipped")));
}

// ---------------------------------------------------------------------------
// The RT-PCR preset
// ---------------------------------------------------------------------------

/// The RT-PCR preset gives a shorter amplicon than the PCR preset on the same
/// input, and carries the bacteria caveat.
///
/// PROVEN TO FAIL: with `Constraints::rt_pcr` leaving `product_min`/
/// `product_max` alone — the plausible "the preset is only about Tm" reading -
/// `assertion failed: (70..=150).contains(&rt.pairs[0].product_bp)` fires and
/// the two presets become indistinguishable on the same input.
#[test]
fn the_rt_pcr_preset_makes_a_shorter_amplicon_than_the_pcr_preset() {
    let template = seq(3_000, 4);
    let region = Region::new(1_001, 1_500);

    let pcr = design(&template, false, region, &Constraints::default()).expect("pcr");
    let rt = design(&template, false, region, &Constraints::default().rt_pcr()).expect("rt");

    assert!(
        rt.pairs[0].product_bp < pcr.pairs[0].product_bp,
        "RT {} bp is not shorter than PCR {} bp",
        rt.pairs[0].product_bp,
        pcr.pairs[0].product_bp
    );
    assert!((70..=150).contains(&rt.pairs[0].product_bp));
    assert_eq!(
        rt.mode,
        Mode::Within,
        "the preset moves both primers inside"
    );
    assert_eq!(pcr.mode, Mode::Contain);

    // The caveat is unconditional, is in the answer rather than on stderr, and
    // says the thing that cannot be softened.
    let caveat = rt
        .warnings
        .iter()
        .find(|w| w.contains("RT-PCR"))
        .expect("the RT-PCR caveat must be in the report");
    assert!(caveat.contains("CANNOT exclude genomic DNA"), "{caveat}");
    assert!(caveat.contains("no introns"), "{caveat}");
    assert!(caveat.contains("no-RT control"), "{caveat}");
    assert!(
        !pcr.warnings.iter().any(|w| w.contains("RT-PCR")),
        "and it does not appear where it does not apply"
    );
    // It survives into JSON, so a --json consumer cannot lose it.
    assert!(rt.json("t").contains("CANNOT exclude genomic DNA"));
}

// ---------------------------------------------------------------------------
// Refusals
// ---------------------------------------------------------------------------

/// A region too short to hold two primers, refused with both numbers named.
///
/// PROVEN TO FAIL: with the `RegionTooShort` guard removed, the run answers
/// `no primer meets these constraints. 6 candidate oligos were built and every
/// one was rejected` — technically true, and useless: it names a symptom rather
/// than the arithmetic, and offers the wrong remedy.
#[test]
fn a_region_too_short_for_two_primers_is_refused_with_the_arithmetic() {
    let template = seq(3_000, 8);
    let c = Constraints {
        mode: Mode::Within,
        ..Default::default()
    };
    let err = design(&template, false, Region::new(400, 418), &c).unwrap_err();
    match err {
        DesignError::RegionTooShort {
            bp,
            shortest_product,
            len_min,
        } => {
            assert_eq!(bp, 19);
            assert_eq!(len_min, 18);
            // Computed as 2 * len_min + 1, never written in: lowering --len
            // must change it.
            assert_eq!(shortest_product, 37);
        }
        other => panic!("{other}"),
    }
    let msg = err.to_string();
    assert!(msg.contains("19 bases"), "{msg}");
    assert!(msg.contains("37 bases"), "{msg}");
    assert!(msg.contains("--mode contain"), "{msg}");

    // The number tracks the constraint rather than being a literal.
    let shorter = Constraints { len_min: 15, ..c };
    match design(&template, false, Region::new(400, 418), &shorter) {
        Err(DesignError::RegionTooShort {
            shortest_product, ..
        }) => assert_eq!(shortest_product, 31),
        other => panic!("a 19 bp region still cannot hold two 15-mers: {other:?}"),
    }
}

/// An annotation track and a sequence-absent record are refused in their own
/// terms, from the one place that owns the sentences.
///
/// PROVEN TO FAIL: with `design_molecule`'s two guards removed, both calls fall
/// through to `design`, which sees an empty slice and answers
/// "this file declares 0 bases and carries none of them" for a file that
/// declares nothing and carries 41 features — the gate reporting a symptom of a
/// question that should never have been asked, which is the failure
/// `seqedit::Editability` was written against.
#[test]
fn an_annotation_track_is_refused_in_its_own_terms() {
    let mut mol = pl_core::Molecule {
        name: "orphan".into(),
        ..Default::default()
    };
    for i in 0..41u64 {
        let mut f = pl_core::Feature::new(format!("f{i}"), "misc_feature");
        f.segments
            .push(pl_core::Segment::new(i * 10 + 1, i * 10 + 9));
        mol.features.push(f);
    }
    assert!(mol.is_annotation_track());
    let err = design_molecule(&mol, Region::new(1, 100), &Constraints::default()).unwrap_err();
    assert_eq!(err, DesignError::AnnotationTrack { features: 41 });
    let msg = err.to_string();
    assert!(msg.contains("41 features and no bases"), "{msg}");
    assert!(msg.contains("nothing here to design against"), "{msg}");

    // The other no-bases file is a different file and gets a different
    // sentence.
    let absent = pl_core::Molecule {
        name: "NC_000913".into(),
        declared_len: Some(4_641_652),
        ..Default::default()
    };
    let err = design_molecule(&absent, Region::new(1, 100), &Constraints::default()).unwrap_err();
    assert_eq!(
        err,
        DesignError::SequenceAbsent {
            declared: 4_641_652
        }
    );
    assert!(err.to_string().contains("4641652 bases"));
}

/// A region that runs backwards on a line is not an origin crossing.
///
/// PROVEN TO FAIL: with the `region.wraps() && !circular` guard disabled, the
/// linear case designs happily against a region that does not exist, and the
/// `assert_eq!` on the error fires.
#[test]
fn a_backwards_region_on_a_line_is_refused_and_on_a_circle_is_not() {
    let template = seq(3_000, 21);
    let c = Constraints::default();
    assert_eq!(
        design(&template, false, Region::new(2_901, 100), &c).unwrap_err(),
        DesignError::BackwardsOnALine {
            start: 2_901,
            end: 100
        }
    );
    assert!(design(&template, true, Region::new(2_901, 100), &c).is_ok());
    // And a coordinate past the end is refused whichever the topology.
    assert!(matches!(
        design(&template, true, Region::new(1, 9_000), &c),
        Err(DesignError::OutsideTemplate { .. })
    ));
}

/// An ambiguity code inside the target is refused rather than scored around.
///
/// PROVEN TO FAIL: with the ambiguity gate never firing, `unwrap_err` gets an
/// `Ok(Report)` back - primers designed around an N, with a Tm that is a
/// different, shorter oligo's.
#[test]
fn an_ambiguity_code_in_the_target_is_refused_the_way_pl_thermo_refuses_one() {
    let mut template = seq(3_000, 14);
    template[1_100] = b'N';
    let err = design(
        &template,
        false,
        Region::new(1_001, 1_400),
        &Constraints::default(),
    )
    .unwrap_err();
    assert_eq!(
        err,
        DesignError::AmbiguousTarget {
            position: 1_101,
            base: b'N'
        }
    );
    let msg = err.to_string();
    assert!(msg.contains("'N' at 1101"), "{msg}");
    assert!(msg.contains("a different oligo's"), "{msg}");
}

// ---------------------------------------------------------------------------
// The criteria whose measurements say how to test them
// ---------------------------------------------------------------------------

/// The dinucleotide-repeat filter, tested on a template that has one.
///
/// The measurement is the reason for the fixture: on random 50%-GC 20-mers a
/// ≥5-unit dinucleotide repeat occurs in 0.010% of sequences, so against random
/// sequence this filter is very nearly a check that cannot fail. Its whole
/// value is on real templates — microsatellites, poly-(CA) tracts,
/// low-complexity intergenic regions in AT-rich bacteria — so the fixture
/// carries an actual `(CA)12` tract.
///
/// PROVEN TO FAIL: with `dinuc_units` returning 0 unconditionally, the first
/// assertion fires with `the (CA)12 tract must reject something` — nothing in
/// the tally, and candidates drawn straight out of the tract on offer.
#[test]
fn a_real_dinucleotide_tract_is_rejected_where_random_sequence_would_prove_nothing() {
    let mut template = seq(3_000, 55);
    // (CA)12 at 1,201, well inside the region.
    //
    // CA and not AT, and that is a finding rather than a preference: the gate
    // runs Tm before composition, and a pure (AT)n 20-mer is 0% GC and fails
    // the Tm window first, so the dinucleotide filter never sees it and its
    // tally reads zero. A (CA)n tract is 50% GC, melts inside the window, and
    // therefore reaches the criterion this test is about. Poly-(CA) is also the
    // tract that actually turns up in bacterial intergenic sequence.
    for i in 0..12 {
        template[1_200 + 2 * i] = b'C';
        template[1_201 + 2 * i] = b'A';
    }
    let c = Constraints {
        mode: Mode::Within,
        ..Default::default()
    };
    let r = design(&template, false, Region::new(1_001, 1_400), &c).expect("pairs exist");
    assert!(
        r.tally.get(pl_design::Reason::DinucRepeat) > 0,
        "the (CA)12 tract must reject something"
    );
    // And nothing offered carries five units of it. Overlapping the tract by
    // four units is allowed and is not a bug -- the criterion is the repeat
    // length, not the coordinate -- so this asserts the criterion rather than a
    // proxy for it.
    for p in &r.pairs {
        for pr in [&p.forward, &p.reverse] {
            let s = String::from_utf8_lossy(&pr.footprint).to_string();
            assert!(
                !s.contains("CACACACACA") && !s.contains("ACACACACAC"),
                "{}..{} carries five units of the tract: {s}",
                pr.start,
                pr.end
            );
        }
    }
}

/// The 3'-end stability threshold rejects the extreme case and accepts the
/// other one, on this project's own scale.
///
/// The literature value is −9 kcal/mol and is arithmetically incapable of
/// firing here; `pl_thermo::no_pentamer_on_this_scale_reaches_minus_nine_kcal_per_mole`
/// pins that. This is the other half: the value that ships does fire.
///
/// PROVEN TO FAIL: with `DG_THREE_PRIME` set back to the literature -9.0, this
/// reports `CGCGC at -8.792520000000003 must be refused by -9` — a threshold
/// arithmetically incapable of firing, printed in the report as a check that
/// was applied and passed.
#[test]
fn the_three_prime_stability_threshold_rejects_cgcgc_and_accepts_tatat() {
    let t = Constraints::DG_TABLE;
    let limit = Constraints::DG_THREE_PRIME;
    let over = pl_thermo::dg37_stacks(b"CGCGC", &t).unwrap();
    let under = pl_thermo::dg37_stacks(b"TATAT", &t).unwrap();
    assert!(over <= limit, "CGCGC at {over} must be refused by {limit}");
    assert!(under > limit, "TATAT at {under} must be accepted");
    // And the gap either side of the threshold is real, so this is not a
    // distinction hiding in the rounding.
    assert!(over < limit - 1.0 && under > limit + 4.0);
}

/// The Tm window is stated on the model's own salt scale, not on a bench one.
///
/// Copying the familiar 57-63 °C window onto a 50 mM Na⁺ model selects primers
/// about five degrees hotter than intended, and `pl_thermo::anneal`'s advice
/// inherits the same offset — so a user reading "Tm 60, Taq Ta 55" is 5 °C off
/// at the bench with nothing on screen to say so.
///
/// PROVEN TO FAIL: with `TM_OPT` raised to 60.0 and the window to 57-63, this
/// fires with `55.4 C at 50 mM sits outside 57.0-63.0`.
#[test]
fn the_tm_window_is_stated_on_the_models_own_salt_scale() {
    let c = Constraints::default();
    // An ordinary 20-mer that a bench protocol would call a 60 °C primer.
    let oligo = b"ACGTGCATGCATGCATCGTA";
    let here = pl_thermo::tm(oligo, &c.tm_method).unwrap().tm;
    let bench = pl_thermo::tm(
        oligo,
        &pl_thermo::Method {
            na_molar: 150e-3,
            ..c.tm_method
        },
    )
    .unwrap()
    .tm;
    assert!(
        (bench - here - 5.29).abs() < 0.1,
        "the offset this window is built around has moved: {here} vs {bench}"
    );
    assert!(
        (c.tm_min..=c.tm_max).contains(&here),
        "{here:.1} C at 50 mM sits outside {:.1}-{:.1}",
        c.tm_min,
        c.tm_max
    );
    // And the method line says which scale, so the number can be reconciled.
    assert!(c.tm_method.describe().contains("50 mM Na+"));
}

/// %GC is reported and does not reject, which is what keeps AT-rich bacteria
/// designable.
///
/// PROVEN TO FAIL: with the `gc_hard` guard in `evaluate` replaced by an
/// unconditional bound, `expect("AT-rich is designable")` fires on a
/// `NoCandidate` whose tally reads 1,357 rejected on Tm and 424 on GC — the
/// organism excluded rather than the design.
#[test]
fn gc_is_reported_and_does_not_gate_unless_asked() {
    // ~30% GC, which is where a hard 40-60% band stops being a preference.
    let mut s = 7u64;
    let template: Vec<u8> = (0..3_000)
        .map(|_| {
            s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            let r = (s >> 33) % 10;
            if r < 3 {
                b"GC"[((s >> 20) & 1) as usize]
            } else {
                b"AT"[((s >> 20) & 1) as usize]
            }
        })
        .collect();
    let c = Constraints {
        // The physical constraint has to be reachable on an AT-rich template,
        // so this widens LENGTH -- which is the order the tally advises.
        len_max: 40,
        // Narrower than the default only to keep this test's runtime sane; the
        // criterion under test does not depend on it.
        flank: 40,
        ..Default::default()
    };
    let r = design(&template, false, Region::new(1_001, 1_400), &c).expect("AT-rich is designable");
    assert!(!r.pairs.is_empty());
    assert_eq!(r.tally.get(pl_design::Reason::Gc), 0, "GC never rejected");
    assert!(r.constraints.contains("reported, not a gate"));
    // Some of what it offers really is outside the band, so the softness is
    // doing work rather than being untested.
    assert!(
        r.pairs
            .iter()
            .any(|p| p.forward.gc < 40.0 || p.reverse.gc < 40.0),
        "an AT-rich template must yield AT-rich primers"
    );

    // Opting in turns it into a gate, and the report says so.
    let strict = Constraints { gc_hard: true, ..c };
    let hard = design(&template, false, Region::new(1_001, 1_400), &strict);
    match hard {
        Err(DesignError::NoCandidate { tally, .. }) => {
            assert!(tally.get(pl_design::Reason::Gc) > 0)
        }
        Ok(r) => assert!(r.pairs.iter().all(|p| p.forward.gc >= 40.0)),
        Err(e) => panic!("{e}"),
    }
}

/// Every reported pair carries its terms, so the total can be taken apart.
///
/// PROVEN TO FAIL: with the `gc` term dropped from the returned breakdown,
/// the parts no longer add to the whole and the first assertion fires.
#[test]
fn the_penalty_can_be_decomposed_into_the_terms_that_made_it() {
    let template = seq(3_000, 3);
    let r = design(
        &template,
        false,
        Region::new(1_001, 1_400),
        &Constraints::default(),
    )
    .unwrap();
    for p in &r.pairs {
        let sum: f64 = p.terms.iter().map(|(_, v)| v).sum();
        assert!(
            (sum - p.penalty).abs() < 1e-9,
            "the parts must add to the whole: {sum} vs {}",
            p.penalty
        );
        assert_eq!(p.terms.len(), 8);
        assert_eq!(p.terms[0].0, "delta_tm");
    }
}
