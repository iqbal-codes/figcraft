//! The persistent bottom composer visible across expanded AI Chat, File, and Assets panels.

use egui::{Align2, CornerRadius, Id, Key, Modifiers, Rect, Sense, Stroke, TextEdit, Ui, pos2, vec2};

use crate::FigApp;
use crate::LeftTab;
use crate::i18n::tr;
use crate::theme::{Tokens, regular, semibold};

/// Height of the persistent composer docked at the bottom of the left panel.
pub const COMPOSER_HEIGHT: f32 = 110.0;

/// Render the persistent composer at the bottom of the left panel.
pub fn show(app: &mut FigApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let full = ui.max_rect();
    let comp_rect = Rect::from_min_max(pos2(full.min.x, full.max.y - COMPOSER_HEIGHT), pos2(full.max.x, full.max.y));

    // Background and top border
    ui.painter().rect_filled(comp_rect, CornerRadius::ZERO, t.panel);
    ui.painter().line_segment([comp_rect.left_top(), comp_rect.right_top()], Stroke::new(1.0, t.border));

    // Target context chip
    let target_text = app.ui.ai.current_target_description(&app.session);
    let target_rect = Rect::from_min_max(pos2(comp_rect.min.x + 10.0, comp_rect.min.y + 6.0), pos2(comp_rect.max.x - 10.0, comp_rect.min.y + 22.0));
    ui.painter().text(pos2(target_rect.min.x, target_rect.center().y), Align2::LEFT_CENTER, format!("✦ {target_text}"), regular(10.0), t.accent_text);

    // Text input area
    let input_rect = Rect::from_min_max(pos2(comp_rect.min.x + 8.0, comp_rect.min.y + 24.0), pos2(comp_rect.max.x - 8.0, comp_rect.max.y - 32.0));

    let is_running = app.ui.ai.is_running;
    let mut submit_pressed = false;

    ui.scope_builder(egui::UiBuilder::new().max_rect(input_rect), |ui| {
        let text_id = Id::unique("ai_composer_input");
        let edit = TextEdit::multiline(&mut app.ui.ai.draft_text)
            .id(text_id)
            .hint_text(tr("Ask Codex to create or revise layers…"))
            .font(regular(12.0))
            .text_color(t.text)
            .desired_width(input_rect.width())
            .desired_rows(2)
            .frame(egui::Frame::NONE);

        let output = edit.show(ui);
        if output.response.has_focus() && !is_running {
            let enter = ui.input(|i| i.key_pressed(Key::Enter));
            let shift = ui.input(|i| i.modifiers.contains(Modifiers::SHIFT));
            if enter && !shift {
                submit_pressed = true;
            }
        }
    });

    // Lower bar: Model chip on left, Stop or Send on right
    let bar_y = comp_rect.max.y - 28.0;
    let model_rect = Rect::from_min_size(pos2(comp_rect.min.x + 8.0, bar_y), vec2(80.0, 22.0));
    let model_text = if app.ui.ai.selected_model.is_empty() { "gpt-5.6-sol" } else { &app.ui.ai.selected_model };
    ui.painter().text(pos2(model_rect.min.x, model_rect.center().y), Align2::LEFT_CENTER, model_text, regular(10.0), t.text_secondary);

    if is_running {
        // Stop button
        let stop_rect = Rect::from_min_size(pos2(comp_rect.max.x - 68.0, bar_y), vec2(60.0, 22.0));
        let stop_resp = ui.interact(stop_rect, ui.scope_id().with("ai_composer_stop"), Sense::click());
        let fill = if stop_resp.hovered() { t.danger.gamma_multiply(0.15) } else { t.hover };
        ui.painter().rect_filled(stop_rect, CornerRadius::same(4), fill);
        ui.painter().text(stop_rect.center(), Align2::CENTER_CENTER, tr("Stop"), semibold(11.0), t.danger);
        if stop_resp.clicked() {
            app.ui.ai.stop_active_task();
        }
    } else {
        // Send button
        let can_send = !app.ui.ai.draft_text.trim().is_empty();
        let send_rect = Rect::from_min_size(pos2(comp_rect.max.x - 68.0, bar_y), vec2(60.0, 22.0));
        let send_resp = ui.interact(send_rect, ui.scope_id().with("ai_composer_send"), if can_send { Sense::click() } else { Sense::hover() });
        let fill = if can_send && send_resp.hovered() {
            t.accent
        } else if can_send {
            t.selected_row
        } else {
            t.hover
        };
        ui.painter().rect_filled(send_rect, CornerRadius::same(4), fill);
        let text_color = if can_send { t.accent_text } else { t.text_secondary };
        ui.painter().text(send_rect.center(), Align2::CENTER_CENTER, tr("Send"), semibold(11.0), text_color);
        if (send_resp.clicked() || submit_pressed) && can_send {
            app.ui.left_tab = LeftTab::AiChat;
            app.ui.open_left_panel();
            app.ui.ai.submit_turn(&mut app.session);
        }
    }
}

/// Render reachable Stop badge on the slim icon rail when the sidebar is collapsed or during a run.
pub fn render_rail_stop(app: &mut FigApp, ui: &mut Ui, r: Rect) {
    if !app.ui.ai.is_running {
        return;
    }
    let t = Tokens::get(ui.ctx());
    let stop_rect = Rect::from_center_size(pos2(r.center().x, r.max.y - 30.0), vec2(36.0, 24.0));
    let stop_resp = ui.interact(stop_rect, ui.scope_id().with("ai_rail_stop"), Sense::click());
    let fill = if stop_resp.hovered() { t.danger.gamma_multiply(0.15) } else { t.hover };
    ui.painter().rect_filled(stop_rect, CornerRadius::same(4), fill);
    ui.painter().text(stop_rect.center(), Align2::CENTER_CENTER, tr("Stop"), semibold(10.0), t.danger);
    if stop_resp.clicked() {
        app.ui.ai.stop_active_task();
    }
}
