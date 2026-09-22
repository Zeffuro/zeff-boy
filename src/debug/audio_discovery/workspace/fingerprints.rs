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
        ui.small("ROM evidence that may help locate sound drivers. These findings do not establish playable songs.");
        egui::ScrollArea::vertical()
            .id_salt("audio-driver-candidates")
            .max_height(180.0)
            .show(ui, |ui| {
                for (index, candidate) in report.driver_candidates.iter().enumerate() {
                    ui.push_id(("driver-candidate", index), |ui| {
                        ui.strong(format!("{} {}", candidate.family, candidate.variant).trim());
                        match candidate.qualification {
                            zeff_audio_discovery::drivers::CandidateQualification::Structural => {
                                draw_structural_evidence(ui, workspace, candidate.inventory.as_ref());
                            }
                            zeff_audio_discovery::drivers::CandidateQualification::StaticCode => {
                                ui.small("Decoded sound-write evidence · playback unverified");
                                ui.small("Driver family, song table and active execution are unknown.");
                                draw_code_evidence(ui, workspace, candidate.code.as_ref());
                            }
                            zeff_audio_discovery::drivers::CandidateQualification::FingerprintOnly => {
                                ui.small("Fingerprint evidence · playback unverified");
                            }
                        }
                        for evidence in &candidate.evidence {
                            span_button(ui, workspace, evidence.span, evidence.signature);
                        }
                    });
                }
            });
    });
}

fn draw_code_evidence(
    ui: &mut egui::Ui,
    workspace: &mut AudioWorkspace,
    code: Option<&zeff_audio_discovery::drivers::CodeInventory>,
) {
    let Some(code) = code else {
        return;
    };
    ui.small(format!(
        "{} sound-register writes · {} possible caller links",
        code.writes.len(),
        code.calls.len()
    ));
    draw_selector_consumers(ui, workspace, &code.selector_consumers);
    draw_command_dispatches(ui, workspace, &code.command_dispatches);
    egui::CollapsingHeader::new("Sound writes and callers")
        .show(ui, |ui| {
            for write in &code.writes {
                span_button(ui, workspace, write.span, &format!("${:04X} writes ${:04X}", write.cpu_address, write.register));
            }
            ui.small("Caller links describe possible local code paths, not executed calls or playable songs.");
            for call in &code.calls {
                span_button(ui, workspace, call.span, &format!("${:04X} calls ${:04X} → writer ${:04X}", call.cpu_address, call.target_cpu_address, call.writer_cpu_address));
                span_button(ui, workspace, call.writer_span, "Linked sound write");
            }
        });
}

fn draw_command_dispatches(
    ui: &mut egui::Ui,
    workspace: &mut AudioWorkspace,
    dispatches: &[zeff_audio_discovery::drivers::CodeCommandDispatch],
) {
    if dispatches.is_empty() {
        return;
    }
    let fetch_count = dispatches
        .iter()
        .map(|dispatch| dispatch.fetches.len())
        .sum::<usize>();
    ui.small(format!(
        "{} command dispatch{} · {fetch_count} guarded byte fetches",
        dispatches.len(),
        plural_suffix(dispatches.len())
    ));
    ui.small("Possible sound-code paths. Handler execution and playback remain unverified.");
    for dispatch in dispatches {
        egui::CollapsingHeader::new(format!(
            "Command dispatch ${:04X}",
            dispatch.entry_cpu_address
        ))
        .id_salt(("code-dispatch", dispatch.entry_cpu_address))
        .show(ui, |ui| {
            span_button(
                ui,
                workspace,
                dispatch.entry_span,
                "Decoded command dispatch",
            );
            ui.label(format!(
                "Table address ${:04X} · bounds unknown",
                dispatch.table_cpu_address
            ));
            for fetch in &dispatch.fetches {
                span_button(
                    ui,
                    workspace,
                    fetch.span,
                    &format!("${:04X} guarded byte fetch", fetch.cpu_address),
                );
                span_button(ui, workspace, fetch.call_span, "Command dispatch call");
                span_button(
                    ui,
                    workspace,
                    fetch.audio_call.span,
                    "Possible sound-caller path",
                );
            }
            for evidence in &dispatch.evidence {
                span_button(ui, workspace, evidence.span, evidence.signature);
            }
        });
    }
}

