use std::sync::atomic::AtomicBool;

mod bindings;
mod dispatch;
mod records;

use zeff_emu_common::system::System;

use super::{
    RomSpan, ScanLimits, ScanReport, SourceSpan, rips::RipFormat, tracker::EmbeddedFormat,
};

const INPUT_CAP: usize = 256 * 1024;
pub fn gba(data: &[u8]) {
    let Some((control, bytes)) = input(data) else {
        return;
    };
    check(
        &super::scan(System::Gba, bytes, limits(control), &cancel(control)),
        bytes,
    );
}

pub fn natsume(data: &[u8]) {
    let Some((control, bytes)) = input(data) else {
        return;
    };
    super::natsume::fuzz_parse(bytes, limits(control), &cancel(control));
}

pub fn tracker(data: &[u8]) {
    let Some((control, source)) = input(data) else {
        return;
    };
    let cancel = cancel(control);
    let limits = limits(control);
    check(&super::scan(System::Gba, source, limits, &cancel), source);
    let format = match control & 3 {
        0 => EmbeddedFormat::Xm,
        1 => EmbeddedFormat::Mod,
        2 => EmbeddedFormat::S3m,
        _ => EmbeddedFormat::It,
    };
    check(
        &super::scan_standalone_tracker(source, format, limits, &cancel),
        source,
    );
}

pub fn vgm(data: &[u8]) {
    let Some((control, bytes)) = input(data) else {
        return;
    };
    check(
        &super::vgm::scan(bytes, limits(control), &cancel(control)),
        bytes,
    );
}

pub fn rip(data: &[u8]) {
    let Some((control, source)) = input(data) else {
        return;
    };
    let format = match control % 3 {
        0 => RipFormat::Gbs,
        1 => RipFormat::Nsf,
        _ => RipFormat::Nsfe,
    };
    check(
        &super::rips::scan(source, format, limits(control), &cancel(control)),
        source,
    );
}

fn input(data: &[u8]) -> Option<(u8, &[u8])> {
    (data.len() <= INPUT_CAP).then(|| {
        data.split_first()
            .map_or((0, data), |(control, bytes)| (*control, bytes))
    })
}

fn limits(control: u8) -> ScanLimits {
    ScanLimits {
        max_work: match (control >> 4) & 3 {
            0 if control & 0x40 != 0 => 8_000_000,
            0 => 100_000,
            1 => 0,
            2 => 1,
            _ => 1_000,
        },
        max_candidates: match (control >> 2) & 3 {
            0 => 8,
            1 => 0,
            2 => 1,
            _ => 2,
        },
    }
}

fn cancel(control: u8) -> AtomicBool {
    AtomicBool::new(control & 0x80 != 0)
}

