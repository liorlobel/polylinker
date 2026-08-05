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

#[test]
fn write_a_figure_both_ways_for_the_resvg_cross_check() {
    let d = dir();
    fs::create_dir_all(&d).expect("a place to write");
    let mol = molecule();
    let opts = pl_draw::Options::default();

    // ONE scene, rendered twice. Rebuilding it for each back end would let the
    // two drift and the comparison would still pass.
    let (scene, _) = pl_draw::scene(&mol, opts);

    for scale in [1u32, 4] {
        let (img, report) = pl_draw::raster::draw(&scene, f64::from(scale), [255, 255, 255]);
        assert!(
            report.unparsed_colours.is_empty() && report.unencodable.is_empty(),
            "the figure did not draw cleanly at {scale}x: {report:?}"
        );
        fs::write(
            d.join(format!("map@{scale}x.png")),
            pl_draw::png::encode(&img, None),
        )
        .expect("the png");
    }
    fs::write(d.join("map.svg"), pl_draw::svg_of(&scene)).expect("the svg");
    fs::write(d.join("SCALES"), "1\n4\n").expect("the manifest");
}