fn draw_selector_consumers(
    ui: &mut egui::Ui,
    workspace: &mut AudioWorkspace,
    consumers: &[zeff_audio_discovery::drivers::CodeSelectorConsumer],
) {
    use zeff_audio_discovery::drivers::CodePointerDisposition;
    if consumers.is_empty() {
        return;
    }
    ui.small(format!(
        "{} selector consumer(s) · sequences unverified",
        consumers.len()
    ));
    let records = consumers.iter().flat_map(|consumer| &consumer.records);
    let (record_count, stream_count) = records.fold((0, 0), |(count, streams), record| {
        (count + 1, streams + record.streams.len())
    });
    if record_count != 0 {
        ui.small(format!(
            "{record_count} record prefixes · {stream_count} stream pointers"
        ));
    }
    let bindings = consumers
        .iter()
        .flat_map(|consumer| &consumer.records)
        .flat_map(|record| &record.streams)
        .filter(|stream| stream.fetch_binding.is_some())
        .count();
    if bindings != 0 {
        ui.small(format!(
            "{bindings} stream pointer-to-reader bindings · execution unverified"
        ));
    }
    let edges = consumers
        .iter()
        .flat_map(|consumer| &consumer.records)
        .flat_map(|record| &record.streams)
        .filter(|stream| stream.conditional_head_command_edge.is_some())
        .count();
    if edges != 0 {
        ui.small(format!(
            "{edges} conditional command-handler links · execution unverified"
        ));
    }
    for consumer in consumers {
        egui::CollapsingHeader::new(format!("Selector entry ${:04X}", consumer.entry_cpu_address))
            .show(ui, |ui| {
                ui.small("Positive entry paths only. Zero returns; controls are separate. Producers and negative inputs remain unverified.");
                ui.small("The inspected pointer aperture is not a proven table extent or song inventory.");
                span_button(ui, workspace, consumer.entry_span, "Decoded selector entry");
                span_button(ui, workspace, consumer.call_span, "Selector call after sound routine");
                span_button(ui, workspace, consumer.audio_call.span, "Associated sound-routine call");
                span_button(ui, workspace, consumer.pointer_aperture, "Inspected pointer aperture");
                for control in &consumer.controls {
                    ui.label(format!("Control {} writes ${:02X} to ${:04X}", control.raw_selector, control.value, consumer.control_address));
                }
                for pointer in &consumer.pointers {
                    let status = match pointer.disposition {
                        CodePointerDisposition::Unparsed => "sequence unparsed",
                        CodePointerDisposition::Unmapped => "outside ROM",
                        CodePointerDisposition::DecodedCode => "overlaps decoded code",
                        CodePointerDisposition::PointerTable => "inside pointer aperture",
                        CodePointerDisposition::RecordPrefix => "inside record prefix",
                    };
                    span_button(ui, workspace, pointer.entry_span, &format!("{} → ${:04X}: {status}", pointer.raw_selector, pointer.target_cpu_address));
                    if let Some(span) = pointer.target_span {
                        span_button(ui, workspace, span, "Pointer target byte");
                    }
                }
                draw_records(ui, workspace, &consumer.records);
            });
    }
}

fn draw_records(
    ui: &mut egui::Ui,
    workspace: &mut AudioWorkspace,
    records: &[zeff_audio_discovery::drivers::CodeRecord],
) {
    use zeff_audio_discovery::drivers::CodePointerDisposition;
    if records.is_empty() {
        return;
    }
    ui.small(format!(
        "{} record prefixes · {} stream pointers · sequences unparsed",
        records.len(),
        records
            .iter()
            .map(|record| record.streams.len())
            .sum::<usize>()
    ));
    ui.small("Pointer fields are conditional on the decoded initialization path. Full record bounds, priority state and playback remain unverified.");
    for record in records {
        egui::CollapsingHeader::new(format!(
            "Selector {} · header ${:02X}",
            record.raw_selector, record.header
        ))
        .id_salt(("code-record", record.raw_selector))
        .show(ui, |ui| {
            span_button(
                ui,
                workspace,
                record.prefix_span,
                "Record pointer-field prefix",
            );
            for stream in &record.streams {
                let status = match stream.disposition {
                    CodePointerDisposition::Unparsed => "unparsed",
                    CodePointerDisposition::Unmapped => "outside ROM",
                    CodePointerDisposition::DecodedCode => "overlaps decoded code",
                    CodePointerDisposition::PointerTable => "inside pointer aperture",
                    CodePointerDisposition::RecordPrefix => "inside record prefix",
                };
                span_button(
                    ui,
                    workspace,
                    stream.entry_span,
                    &format!(
                        "Stream pointer → ${:04X} · {status}",
                        stream.target_cpu_address
                    ),
                );
                if let Some(span) = stream.target_span {
                    span_button(ui, workspace, span, "Stream target byte");
                }
                if let Some(binding) = &stream.fetch_binding {
                    draw_stream_binding(ui, workspace, binding);
                }
                if let Some(edge) = &stream.conditional_head_command_edge {
                    draw_head_edge(ui, workspace, edge);
                }
            }
            for evidence in &record.evidence {
                span_button(ui, workspace, evidence.span, evidence.signature);
            }
        });
    }
}