fn check(report: &ScanReport, bytes: &[u8]) {
    check_graphs(report);
    assert!(report.work_used <= report.limits.max_work);
    assert_eq!(report.media.byte_len, bytes.len() as u64);
    assert!(
        report
            .detector_outcomes
            .iter()
            .map(|outcome| outcome.retained_matches)
            .sum::<u32>()
            <= report.limits.max_candidates
    );
    assert_eq!(
        report.detector_outcomes.len(),
        report.applicable_detectors.len()
    );
    assert_eq!(
        report
            .detector_outcomes
            .iter()
            .map(|outcome| outcome.work_used)
            .sum::<u64>(),
        report.work_used
    );
    for (outcome, descriptor) in report
        .detector_outcomes
        .iter()
        .zip(report.applicable_detectors)
    {
        assert_eq!(outcome.descriptor, *descriptor);
    }
    for (index, outcome) in report.detector_outcomes.iter().enumerate() {
        assert!(
            !report.detector_outcomes[..index]
                .iter()
                .any(|prior| prior.descriptor.id == outcome.descriptor.id)
        );
    }
    assert_eq!(
        report
            .detector_outcomes
            .iter()
            .map(|outcome| outcome.retained_matches as usize)
            .sum::<usize>(),
        report.song_count() + report.driver_candidates.len()
    );
    for candidate in &report.driver_candidates {
        if let Some(code) = &candidate.code {
            assert_eq!(
                candidate.qualification,
                crate::drivers::CandidateQualification::StaticCode
            );
            assert!(candidate.inventory.is_none());
            assert!(code.writes.len() <= 22 && code.calls.len() <= 64);
            assert!(code.selector_consumers.len() <= 16);
            dispatch::check(code, bytes);
            bindings::check(code, bytes);
            for consumer in &code.selector_consumers {
                check_selector(consumer, code, bytes);
            }
            for write in &code.writes {
                source_span(write.span.into(), bytes.len());
                assert_eq!(write.span.byte_len, 3);
                let at = write.span.offset as usize;
                assert!(matches!(bytes[at], 0x8c..=0x8e));
                assert_eq!(
                    u16::from_le_bytes([bytes[at + 1], bytes[at + 2]]),
                    write.register
                );
            }
            for call in &code.calls {
                source_span(call.span.into(), bytes.len());
                source_span(call.writer_span.into(), bytes.len());
                let at = call.span.offset as usize;
                assert_eq!(call.span.byte_len, 3);
                assert_eq!(bytes[at], 0x20);
                assert_eq!(
                    u16::from_le_bytes([bytes[at + 1], bytes[at + 2]]),
                    call.target_cpu_address
                );
                assert!(
                    code.writes
                        .iter()
                        .any(|write| write.cpu_address == call.writer_cpu_address
                            && write.span == call.writer_span)
                );
            }
        }
        for evidence in &candidate.evidence {
            source_span(evidence.span.into(), bytes.len());
        }
        assert_eq!(
            candidate.inventory.is_some(),
            candidate.qualification == crate::drivers::CandidateQualification::Structural
        );
        if let Some(inventory) = &candidate.inventory {
            rom_span(inventory.mapped_window, bytes);
            assert!(matches!(
                (
                    inventory.mapped_window.canonical_cpu_address,
                    inventory.mapped_window.byte_len
                ),
                (0x8000 | 0xa000, 0x2000) | (0x8000 | 0xc000, 0x4000)
            ));
            let in_window = |span: crate::tracker::FileSpan| {
                assert!(span.offset >= inventory.mapped_window.effective_offset);
                assert!(
                    u64::from(span.offset) + u64::from(span.byte_len)
                        <= u64::from(inventory.mapped_window.effective_offset)
                            + u64::from(inventory.mapped_window.byte_len)
                );
            };
            in_window(inventory.descriptor_probe);
            for evidence in &candidate.evidence {
                in_window(evidence.span);
            }
            assert!(inventory.selector_input_count <= 256);
            assert_eq!(
                inventory.inspected_selectors + 3,
                inventory.selector_input_count
            );
            assert!(inventory.inspected_selectors <= 256);
            assert!(
                inventory.entries.len() + inventory.held.len()
                    <= usize::from(inventory.inspected_selectors)
            );
            for selection in &inventory.entries {
                assert!(selection.raw_selector < inventory.inspected_selectors);
                in_window(selection.descriptor);
                assert_eq!(selection.tracks.len(), 4);
                let mut channels = 0_u8;
                for track in &selection.tracks {
                    assert!(track.channel < 4);
                    assert_eq!(channels & (1 << track.channel), 0);
                    channels |= 1 << track.channel;
                    for span in &track.source_spans {
                        in_window(*span);
                    }
                }
            }
            for held in &inventory.held {
                assert!(held.raw_selector < inventory.inspected_selectors);
            }
        }
    }
    for candidate in &report.candidates {
        rom_span(candidate.header, bytes);
        for entry in &candidate.table_entries {
            rom_span(entry.entry, bytes);
        }
        for track in &candidate.tracks {
            for span in &track.spans {
                rom_span(*span, bytes);
            }
        }
        for instrument in &candidate.instruments {
            tone(&instrument.tone, bytes);
            instrument
                .key_map
                .into_iter()
                .for_each(|span| rom_span(span, bytes));
            for region in &instrument.regions {
                if let Some(value) = &region.tone {
                    tone(value, bytes);
                }
            }
        }
    }
    for song in &report.natsume_songs {
        rom_span(song.table_entry, bytes);
        rom_span(song.header, bytes);
        for span in &song.mapped_spans {
            rom_span(*span, bytes);
        }
        for channel in &song.channels {
            rom_span(channel.entry, bytes);
        }
    }
    for table in &report.song_tables {
        for span in [
            table.selector,
            table.settings,
            table.settings_fields.sound_mode,
            table.settings_fields.player_count,
            table.settings_fields.player_table_pointer,
            table.table,
        ] {
            rom_span(span, bytes);
        }
        for entry in &table.entries {
            rom_span(entry.entry, bytes);
        }
        use super::tables::SongTableBoundary;
        match table.boundary {
            SongTableBoundary::NullTerminator { entry }
            | SongTableBoundary::InvalidPlayer { entry, .. }
            | SongTableBoundary::InvalidHeaderPointer { entry, .. } => rom_span(entry, bytes),
            SongTableBoundary::MediaEnd { effective_offset } => {
                assert!(effective_offset as usize <= bytes.len())
            }
        }
    }
    for song in &report.gax_songs {
        rom_span(song.header, bytes);
        rom_span(song.version_span, bytes);
        rom_span(song.metadata, bytes);
        for span in &song.mapped_spans {
            rom_span(*span, bytes);
        }
    }
    for module in &report.tracker_modules {
        source_span(module.span.into(), bytes.len());
    }
    for spans in report
        .engine_software_songs
        .iter()
        .map(|song| &song.mapped_spans)
        .chain(report.krawall_songs.iter().map(|song| &song.mapped_spans))
        .chain(
            report
                .gax_native_songs
                .iter()
                .map(|song| &song.mapped_spans),
        )
        .chain(report.musyx_songs.iter().map(|song| &song.mapped_spans))
        .chain(report.gb_musyx_songs.iter().map(|song| &song.mapped_spans))
        .chain(report.gb_tose_songs.iter().map(|song| &song.mapped_spans))
        .chain(
            report
                .gb_quickthunder_songs
                .iter()
                .map(|song| &song.mapped_spans),
        )
        .chain(report.gb_ghx_songs.iter().map(|song| &song.mapped_spans))
        .chain(
            report
                .gb_carillon_songs
                .iter()
                .map(|song| &song.mapped_spans),
        )
        .chain(
            report
                .gb_sound_system_songs
                .iter()
                .map(|song| &song.mapped_spans),
        )
        .chain(report.ws_tose_songs.iter().map(|song| &song.mapped_spans))
        .chain(report.nes_tose_songs.iter().map(|song| &song.mapped_spans))
        .chain(report.aas_songs.iter().map(|song| &song.mapped_spans))
        .chain(
            report
                .descriptor_midi_songs
                .iter()
                .map(|song| &song.mapped_spans),
        )
        .chain(report.nsq_songs.iter().map(|song| &song.mapped_spans))
        .chain(report.radriver_songs.iter().map(|song| &song.mapped_spans))
    {
        for span in spans {
            rom_span(*span, bytes);
        }
    }
    for log in &report.vgm_logs {
        source_span(log.source.into(), bytes.len());
        let logical_len = log.logical.byte_len as usize;
        vgm_span(log.header.offset, log.header.byte_len, logical_len);
        vgm_span(log.commands.offset, log.commands.byte_len, logical_len);
        if let Some(gd3) = log.gd3 {
            vgm_span(gd3.offset, gd3.byte_len, logical_len);
        }
    }
    for rip in &report.music_rips {
        for offset in [
            rip.init.initial_source_offset,
            rip.play.initial_source_offset,
        ]
        .into_iter()
        .flatten()
        {
            assert!((offset as usize) < bytes.len());
        }
        source_span(rip.source.into(), bytes.len());
        source_span(rip.header.into(), bytes.len());
        source_span(rip.program.into(), bytes.len());
        rip.opaque_metadata
            .into_iter()
            .for_each(|span| source_span(span.into(), bytes.len()));
        if let super::rips::RipDetails::Nsfe {
            chunks,
            info_header,
            data_header,
            bank_payload,
            ..
        } = &rip.details
        {
            source_span((*info_header).into(), bytes.len());
            source_span((*data_header).into(), bytes.len());
            bank_payload
                .iter()
                .for_each(|span| source_span((*span).into(), bytes.len()));
            for chunk in chunks {
                source_span(chunk.header.into(), bytes.len());
                source_span(chunk.payload.into(), bytes.len());
            }
        }
    }
}

