use super::*;

pub(super) fn draw(ui: &mut egui::Ui, workspace: &mut AudioWorkspace, report: &ScanReport) {
    if report.driver_candidates.is_empty() {
        return;
    }
    egui::CollapsingHeader::new(format!(
        "Possible sound drivers ({})",
        report.driver_candidates.len()
    ))
    .default_open(report.song_count() == 0)
    .show(ui, |ui| {
        ui.small("These signatures suggest a driver family. Playable songs have not been established by these matches.");
        egui::ScrollArea::vertical()
            .id_salt("audio-driver-candidates")
            .max_height(180.0)
            .show(ui, |ui| {
                for candidate in &report.driver_candidates {
                    ui.strong(format!("{} {}", candidate.family, candidate.variant).trim());
                    for evidence in &candidate.evidence {
                        span_button(ui, workspace, evidence.span, evidence.signature);
                    }
                }
            });
    });
}