fn draw_head_edge(
    ui: &mut egui::Ui,
    workspace: &mut AudioWorkspace,
    edge: &zeff_audio_discovery::drivers::CodeConditionalHeadCommandEdge,
) {
    ui.label(format!(
        "If fetched: ${:02X} → handler ${:04X}",
        edge.head_byte, edge.handler_cpu_address
    ));
    ui.small("Pointer lifetime and actual command consumption remain unverified.");
    ui.label(format!(
        "{} operand bytes · destination ${:04X}–${:04X}",
        edge.operand_count, edge.destination_start, edge.destination_end_inclusive
    ));
    for evidence in &edge.evidence {
        span_button(ui, workspace, evidence.span, evidence.signature);
    }
}

fn draw_stream_binding(
    ui: &mut egui::Ui,
    workspace: &mut AudioWorkspace,
    binding: &zeff_audio_discovery::drivers::CodeStreamBinding,
) {
    ui.label(format!(
        "Pointer state ${:04X} → reader ${:04X}",
        binding.state_pointer_address, binding.fetch_cpu_address
    ));
    for (span, label) in [
        (binding.scheduler_span, "Decoded channel scheduling"),
        (binding.consumer_entry_span, "Pointer state reload"),
        (binding.fetch_span, "Guarded stream reader"),
    ] {
        span_button(ui, workspace, span, label);
    }
    for evidence in &binding.evidence {
        span_button(ui, workspace, evidence.span, evidence.signature);
    }
}

fn draw_structural_evidence(
    ui: &mut egui::Ui,
    workspace: &mut AudioWorkspace,
    inventory: Option<&zeff_audio_discovery::drivers::StructuralInventory>,
) {
    ui.small("Structural evidence · playback unverified");
    let Some(inventory) = inventory else {
        ui.small("No bounded selector inventory is available.");
        return;
    };
    ui.small(format!(
        "{} bounded descriptor group{} · inspected group-start IDs {} ({} driver inputs)",
        inventory.entries.len(),
        plural_suffix(inventory.entries.len()),
        selector_range(inventory.inspected_selectors),
        inventory.selector_input_count,
    ));
    ui.small("Music/SFX role and complete soundtrack coverage are unknown.");
    span_button(
        ui,
        workspace,
        inventory.mapped_window,
        "Required ROM mapping",
    );
    span_button(
        ui,
        workspace,
        inventory.descriptor_probe,
        "Descriptor probe aperture",
    );
    for selection in &inventory.entries {
        egui::CollapsingHeader::new(format!(
            "Selector 0x{:04X} · {} channel{}",
            selection.raw_selector,
            selection.tracks.len(),
            plural_suffix(selection.tracks.len()),
        ))
        .id_salt(("structural-selector", selection.raw_selector))
        .show(ui, |ui| {
            span_button(ui, workspace, selection.descriptor, "Descriptor");
            for track in &selection.tracks {
                ui.label(format!(
                    "Channel {} · {} notes",
                    track.channel, track.note_count
                ));
                for (index, span) in track.source_spans.iter().enumerate() {
                    span_button(
                        ui,
                        workspace,
                        *span,
                        &format!("Channel {} source {}", track.channel, index + 1),
                    );
                }
            }
        });
    }
    if !inventory.held.is_empty() {
        ui.small("Held selectors");
        for hold in &inventory.held {
            ui.label(format!(
                "0x{:04X} · {}",
                hold.raw_selector,
                hold_reason(hold.reason)
            ));
        }
    }
}

fn selector_range(inspected: u16) -> String {
    if inspected == 0 {
        "none".to_owned()
    } else {
        format!("0x0000–0x{:04X}", inspected - 1)
    }
}

fn plural_suffix(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

fn hold_reason(reason: zeff_audio_discovery::drivers::SelectorHoldReason) -> &'static str {
    use zeff_audio_discovery::drivers::SelectorHoldReason;

    match reason {
        SelectorHoldReason::SourceOverlap => "descriptor overlaps code or sequence data",
        SelectorHoldReason::SequenceOutOfRange => "sequence out of range",
        SelectorHoldReason::PointerRebase => "pointer rebase",
        SelectorHoldReason::UnsupportedCommand => "unsupported command",
        SelectorHoldReason::NonYieldingLoop => "non-yielding loop",
        SelectorHoldReason::Restart => "restart",
        SelectorHoldReason::CommandBatchLimit => "command batch limit",
        SelectorHoldReason::WalkLimit => "walk limit",
        SelectorHoldReason::NoNotes => "no notes observed",
    }
}
