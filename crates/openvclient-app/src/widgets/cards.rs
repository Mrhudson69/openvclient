//! Fixed-size card grid: equal cards, equal gaps, reflows with width.

use egui::{Rect, Response, Sense, Ui};

use crate::theme;

/// Lay `items` out as `w`×`h` cards. `body` draws one card inside its own
/// child Ui (it never shifts the grid) and receives the card's response.
pub fn grid<T>(ui: &mut Ui, items: &[T], w: f32, h: f32, mut body: impl FnMut(&mut Ui, &T, &Response)) {
    let gap = theme::space::MD;
    let avail = ui.available_width();
    // Stretch cards to fill the row so the right edge lines up.
    let cols = (((avail + gap) / (w + gap)).floor() as usize).max(1);
    let w = ((avail - gap * (cols as f32 - 1.0)) / cols as f32).max(w.min(avail));
    for row in items.chunks(cols) {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for (i, item) in row.iter().enumerate() {
                if i > 0 {
                    ui.add_space(gap);
                }
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), Sense::click());
                card_frame(ui, rect, &resp);
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(14.0, 10.0))).layout(egui::Layout::top_down(egui::Align::Min)));
                child.spacing_mut().item_spacing = egui::vec2(theme::space::SM, 4.0);
                // Labels inside a clickable card must not grab the pointer
                // (text cursor, swallowed clicks): the whole card is the button.
                child.style_mut().interaction.selectable_labels = false;
                body(&mut child, item, &resp);
            }
        });
        ui.add_space(gap);
    }
}

pub fn card_frame(ui: &Ui, rect: Rect, resp: &Response) {
    let t = theme::tokens(ui);
    let hot = resp.hovered() && resp.sense.senses_click();
    ui.painter().rect(rect, 10.0, if hot { t.muted } else { t.card }, egui::Stroke::new(if hot { 1.5 } else { 1.0 }, if hot { t.ring } else { t.border }), egui::StrokeKind::Inside);
}
