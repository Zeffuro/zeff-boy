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

const TOAST_DURATION_SECS: f32 = 3.0;
const FADE_START: f32 = TOAST_DURATION_SECS - 0.5;

pub(crate) struct ToastManager {
    toasts: Vec<Toast>,
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

        if self.toasts.is_empty() {
            return;
        }

        egui::Area::new(egui::Id::new("toast_overlay"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-10.0, -10.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
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
                        .inner_margin(egui::Margin::symmetric(12, 6))
                        .corner_radius(4.0)
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(&toast.message)
                                    .color(egui::Color32::from_rgba_unmultiplied(
                                        255, 255, 255, text_alpha,
                                    ))
                                    .size(14.0),
                            );
                        });
                    ui.add_space(4.0);
                }
            });

        ctx.request_repaint();
    }
}

