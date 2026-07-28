//! Sanger chromatograms as a [`Scene`], and so as SVG or PDF.
//!
//! Takes plain slices rather than a parsed trace type, which keeps this crate
//! independent of the one that reads `.ab1` — the renderer should not care
//! where the numbers came from.
//!
//! # Four things a chromatogram renderer gets wrong
//!
//! **Which channel is which base.** ABIF stores the four traces in `DATA9`
//! through `DATA12` and names their bases separately, in `FWO_`. Assuming the
//! conventional `GATC` order instead of reading `FWO_` mislabels every peak in
//! the file on any machine that writes a different one, and the picture still
//! looks like a perfectly good chromatogram. [`View::base_order`] is required
//! here for that reason; there is no default.
//!
//! **The x-axis is samples, not bases.** Called bases are not evenly spaced
//! along the trace — that spacing is exactly what mobility correction adjusts,
//! and where peaks crowd together is diagnostic. Drawing one base per fixed
//! width and stretching the trace to fit throws that away and makes a
//! compressed region look normal.
//!
//! **Decimation by stride skips peaks.** A wide window has more samples than
//! pixels, and taking every *k*-th one can land between two peaks and miss a
//! base entirely. [`View::to_scene`] takes the maximum within each bucket
//! instead, which preserves peak height and cannot drop a peak.
//!
//! **Red and green.** The classic Sanger palette puts T in red and A in green,
//! the one pair of colours a red–green colour-blind reader cannot separate —
//! and A/T confusion is not a small error. [`Palette::Accessible`] uses the
//! Okabe–Ito set instead. The classic one is kept because it is what everyone
//! expects, and defaulting away from it silently would be its own surprise.

use crate::scene::{Anchor, Item, Scene, Seg};

/// Which colours to draw the four bases in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Palette {
    /// A green, C blue, G black, T red — what every other trace viewer uses.
    #[default]
    Classic,
    /// Okabe–Ito, which stays distinguishable to red–green colour-blind
    /// readers. A bluish green, C blue, G black, T vermillion.
    Accessible,
}

impl Palette {
    /// The colour for a base, by its letter.
    pub fn color(&self, base: u8) -> &'static str {
        match (self, base.to_ascii_uppercase()) {
            (Palette::Classic, b'A') => "#10a010",
            (Palette::Classic, b'C') => "#1030d0",
            (Palette::Classic, b'G') => "#101010",
            (Palette::Classic, b'T') => "#d01010",
            (Palette::Accessible, b'A') => "#009e73",
            (Palette::Accessible, b'C') => "#0072b2",
            (Palette::Accessible, b'G') => "#000000",
            (Palette::Accessible, b'T') => "#d55e00",
            _ => "#808080",
        }
    }
}

/// Everything needed to draw a trace.
#[derive(Debug, Clone, Copy)]
pub struct View<'a> {
    /// The four analysed channels, in file order.
    pub channels: [&'a [u16]; 4],
    /// Which base each channel carries, from `FWO_`. Required, never assumed.
    pub base_order: [u8; 4],
    /// Sample index of each called base, from `PLOC2`. May be empty.
    pub peaks: &'a [u16],
    pub sequence: &'a [u8],
    /// Per-base Phred. May be empty.
    pub quality: &'a [u8],
    pub title: &'a str,
}

/// Layout and styling.
#[derive(Debug, Clone, Copy)]
pub struct Options {
    /// First and last called base to draw, 1-based inclusive. `None` draws all.
    pub bases: Option<(usize, usize)>,
    /// Drawing width in points, before any margin.
    pub width: f64,
    pub height: f64,
    pub palette: Palette,
    /// Draw the per-base quality as a bar chart under the trace.
    pub quality_bars: bool,
    /// Most polyline points per channel. Above this the trace is decimated by
    /// taking each bucket's maximum, which cannot lose a peak.
    pub max_points: usize,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            bases: None,
            width: 1200.0,
            height: 260.0,
            palette: Palette::default(),
            quality_bars: true,
            max_points: 4000,
        }
    }
}

