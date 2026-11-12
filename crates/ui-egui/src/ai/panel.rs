//! The AI Chat panel rendering: Conversation view and Sessions subview.

use egui::{Align2, Color32, CornerRadius, FontId, Rect, ScrollArea, Sense, Stroke, Ui, UiBuilder, pos2, vec2};
use serde_json::json;

use super::draft::ObjectMention;
use super::sessions::{AiMessage, MessageRole, PendingApproval, ToolCallInfo, ToolStatus, TurnStatus};
use crate::FigApp;
use crate::a11y::{self, Tip};
use crate::i18n::{tr, trf};
use crate::icons::{self, Icon};
use crate::theme::{Tokens, regular, semibold};
use crate::widgets;

/// The widest the conversation's reading column grows, however wide the panel is.
const COLUMN_MAX: f32 = 640.0;
/// The share of the column a message may take; the rest keeps it off the column's edge.
const MESSAGE_SHARE: f32 = 0.85;
/// The panel's own header: the conversation's title and id, Close, and the subview tabs.
const HEADER_H: f32 = 46.0;
/// Space between messages.
const MESSAGE_GAP: f32 = 8.0;

/// Subview within the AI Chat content panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AiPanelSubview {
    #[default]
    Conversation,
    SessionsList,
}

/// Render the AI Chat content panel.
pub fn show(app: &mut FigApp, ui: &mut Ui) {
    let subview = app.ui.ai.subview;
    match subview {
        AiPanelSubview::Conversation => show_conversation_view(app, ui),
        AiPanelSubview::SessionsList => show_sessions_view(app, ui),
    }
}

fn show_conversation_view(app: &mut FigApp, ui: &mut Ui) {
    let Some(session) = app.ui.ai.sessions.get(app.ui.ai.active_session_idx) else {
        header(app, ui, "", "");
        conversation_body(app, ui, "", "", false);
        return;
    };
    let (title, session_id) = (session.title.clone(), session.session_id.clone());
    let (target_name, has_messages) = (session.target_name.clone(), !session.messages.is_empty());

    header(app, ui, &title, &session_id);
    conversation_body(app, ui, &session_id, &target_name, has_messages);
}

/// The centered, bounded column the conversation reads in, above the composer.
fn conversation_body(app: &mut FigApp, ui: &mut Ui, session_id: &str, target_name: &str, show_context: bool) {
    let body = ui.available_rect_before_wrap();
    if body.width() <= 0.0 || body.height() <= 0.0 {
        return;
    }
    let column_w = body.width().min(COLUMN_MAX);
    let column = Rect::from_center_size(body.center(), vec2(column_w, body.height()));

    ui.scope_builder(UiBuilder::new().max_rect(column), |ui| {
        // Nothing may paint outside the column, so a long row cannot widen the sidebar.
        ui.set_clip_rect(column.intersect(ui.clip_rect()));
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        if show_context && !target_name.is_empty() {
            // What the turn was sent for, frozen at submission: never the live selection.
            ui.add_space(4.0);
            frozen_context(ui, target_name);
            ui.add_space(2.0);
        }
        let scroll_h = ui.available_height().max(0.0);
        // One salt per conversation, so each keeps its own scroll position and stickiness.
        ScrollArea::vertical()
            .id_salt(("ai_conversation_scroll", session_id))
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .max_height(scroll_h)
            .show(ui, |ui| {
                ui.add_space(MESSAGE_GAP);
                render_messages(app, ui);
                ui.add_space(MESSAGE_GAP);
            });
    });
}

