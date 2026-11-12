//! The AI Chat composer: its draft editor, and the model, reasoning, Send and Stop controls.
//!
//! The composer exists only while AI Chat is on screen, but the draft it edits lives in
//! [`crate::ai::AiState`]: switching to File or Assets, or collapsing the panel, loses neither the
//! text nor the objects it mentions.

use std::sync::Arc;

use egui::text::{CCursor, CCursorRange, CharIndex};
use egui::{Align2, CornerRadius, Id, Key, Modifiers, Rect, ScrollArea, Sense, Stroke, TextBuffer, TextEdit, Ui, pos2, vec2};

use super::draft::{self, DraftMetadata};
use crate::FigApp;
use crate::i18n::tr;
use crate::theme::{Tokens, regular, semibold};
use crate::widgets;

/// The height the composer asks the left panel for.
pub const COMPOSER_HEIGHT: f32 = 120.0;

/// Space between the composer's edge and its contents.
const INSET: f32 = 8.0;
/// Space between the draft editor and the footer, and between footer items.
const GAP: f32 = 6.0;
/// The footer's Send and Stop button.
const SEND_W: f32 = 48.0;
/// The footer's reasoning dropdown.
const EFFORT_W: f32 = 56.0;
/// The room the model dropdown keeps before it starts giving width to the other items.
const MODEL_MIN_W: f32 = 44.0;
/// The line an inline draft error takes above the footer.
const ERROR_H: f32 = 15.0;

/// The draft editor's stable id: its own state holds the caret, and an insertion made from the
/// canvas writes here even while the composer is hidden.
pub(crate) fn input_id() -> Id {
    Id::unique("ai_composer_input")
}

/// Render the composer inside `rect`, which the left panel allocated for it.
///
/// Everything drawn stays inside `rect`: the footer is reserved first so Send and Stop are always
/// reachable, and the editor takes whatever height is left.
pub fn show(app: &mut FigApp, ui: &mut Ui, rect: Rect) {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    let t = Tokens::get(ui.ctx());
    // The composer is on screen in this pass: a canvas click may now mention a layer in it.
    app.ui.ai.composer_visible = true;
    ui.painter().rect_filled(rect, CornerRadius::ZERO, t.panel);
    ui.painter().line_segment([rect.left_top(), rect.right_top()], Stroke::new(1.0, t.border));

    let inner = rect.shrink(INSET);
    if inner.width() <= 0.0 || inner.height() <= 0.0 {
        return;
    }

    let footer_h = widgets::FIELD_H.min(inner.height());
    let footer = Rect::from_min_size(pos2(inner.min.x, inner.max.y - footer_h), vec2(inner.width(), footer_h));
    let above_footer = Rect::from_min_max(inner.min, pos2(inner.max.x, (footer.min.y - GAP).max(inner.min.y)));

    let error = app.ui.ai.draft().error.clone();
    let (input_rect, error_rect) = match &error {
        Some(_) => {
            let error_h = ERROR_H.min(above_footer.height());
            (
                Rect::from_min_max(above_footer.min, pos2(above_footer.max.x, (above_footer.max.y - error_h).max(above_footer.min.y))),
                Some(Rect::from_min_size(pos2(above_footer.min.x, above_footer.max.y - error_h), vec2(above_footer.width(), error_h))),
            )
        }
        None => (above_footer, None),
    };

    let mut submit = false;
    if input_rect.height() > 0.0 {
        submit = draft_editor(&mut app.ui.ai, ui, input_rect);
    }
    if let (Some(error), Some(error_rect)) = (&error, error_rect) {
        ui.scope_builder(egui::UiBuilder::new().max_rect(error_rect), |ui| {
            ui.add(egui::Label::new(egui::RichText::new(error).font(regular(10.0)).color(t.danger)).truncate());
        });
    }
    footer_controls(app, ui, footer, submit);
}

