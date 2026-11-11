//! The AI Chat panel rendering: Conversation view and Sessions subview.

use egui::{Align2, CornerRadius, Rect, ScrollArea, Sense, Stroke, Ui, pos2, vec2};
use serde_json::json;

use super::sessions::{MessageRole, ToolStatus, TurnStatus};
use crate::FigApp;
use crate::i18n::tr;
use crate::icons::{self, Icon};
use crate::theme::{Tokens, regular, semibold};

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
    let t = Tokens::get(ui.ctx());
    let w = ui.available_width();

    // Top control bar inside AI Chat: model picker, session title/toggle, status
    let (head, _) = ui.allocate_exact_size(vec2(w, 36.0), Sense::hover());
    ui.painter().rect_filled(head, CornerRadius::ZERO, t.panel);
    ui.painter().line_segment([head.left_bottom(), head.right_bottom()], Stroke::new(1.0, t.border));

    // Sessions history button on left
    let sess_btn = Rect::from_min_size(pos2(head.min.x + 8.0, head.min.y + 6.0), vec2(70.0, 24.0));
    let sess_resp = ui.interact(sess_btn, ui.scope_id().with("ai_sessions_btn"), Sense::click());
    sess_resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, false, tr("Sessions ▾")));
    if sess_resp.hovered() {
        ui.painter().rect_filled(sess_btn, CornerRadius::same(4), t.hover);
    }
    ui.painter().text(sess_btn.center(), Align2::CENTER_CENTER, tr("Sessions ▾"), regular(11.0), t.text_secondary);
    if sess_resp.clicked() {
        app.ui.ai.subview = AiPanelSubview::SessionsList;
    }

    // Model name / status on right
    let model_text = if app.ui.ai.selected_model.is_empty() { "gpt-6-luna" } else { &app.ui.ai.selected_model };
    let model_btn = Rect::from_min_size(pos2(head.max.x - 146.0, head.min.y + 6.0), vec2(74.0, 24.0));
    let model_resp = ui.interact(model_btn, ui.scope_id().with("ai_model_picker"), Sense::click());
    model_resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, false, format!("Model: {model_text}")));
    if model_resp.hovered() {
        ui.painter().rect_filled(model_btn, CornerRadius::same(4), t.hover);
    }
    ui.painter().text(model_btn.center(), Align2::CENTER_CENTER, model_text, regular(10.0), t.accent_text);
    if model_resp.clicked() && !app.ui.ai.available_models.is_empty() {
        let next_model = if let Some(pos) = app.ui.ai.available_models.iter().position(|m| m == &app.ui.ai.selected_model) {
            let next_idx = (pos + 1) % app.ui.ai.available_models.len();
            app.ui.ai.available_models.get(next_idx).cloned()
        } else {
            app.ui.ai.available_models.first().cloned()
        };
        if let Some(m) = next_model {
            app.ui.ai.select_model(&m);
        }
    }

    // Effort selector button
    let supported_efforts = app.ui.ai.supported_efforts_for_selected_model();
    let effort_text = app.ui.ai.selected_effort.as_deref().unwrap_or("medium");
    let effort_btn = Rect::from_min_size(pos2(head.max.x - 68.0, head.min.y + 6.0), vec2(60.0, 24.0));
    let effort_resp = ui.interact(effort_btn, ui.scope_id().with("ai_effort_picker"), Sense::click());
    effort_resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, false, format!("Effort: {effort_text}")));
    if effort_resp.hovered() {
        ui.painter().rect_filled(effort_btn, CornerRadius::same(4), t.hover);
    }
    ui.painter().text(effort_btn.center(), Align2::CENTER_CENTER, effort_text, regular(10.0), t.text_secondary);
    if effort_resp.clicked() && !supported_efforts.is_empty() {
        let next_effort = if let Some(pos) = supported_efforts.iter().position(|e| e == effort_text) {
            let next_idx = (pos + 1) % supported_efforts.len();
            supported_efforts.get(next_idx).cloned()
        } else {
            supported_efforts.first().cloned()
        };
        app.ui.ai.select_effort(next_effort);
    }

    // Scrollable message area
    let avail_h = ui.available_height();
    ScrollArea::vertical().id_salt("ai_conversation_scroll").auto_shrink([false, false]).max_height(avail_h).show(ui, |ui| {
        ui.add_space(8.0);
        render_messages(app, ui);
    });
}

