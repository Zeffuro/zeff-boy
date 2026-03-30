use std::time::Instant;

#[derive(Clone, Copy)]
pub(crate) enum ToastKind {
    Info,
    Success,
    Error,
}

struct Toast {
    message: String,
    created: Instant,
    kind: ToastKind,
}

struct PersistentToast {
    id: &'static str,
    label: String,
    color: egui::Color32,
    started: Option<Instant>,
}

impl PersistentToast {
    fn display_text(&self, now: Instant) -> String {
        match self.started {
            Some(start) => {
                let elapsed = now.duration_since(start).as_secs();
                let h = elapsed / 3600;
                let m = (elapsed % 3600) / 60;
                let s = elapsed % 60;
                let time_str = if h > 0 {
                    format!("{h:02}:{m:02}:{s:02}")
                } else {
                    format!("{m:02}:{s:02}")
                };
                format!("{}:{time_str}", self.label)
            }
            None => self.label.clone(),
        }
    }
}

const TOAST_DURATION_SECS: f32 = 3.0;
const FADE_START: f32 = TOAST_DURATION_SECS - 0.5;

pub(crate) struct ToastManager {
    toasts: Vec<Toast>,
    persistent: Vec<PersistentToast>,
}

impl Default for ToastManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ToastManager {
    pub(crate) fn new() -> Self {
        Self {
            toasts: Vec::new(),
            persistent: Vec::new(),
        }
    }

    pub(crate) fn info(&mut self, msg: impl Into<String>) {
        self.push(msg.into(), ToastKind::Info);
    }

    pub(crate) fn success(&mut self, msg: impl Into<String>) {
        self.push(msg.into(), ToastKind::Success);
    }

    pub(crate) fn error(&mut self, msg: impl Into<String>) {
        self.push(msg.into(), ToastKind::Error);
    }

    pub(crate) fn set_persistent(
        &mut self,
        id: &'static str,
        active: bool,
        label: &str,
        color: egui::Color32,
        with_timer: bool,
    ) {
        if active {
            if !self.persistent.iter().any(|p| p.id == id) {
                self.persistent.push(PersistentToast {
                    id,
                    label: label.to_owned(),
                    color,
                    started: if with_timer {
                        Some(Instant::now())
                    } else {
                        None
                    },
                });
            }
        } else {
            self.persistent.retain(|p| p.id != id);
        }
    }

    pub(crate) fn set_paused(&mut self, active: bool) {
        self.set_persistent(
            "paused",
            active,
            "⏸ Paused",
            egui::Color32::from_rgba_unmultiplied(50, 50, 90, 220),
            false,
        );
    }

    pub(crate) fn set_recording(&mut self, active: bool) {
        self.set_persistent(
            "recording",
            active,
            "🔴 Recording",
            egui::Color32::from_rgba_unmultiplied(140, 30, 30, 220),
            true,
        );
    }

    pub(crate) fn set_replay_recording(&mut self, active: bool) {
        self.set_persistent(
            "replay_recording",
            active,
            "⏺ Recording Replay",
            egui::Color32::from_rgba_unmultiplied(130, 80, 30, 220),
            true,
        );
    }

    fn push(&mut self, message: String, kind: ToastKind) {
        self.toasts.push(Toast {
            message,
            created: Instant::now(),
            kind,
        });
    }

    pub(crate) fn draw(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        self.toasts
            .retain(|t| now.duration_since(t.created).as_secs_f32() < TOAST_DURATION_SECS);

        let has_content = !self.toasts.is_empty() || !self.persistent.is_empty();
        if !has_content {
            return;
        }

        egui::Area::new(egui::Id::new("toast_overlay"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-10.0, -10.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.set_min_width(160.0);

                for pt in &self.persistent {
                    let text = pt.display_text(now);
                    egui::Frame::new()
                        .fill(pt.color)
                        .inner_margin(egui::Margin::symmetric(16, 8))
                        .corner_radius(4.0)
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(text)
                                    .color(egui::Color32::WHITE)
                                    .size(15.0),
                            );
                        });
                    ui.add_space(4.0);
                }

                for toast in self.toasts.iter().rev() {
                    let elapsed = now.duration_since(toast.created).as_secs_f32();
                    let alpha = if elapsed > FADE_START {
                        ((TOAST_DURATION_SECS - elapsed) / 0.5).clamp(0.0, 1.0)
                    } else {
                        1.0
                    };

                    let bg = match toast.kind {
                        ToastKind::Info => {
                            egui::Color32::from_rgba_unmultiplied(50, 50, 70, (alpha * 230.0) as u8)
                        }
                        ToastKind::Success => {
                            egui::Color32::from_rgba_unmultiplied(30, 80, 30, (alpha * 230.0) as u8)
                        }
                        ToastKind::Error => egui::Color32::from_rgba_unmultiplied(
                            120,
                            30,
                            30,
                            (alpha * 230.0) as u8,
                        ),
                    };
                    let text_alpha = (alpha * 255.0) as u8;

                    egui::Frame::new()
                        .fill(bg)
                        .inner_margin(egui::Margin::symmetric(16, 8))
                        .corner_radius(4.0)
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(&toast.message)
                                    .color(egui::Color32::from_rgba_unmultiplied(
                                        255, 255, 255, text_alpha,
                                    ))
                                    .size(15.0),
                            );
                        });
                    ui.add_space(4.0);
                }
            });

        ctx.request_repaint();
    }
}