/// The panel header: the conversation's title and session id, one Close control, and the subview
/// tabs. Model, reasoning, Send and Stop belong to the composer below, not here.
fn header(app: &mut FigApp, ui: &mut Ui, title: &str, session_id: &str) {
    let t = Tokens::get(ui.ctx());
    let w = ui.available_width();

    // A short panel gives the header only what it has, so nothing paints into the composer.
    let head_h = HEADER_H.min(ui.available_height().max(0.0));
    let (head, _) = ui.allocate_exact_size(vec2(w, head_h), Sense::hover());
    ui.painter().rect_filled(head, CornerRadius::ZERO, t.panel);
    ui.painter().line_segment([head.left_bottom(), head.right_bottom()], Stroke::new(1.0, t.border));
    if head.height() < 20.0 {
        return;
    }

    // Close the AI panel through the left sidebar's own command: the same control the rail uses.
    let close_rect = Rect::from_center_size(pos2(head.max.x - 20.0, head.min.y + 17.0), vec2(24.0, 24.0));
    let close = ui.interact(close_rect, ui.scope_id().with("ai_close_btn"), Sense::click()).tip(tr("Close AI chat"));
    if close.hovered() {
        ui.painter().rect_filled(close_rect, CornerRadius::same(5), t.hover);
    }
    icons::paint(ui.painter(), close_rect.shrink(5.0), Icon::Close, t.icon);
    if close.clicked() {
        app.run("view.leftSidebar", json!({}));
    }

    // Which conversation this is. The id is data, so it is shown as it is, elided to its room.
    let id_w = (head.width() * 0.3).clamp(0.0, 64.0);
    let id_rect = Rect::from_min_max(pos2(close_rect.min.x - 8.0 - id_w, head.min.y + 4.0), pos2(close_rect.min.x - 8.0, head.min.y + 24.0));
    if !session_id.is_empty() && id_rect.width() > 8.0 {
        let shown = widgets::elide_font(ui, session_id, id_rect.width(), regular(10.0));
        ui.painter().text(id_rect.right_center(), Align2::RIGHT_CENTER, shown, regular(10.0), t.text_tertiary);
        let id_resp = ui.interact(id_rect, ui.scope_id().with("ai_session_id"), Sense::hover());
        id_resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, ui.is_enabled(), session_id));
    }

    let title_rect = Rect::from_min_max(pos2(head.min.x + 10.0, head.min.y + 4.0), pos2(id_rect.min.x - 6.0, head.min.y + 24.0));
    if title_rect.width() > 0.0 {
        ui.scope_builder(UiBuilder::new().max_rect(title_rect), |ui| {
            let title = if title.is_empty() { tr("New Conversation") } else { title };
            ui.add(egui::Label::new(egui::RichText::new(title).font(semibold(12.0)).color(t.text)).truncate());
        });
    }

    if head.height() >= HEADER_H - 0.5 {
        subview_tabs(app, ui, head);
    }
}

/// The Conversation / Sessions subview tabs.
fn subview_tabs(app: &mut FigApp, ui: &mut Ui, head: Rect) {
    let t = Tokens::get(ui.ctx());
    let tab_h = 18.0;
    let y = head.min.y + 25.0;
    let mut x = head.min.x + 8.0;
    for (label, sub) in [(tr("Conversation"), AiPanelSubview::Conversation), (tr("Sessions"), AiPanelSubview::SessionsList)] {
        let text_w = ui.painter().layout_no_wrap(label.to_string(), semibold(11.0), t.text).size().x;
        let tab_w = (text_w + 16.0).max(44.0);
        let rect = Rect::from_min_size(pos2(x, y), vec2(tab_w, tab_h));
        // Keep clear of the Close control in the row above.
        if rect.max.x > head.max.x - 36.0 {
            break;
        }
        let resp = ui.interact(rect, ui.scope_id().with(("ai_panel_tab", label)), Sense::click());
        let selected = app.ui.ai.subview == sub;
        a11y::tab(&resp, label, selected);
        if selected {
            ui.painter().rect_filled(rect, CornerRadius::same(4), t.selected_row);
        } else if resp.hovered() {
            ui.painter().rect_filled(rect, CornerRadius::same(4), t.hover);
        }
        let color = if selected { t.text } else { t.text_secondary };
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, semibold(11.0), color);
        if resp.clicked() {
            app.ui.ai.subview = sub;
        }
        x += tab_w + 4.0;
    }
}

/// The references the conversation was last sent for, in place of a live selection description.
fn frozen_context(ui: &mut Ui, names: &str) {
    let t = Tokens::get(ui.ctx());
    let line = trf("Sent for {names}", &[("names", &names)]);
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.add(egui::Label::new(egui::RichText::new(&line).font(regular(10.0)).color(t.text_secondary)).truncate()).on_hover_text(names);
    });
}