/// What was drawn, and what could not be.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    pub bases_drawn: usize,
    pub samples: usize,
    /// Points per channel after decimation, or 0 when none was needed.
    pub decimated_to: usize,
    /// Y scaling is per drawn window, so two pictures of different windows are
    /// not comparable by peak height. This is that maximum, so a caller that
    /// wants comparable pictures can say so.
    pub scale_max: u16,
    pub notes: Vec<String>,
}

const PAD: f64 = 12.0;
const LETTERS: f64 = 16.0; // strip for base letters
const QUAL: f64 = 34.0; // strip for quality bars

impl View<'_> {
    /// Build the picture.
    pub fn to_scene(&self, o: &Options) -> (Scene, Report) {
        let mut rep = Report::default();
        let mut items = Vec::new();

        let chan_len = self.channels.iter().map(|c| c.len()).min().unwrap_or(0);
        if chan_len == 0 {
            rep.notes
                .push("this file carries no trace data, only base calls".into());
            return (
                Scene {
                    width: o.width,
                    height: o.height,
                    title: self.title.to_string(),
                    items,
                },
                rep,
            );
        }

        // The window, in *samples*. Bases select it; the drawing is in sample
        // space, because that is what the instrument measured.
        let (b0, b1) = match o.bases {
            Some((a, b)) if !self.peaks.is_empty() => (
                a.saturating_sub(1).min(self.peaks.len() - 1),
                b.min(self.peaks.len()).max(1) - 1,
            ),
            _ => (0, self.peaks.len().saturating_sub(1)),
        };
        let (s0, s1) = if self.peaks.is_empty() {
            (0usize, chan_len - 1)
        } else {
            let lo = self.peaks[b0] as usize;
            let hi = self.peaks[b1] as usize;
            // Half a base of air on each side, so the end peaks are not clipped.
            //
            // `hi > lo` is checked as well as `b1 > b0`: PLOC2 is *supposed* to
            // ascend, and a corrupt or hostile file is under no obligation to
            // make it. A descending pair underflowed this subtraction — a panic
            // in debug, and in release a wrapped `air` near usize::MAX that
            // then poisoned the window.
            let air = if b1 > b0 && hi > lo {
                (hi - lo) / (b1 - b0).max(1) / 2
            } else {
                8
            };
            (
                lo.saturating_sub(air),
                hi.saturating_add(air).min(chan_len - 1),
            )
        };
        if s1 <= s0 {
            rep.notes
                .push("the requested window holds no samples".into());
            return (
                Scene {
                    width: o.width,
                    height: o.height,
                    title: self.title.to_string(),
                    items,
                },
                rep,
            );
        }
        rep.samples = s1 - s0 + 1;
        rep.bases_drawn = if self.peaks.is_empty() {
            0
        } else {
            b1 - b0 + 1
        };

        // Scale to the window's own maximum. A single tall peak elsewhere in
        // the run would otherwise flatten everything here into a straight line.
        let scale = self
            .channels
            .iter()
            .flat_map(|c| c[s0..=s1.min(c.len() - 1)].iter())
            .copied()
            .max()
            .unwrap_or(1)
            .max(1);
        rep.scale_max = scale;

        let plot_top = PAD;
        let plot_bottom = o.height - PAD - LETTERS - if o.quality_bars { QUAL } else { 0.0 };
        let plot_h = (plot_bottom - plot_top).max(1.0);
        let xs = |s: usize| PAD + (s - s0) as f64 * (o.width - 2.0 * PAD) / (s1 - s0) as f64;

        // Quality first, so it sits behind everything.
        if o.quality_bars && !self.quality.is_empty() && !self.peaks.is_empty() {
            let top = plot_bottom + LETTERS;
            for b in b0..=b1 {
                let (Some(&pk), Some(&q)) = (self.peaks.get(b), self.quality.get(b)) else {
                    continue;
                };
                let pk = pk as usize;
                if pk < s0 || pk > s1 {
                    continue;
                }
                // Phred 60 is the top of the bar; real Sanger rarely exceeds it.
                let h = (q as f64 / 60.0).min(1.0) * (QUAL - 6.0);
                let half = ((o.width - 2.0 * PAD) / (b1 - b0 + 1).max(1) as f64 / 2.0).min(6.0);
                items.push(Item::Path {
                    segs: vec![
                        Seg::Move(xs(pk) - half, top + QUAL - 6.0 - h),
                        Seg::Line(xs(pk) + half, top + QUAL - 6.0 - h),
                        Seg::Line(xs(pk) + half, top + QUAL - 6.0),
                        Seg::Line(xs(pk) - half, top + QUAL - 6.0),
                        Seg::Close,
                    ],
                    // Below Q20 in grey: the same threshold the rest of the
                    // project uses to decide what a difference is worth.
                    fill: Some(if q >= 20 { "#b8c4d0" } else { "#e6a0a0" }.into()),
                    stroke: None,
                    stroke_width: 0.0,
                    title: Some(format!("base {} Q{q}", b + 1)),
                });
            }
        }

        // The four traces.
        let bucket = (rep.samples as f64 / o.max_points as f64).ceil().max(1.0) as usize;
        if bucket > 1 {
            rep.decimated_to = rep.samples.div_ceil(bucket);
        }
        for (ci, chan) in self.channels.iter().enumerate() {
            let base = self.base_order[ci];
            let mut segs = Vec::new();
            let mut s = s0;
            while s <= s1 {
                let end = (s + bucket - 1).min(s1);
                // The bucket's *maximum*, not its first sample: a stride can
                // land between two peaks and lose a base.
                let v = chan[s..=end.min(chan.len() - 1)]
                    .iter()
                    .copied()
                    .max()
                    .unwrap_or(0);
                let y = plot_bottom - (v as f64 / scale as f64) * plot_h;
                let x = xs(s);
                if segs.is_empty() {
                    segs.push(Seg::Move(x, y));
                } else {
                    segs.push(Seg::Line(x, y));
                }
                s = end + 1;
            }
            items.push(Item::Path {
                segs,
                fill: None,
                stroke: Some(o.palette.color(base).into()),
                stroke_width: 1.1,
                title: Some(format!("{} channel", base as char)),
            });
        }

        // Base calls, at their own sample positions.
        for b in b0..=b1 {
            let Some(&pk) = self.peaks.get(b) else {
                continue;
            };
            let pk = pk as usize;
            if pk < s0 || pk > s1 {
                continue;
            }
            let letter = self.sequence.get(b).copied().unwrap_or(b'N');
            items.push(Item::Text {
                x: xs(pk),
                y: plot_bottom + LETTERS / 2.0,
                size: 11.0,
                anchor: Anchor::Middle,
                color: o.palette.color(letter).into(),
                bold: false,
                text: (letter as char).to_string(),
            });
        }

        if self.peaks.is_empty() {
            rep.notes
                .push("no base positions in this file: the trace is drawn without calls".into());
        }
        if self.quality.is_empty() && o.quality_bars {
            rep.notes.push("no quality values in this file".into());
        }

        (
            Scene {
                width: o.width,
                height: o.height,
                title: self.title.to_string(),
                items,
            },
            rep,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic trace: `n` bases, one Gaussian-ish peak each, 12 samples
    /// apart, in the channel the base belongs to.
    fn synth(seq: &[u8], order: [u8; 4]) -> (Vec<Vec<u16>>, Vec<u16>) {
        let spacing = 12usize;
        let len = seq.len() * spacing + spacing;
        let mut ch = vec![vec![0u16; len]; 4];
        let mut peaks = Vec::new();
        for (i, b) in seq.iter().enumerate() {
            let centre = spacing / 2 + i * spacing;
            peaks.push(centre as u16);
            let c = order.iter().position(|x| x == b).expect("base in order");
            for d in 0..spacing {
                let off = d as i64 - spacing as i64 / 2;
                let v = (1000.0 * (-(off * off) as f64 / 8.0).exp()) as u16;
                let at = centre as i64 + off;
                if at >= 0 && (at as usize) < len {
                    ch[c][at as usize] = ch[c][at as usize].max(v);
                }
            }
        }
        (ch, peaks)
    }

    fn view<'a>(
        ch: &'a [Vec<u16>],
        peaks: &'a [u16],
        seq: &'a [u8],
        qual: &'a [u8],
        order: [u8; 4],
    ) -> View<'a> {
        View {
            channels: [&ch[0], &ch[1], &ch[2], &ch[3]],
            base_order: order,
            peaks,
            sequence: seq,
            quality: qual,
            title: "t",
        }
    }

    #[test]
    fn every_base_is_drawn_in_the_colour_its_own_channel_says() {
        // The failure this prevents: colouring by array position instead of by
        // FWO_. On a machine whose channel order is not GATC, every peak is
        // then labelled with the wrong base, and the picture still looks like a
        // perfectly ordinary chromatogram.
        let seq = b"ACGTACGT";
        for order in [*b"GATC", *b"ACGT", *b"TCGA"] {
            let (ch, peaks) = synth(seq, order);
            let v = view(&ch, &peaks, seq, &[], order);
            let (sc, rep) = v.to_scene(&Options::default());
            assert_eq!(rep.bases_drawn, 8);
            let p = Palette::Classic;
            for (ci, stroke) in sc
                .items
                .iter()
                .filter_map(|i| match i {
                    Item::Path {
                        stroke: Some(s),
                        fill: None,
                        ..
                    } => Some(s.clone()),
                    _ => None,
                })
                .enumerate()
            {
                assert_eq!(
                    stroke,
                    p.color(order[ci]),
                    "channel {ci} of order {:?} must be drawn as {}",
                    order.map(|b| b as char),
                    order[ci] as char
                );
            }
        }
    }

    #[test]
    fn the_x_axis_is_samples_so_crowded_bases_stay_crowded() {
        // Called bases are not evenly spaced, and where they crowd is
        // diagnostic. Drawing one base per fixed width would hide it.
        let seq = b"ACGTACGT";
        let (ch, mut peaks) = synth(seq, *b"GATC");
        // Squeeze the last three calls together, as a compression does.
        peaks[5] = peaks[4] + 3;
        peaks[6] = peaks[4] + 6;
        peaks[7] = peaks[4] + 9;
        let v = view(&ch, &peaks, seq, &[], *b"GATC");
        let (sc, _) = v.to_scene(&Options::default());
        let xs: Vec<f64> = sc
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Text { x, .. } => Some(*x),
                _ => None,
            })
            .collect();
        assert_eq!(xs.len(), 8);
        let wide = xs[1] - xs[0];
        let tight = xs[6] - xs[5];
        assert!(
            tight < wide / 2.0,
            "the compression must survive into the picture: {tight} vs {wide}"
        );
    }

    #[test]
    fn decimation_takes_the_maximum_and_so_cannot_lose_a_peak() {
        // A stride can land between two peaks and drop a base entirely.
        let seq: Vec<u8> = b"ACGT".iter().cycle().take(400).copied().collect();
        let (ch, peaks) = synth(&seq, *b"GATC");
        let opts = Options {
            max_points: 200,
            ..Default::default()
        };
        let v = view(&ch, &peaks, &seq, &[], *b"GATC");
        let (sc, rep) = v.to_scene(&opts);
        assert!(rep.decimated_to > 0, "this window needs decimating");
        assert!(rep.decimated_to <= 200 + 1);

        // The tallest sample in the data still reaches the top of the plot.
        let top = sc
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Path {
                    segs,
                    stroke: Some(_),
                    ..
                } => segs
                    .iter()
                    .filter_map(|s| match s {
                        Seg::Move(_, y) | Seg::Line(_, y) => Some(*y),
                        _ => None,
                    })
                    .fold(None, |a: Option<f64>, y| Some(a.map_or(y, |a| a.min(y)))),
                _ => None,
            })
            .fold(f64::MAX, f64::min);
        assert!(
            (top - PAD).abs() < 1.0,
            "a full-height peak survived decimation: {top}"
        );
    }

    #[test]
    fn the_window_is_scaled_to_itself_and_the_report_says_so() {
        // A single huge peak elsewhere in the run would otherwise flatten this
        // window into a straight line.
        let seq = b"ACGTACGTACGTACGT";
        let (mut ch, peaks) = synth(seq, *b"GATC");
        ch[0][peaks[15] as usize] = 60000; // a spike at the far end
        let v = view(&ch, &peaks, seq, &[], *b"GATC");
        let (_, whole) = v.to_scene(&Options::default());
        let (_, early) = v.to_scene(&Options {
            bases: Some((1, 4)),
            ..Default::default()
        });
        assert_eq!(whole.scale_max, 60000);
        assert!(
            early.scale_max < 2000,
            "the early window is scaled to itself: {}",
            early.scale_max
        );
        assert_eq!(early.bases_drawn, 4);
    }

    #[test]
    fn a_file_with_no_trace_says_so_instead_of_drawing_nothing() {
        let empty: Vec<u16> = vec![];
        let v = View {
            channels: [&empty, &empty, &empty, &empty],
            base_order: *b"GATC",
            peaks: &[],
            sequence: b"ACGT",
            quality: &[],
            title: "t",
        };
        let (sc, rep) = v.to_scene(&Options::default());
        assert!(sc.items.is_empty());
        assert_eq!(rep.bases_drawn, 0);
        assert!(!rep.notes.is_empty(), "the absence is reported");
    }

    #[test]
    fn low_quality_bases_are_marked_at_the_same_threshold_the_rest_of_the_project_uses() {
        let seq = b"ACGTACGT";
        let (ch, peaks) = synth(seq, *b"GATC");
        let qual: Vec<u8> = vec![50, 50, 50, 5, 5, 50, 50, 50];
        let v = view(&ch, &peaks, seq, &qual, *b"GATC");
        let (sc, _) = v.to_scene(&Options::default());
        let bars: Vec<String> = sc
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Path { fill: Some(f), .. } => Some(f.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(bars.len(), 8);
        assert_eq!(bars.iter().filter(|f| f.as_str() == "#e6a0a0").count(), 2);
    }

    #[test]
    fn the_accessible_palette_does_not_put_a_and_t_in_red_and_green() {
        // A/T confusion is not a small error, and red-green is the one pair a
        // colour-blind reader cannot separate.
        assert_ne!(
            Palette::Accessible.color(b'A'),
            Palette::Classic.color(b'A')
        );
        assert_eq!(Palette::Accessible.color(b'A'), "#009e73");
        assert_eq!(Palette::Accessible.color(b'T'), "#d55e00");
        // And every base is still distinct from every other.
        let c: Vec<&str> = b"ACGT"
            .iter()
            .map(|b| Palette::Accessible.color(*b))
            .collect();
        for i in 0..4 {
            for j in i + 1..4 {
                assert_ne!(c[i], c[j]);
            }
        }
    }

    #[test]
    fn drawing_the_same_trace_twice_gives_the_same_picture() {
        let seq = b"ACGTACGTTTGA";
        let (ch, peaks) = synth(seq, *b"GATC");
        let qual = vec![40u8; seq.len()];
        let v = view(&ch, &peaks, seq, &qual, *b"GATC");
        let a = v.to_scene(&Options::default());
        let b = v.to_scene(&Options::default());
        assert_eq!(a.0, b.0);
        assert_eq!(a.1, b.1);
    }
}
