use crate::debug::TasEditorLiveStatus;

pub(super) fn label(status: &TasEditorLiveStatus) -> Option<String> {
    match status {
        TasEditorLiveStatus::Acquiring => Some("TAS · Connecting…".to_owned()),
        TasEditorLiveStatus::Staging { completed, total } => {
            Some(format!("TAS · Connecting… {completed}/{total}"))
        }
        TasEditorLiveStatus::Linked { cursor, .. } => {
            Some(format!("TAS · Connected · before input {cursor}"))
        }
        TasEditorLiveStatus::Playing { cursor, .. } => {
            Some(format!("TAS · ▶ Playing · B({cursor})"))
        }
        TasEditorLiveStatus::AdvancingFrame => Some("TAS · Recording frame…".to_owned()),
        TasEditorLiveStatus::Recording => Some("TAS · Recording".to_owned()),
        TasEditorLiveStatus::Returning => Some("TAS · Restoring game…".to_owned()),
        TasEditorLiveStatus::Keeping => Some("TAS · Keeping game…".to_owned()),
        TasEditorLiveStatus::Terminal(_) => Some("TAS · Needs attention".to_owned()),
        TasEditorLiveStatus::Unavailable(_)
        | TasEditorLiveStatus::ReloadRequired(_)
        | TasEditorLiveStatus::Ready { .. } => None,
    }
}

pub(super) fn fits(available_width: f32, indicator_width: f32, essential_width: f32) -> bool {
    available_width >= indicator_width + essential_width
}

pub(super) fn measure(ui: &egui::Ui, label: &str) -> f32 {
    ui.painter()
        .layout_no_wrap(
            label.to_owned(),
            egui::TextStyle::Small.resolve(ui.style()),
            ui.visuals().text_color(),
        )
        .size()
        .x
}

pub(super) fn compact_width(ui: &egui::Ui) -> f32 {
    measure(ui, "TAS") + 6.0
}

pub(super) fn draw(ui: &mut egui::Ui, status: &TasEditorLiveStatus, label: &str) {
    let color = match status {
        TasEditorLiveStatus::Recording => ui.visuals().hyperlink_color,
        TasEditorLiveStatus::Terminal(_) => ui.visuals().error_fg_color,
        _ => ui.visuals().warn_fg_color,
    };
    ui.label(egui::RichText::new(label).small().color(color))
        .on_hover_text(status.primary_label());
}

pub(super) fn draw_compact(ui: &mut egui::Ui, status: &TasEditorLiveStatus) {
    let color = match status {
        TasEditorLiveStatus::Recording => ui.visuals().hyperlink_color,
        TasEditorLiveStatus::Terminal(_) => ui.visuals().error_fg_color,
        _ => ui.visuals().warn_fg_color,
    };
    ui.label(egui::RichText::new("TAS").small().color(color))
        .on_hover_text(status.primary_label());
}

#[cfg(test)]
mod tests {
    use super::{compact_width, fits, label};
    use crate::debug::TasEditorLiveStatus;

    #[test]
    fn session_indicator_covers_live_coordinator_states() {
        assert_eq!(
            label(&TasEditorLiveStatus::Acquiring).as_deref(),
            Some("TAS · Connecting…")
        );
        assert_eq!(
            label(&TasEditorLiveStatus::Staging {
                completed: 4,
                total: 9,
            })
            .as_deref(),
            Some("TAS · Connecting… 4/9")
        );
        assert_eq!(
            label(&TasEditorLiveStatus::Linked {
                cursor: 42,
                recording_available: true,
            })
            .as_deref(),
            Some("TAS · Connected · before input 42")
        );
        assert_eq!(
            label(&TasEditorLiveStatus::AdvancingFrame).as_deref(),
            Some("TAS · Recording frame…")
        );
        assert_eq!(
            label(&TasEditorLiveStatus::Recording).as_deref(),
            Some("TAS · Recording")
        );
        assert_eq!(
            label(&TasEditorLiveStatus::Returning).as_deref(),
            Some("TAS · Restoring game…")
        );
        assert_eq!(
            label(&TasEditorLiveStatus::Keeping).as_deref(),
            Some("TAS · Keeping game…")
        );
        assert_eq!(
            label(&TasEditorLiveStatus::Terminal("worker lost".to_owned())).as_deref(),
            Some("TAS · Needs attention")
        );
    }

    #[test]
    fn session_indicator_hides_before_essential_toolbar_control() {
        assert!(fits(110.0, 80.0, 30.0));
        assert!(!fits(109.0, 80.0, 30.0));
    }

    #[test]
    fn compact_session_indicator_has_a_nonzero_reserved_width() {
        let context = egui::Context::default();
        let _ = context.run_ui(Default::default(), |ui| {
            assert!(compact_width(ui) > 0.0);
        });
    }

    #[test]
    fn detached_states_do_not_claim_a_live_session() {
        assert!(
            label(&TasEditorLiveStatus::Ready {
                recording_available: true,
            })
            .is_none()
        );
        assert!(
            label(&TasEditorLiveStatus::ReloadRequired(
                "sample rate".to_owned()
            ))
            .is_none()
        );
        assert!(label(&TasEditorLiveStatus::Unavailable("No game".to_owned())).is_none());
    }
}