/// The conversation timeline: messages, any approval card, and one progress indicator.
fn render_messages(app: &mut FigApp, ui: &mut Ui) {
    let session_idx = app.ui.ai.active_session_idx;
    if app.ui.ai.sessions.get(session_idx).is_none_or(|session| session.messages.is_empty()) {
        empty_state(ui);
        progress_status(app, ui, session_idx);
        return;
    }

    let mut pending_dismiss: Option<serde_json::Value> = None;
    {
        let Some(session) = app.ui.ai.sessions.get(session_idx) else { return };
        let max_w = (ui.available_width() * MESSAGE_SHARE).max(0.0);
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            // Failed calls are one story per call id: an identical retry does not repeat itself.
            let mut failed: Vec<(&str, &str)> = Vec::new();
            for msg in &session.messages {
                match msg.role {
                    MessageRole::User => user_message(ui, msg, max_w),
                    MessageRole::Assistant => assistant_message(ui, &msg.text, max_w),
                    MessageRole::System => system_message(ui, &msg.text, max_w),
                    MessageRole::Tool => {
                        if let Some(tool) = msg.tool_call.as_ref() {
                            failed_tool_message(ui, tool, max_w, &mut failed);
                        }
                    }
                }
            }
        });
        // The runtime already declined the request; the card stays until dismissed.
        if let Some(approval) = &session.pending_approval {
            pending_dismiss = approval_card(ui, approval, max_w);
        }
    }

    if let Some(request_id) = pending_dismiss
        && let Some(session) = app.ui.ai.sessions.get_mut(session_idx)
        && session.pending_approval.as_ref().is_some_and(|request| request.request_id == request_id)
    {
        session.pending_approval = None;
    }

    progress_status(app, ui, session_idx);
}

/// One progress indicator for a running turn, below the newest text and above the composer. A
/// turn running in another conversation is named there, without a spinner in this idle one.
fn progress_status(app: &FigApp, ui: &mut Ui, session_idx: usize) {
    if !app.ui.ai.is_running {
        return;
    }
    let running_here = app.ui.ai.sessions.get(session_idx).is_some_and(|session| session.status == TurnStatus::Running);
    let running_elsewhere = !running_here && app.ui.ai.sessions.iter().any(|session| session.status == TurnStatus::Running);
    let t = Tokens::get(ui.ctx());
    ui.add_space(MESSAGE_GAP);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if running_elsewhere {
            ui.add(egui::Label::new(egui::RichText::new(tr("AI is working in another conversation.")).font(regular(11.0)).color(t.text_secondary)));
        } else {
            ui.add(egui::Spinner::new().size(12.0).color(t.text_secondary));
            ui.add(egui::Label::new(egui::RichText::new(tr("Working…")).font(regular(11.0)).color(t.text_secondary)));
        }
    });
}

fn empty_state(ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(24.0);
    ui.vertical_centered(|ui| {
        let center_x = ui.max_rect().center().x;
        let y = ui.cursor().min.y + 16.0;
        icons::paint(ui.painter(), Rect::from_center_size(pos2(center_x, y), vec2(28.0, 28.0)), Icon::AiChat, t.accent_text);
        ui.add_space(36.0);
        ui.add(egui::Label::new(egui::RichText::new(tr("Design with Codex")).font(semibold(13.0)).color(t.text)).wrap());
        ui.add_space(4.0);
        ui.add(
            egui::Label::new(
                egui::RichText::new(tr("Describe a screen, or click an object on the canvas to mention it."))
                    .font(regular(11.0))
                    .color(t.text_secondary),
            )
            .wrap(),
        );
    });
}

/// A user bubble, capped at the message width, with its frozen mentions.
fn user_message(ui: &mut Ui, msg: &AiMessage, max_w: f32) {
    let t = Tokens::get(ui.ctx());
    let text_w = (max_w - 16.0).max(0.0);
    ui.add_space(MESSAGE_GAP);
    let frame = egui::Frame::NONE.fill(t.hover).corner_radius(CornerRadius::same(6)).inner_margin(egui::Margin::same(8));
    frame.show(ui, |ui| {
        ui.set_max_width(text_w);
        wrapped_text(ui, &msg.text, regular(12.0), t.text, text_w);
        if !msg.mentions.is_empty() {
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                for mention in &msg.mentions {
                    mention_chip(ui, mention);
                }
            });
        }
    });
}