pub(crate) fn check_graphs(report: &ScanReport) {
    use crate::relations::{AssetLocation, GraphLimits, GraphStatus, GraphStop};
    let limits = GraphLimits {
        max_nodes: 128,
        max_edges: 256,
        max_work: 1024,
    };
    for id in report.song_ids().take(2) {
        let graph = report.asset_relations(id, limits, &AtomicBool::new(false));
        assert!(graph.nodes.len() <= limits.max_nodes as usize);
        assert!(graph.edges.len() <= limits.max_edges as usize);
        assert!(graph.work_used <= limits.max_work);
        assert!(!matches!(
            graph.status,
            GraphStatus::Incomplete(GraphStop::InvalidLocation | GraphStop::MissingSong)
        ));
        for (index, node) in graph.nodes.iter().enumerate() {
            assert_eq!(node.id as usize, index);
            if let Some(AssetLocation::MediaBytes {
                offset, byte_len, ..
            }) = node.location
            {
                assert!(u64::from(offset) + u64::from(byte_len) <= report.media.byte_len);
            }
        }
        for edge in &graph.edges {
            assert!((edge.from as usize) < graph.nodes.len());
            assert!((edge.to as usize) < graph.nodes.len());
        }
        let cancelled = report.asset_relations(id, limits, &AtomicBool::new(true));
        assert_eq!(
            cancelled.status,
            GraphStatus::Incomplete(GraphStop::Cancelled)
        );
        assert!(cancelled.nodes.is_empty() && cancelled.edges.is_empty());
    }
}