/// The draft editor. Returns whether Enter asked to send.
fn draft_editor(ai: &mut super::AiState, ui: &mut Ui, rect: Rect) -> bool {
    let t = Tokens::get(ui.ctx());
    let ctx = ui.ctx().clone();
    let text_id = input_id();
    let running = ai.is_running;

    // An IME composition's text is transient: it is in the buffer, but it is not the draft.
    let (preedit, ended) = ui.input(|i| {
        let (mut preedit, mut ended) = (false, false);
        for ev in &i.events {
            match ev {
                egui::Event::Ime(egui::ImeEvent::Preedit { text, .. }) => {
                    preedit = !text.is_empty();
                    ended |= text.is_empty();
                }
                egui::Event::Ime(egui::ImeEvent::Commit(_)) => {
                    preedit = false;
                    ended = true;
                }
                _ => {}
            }
        }
        (preedit, ended)
    });
    if preedit {
        ai.draft_meta.set_composing(true);
    } else if ended {
        ai.draft_meta.set_composing(false);
    }
    let composing = ai.draft_meta.composing();

    // Undo, redo and send are the draft's own: egui's undo keeps text alone, and would restore
    // characters whose mention metadata no longer matches.
    let mut undo = false;
    let mut redo = false;
    let mut send_key = false;
    if ctx.memory(|m| m.has_focus(text_id)) {
        ui.input_mut(|i| {
            undo = i.consume_key(Modifiers::COMMAND, Key::Z);
            redo = i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z) || i.consume_key(Modifiers::CTRL, Key::Y);
            // Enter that confirms an IME candidate belongs to the composition, not to sending.
            if !composing && !ended && !i.modifiers.shift && i.key_pressed(Key::Enter) {
                send_key = i.consume_key(Modifiers::NONE, Key::Enter);
            }
        });
    }
    if undo || redo {
        let changed = if undo { ai.draft_meta.undo(&mut ai.draft_text) } else { ai.draft_meta.redo(&mut ai.draft_text) };
        if changed {
            sync_draft_editor(&ctx, &ai.draft_text, &mut ai.draft_meta);
        }
    }

    // The layouter draws the tokens on their own background; it never substitutes a label for the
    // characters, so its galleys stay the text the editor edits.
    let mentions = ai.draft_meta.mentions().to_vec();
    let font = regular(12.0);
    let (text_color, token_background) = (t.text, t.selected_row);
    let mut layouter = |ui: &Ui, buffer: &dyn TextBuffer, wrap_width: f32| -> Arc<egui::Galley> {
        let job = draft::draft_layout(buffer.as_str(), &mentions, &font, text_color, token_background, wrap_width);
        ui.fonts_mut(|fonts| fonts.layout_job(job))
    };

    let reported = {
        let mut buffer = draft::DraftBuffer { text: &mut ai.draft_text, meta: &mut ai.draft_meta };
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
            ScrollArea::vertical()
                .id_salt("ai_draft_scroll")
                .auto_shrink([false, false])
                .max_height(rect.height())
                .show(ui, |ui| {
                    TextEdit::multiline(&mut buffer)
                        .id(text_id)
                        .hint_text(tr("Ask Codex to create or revise layers…"))
                        .font(font.clone())
                        .text_color(text_color)
                        .desired_width(f32::INFINITY)
                        .desired_rows(2)
                        .frame(egui::Frame::NONE)
                        .layouter(&mut layouter)
                        .show(ui)
                        .cursor_range
                })
                .inner
        })
        .inner
    };

    // A caret or a selection that reaches into a token moves out of it, so the editor can never
    // split one name into two.
    if let Some(range) = reported {
        let [start, end] = range.sorted_cursors();
        let snapped = ai.draft_meta.snap_range(start.index.0, end.index.0);
        ai.draft_meta.set_selection(snapped.0, snapped.1);
        if snapped != (start.index.0, end.index.0) {
            sync_draft_editor(&ctx, &ai.draft_text, &mut ai.draft_meta);
        }
    }
    ai.draft_meta.end_frame(&ai.draft_text);

    send_key && !running && !composing
}

/// The widths of the footer's three items in a composer `inner_w` wide: the model dropdown, the
/// reasoning dropdown and Send or Stop.
///
/// The footer never wraps and never overflows: the model gives up width first, then the reasoning
/// dropdown, and Send keeps its size so it is always the same target. A model that would be too
/// narrow to read is dropped instead (width 0, not drawn).
fn footer_widths(inner_w: f32) -> (f32, f32, f32) {
    let model = (inner_w - 2.0 * GAP - EFFORT_W - SEND_W).max(0.0);
    let model = if model >= MODEL_MIN_W { model } else { 0.0 };
    let used = if model > 0.0 { model + GAP } else { 0.0 };
    let effort = EFFORT_W.min((inner_w - used - GAP - SEND_W).max(0.0));
    (model, effort, SEND_W)
}

/// The footer's item widths for the tests, which check the row fits the panel it is given.
#[cfg(test)]
pub(crate) fn footer_widths_for_test(inner_w: f32) -> (f32, f32, f32) {
    footer_widths(inner_w)
}