/// An assistant message: plain text, wrapped inside the column.
fn assistant_message(ui: &mut Ui, text: &str, max_w: f32) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(MESSAGE_GAP);
    wrapped_text(ui, text, regular(12.0), t.text, max_w);
}

/// A system notice: an informational line, wrapped inside the column.
fn system_message(ui: &mut Ui, text: &str, max_w: f32) {
    if text.is_empty() {
        return;
    }
    let t = Tokens::get(ui.ctx());
    ui.add_space(4.0);
    wrapped_text(ui, text, regular(10.0), t.text_secondary, max_w);
}

/// A failed tool call: routine queued, running and done records stay in the data, unseen.
fn failed_tool_message<'a>(ui: &mut Ui, tool: &'a ToolCallInfo, max_w: f32, failed: &mut Vec<(&'a str, &'a str)>) {
    if tool.status != ToolStatus::Failed {
        return;
    }
    let summary = tool.result_summary.as_deref().unwrap_or_default();
    if failed.iter().any(|(id, text)| *id == tool.tool_id && *text == summary) {
        return;
    }
    failed.push((&tool.tool_id, summary));
    let t = Tokens::get(ui.ctx());
    let line = if summary.is_empty() { tool.tool_name.clone() } else { format!("{}: {summary}", tool.tool_name) };
    ui.add_space(4.0);
    wrapped_text(ui, &line, regular(10.0), t.danger, max_w);
}

/// The mention a message was sent with: `@Label`, capped to the room it has.
fn mention_chip(ui: &mut Ui, mention: &ObjectMention) {
    let t = Tokens::get(ui.ctx());
    let room = (ui.available_width() - 10.0).max(0.0);
    if room <= 0.0 {
        return;
    }
    let text = widgets::elide_font(ui, &format!("@{}", mention.label), room, regular(10.0));
    let galley = ui.painter().layout_no_wrap(text.clone(), regular(10.0), t.accent_text);
    let width = (galley.size().x + 10.0).min(ui.available_width()).max(0.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 16.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(4), t.selected_row);
    ui.painter().galley(pos2(rect.min.x + 5.0, rect.center().y - galley.size().y * 0.5), galley, t.accent_text);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, ui.is_enabled(), text.clone()));
}

/// Text laid out to wrap inside `max_w`, breaking a word that has no break of its own so a long
/// line can never overflow the column.
fn wrapped_text(ui: &mut Ui, text: &str, font: FontId, color: Color32, max_w: f32) {
    if text.is_empty() || max_w <= 0.0 {
        return;
    }
    let job = |break_anywhere: bool| {
        let mut job = egui::text::LayoutJob::default();
        job.wrap.max_width = max_w;
        job.wrap.break_anywhere = break_anywhere;
        job.append(text, 0.0, egui::TextFormat { font_id: font.clone(), color, ..Default::default() });
        job
    };
    let galley = ui.painter().fonts_mut(|f| f.layout_job(job(false)));
    let galley = if galley.size().x > max_w + 0.5 { ui.painter().fonts_mut(|f| f.layout_job(job(true))) } else { galley };
    ui.add(egui::Label::new(egui::WidgetText::Galley(galley)));
}

/// The pending approval card. Any pending approval is the reader's, so it always draws.
fn approval_card(ui: &mut Ui, approval: &PendingApproval, max_w: f32) -> Option<serde_json::Value> {
    let t = Tokens::get(ui.ctx());
    let mut dismiss = None;
    ui.add_space(MESSAGE_GAP);
    ui.scope(|ui| {
        ui.set_max_width(max_w);
        let frame = egui::Frame::NONE
            .fill(t.warning.gamma_multiply(0.15))
            .stroke(Stroke::new(1.0, t.warning.gamma_multiply(0.4)))
            .corner_radius(CornerRadius::same(6))
            .inner_margin(egui::Margin::same(10));
        frame.show(ui, |ui| {
            let inner_w = (max_w - 20.0).max(0.0);
            ui.set_max_width(inner_w);
            wrapped_text(ui, tr("External Permission Denied"), semibold(12.0), t.warning, inner_w);
            ui.add_space(2.0);
            wrapped_text(
                ui,
                tr("External shell/filesystem execution is disabled under canvas design scope."),
                regular(10.0),
                t.text_secondary,
                inner_w,
            );
            ui.add_space(4.0);
            wrapped_text(ui, &approval.action, regular(11.0), t.text, inner_w);
            if !approval.target.is_empty() {
                let line = trf("Target: {target}", &[("target", &approval.target)]);
                wrapped_text(ui, &line, regular(10.0), t.text_secondary, inner_w);
            }
            ui.add_space(6.0);
            if ui.button(tr("Dismiss (Deny)")).clicked() {
                dismiss = Some(approval.request_id.clone());
            }
        });
    });
    dismiss
}

