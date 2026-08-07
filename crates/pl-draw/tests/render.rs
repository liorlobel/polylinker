//! One scene, written as SVG and as pixels, for resvg to judge.
//!
//! Every other check on the rasterizer is a property: this pixel is dark, that
//! area is right, this colour parses. None of them can say the *picture* is
//! right. resvg renders the SVG this crate already emits — from the same font
//! binary this crate fills outlines from, forced with `skip_system_fonts` — and
//! `reference/python/tests/xcheck_render.py` compares the two images.
//!
//! That single comparison covers arc flattening, winding, stroke construction,
//! antialiasing, glyph decoding, glyph placement, the baseline constant, the
//! anchor arithmetic and colour parsing at once, against an implementation that
//! shares nothing with this one.
//!
//! # Two figures, and what the second one buys
//!
//! **This is an oracle for the RASTERIZER, not for the geometry.** resvg is
//! handed the SVG this crate emitted, so a scene that is wrong is wrong in both
//! images and the comparison passes: moving an arrowhead's tip 2 units was
//! tried, and all four comparisons stayed clean. What it does catch is our
//! rasterizer drawing a correct scene differently from an independent renderer
//! — half a unit on `raster`'s baseline constant fails eight checks across
//! both figures.
//!
//! So the linear figure is here because it is a different WORKLOAD, not because
//! it is a different scene: long thin boxes, concave pentagons with barbs
//! stepping outside the band, near-horizontal hairlines a pixel high, and rows
//! of small text packed tight. The ring has arcs, thick strokes and sparse
//! text and exercises almost none of that. The geometry is asserted in
//! `src/tests.rs` and `src/linear.rs`, where the scene can be read directly.

use std::fs;
use std::path::PathBuf;

use pl_core::{Feature, Molecule, Segment, Strand, Topology};

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("render")
}

/// A plasmid with the things that are hard to draw: features on both strands,
/// so both arrowhead directions and both windings appear; labels long enough to
/// need leader lines; and a name for the centre title, which is the only bold
/// text on the figure.
fn molecule() -> Molecule {
    let mut seq = Vec::new();
    for i in 0..2400u32 {
        seq.push(b"ACGT"[(i as usize * 7 + i as usize / 13) % 4]);
    }
    let feat = |name: &str, start: u64, end: u64, rev: bool| {
        let mut f = Feature::new(name, "CDS");
        f.strand = if rev {
            Strand::Reverse
        } else {
            Strand::Forward
        };
        f.segments = vec![Segment::new(start, end)];
        f
    };
    Molecule {
        name: "pRASTER".into(),
        seq,
        topology: Topology::Circular,
        features: vec![
            feat("AmpR", 100, 900, false),
            feat("ori", 1000, 1500, true),
            feat("lacZ-alpha", 1600, 2000, false),
            feat("T7 promoter", 2100, 2160, true),
        ],
        ..Default::default()
    }
}

/// One figure to write: what to call it, what to draw, and what to cut it with.
type Figure = (&'static str, Molecule, Vec<(String, u64)>);

/// The same molecule as a line, with cut sites on it.
///
/// Linear, so `pl_draw::scene` builds the track, and with sites, because the
/// ticks and the stacked label rows are most of what the linear figure draws
/// that the circular one does not.
fn strand() -> Molecule {
    let mut m = molecule();
    m.name = "pRASTER linear".into();
    m.topology = Topology::Linear;
    m
}

#[test]
fn write_both_figures_both_ways_for_the_resvg_cross_check() {
    let d = dir();
    fs::create_dir_all(&d).expect("a place to write");

    let figures: [Figure; 2] = [
        ("map", molecule(), Vec::new()),
        (
            "linear",
            strand(),
            vec![
                ("EcoRI".to_string(), 240),
                ("BamHI".to_string(), 1_180),
                ("HindIII".to_string(), 2_260),
            ],
        ),
    ];

    for (stem, mol, sites) in figures {
        let opts = pl_draw::Options {
            sites,
            ..Default::default()
        };
        // ONE scene, rendered twice. Rebuilding it for each back end would let
        // the two drift and the comparison would still pass.
        let (scene, _) = pl_draw::scene(&mol, opts);
        for scale in [1u32, 4] {
            let (img, report) = pl_draw::raster::draw(&scene, f64::from(scale), [255, 255, 255]);
            assert!(
                report.unparsed_colours.is_empty() && report.unencodable.is_empty(),
                "{stem} did not draw cleanly at {scale}x: {report:?}"
            );
            fs::write(
                d.join(format!("{stem}@{scale}x.png")),
                pl_draw::png::encode(&img, None),
            )
            .expect("the png");
        }
        fs::write(d.join(format!("{stem}.svg")), pl_draw::svg_of(&scene)).expect("the svg");
    }
    fs::write(
        d.join("SCALES"),
        "1
4
",
    )
    .expect("the manifest");
    // Named here rather than hard-coded in the checker, so a third figure is
    // one line in one file and the gate picks it up.
    fs::write(
        d.join("FIGURES"),
        "map
linear
",
    )
    .expect("the manifest");
}
