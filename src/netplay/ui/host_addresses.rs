use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use anyhow::{Result, ensure};

use crate::netplay::adapters::{self, HostAddress};

pub(super) struct HostAddresses {
    choices: Vec<HostAddress>,
    selected: Option<HostAddress>,
    pending: Option<Receiver<Vec<HostAddress>>>,
    ready: bool,
    failed: bool,
    custom: bool,
    forwarded: bool,
}

impl HostAddresses {
    pub(super) fn new(forwarded: bool) -> Self {
        Self {
            choices: vec![HostAddress {
                adapter: if forwarded {
                    "Forwarded connection"
                } else {
                    "Loopback"
                }
                .into(),
                address: std::net::Ipv4Addr::LOCALHOST.into(),
                recommended: forwarded,
            }],
            selected: None,
            pending: None,
            ready: false,
            failed: false,
            custom: false,
            forwarded,
        }
    }

    fn refresh(&mut self) {
        if self.pending.is_some() {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        match thread::Builder::new()
            .name("netplay-adapters".into())
            .spawn(move || {
                let _ = sender.send(adapters::discover());
            }) {
            Ok(_) => {
                self.pending = Some(receiver);
                self.failed = false;
            }
            Err(_) => {
                self.ready = true;
                self.failed = true;
            }
        }
    }

    fn poll(&mut self, address: &mut String) {
        if !self.ready && self.pending.is_none() {
            self.refresh();
        }
        let Some(receiver) = &self.pending else {
            return;
        };
        match receiver.try_recv() {
            Ok(choices) => {
                self.pending = None;
                self.apply_snapshot(choices, address);
            }
            Err(TryRecvError::Disconnected) => {
                self.pending = None;
                self.ready = true;
                self.failed = true;
            }
            Err(TryRecvError::Empty) => {}
        }
    }

    fn apply_snapshot(&mut self, mut choices: Vec<HostAddress>, address: &mut String) {
        if self.forwarded {
            for choice in &mut choices {
                choice.recommended = choice.address.is_loopback();
                if choice.address.is_loopback() {
                    choice.adapter = "Forwarded connection".into();
                }
            }
        }
        self.choices = choices;
        self.ready = true;
        self.failed = false;
        if self.selected.is_none() && !self.custom {
            let choice = if address.trim().is_empty() {
                self.choices.iter().find(|choice| choice.recommended)
            } else {
                self.choices
                    .iter()
                    .find(|choice| choice.address.to_string() == address.trim())
            };
            if let Some(choice) = choice {
                *address = choice.address.to_string();
                self.selected = Some(choice.clone());
            } else if !address.trim().is_empty() {
                self.custom = true;
            }
        }
    }

    fn available(&self, choice: &HostAddress) -> bool {
        self.choices
            .iter()
            .any(|entry| entry.adapter == choice.adapter && entry.address == choice.address)
    }

    pub(super) fn validate(&self, address: &str) -> Result<()> {
        if let Some(choice) = &self.selected
            && choice.address.to_string() == address.trim()
        {
            ensure!(
                self.available(choice),
                "Selected adapter is unavailable. Refresh and choose another address."
            );
        }
        Ok(())
    }

    fn label(choice: &HostAddress) -> String {
        format!(
            "{} — {}{}",
            choice.adapter,
            choice.address,
            if choice.recommended {
                " (Recommended)"
            } else {
                ""
            }
        )
    }

    pub(super) fn draw(&mut self, ui: &mut egui::Ui, address: &mut String) {
        self.poll(address);
        ui.label("Adapter");
        let label = if self.custom {
            "Custom IP".into()
        } else if let Some(choice) = &self.selected {
            if self.available(choice) {
                let current = self
                    .choices
                    .iter()
                    .find(|row| row.adapter == choice.adapter && row.address == choice.address)
                    .unwrap();
                Self::label(current)
            } else {
                format!("Unavailable — {}", choice.address)
            }
        } else {
            "Choose an adapter".into()
        };
        let mut chosen = None;
        let mut custom = false;
        ui.horizontal(|ui| {
            let refresh_width = ui
                .painter()
                .layout_no_wrap(
                    "Refresh".into(),
                    egui::TextStyle::Button.resolve(ui.style()),
                    ui.visuals().text_color(),
                )
                .size()
                .x
                + 2.0 * ui.spacing().button_padding.x;
            let width =
                (ui.available_width() - refresh_width - ui.spacing().item_spacing.x).max(24.0);
            ui.allocate_ui_with_layout(
                egui::vec2(width, ui.spacing().interact_size.y),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    egui::ComboBox::from_id_salt("netplay-host-adapter")
                        .selected_text(label)
                        .truncate()
                        .width(width)
                        .show_ui(ui, |ui| {
                            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                            ui.set_width(
                                (ui.ctx().content_rect().width() - 24.0).clamp(120.0, 420.0),
                            );
                            for choice in &self.choices {
                                let selected = self.selected.as_ref().is_some_and(|row| {
                                    row.adapter == choice.adapter && row.address == choice.address
                                });
                                if ui.selectable_label(selected, Self::label(choice)).clicked() {
                                    chosen = Some(choice.clone());
                                }
                            }
                            custom = ui.selectable_label(self.custom, "Custom IP").clicked();
                        });
                },
            );
            if ui
                .add_enabled(
                    self.pending.is_none(),
                    egui::Button::new("Refresh").wrap_mode(egui::TextWrapMode::Extend),
                )
                .clicked()
            {
                self.refresh();
            }
        });
        if let Some(choice) = chosen {
            *address = choice.address.to_string();
            self.selected = Some(choice);
            self.custom = false;
        } else if custom {
            self.selected = None;
            self.custom = true;
        }
        if self.custom {
            ui.add(
                egui::TextEdit::singleline(address)
                    .desired_width(f32::INFINITY)
                    .hint_text("Numeric local/private IP"),
            );
        }
        if self.pending.is_some() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(50));
            ui.small("Finding adapters…");
        } else if self.failed {
            ui.small("Could not list adapters. Refresh or use Custom IP.");
        } else if self.ready && !self.choices.iter().any(|row| !row.address.is_loopback()) {
            ui.small("No LAN addresses found.");
        }
    }

    #[cfg(test)]
    pub(super) fn supply(&mut self, choices: Vec<HostAddress>, address: &mut String) {
        self.pending = None;
        self.apply_snapshot(choices, address);
    }

    #[cfg(test)]
    pub(super) fn is_ready(&self) -> bool {
        self.ready
    }
}

#[cfg(test)]
mod tests;