fn show_sessions_view(app: &mut FigApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let w = ui.available_width();

    // Header
    let (head, _) = ui.allocate_exact_size(vec2(w, 36.0), Sense::hover());
    ui.painter().rect_filled(head, CornerRadius::ZERO, t.panel);
    ui.painter().line_segment([head.left_bottom(), head.right_bottom()], Stroke::new(1.0, t.border));

    let back_btn = Rect::from_min_size(pos2(head.min.x + 8.0, head.min.y + 6.0), vec2(60.0, 24.0));
    let back_resp = ui.interact(back_btn, ui.scope_id().with("ai_sess_back"), Sense::click());
    if back_resp.hovered() {
        ui.painter().rect_filled(back_btn, CornerRadius::same(4), t.hover);
    }
    ui.painter().text(back_btn.center(), Align2::CENTER_CENTER, tr("← Chat"), regular(11.0), t.text_secondary);
    if back_resp.clicked() {
        app.ui.ai.subview = AiPanelSubview::Conversation;
    }

    // New session button on right
    let new_btn = Rect::from_min_size(pos2(head.max.x - 70.0, head.min.y + 6.0), vec2(62.0, 24.0));
    let new_resp = ui.interact(new_btn, ui.scope_id().with("ai_sess_new"), Sense::click());
    if new_resp.hovered() {
        ui.painter().rect_filled(new_btn, CornerRadius::same(4), t.hover);
    }
    ui.painter().text(new_btn.center(), Align2::CENTER_CENTER, tr("+ New"), semibold(11.0), t.accent_text);
    if new_resp.clicked() {
        app.ui.ai.create_new_session(&app.session);
        app.ui.ai.subview = AiPanelSubview::Conversation;
    }

    // List of sessions
    ScrollArea::vertical().id_salt("ai_sessions_list_scroll").auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space(8.0);
        let mut switch_idx = None;
        for (i, session) in app.ui.ai.sessions.iter().enumerate() {
            let is_active = i == app.ui.ai.active_session_idx;
            let row_w = (ui.available_width() - 16.0).max(0.0);
            let (row, _) = ui.allocate_exact_size(vec2(row_w, 48.0), Sense::hover());
            let resp = ui.interact(row, ui.scope_id().with(("sess_row", i)), Sense::click());

            let fill = if is_active {
                t.selected_row
            } else if resp.hovered() {
                t.hover
            } else {
                t.panel
            };
            ui.painter().rect_filled(row, CornerRadius::same(6), fill);

            let status_dot = match session.status {
                TurnStatus::Running => t.accent_text,
                TurnStatus::Stopped => t.warning,
                TurnStatus::Failed => t.danger,
                _ => t.text_secondary,
            };
            ui.painter().circle_filled(pos2(row.min.x + 12.0, row.center().y), 3.5, status_dot);

            // A long name or reference summary stays inside the row.
            let text_room = (row.width() - 32.0).max(0.0);
            let title = widgets::elide_font(ui, &session.title, text_room, semibold(12.0));
            ui.painter().text(pos2(row.min.x + 24.0, row.min.y + 14.0), Align2::LEFT_CENTER, title, semibold(12.0), t.text);
            let summary = widgets::elide_font(ui, &format!("{} • {}", session.target_name, session.created_at), text_room, regular(10.0));
            ui.painter().text(pos2(row.min.x + 24.0, row.min.y + 32.0), Align2::LEFT_CENTER, summary, regular(10.0), t.text_secondary);

            if resp.clicked() {
                switch_idx = Some(i);
            }
            ui.add_space(4.0);
        }

        if let Some(idx) = switch_idx {
            app.ui.ai.active_session_idx = idx;
            app.ui.ai.subview = AiPanelSubview::Conversation;
        }
    });
}