fn tone(value: &super::ToneInventory, bytes: &[u8]) {
    rom_span(value.descriptor, bytes);
    for span in [value.sample_header, value.waveform].into_iter().flatten() {
        rom_span(span, bytes);
    }
    if let Some(sample) = value.sample {
        rom_span(sample.header, bytes);
        rom_span(sample.data, bytes);
    }
}

fn check_selector(
    consumer: &crate::drivers::CodeSelectorConsumer,
    code: &crate::drivers::CodeInventory,
    bytes: &[u8],
) {
    use crate::drivers::CodePointerDisposition;
    records::check(consumer, bytes);
    assert!(code.calls.contains(&consumer.audio_call));
    assert_eq!(
        consumer.call_cpu_address,
        consumer.audio_call.cpu_address + 3
    );
    for span in [
        consumer.entry_span,
        consumer.call_span,
        consumer.pointer_aperture,
    ] {
        source_span(span.into(), bytes.len());
    }
    assert_eq!(consumer.entry_span.byte_len, 56);
    assert_eq!(consumer.call_span.byte_len, 3);
    let call = consumer.call_span.offset as usize;
    assert_eq!(bytes[call], 0x20);
    assert_eq!(
        u16::from_le_bytes([bytes[call + 1], bytes[call + 2]]),
        consumer.entry_cpu_address
    );
    assert!((4..=128).contains(&consumer.upper_bound_exclusive));
    assert_eq!(consumer.controls.len(), 2);
    assert_ne!(
        consumer.controls[0].raw_selector,
        consumer.controls[1].raw_selector
    );
    assert_eq!(
        consumer.pointers.len(),
        usize::from(consumer.upper_bound_exclusive) - 3
    );
    assert_eq!(
        consumer.pointer_aperture.byte_len,
        u32::from(consumer.upper_bound_exclusive - 1) * 2
    );
    for control in &consumer.controls {
        assert!((1..consumer.upper_bound_exclusive).contains(&control.raw_selector));
    }
    for pointer in &consumer.pointers {
        assert!((1..consumer.upper_bound_exclusive).contains(&pointer.raw_selector));
        assert!(
            !consumer
                .controls
                .iter()
                .any(|control| control.raw_selector == pointer.raw_selector)
        );
        source_span(pointer.entry_span.into(), bytes.len());
        assert_eq!(pointer.entry_span.byte_len, 2);
        assert_eq!(
            pointer.entry_span.offset,
            consumer.pointer_aperture.offset + u32::from(pointer.raw_selector - 1) * 2
        );
        let at = pointer.entry_span.offset as usize;
        assert_eq!(
            u16::from_le_bytes([bytes[at], bytes[at + 1]]),
            pointer.target_cpu_address
        );
        assert_eq!(
            pointer.target_span.is_none(),
            pointer.disposition == CodePointerDisposition::Unmapped
        );
        if let Some(span) = pointer.target_span {
            source_span(span.into(), bytes.len());
            assert_eq!(span.byte_len, 1);
        }
    }
}

fn rom_span(span: RomSpan, bytes: &[u8]) {
    source_span(span.into(), bytes.len());
}

fn source_span(span: SourceSpan, len: usize) {
    let end = span.effective_offset as usize + span.byte_len as usize;
    assert!(end <= len);
}

fn vgm_span(offset: u32, len: u32, source_len: usize) {
    assert!(offset as usize + len as usize <= source_len);
}

#[cfg(test)]
#[test]
fn structural_candidate_invariants_cover_mutations_and_scan_limits() {
    for source in [
        crate::drivers::nes_tose_structure::synthetic_rom(),
        crate::drivers::nes_sound_writes::synthetic_rom(),
        crate::drivers::nes_sound_writes::synthetic_selector_rom(),
        crate::drivers::nes_sound_writes::synthetic_record_rom(),
        crate::drivers::nes_sound_writes::synthetic_dispatch_rom(),
        crate::drivers::nes_sound_writes::synthetic_binding_rom(),
        crate::drivers::nes_sound_writes::synthetic_head_edge_rom(),
    ] {
        for control in 0..=255 {
            let mut bytes = source.clone();
            if control & 1 != 0 {
                bytes[16 + 0x402] = control;
            }
            let report = crate::scan(System::Nes, &bytes, limits(control), &cancel(control));
            check(&report, &bytes);
        }
    }
}
