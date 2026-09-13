use super::{
    Budget, DetectorState, MAX_ROM_BYTES, MediaIdentity, ScanLimits, ScanReport, ScanStatus,
    ScanStop, SongTableEntryKind, SongTableReference, Warning, camelot, detectors, gax, gb_music,
    mp2k, natsume, nes_music, tables, tracker,
};
use std::sync::atomic::{AtomicBool, Ordering};
use zeff_emu_common::system::System;

pub fn scan(system: System, bytes: &[u8], limits: ScanLimits, cancel: &AtomicBool) -> ScanReport {
    let cancelled = cancel.load(Ordering::Relaxed);
    let mut report = ScanReport::new(
        "multi-engine-structural",
        26,
        detectors::cartridge(system),
        &[
            "Structural candidates alone do not prove engine identity; recognized selectors and table references are separate evidence.",
            "MP2k blockCount=0 and verified song-ID headers use canonical 0x08/0x09 ROM pointers.",
            "Song tables have no intrinsic count; each recognized table reports its observed termination boundary.",
            "Only referenced voices and note-key regions are inspected. Verified Camelot pulse/saw/triangle recipes are retained; other custom or compressed samples remain unresolved.",
            "XCMD pseudo-echo parameters and unconditional MEMACC writes are retained; conditional guest-memory control flow remains unresolved.",
            "Track spans contain only visited command bytes; they are not complete extracted songs.",
            "Work units count bounded groups of linear probes and individual validation steps; the bounded media-identity hash is a separate pass.",
            "GAX 3 requires a consistent version marker or an original-driver binding within its executable image, plus a validated song/instrument/sample graph; conversion availability is reported per song.",
            "Validated embedded XM 1.04, 31-instrument MOD, S3M and IT modules can be preserved across cartridge systems. This does not identify arbitrary native sound drivers in those systems.",
            "S3M OPL instruments and packed samples, and IT compressed or ADPCM samples, are outside the supported structural profiles.",
            "Game Boy music detection requires an exact supported banked driver profile and bounded sequence data.",
            "Native Game Boy playback uses the original banked driver under its qualified hardware profile; song-end and loop bounds are available only for profiles that explicitly report them.",
            "NES music detection requires the supported NROM queue driver layout, matching native routines and music data; unrelated cartridge bytes may differ.",
            "Natsume detection requires one of two exact GBA driver profiles. Static mapping preserves table, header and visited sequence structure; original-driver preview is separate from instrument-bank and MIDI conversion.",
            "Engine Software-format banks provide a validated XM projection and approximate playback; the format does not by itself establish vendor identity.",
            "Krawall, GAX native, MusyX and Apex Audio System playback require recognized original-driver routines and bounded song structures. Their mapped ranges are a partial inventory, not a complete sample bank or soundtrack.",
            "GBA descriptor MIDI requires a recognized native player, exact sparse selector binding, bounded MIDI events and referenced instrument/sample backing; preserved MIDI uses the original bank assignments.",
            "GBA NSQ/NPF requires a recognized native driver, exact hashed filesystem assets and selector/bank bindings; playback executes the original sample decoder and sequencer.",
            "Sega PSG discovery requires an exact supported ROM identity, original driver and bounded music selector table; playback uses the reported NTSC hardware profile.",
            "RADriver discovery preserves bound effects and compressed music selectors; effect-only banks do not establish soundtrack coverage.",
            "GBASS discovery requires an exact supported original driver, bounded song selectors and isolated native playback; mapped spans are a partial inventory.",
            "AAS stream discovery requires an exact original driver and bounded compressed music selectors; mapped data includes native decoder lookahead.",
            "AAS PCM discovery requires an exact original driver and bounded sound-cue selectors; effects and speech are included, and playback uses the source channel at full volume.",
            "NES native playback requires an exact supported ROM and bounded audio selectors; qualified cues do not establish full soundtrack coverage.",
            "Applicable detectors describe the selected source's supported analysis scope, not a claim of a match or completion. A complete empty result means those detectors found no supported songs.",
        ],
        MediaIdentity {
            system: system.code(),
            byte_len: bytes.len() as u64,
            sha256: (bytes.len() <= MAX_ROM_BYTES && !cancelled)
                .then(|| const_hex::encode(zeff_firmware::sha256_bytes(bytes))),
        },
        limits,
    );
    if report.preflight(cancel).is_some() {
        return report;
    }

    let mut budget = Budget {
        cancel,
        remaining: limits.max_work,
    };
    let tracker_start = budget.remaining;
    let tracker_result = tracker::scan(
        bytes,
        &mut report.tracker_modules,
        &mut budget,
        limits.max_candidates as usize,
    );
    let tracker_work = tracker_start - budget.remaining;
    match tracker_result {
        Ok(()) => report.record_detector(
            report.applicable_detectors[0],
            DetectorState::Complete,
            report.tracker_modules.len(),
            tracker_work,
        ),
        Err(reason) => {
            report.record_detector(
                report.applicable_detectors[0],
                DetectorState::Incomplete(reason),
                report.tracker_modules.len(),
                tracker_work,
            );
            report.status = ScanStatus::Incomplete(reason);
            report.work_used = limits.max_work - budget.remaining;
            if reason != ScanStop::ValidationLimit {
                report.finish_not_run(reason);
                return report;
            }
        }
    }

    let native_result = match system {
        System::Nes => {
            let start = budget.remaining;
            let result = nes_music::scan(
                bytes,
                &mut report.nes_songs,
                &mut budget,
                limits.max_candidates as usize - report.tracker_modules.len(),
            );
            let work_used = start - budget.remaining;
            (Some((result, report.nes_songs.len(), work_used)), None)
        }
        System::Gb => {
            let start = budget.remaining;
            let result = gb_music::scan(
                bytes,
                &mut report.gb_songs,
                &mut budget,
                limits.max_candidates as usize - report.tracker_modules.len(),
            );
            let work_used = start - budget.remaining;
            (Some((result, report.gb_songs.len(), work_used)), None)
        }
        System::Sms | System::Gg => {
            let start = budget.remaining;
            let result = super::sega_psg::scan(
                bytes,
                system,
                &mut report.sega_psg_songs,
                &mut budget,
                limits.max_candidates as usize - report.tracker_modules.len(),
            );
            let work_used = start - budget.remaining;
            (Some((result, report.sega_psg_songs.len(), work_used)), None)
        }
        System::Gba => (None, Some(())),
        _ => (None, None),
    };
    if let Some((result, retained_matches, work_used)) = native_result.0 {
        match result {
            Ok(()) => report.record_detector(
                report.applicable_detectors[1],
                DetectorState::Complete,
                retained_matches,
                work_used,
            ),
            Err(reason) => {
                report.record_detector(
                    report.applicable_detectors[1],
                    DetectorState::Incomplete(reason),
                    retained_matches,
                    work_used,
                );
                report.status = ScanStatus::Incomplete(reason);
                report.work_used = limits.max_work - budget.remaining;
                report.finish_not_run(reason);
                return report;
            }
        }
    }
    if system == System::Gb {
        let start = budget.remaining;
        let capacity = limits.max_candidates as usize - report.song_count();
        let result =
            super::gb_native::scan(bytes, &mut report.gb_native_songs, &mut budget, capacity);
        let state = match result {
            Ok(()) => DetectorState::Complete,
            Err(reason) => {
                report.status = ScanStatus::Incomplete(reason);
                DetectorState::Incomplete(reason)
            }
        };
        report.record_detector(
            report.applicable_detectors[2],
            state,
            report.gb_native_songs.len(),
            start - budget.remaining,
        );
    }
    if system == System::Nes {
        let start = budget.remaining;
        let capacity = limits.max_candidates as usize - report.song_count();
        let result =
            super::nes_native::scan(bytes, &mut report.nes_native_songs, &mut budget, capacity);
        let state = match result {
            Ok(()) => DetectorState::Complete,
            Err(reason) => {
                report.status = ScanStatus::Incomplete(reason);
                DetectorState::Incomplete(reason)
            }
        };
        report.record_detector(
            report.applicable_detectors[2],
            state,
            report.nes_native_songs.len(),
            start - budget.remaining,
        );
    }
    if native_result.1.is_none() {
        report.work_used = limits.max_work - budget.remaining;
        debug_assert_eq!(
            report.detector_outcomes.len(),
            report.applicable_detectors.len()
        );
        return report;
    }

    let gax_start = budget.remaining;
    let gax_result = gax::scan(
        bytes,
        &mut report.gax_songs,
        &mut budget,
        limits.max_candidates as usize - report.tracker_modules.len(),
    );
    let gax_work = gax_start - budget.remaining;
    match gax_result {
        Ok(()) => report.record_detector(
            report.applicable_detectors[1],
            DetectorState::Complete,
            report.gax_songs.len(),
            gax_work,
        ),
        Err(reason) => {
            report.record_detector(
                report.applicable_detectors[1],
                DetectorState::Incomplete(reason),
                report.gax_songs.len(),
                gax_work,
            );
            report.status = ScanStatus::Incomplete(reason);
            report.work_used = limits.max_work - budget.remaining;
            if reason != ScanStop::ValidationLimit {
                report.finish_not_run(reason);
                return report;
            }
        }
    }

    let mp2k_start = budget.remaining;
    let mp2k_result =
        tables::discover(bytes, &mut report.song_tables, &mut budget).and_then(|()| {
            let metadata_headers = report
                .song_tables
                .iter()
                .filter(|table| table.dialect == tables::SongDialect::SongIdHeader)
                .flat_map(|table| &table.entries)
                .filter(|entry| entry.kind == SongTableEntryKind::Song)
                .map(|entry| (entry.header_address - 0x0800_0000) as usize)
                .collect::<std::collections::BTreeSet<_>>();
            mp2k::scan(
                bytes,
                &mut report.candidates,
                ScanLimits {
                    max_candidates: limits.max_candidates
                        - report.gax_songs.len() as u32
                        - report.tracker_modules.len() as u32,
                    ..limits
                },
                &mut budget,
                &metadata_headers,
            )
        });
    link_song_table_evidence(&mut report);
    let mp2k_result =
        mp2k_result.and_then(|()| camelot::enrich(bytes, &mut report.candidates, &mut budget));
    let mp2k_work = mp2k_start - budget.remaining;
    let mp2k_matches = report.candidates.len();
    let mp2k_state = match mp2k_result {
        Err(reason) => DetectorState::Incomplete(reason),
        Ok(())
            if report.candidates.iter().any(|candidate| {
                candidate
                    .warnings
                    .iter()
                    .any(|warning| matches!(warning, Warning::TrackLimit { .. }))
            }) =>
        {
            DetectorState::Incomplete(ScanStop::ValidationLimit)
        }
        Ok(()) => DetectorState::Complete,
    };
    report.record_detector(
        report.applicable_detectors[2],
        mp2k_state,
        mp2k_matches,
        mp2k_work,
    );
    report.work_used = limits.max_work - budget.remaining;
    if let DetectorState::Incomplete(reason) = mp2k_state {
        report.status = ScanStatus::Incomplete(reason);
        if reason != ScanStop::ValidationLimit {
            report.finish_not_run(reason);
            return report;
        }
    }
    let natsume_start = budget.remaining;
    let natsume_result = natsume::scan(
        bytes,
        &mut report.natsume_songs,
        &mut budget,
        limits.max_candidates as usize
            - report.tracker_modules.len()
            - report.gax_songs.len()
            - report.candidates.len(),
    );
    let natsume_state = match natsume_result {
        Ok(()) => DetectorState::Complete,
        Err(reason) => {
            report.status = ScanStatus::Incomplete(reason);
            DetectorState::Incomplete(reason)
        }
    };
    report.record_detector(
        report.applicable_detectors[3],
        natsume_state,
        report.natsume_songs.len(),
        natsume_start - budget.remaining,
    );
    if let Err(reason) = natsume_result {
        report.work_used = limits.max_work - budget.remaining;
        if reason != ScanStop::ValidationLimit {
            report.finish_not_run(reason);
            return report;
        }
    }
    let start = budget.remaining;
    let capacity = limits.max_candidates as usize - report.song_count();
    let result = super::engine_software::scan(
        bytes,
        &mut report.engine_software_songs,
        &mut budget,
        capacity,
    );
    let state = match result {
        Ok(()) => DetectorState::Complete,
        Err(reason) => {
            report.status = ScanStatus::Incomplete(reason);
            DetectorState::Incomplete(reason)
        }
    };
    report.record_detector(
        report.applicable_detectors[4],
        state,
        report.engine_software_songs.len(),
        start - budget.remaining,
    );
    if let Err(reason) = result {
        report.work_used = limits.max_work - budget.remaining;
        if reason != ScanStop::ValidationLimit {
            report.finish_not_run(reason);
            return report;
        }
    }
    let start = budget.remaining;
    let capacity = limits.max_candidates as usize - report.song_count();
    let result = super::krawall::scan(bytes, &mut report.krawall_songs, &mut budget, capacity);
    let state = match result {
        Ok(()) => DetectorState::Complete,
        Err(reason) => {
            report.status = ScanStatus::Incomplete(reason);
            DetectorState::Incomplete(reason)
        }
    };
    report.record_detector(
        report.applicable_detectors[5],
        state,
        report.krawall_songs.len(),
        start - budget.remaining,
    );
    if let Err(reason) = result {
        report.work_used = limits.max_work - budget.remaining;
        if reason != ScanStop::ValidationLimit {
            report.finish_not_run(reason);
            return report;
        }
    }
    let start = budget.remaining;
    let capacity = limits.max_candidates as usize - report.song_count();
    let result =
        super::gax_native::scan(bytes, &mut report.gax_native_songs, &mut budget, capacity);
    let state = match result {
        Ok(()) => DetectorState::Complete,
        Err(reason) => {
            report.status = ScanStatus::Incomplete(reason);
            DetectorState::Incomplete(reason)
        }
    };
    report.record_detector(
        report.applicable_detectors[6],
        state,
        report.gax_native_songs.len(),
        start - budget.remaining,
    );
    if let Err(reason) = result
        && reason != ScanStop::ValidationLimit
    {
        report.work_used = limits.max_work - budget.remaining;
        report.finish_not_run(reason);
        return report;
    }
    let start = budget.remaining;
    let capacity = limits.max_candidates as usize - report.song_count();
    let result = super::musyx::scan(bytes, &mut report.musyx_songs, &mut budget, capacity);
    let state = match result {
        Ok(()) => DetectorState::Complete,
        Err(reason) => {
            report.status = ScanStatus::Incomplete(reason);
            DetectorState::Incomplete(reason)
        }
    };
    report.record_detector(
        report.applicable_detectors[7],
        state,
        report.musyx_songs.len(),
        start - budget.remaining,
    );
    if let Err(reason) = result
        && reason != ScanStop::ValidationLimit
    {
        report.work_used = limits.max_work - budget.remaining;
        report.finish_not_run(reason);
        return report;
    }
    let start = budget.remaining;
    let capacity = limits.max_candidates as usize - report.song_count();
    let result = super::aas::scan(bytes, &mut report.aas_songs, &mut budget, capacity);
    let state = match result {
        Ok(()) => DetectorState::Complete,
        Err(reason) => {
            report.status = ScanStatus::Incomplete(reason);
            DetectorState::Incomplete(reason)
        }
    };
    report.record_detector(
        report.applicable_detectors[8],
        state,
        report.aas_songs.len(),
        start - budget.remaining,
    );
    if let Err(reason) = result
        && reason != ScanStop::ValidationLimit
    {
        report.work_used = limits.max_work - budget.remaining;
        report.finish_not_run(reason);
        return report;
    }
    let start = budget.remaining;
    let capacity = limits.max_candidates as usize - report.song_count();
    let result = super::descriptor_midi::scan(
        bytes,
        &mut report.descriptor_midi_songs,
        &mut budget,
        capacity,
    );
    let state = match result {
        Ok(()) => DetectorState::Complete,
        Err(reason) => {
            report.status = ScanStatus::Incomplete(reason);
            DetectorState::Incomplete(reason)
        }
    };
    report.record_detector(
        report.applicable_detectors[9],
        state,
        report.descriptor_midi_songs.len(),
        start - budget.remaining,
    );
    if let Err(reason) = result
        && reason != ScanStop::ValidationLimit
    {
        report.work_used = limits.max_work - budget.remaining;
        report.finish_not_run(reason);
        return report;
    }
    let start = budget.remaining;
    let capacity = limits.max_candidates as usize - report.song_count();
    let result = super::nsq::scan(bytes, &mut report.nsq_songs, &mut budget, capacity);
    let state = match result {
        Ok(()) => DetectorState::Complete,
        Err(reason) => {
            report.status = ScanStatus::Incomplete(reason);
            DetectorState::Incomplete(reason)
        }
    };
    report.record_detector(
        report.applicable_detectors[10],
        state,
        report.nsq_songs.len(),
        start - budget.remaining,
    );
    if let Err(reason) = result
        && reason != ScanStop::ValidationLimit
    {
        report.work_used = limits.max_work - budget.remaining;
        report.finish_not_run(reason);
        return report;
    }
    let start = budget.remaining;
    let capacity = limits.max_candidates as usize - report.song_count();
    let result = super::radriver::scan(bytes, &mut report.radriver_songs, &mut budget, capacity);
    let state = match result {
        Ok(()) => DetectorState::Complete,
        Err(reason) => {
            report.status = ScanStatus::Incomplete(reason);
            DetectorState::Incomplete(reason)
        }
    };
    report.record_detector(
        report.applicable_detectors[11],
        state,
        report.radriver_songs.len(),
        start - budget.remaining,
    );
    if let Err(reason) = result
        && reason != ScanStop::ValidationLimit
    {
        report.work_used = limits.max_work - budget.remaining;
        report.finish_not_run(reason);
        return report;
    }
    let start = budget.remaining;
    let capacity = limits.max_candidates as usize - report.song_count();
    let result = super::gbass::scan(bytes, &mut report.gbass_songs, &mut budget, capacity);
    let state = match result {
        Ok(()) => DetectorState::Complete,
        Err(reason) => {
            report.status = ScanStatus::Incomplete(reason);
            DetectorState::Incomplete(reason)
        }
    };
    report.record_detector(
        report.applicable_detectors[12],
        state,
        report.gbass_songs.len(),
        start - budget.remaining,
    );
    if let Err(reason) = result
        && reason != ScanStop::ValidationLimit
    {
        report.work_used = limits.max_work - budget.remaining;
        report.finish_not_run(reason);
        return report;
    }
    let start = budget.remaining;
    let capacity = limits.max_candidates as usize - report.song_count();
    let result =
        super::aas_stream::scan(bytes, &mut report.aas_stream_songs, &mut budget, capacity);
    let state = match result {
        Ok(()) => DetectorState::Complete,
        Err(reason) => {
            report.status = ScanStatus::Incomplete(reason);
            DetectorState::Incomplete(reason)
        }
    };
    report.record_detector(
        report.applicable_detectors[13],
        state,
        report.aas_stream_songs.len(),
        start - budget.remaining,
    );
    if let Err(reason) = result
        && reason != ScanStop::ValidationLimit
    {
        report.work_used = limits.max_work - budget.remaining;
        report.finish_not_run(reason);
        return report;
    }
    let start = budget.remaining;
    let capacity = limits.max_candidates as usize - report.song_count();
    let result = super::aas_pcm::scan(bytes, &mut report.aas_pcm_songs, &mut budget, capacity);
    let state = match result {
        Ok(()) => DetectorState::Complete,
        Err(reason) => {
            report.status = ScanStatus::Incomplete(reason);
            DetectorState::Incomplete(reason)
        }
    };
    report.record_detector(
        report.applicable_detectors[14],
        state,
        report.aas_pcm_songs.len(),
        start - budget.remaining,
    );
    report.work_used = limits.max_work - budget.remaining;
    debug_assert_eq!(
        report.detector_outcomes.len(),
        report.applicable_detectors.len()
    );
    report
}

fn link_song_table_evidence(report: &mut ScanReport) {
    let candidate_indices = report
        .candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| (candidate.header.canonical_cpu_address, index))
        .collect::<std::collections::BTreeMap<_, _>>();
    for table in &report.song_tables {
        for entry in &table.entries {
            if let Some(&index) = candidate_indices.get(&entry.header_address) {
                let candidate = &mut report.candidates[index];
                candidate.table_entries.push(SongTableReference {
                    table_offset: table.table.effective_offset,
                    index: entry.index,
                    entry: entry.entry,
                    player: entry.player,
                });
                candidate.evidence.engine_signature_verified = true;
                candidate.evidence.song_table_verified = true;
            }
        }
    }
}
