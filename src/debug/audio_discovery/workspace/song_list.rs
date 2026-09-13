use super::*;

pub(super) fn draw_song_list(
    ui: &mut egui::Ui,
    workspace: &mut AudioWorkspace,
    report: &ScanReport,
    classifications: &crate::audio_discovery::roles::Classifications,
    can_preview: &impl Fn(SongId) -> bool,
) {
    ui.horizontal_wrapped(|ui| {
        ui.heading("Audio entries");
        ui.menu_button("Tables", |ui| {
            draw_tables(ui, workspace, report);
        });
    });
    ui.add(
        egui::TextEdit::singleline(&mut workspace.filter)
            .hint_text("Filter songs or offsets")
            .desired_width(f32::INFINITY),
    );
    use zeff_audio_discovery::classification::AudioRole;
    egui::ComboBox::from_id_salt("audio-role-filter")
        .selected_text(workspace.role_filter.map_or("All roles", AudioRole::label))
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut workspace.role_filter, None, "All roles");
            for role in AudioRole::ALL {
                ui.selectable_value(&mut workspace.role_filter, Some(role), role.label());
            }
        });
    let filtered: Vec<_> = filtered_song_ids(report, &workspace.filter)
        .into_iter()
        .filter(|&id| {
            let song = report.song(id).expect("catalog entry");
            let role = classifications
                .get(&song.classification_key())
                .map_or_else(|| song.classification().role, |entry| entry.current.role);
            workspace.role_filter.is_none_or(|filter| filter == role)
        })
        .collect();
    ui.small(format!(
        "{} shown · {} song tables",
        filtered.len(),
        report.song_tables.len()
    ));
    ui.small("Right-click an entry to assign its role.");
    let row_height = ui.text_style_height(&egui::TextStyle::Body) + 4.0;
    egui::ScrollArea::vertical()
        .id_salt("audio-discovery-songs")
        .max_height(ui.available_height())
        .show_rows(ui, row_height, filtered.len(), |ui, rows| {
            for visible_index in rows {
                let id = filtered[visible_index];
                let song = report
                    .song(id)
                    .expect("filtered songs come from this report");
                let selected = workspace.selected_candidate == Some(id);
                let classification = classifications
                    .get(&song.classification_key())
                    .map_or_else(|| song.classification(), |entry| entry.current.clone());
                let label = format!("{} · {}", song_label(song), classification.role.label());
                let previewable = can_preview(id);
                let response = ui
                    .add(egui::Button::selectable(selected, &label).truncate())
                    .on_hover_text(if previewable {
                        format!("{label}\nDouble-click to preview.")
                    } else {
                        format!("{label}\nPreview is unavailable for this entry.")
                    });
                if response.clicked() {
                    select_song(workspace, id, song);
                }
                if previewable
                    && (response.double_clicked()
                        || (selected
                            && response.has_focus()
                            && ui.input(|input| input.key_pressed(egui::Key::Enter))))
                {
                    workspace.preview_request = Some(id);
                }
                response.context_menu(|ui| {
                    ui.label(classification.role.label());
                    ui.small(&classification.reason);
                    ui.menu_button("Assign role", |ui| {
                        if ui.button("Automatic (driver evidence)").clicked() {
                            workspace.role_request = Some((id, None));
                            ui.close();
                        }
                        for role in AudioRole::ALL {
                            if ui.button(role.label()).clicked() {
                                workspace.role_request = Some((id, Some(role)));
                                ui.close();
                            }
                        }
                    });
                    ui.separator();
                    if ui
                        .add_enabled(previewable, egui::Button::new("Preview"))
                        .clicked()
                    {
                        select_song(workspace, id, song);
                        workspace.preview_request = Some(id);
                        ui.close();
                    }
                    if ui.button("View song details").clicked() {
                        select_song(workspace, id, song);
                        workspace.show_details = true;
                        workspace.compact_tab = CompactTab::Details;
                        ui.close();
                    }
                    if ui.button("Open export controls").clicked() {
                        select_song(workspace, id, song);
                        workspace.preview_request = None;
                        workspace.export_request = Some(id);
                        ui.close();
                    }
                });
            }
        });
}

pub(super) fn select_song(workspace: &mut AudioWorkspace, id: SongId, song: SongRef<'_>) {
    workspace.selected_candidate = Some(id);
    workspace.show_details = false;
    workspace.relationships = None;
    if let Some(span) = song.span() {
        select_span(workspace, span, format!("{} header", song.title()));
    } else {
        workspace.selected_span = None;
        workspace.hex_reset = true;
    }
}

fn draw_tables(ui: &mut egui::Ui, workspace: &mut AudioWorkspace, report: &ScanReport) {
    use crate::audio_discovery::SongTableEntryKind;
    for table in &report.song_tables {
        let placeholders = table
            .entries
            .iter()
            .filter(|entry| {
                matches!(
                    entry.kind,
                    SongTableEntryKind::Null | SongTableEntryKind::Placeholder
                )
            })
            .count();
        egui::CollapsingHeader::new(format!(
            "Table +{:06X} · {} slots",
            table.table.effective_offset,
            table.entries.len()
        ))
        .show(ui, |ui| {
            ui.small(format!("{placeholders} empty / placeholder slots"));
            span_button(ui, workspace, table.selector, "MP2k song selector");
            span_button(ui, workspace, table.settings, "Engine settings");
            span_button(ui, workspace, table.table, "Song table");
            ui.small(format!("Observed boundary: {:?}", table.boundary));
            let row_height = ui.text_style_height(&egui::TextStyle::Body) + 4.0;
            egui::ScrollArea::vertical()
                .id_salt(("audio-table-entries", table.table.effective_offset))
                .max_height(160.0)
                .show_rows(ui, row_height, table.entries.len(), |ui, rows| {
                    for index in rows {
                        let entry = &table.entries[index];
                        let label = format!(
                            "Song {} · {:?} · player {}",
                            entry.index, entry.kind, entry.player
                        );
                        if ui
                            .add(egui::Button::new(&label).truncate())
                            .on_hover_text(&label)
                            .clicked()
                        {
                            select_span(
                                workspace,
                                entry.entry,
                                format!("Song {} table entry", entry.index),
                            );
                        }
                    }
                });
        });
    }
}