fn render_messages(app: &mut FigApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let _w = ui.available_width();

    let session_idx = app.ui.ai.active_session_idx;
    let Some(session) = app.ui.ai.sessions.get(session_idx) else {
        // Empty state
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            icons::paint(ui.painter(), Rect::from_center_size(ui.cursor().center(), vec2(32.0, 32.0)), Icon::AiChat, t.accent_text);
            ui.add_space(40.0);
            ui.label(egui::RichText::new(tr("Design with Codex")).font(semibold(13.0)).color(t.text));
            ui.add_space(4.0);
            ui.label(egui::RichText::new(tr("Describe a screen or select a frame to edit.")).font(regular(11.0)).color(t.text_secondary));
        });
        return;
    };

    if session.messages.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(30.0);
            ui.label(egui::RichText::new(tr("Ready for your brief")).font(semibold(12.0)).color(t.text));
            ui.add_space(4.0);
            ui.label(egui::RichText::new(format!("Target: {}", session.target_name)).font(regular(11.0)).color(t.accent_text));
        });
        return;
    }

    let mut pending_reply: Option<(serde_json::Value, bool)> = None;

    ui.scope(|ui| {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);

        for msg in &session.messages {
            match msg.role {
                MessageRole::User => {
                    let bubble_w = (ui.available_width() - 24.0).max(80.0);
                    ui.horizontal(|ui| {
                        ui.add_space(12.0);
                        let frame = egui::Frame::NONE.fill(t.hover).corner_radius(CornerRadius::same(6)).inner_margin(egui::Margin::same(8));
                        frame.show(ui, |ui| {
                            ui.set_max_width(bubble_w);
                            ui.add(egui::Label::new(egui::RichText::new(&msg.text).font(regular(12.0)).color(t.text)).wrap());
                        });
                    });
                    ui.add_space(8.0);
                }
                MessageRole::Assistant => {
                    let avail = (ui.available_width() - 16.0).max(80.0);
                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        ui.vertical(|ui| {
                            ui.set_max_width(avail);
                            ui.add(egui::Label::new(egui::RichText::new(&msg.text).font(regular(12.0)).color(t.text)).wrap());
                        });
                    });
                    ui.add_space(8.0);
                }
                MessageRole::Tool => {
                    if let Some(tool) = &msg.tool_call {
                        let row_w = (ui.available_width() - 20.0).max(80.0);
                        ui.horizontal(|ui| {
                            ui.add_space(10.0);
                            let frame = egui::Frame::NONE
                                .fill(t.selected_row)
                                .stroke(Stroke::new(1.0, t.border))
                                .corner_radius(CornerRadius::same(4))
                                .inner_margin(egui::Margin::symmetric(8, 6));
                            frame.show(ui, |ui| {
                                ui.set_max_width(row_w);
                                let status_str = match tool.status {
                                    ToolStatus::Queued => tr("queued"),
                                    ToolStatus::Running => tr("running…"),
                                    ToolStatus::Succeeded => tr("done"),
                                    ToolStatus::Failed => tr("failed"),
                                };
                                let status_color = match tool.status {
                                    ToolStatus::Succeeded => t.accent_text,
                                    ToolStatus::Failed => t.danger,
                                    _ => t.text_secondary,
                                };
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new(&tool.tool_name).font(semibold(11.0)).color(t.text));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.label(egui::RichText::new(status_str).font(regular(10.0)).color(status_color));
                                        let (dot_rect, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                                        ui.painter().circle_filled(dot_rect.center(), 2.5, status_color);
                                    });
                                });
                                if let Some(summary) = &tool.result_summary {
                                    ui.add_space(2.0);
                                    ui.add(egui::Label::new(egui::RichText::new(summary).font(regular(10.0)).color(t.text_secondary)).wrap());
                                }
                            });
                        });
                        ui.add_space(6.0);
                    }
                }
                MessageRole::System => {}
            }
        }
    });

    // Pending approval card if required
    if let Some(approval) = &session.pending_approval {
        ui.add_space(6.0);
        let frame = egui::Frame::NONE
            .fill(t.warning.gamma_multiply(0.15))
            .stroke(Stroke::new(1.0, t.warning.gamma_multiply(0.4)))
            .corner_radius(CornerRadius::same(6))
            .inner_margin(egui::Margin::same(10));

        let is_unsupported_external = approval.action.contains("command")
            || approval.action.contains("shell")
            || approval.action.contains("file")
            || !approval.target.is_empty();

        frame.show(ui, |ui| {
            if is_unsupported_external {
                ui.label(egui::RichText::new(tr("External Permission Denied")).font(semibold(12.0)).color(t.warning));
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(tr("External shell/filesystem execution is disabled under canvas design scope."))
                        .font(regular(10.0))
                        .color(t.text_secondary),
                );
                ui.add_space(4.0);
                ui.label(egui::RichText::new(&approval.action).font(regular(11.0)).color(t.text));
                if !approval.target.is_empty() {
                    ui.label(egui::RichText::new(format!("Target: {}", approval.target)).font(regular(10.0)).color(t.text_secondary));
                }
                ui.add_space(6.0);
                // Do NOT offer "Allow" button for external shell/file execution; visibly decline with schema-correct denial
                if ui.button(tr("Dismiss (Deny)")).clicked() {
                    pending_reply = Some((approval.request_id.clone(), false));
                }
            } else {
                ui.label(egui::RichText::new(tr("Permission Required")).font(semibold(12.0)).color(t.warning));
                ui.add_space(2.0);
                ui.label(egui::RichText::new(&approval.action).font(regular(11.0)).color(t.text));
                if !approval.target.is_empty() {
                    ui.label(egui::RichText::new(format!("Target: {}", approval.target)).font(regular(10.0)).color(t.text_secondary));
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button(tr("Allow")).clicked() {
                        pending_reply = Some((approval.request_id.clone(), true));
                    }
                    if ui.button(tr("Deny")).clicked() {
                        pending_reply = Some((approval.request_id.clone(), false));
                    }
                });
            }
        });
        ui.add_space(6.0);
    }

    // Apply pending approval response if user clicked
    if let Some((req_id, allow)) = pending_reply {
        if let Some(s) = app.ui.ai.sessions.get_mut(session_idx) {
            s.pending_approval = None;
            if s.status == TurnStatus::AwaitingApproval {
                s.status = TurnStatus::Running;
            }
        }
        if let Some(sup) = app.ui.ai.supervisor.as_mut() {
            if allow {
                sup.record_and_send_response(&req_id, json!({ "decision": "allow" }));
            } else {
                sup.record_and_send_response(&req_id, json!({ "decision": "decline", "denied": { "rejection": "Action declined by user" } }));
            }
        }
    }
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
            let (row, _) = ui.allocate_exact_size(vec2(w - 16.0, 48.0), Sense::hover());
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

            ui.painter().text(pos2(row.min.x + 24.0, row.min.y + 14.0), Align2::LEFT_CENTER, &session.title, semibold(12.0), t.text);
            ui.painter().text(
                pos2(row.min.x + 24.0, row.min.y + 32.0),
                Align2::LEFT_CENTER,
                format!("{} • {}", session.target_name, session.created_at),
                regular(10.0),
                t.text_secondary,
            );

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
