use crate::debug::MenuAction;

use super::Ui;

impl Ui {
    pub(crate) fn chat_sent(&mut self, text: &str) {
        self.chat_pending = false;
        self.chat.error.clear();
        if self.chat_draft.trim() == text {
            self.chat_draft.clear();
        }
    }

    pub(crate) fn chat_failed(&mut self, error: String) {
        self.chat_pending = false;
        self.chat.error = error;
    }

    pub(crate) fn clear_chat(&mut self) {
        self.chat_draft.clear();
        self.chat_pending = false;
        self.chat.clear();
    }

    pub(super) fn draw_chat(&mut self, ui: &mut egui::Ui) -> Option<MenuAction> {
        egui::CollapsingHeader::new("Chat")
            .default_open(true)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("netplay-chat-messages")
                    .max_height(72.0)
                    .stick_to_bottom(true)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        if self.chat.messages().next().is_none() {
                            ui.weak("Say hello to the other player.");
                        }
                        for message in self.chat.messages() {
                            ui.label(format!(
                                "{}: {}",
                                if message.local { "You" } else { "Other player" },
                                message.text
                            ));
                        }
                    });
                if !self.chat.error.is_empty() {
                    ui.small(&self.chat.error);
                }
                if !self.connected {
                    ui.weak("Chat is available while connected.");
                    return None;
                }
                let mut send = false;
                ui.horizontal(|ui| {
                    let send_width = 48.0;
                    let width =
                        (ui.available_width() - send_width - ui.spacing().item_spacing.x).max(24.0);
                    let response = ui.add_sized(
                        [width, ui.spacing().interact_size.y],
                        egui::TextEdit::singleline(&mut self.chat_draft)
                            .id_salt("netplay-chat-input")
                            .hint_text("Message")
                            .char_limit(super::super::chat::MAX_BYTES),
                    );
                    let draft = self.chat_draft.trim();
                    let valid =
                        !self.chat_pending && zeff_netplay::wire::validate_chat(draft).is_ok();
                    let enter = response.lost_focus()
                        && ui.input(|input| input.key_pressed(egui::Key::Enter));
                    let clicked = ui
                        .add_enabled(
                            valid,
                            egui::Button::new("Send")
                                .wrap_mode(egui::TextWrapMode::Extend)
                                .min_size(egui::vec2(send_width, 0.0)),
                        )
                        .clicked();
                    send = valid && (clicked || enter);
                    if send || enter {
                        response.request_focus();
                    }
                });
                if self.chat_draft.trim().len() > super::super::chat::MAX_BYTES {
                    ui.small("Message is too long.");
                }
                send.then(|| {
                    self.chat_pending = true;
                    MenuAction::SendNesNetplayChat(self.chat_draft.trim().to_owned())
                })
            })
            .body_returned
            .flatten()
    }
}