/// The footer: model and reasoning dropdowns, then Send or Stop.
fn footer_controls(app: &mut FigApp, ui: &mut Ui, rect: Rect, submit: bool) {
    let running = app.ui.ai.is_running;
    let models = app.ui.ai.model_infos.clone();
    let selected_model = app.ui.ai.selected_model.clone();
    let selected_effort = app.ui.ai.selected_effort.clone();
    let efforts = app.ui.ai.supported_efforts_for_selected_model();
    let can_send = !running && !models.is_empty() && !app.ui.ai.draft_text.trim().is_empty();

    let (model_w, effort_w, _) = footer_widths(rect.width());
    let show_model = model_w > 0.0;

    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        ui.horizontal(|ui| {
            if show_model {
                let label = match models.iter().find(|m| m.id == selected_model) {
                    Some(info) => info.display_name.clone().unwrap_or_else(|| info.id.clone()),
                    None if models.is_empty() => tr("No models available").to_string(),
                    None => selected_model.clone(),
                };
                let chosen = ui.add_enabled_ui(!running && !models.is_empty(), |ui| {
                    let resp = widgets::dropdown(ui, &label, model_w, None);
                    crate::a11y::combo(&resp, tr("Model"), &label, crate::a11y::menu_open(&resp));
                    let mut pick = None;
                    crate::right::menu_popup(&resp).show(|ui| {
                        for info in &models {
                            let shown = info.display_name.clone().unwrap_or_else(|| info.id.clone());
                            let row = if shown == info.id { shown } else { format!("{shown}  {}", info.id) };
                            if crate::right::menu_check(ui, info.id == selected_model, &row, "") {
                                pick = Some(info.id.clone());
                                ui.close();
                            }
                        }
                    });
                    pick
                });
                if let Some(id) = chosen.inner {
                    app.ui.ai.select_model(&id);
                }
            }

            let effort_label =
                if efforts.is_empty() { tr("Not supported").to_string() } else { selected_effort.clone().unwrap_or_else(|| tr("None").to_string()) };
            if effort_w > 0.0 {
                let chosen = ui.add_enabled_ui(!running && !efforts.is_empty(), |ui| {
                    let resp = widgets::dropdown(ui, &effort_label, effort_w, None);
                    crate::a11y::combo(&resp, tr("Reasoning effort"), &effort_label, crate::a11y::menu_open(&resp));
                    let mut pick = None;
                    crate::right::menu_popup(&resp).show(|ui| {
                        for effort in &efforts {
                            if crate::right::menu_check(ui, selected_effort.as_deref() == Some(effort.as_str()), effort, "") {
                                pick = Some(effort.clone());
                                ui.close();
                            }
                        }
                    });
                    pick
                });
                if let Some(effort) = chosen.inner {
                    app.ui.ai.select_effort(Some(effort));
                }
            }

            let (r, resp) = ui.allocate_exact_size(vec2(SEND_W, widgets::FIELD_H), Sense::click());
            let t = Tokens::get(ui.ctx());
            if running {
                let enabled = true;
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, tr("Stop")));
                let fill = if resp.hovered() { t.danger.gamma_multiply(0.15) } else { t.hover };
                ui.painter().rect_filled(r, CornerRadius::same(5), fill);
                ui.painter().text(r.center(), Align2::CENTER_CENTER, tr("Stop"), semibold(11.0), t.danger);
                if resp.clicked() {
                    app.ui.ai.stop_running_task();
                }
            } else {
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, can_send, tr("Send")));
                let fill = if can_send && resp.hovered() {
                    t.accent
                } else if can_send {
                    t.selected_row
                } else {
                    t.hover
                };
                ui.painter().rect_filled(r, CornerRadius::same(5), fill);
                let text_color = if can_send { t.accent_text } else { t.text_secondary };
                ui.painter().text(r.center(), Align2::CENTER_CENTER, tr("Send"), semibold(11.0), text_color);
                if (resp.clicked() || submit) && can_send {
                    app.ui.ai.submit_turn(&mut app.session);
                }
            }
        });
    });
}

/// Write the draft's remembered caret into the editor's state, so an insertion made from the canvas
/// carries on where the draft last left the caret, even while the composer is hidden.
///
/// A cancelled IME composition leaves a preedit range in the editor's state; a replacement state
/// drops it, so the stale range can never delete committed characters.
pub(crate) fn sync_draft_editor(ctx: &egui::Context, text: &str, meta: &mut DraftMetadata) {
    let total = text.chars().count();
    let (start, end) = meta.selection();
    let (start, end) = (start.min(total), end.min(total));
    let reset = meta.take_editor_reset();
    let id = input_id();
    let mut state = if reset { egui::widgets::text_edit::TextEditState::default() } else { TextEdit::load_state(ctx, id).unwrap_or_default() };
    state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(CharIndex(start)), CCursor::new(CharIndex(end)))));
    TextEdit::store_state(ctx, id, state);
}

/// Render reachable Stop badge on the slim icon rail when the sidebar is collapsed or during a run.
pub fn render_rail_stop(app: &mut FigApp, ui: &mut Ui, r: Rect) {
    if !app.ui.ai.is_running {
        return;
    }
    let t = Tokens::get(ui.ctx());
    let stop_rect = Rect::from_center_size(pos2(r.center().x, r.max.y - 30.0), vec2(36.0, 24.0));
    let stop_resp = ui.interact(stop_rect, ui.scope_id().with("ai_rail_stop"), Sense::click());
    stop_resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), tr("Stop")));
    let fill = if stop_resp.hovered() { t.danger.gamma_multiply(0.15) } else { t.hover };
    ui.painter().rect_filled(stop_rect, CornerRadius::same(4), fill);
    ui.painter().text(stop_rect.center(), Align2::CENTER_CENTER, tr("Stop"), semibold(10.0), t.danger);
    if stop_resp.clicked() {
        app.ui.ai.stop_running_task();
    }
}
