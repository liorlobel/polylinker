//! Minimal reproduction: on a Windows display scaled above 100%, eframe's
//! screen rect does not match the window, so content is drawn larger than the
//! window and clipped at every edge.
//!
//! Run it. Expected: a red border inset 4 px from every edge, fully visible,
//! and `screen_rect == inner_rect / native_ppp`.
//!
//! Observed on Windows 11, 125% scaling: the right and bottom border are off
//! screen, and `screen_rect` equals the window's *physical* size in points.
//!
//! Everything is printed to stdout each second so the numbers can be compared
//! without a screenshot.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Stroke, StrokeKind, Ui};

struct Repro {
    frames: u64,
    last_print: std::time::Instant,
}

impl eframe::App for Repro {
    fn ui(&mut self, ui: &mut Ui, _f: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.frames += 1;

        let ppp = ctx.pixels_per_point();
        let (inner, native_ppp) = ctx.input(|i| {
            let vp = i.viewport();
            (vp.inner_rect, vp.native_pixels_per_point)
        });

        egui::CentralPanel::default().show(ui, |ui| {
            let screen = ui.max_rect();
            let p = ui.painter();

            // A border inset 4 points from every edge of what egui believes the
            // screen is. If egui and the window agree, all four edges are
            // visible with a small margin.
            p.rect(
                screen.shrink(4.0),
                0,
                Color32::TRANSPARENT,
                Stroke::new(3.0, Color32::from_rgb(230, 60, 60)),
                StrokeKind::Inside,
            );
            // Corner markers, so a clipped edge is unmistakable.
            for c in [
                screen.left_top(),
                screen.right_top(),
                screen.left_bottom(),
                screen.right_bottom(),
            ] {
                p.circle_filled(c, 10.0, Color32::from_rgb(230, 60, 60));
            }

            let expected = inner.map(|r| r.size() / native_ppp.unwrap_or(1.0));
            let lines = [
                format!("egui screen_rect      : {:?}", screen.size()),
                format!("viewport inner_rect   : {:?}", inner.map(|r| r.size())),
                format!("native_pixels_per_point: {native_ppp:?}"),
                format!("ctx.pixels_per_point() : {ppp}"),
                format!("expected screen_rect   : {expected:?}   (inner_rect / native_ppp)"),
                String::new(),
                "If the red border is clipped, egui is drawing outside the window.".into(),
            ];
            for (i, line) in lines.iter().enumerate() {
                p.text(
                    Pos2::new(screen.left() + 24.0, screen.top() + 24.0 + i as f32 * 22.0),
                    Align2::LEFT_TOP,
                    line,
                    FontId::monospace(14.0),
                    ui.visuals().text_color(),
                );
            }

            if self.last_print.elapsed() > std::time::Duration::from_secs(1) {
                self.last_print = std::time::Instant::now();
                println!(
                    "frame {:>4}  screen_rect={:?}  inner_rect={:?}  native_ppp={:?}  ppp={}  expected={:?}",
                    self.frames,
                    screen.size(),
                    inner.map(|r| r.size()),
                    native_ppp,
                    ppp,
                    expected
                );
            }
        });
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
    }
}

fn main() -> eframe::Result {
    eframe::run_native(
        "eframe HiDPI repro",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([800.0, 600.0]),
            ..Default::default()
        },
        Box::new(|_| {
            Ok(Box::new(Repro {
                frames: 0,
                last_print: std::time::Instant::now(),
            }))
        }),
    )
}
