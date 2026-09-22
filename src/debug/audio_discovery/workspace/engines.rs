use super::*;

pub(super) fn draw_sega_psg_details(
    ui: &mut egui::Ui,
    workspace: &mut AudioWorkspace,
    song: &zeff_audio_discovery::sega_psg::SegaPsgSong,
) {
    ui.label(&song.title);
    ui.small(format!(
        "{} · NTSC · {:?} · {} channels · selector ${:02X}",
        song.system.code(),
        song.region,
        song.channels.len(),
        song.raw_index
    ));
    ui.small(format!(
        "Driver update every {} video frame(s)",
        song.frame_divider
    ));
    span_button(ui, workspace, song.table_entry, "Music selector");
    span_button(ui, workspace, song.header, "Song header");
    span_button(ui, workspace, song.driver, "Driver entry point");
    for channel in &song.channels {
        span_button(
            ui,
            workspace,
            channel.sequence,
            &format!("Channel {} sequence entry", channel.number),
        );
    }
    for warning in &song.warnings {
        ui.small(warning);
    }
}

pub(super) fn draw_gax_details(
    ui: &mut egui::Ui,
    workspace: &mut AudioWorkspace,
    song: &crate::audio_discovery::gax::GaxSong,
) {
    ui.label(format!("{} · {}", song.title, song.version));
    if !song.artist.is_empty() {
        ui.small(format!("Artist: {}", song.artist));
    }
    ui.small(format!(
        "{} channels · {} rows/pattern · restart order {} · volume {} · {} Hz",
        song.channels.len(),
        song.rows_per_pattern,
        song.restart_order,
        song.volume,
        song.sample_rate
    ));
    span_button(ui, workspace, song.header, "GAX header");
    span_button(ui, workspace, song.version_span, "GAX version");
    span_button(ui, workspace, song.metadata, "GAX metadata");
    egui::CollapsingHeader::new(format!("Order graph · {} channels", song.channels.len()))
        .default_open(true)
        .show(ui, |ui| {
            for (channel, orders) in song.channels.iter().enumerate() {
                span_button(
                    ui,
                    workspace,
                    orders.order_table,
                    &format!("Channel {channel} orders"),
                );
                ui.small(
                    orders
                        .orders
                        .iter()
                        .enumerate()
                        .map(|(order, value)| {
                            format!("{order}: P{} {:+}", value.pattern, value.transpose)
                        })
                        .collect::<Vec<_>>()
                        .join("  "),
                );
            }
        });
    egui::CollapsingHeader::new(format!("Patterns · {}", song.patterns.len())).show(ui, |ui| {
        for (index, pattern) in song.patterns.iter().enumerate() {
            span_button(ui, workspace, pattern.source, &format!("Pattern {index}"));
            ui.small(format!("{} decoded cells", pattern.rows.len()));
        }
    });
    egui::CollapsingHeader::new(format!("Instruments · {}", song.instruments.len())).show(
        ui,
        |ui| {
            for instrument in &song.instruments {
                egui::CollapsingHeader::new(format!(
                    "Instrument {}{}",
                    instrument.index,
                    if instrument.blank { " · blank" } else { "" }
                ))
                .show(ui, |ui| {
                    span_button(ui, workspace, instrument.pointer, "Pointer");
                    span_button(ui, workspace, instrument.descriptor, "Descriptor");
                    span_button(ui, workspace, instrument.rows_span, "Rows");
                    span_button(ui, workspace, instrument.envelope.source, "Envelope");
                    ui.small(format!(
                        "Samples {:?} · {} rows · vibrato {:?} · row speed {}",
                        instrument.sample_indices,
                        instrument.rows.len(),
                        instrument.vibrato,
                        instrument.row_speed
                    ));
                    for (index, settings) in instrument.sample_settings.iter().enumerate() {
                        span_button(
                            ui,
                            workspace,
                            settings.source,
                            &format!("Sample setting {index}"),
                        );
                        ui.small(format!(
                            "pitch {} · start {} · loop {}..{}{}",
                            settings.pitch,
                            settings.start_position,
                            settings.loop_start,
                            settings.loop_end,
                            if settings.bidirectional {
                                " · bidirectional"
                            } else {
                                ""
                            }
                        ));
                    }
                });
            }
        },
    );
    egui::CollapsingHeader::new(format!("Samples · {}", song.samples.len())).show(ui, |ui| {
        for sample in &song.samples {
            span_button(
                ui,
                workspace,
                sample.header,
                &format!("Sample {} header", sample.index),
            );
            span_button(
                ui,
                workspace,
                sample.data,
                &format!("Sample {} PCM", sample.index),
            );
        }
    });
    if !song.warnings.is_empty() {
        ui.separator();
        ui.label("Warnings");
        for warning in &song.warnings {
            ui.small(format!("+{:06X}: {}", warning.offset, warning.reason));
        }
    }
    if !song.projection_limitations.is_empty() {
        ui.separator();
        ui.label("Projection limits");
        for limitation in &song.projection_limitations {
            ui.small(limitation);
        }
    }
}

pub(super) fn draw_module_details(
    ui: &mut egui::Ui,
    workspace: &mut AudioWorkspace,
    module: &crate::audio_discovery::tracker::EmbeddedModule,
) {
    ui.label(if module.name.is_empty() {
        "Untitled module"
    } else {
        &module.name
    });
    ui.small(format!(
        "{} · validated extent · {} bytes",
        module.format.label(),
        module.span.byte_len
    ));
    span_button(ui, workspace, module.span, "Module data");
    match module.source {
        crate::audio_discovery::tracker::ModuleSource::Embedded => {
            ui.small("Exports preserve the validated module extent. Unrecognized trailing chunks are not included.");
        }
        crate::audio_discovery::tracker::ModuleSource::Standalone { trailing_bytes } => {
            ui.small(format!("Exports preserve the complete source file, including {trailing_bytes} unrecognized trailing bytes."));
        }
    }
    ui.small(format!(
        "{} channels · {} orders · {} patterns · {} instruments · {} samples · {} sample points",
        module.channels,
        module.orders,
        module.patterns,
        module.instruments,
        module.samples,
        module.sample_points
    ));
}

pub(super) fn draw_gb_details(
    ui: &mut egui::Ui,
    workspace: &mut AudioWorkspace,
    song: &crate::audio_discovery::gb_music::GbSong,
) {
    ui.label(format!("Song {} · {}", song.index, song.title));
    ui.small(format!("Verified profile: {}", song.profile));
    ui.small(format!(
        "ROM bank {:02X} · CPU {:04X} · {} channels",
        song.bank,
        song.cpu_address,
        song.channels.len()
    ));
    span_button(ui, workspace, song.table_entry, "Song table entry");
    span_button(ui, workspace, song.header, "Song header");
    ui.small(if song.midi_exportable {
        "MIDI exports the interpreted notes with approximate instruments. Original Game Boy synthesis is not reproduced."
    } else {
        "MIDI is unavailable because this song needs unsupported command behavior. Its mapped original structures can still be exported."
    });
    for channel in &song.channels {
        egui::CollapsingHeader::new(format!(
            "Channel {} · {} notes · {:?}",
            channel.number, channel.note_count, channel.termination
        ))
        .id_salt(("gb-music-channel", song.header.offset, channel.number))
        .default_open(true)
        .show(ui, |ui| {
            span_button(ui, workspace, channel.entry, "Channel entry");
            ui.small(format!(
                "CPU {:04X} · {} commands · ends at frame {}",
                channel.cpu_address, channel.event_count, channel.end_frame
            ));
            if let Some(frame) = channel.loop_start_frame {
                ui.small(format!("Loop starts at frame {frame}"));
            }
        });
    }
    egui::CollapsingHeader::new(format!(
        "Mapped structures · {} ranges",
        song.mapped_spans.len()
    ))
    .show(ui, |ui| {
        for (index, span) in song.mapped_spans.iter().enumerate() {
            span_button(ui, workspace, *span, &format!("Source range {}", index + 1));
        }
    });
    if !song.warnings.is_empty() {
        ui.separator();
        ui.label("Projection limits");
        for warning in &song.warnings {
            ui.small(format!("+{:06X}: {}", warning.offset, warning.reason));
        }
    }
}

pub(super) fn draw_nes_details(
    ui: &mut egui::Ui,
    workspace: &mut AudioWorkspace,
    song: &crate::audio_discovery::nes_music::NesSong,
) {
    ui.label(format!("Song {} · {}", song.index, song.title));
    ui.small(format!(
        "Verified profile: {} · {:?} queue · selector {:02X}",
        song.profile, song.queue, song.selector
    ));
    ui.small("NTSC timing and normal tempo. MIDI uses approximate instruments; NES hardware synthesis and runtime effects are not reproduced.");
    span_button(ui, workspace, song.table_entry, "Song selector");
    span_button(ui, workspace, song.header, "Initial song header");
    for channel in &song.channels {
        ui.small(format!(
            "Channel {} · {} notes · {} commands · ends at frame {} · {:?}",
            channel.number,
            channel.note_count,
            channel.event_count,
            channel.end_frame,
            channel.termination
        ));
        if let Some(frame) = channel.loop_start_frame {
            ui.small(format!("Loop starts at frame {frame}"));
        }
    }
    egui::CollapsingHeader::new(format!("Sections · {}", song.sections.len())).show(ui, |ui| {
        for (index, section) in song.sections.iter().enumerate() {
            span_button(
                ui,
                workspace,
                section.header,
                &format!(
                    "Section {} · frames {}–{}",
                    index + 1,
                    section.start_frame,
                    section.end_frame
                ),
            );
        }
    });
    egui::CollapsingHeader::new(format!(
        "Mapped structures · {} ranges",
        song.mapped_spans.len()
    ))
    .show(ui, |ui| {
        for (index, span) in song.mapped_spans.iter().enumerate() {
            span_button(ui, workspace, *span, &format!("Source range {}", index + 1));
        }
    });
    for warning in &song.warnings {
        ui.small(format!("+{:06X}: {}", warning.offset, warning.reason));
    }
}

pub(super) fn draw_vgm_details(
    ui: &mut egui::Ui,
    workspace: &mut AudioWorkspace,
    log: &crate::audio_discovery::vgm::VgmLog,
) {
    ui.label(SongRef::Vgm(log).title());
    ui.small(format!(
        "VGM {:x}.{:02x} · {:?} source · {} commands · {:.3} seconds",
        log.version >> 8,
        log.version & 0xff,
        log.encoding,
        log.command_count,
        log.samples as f64 / f64::from(crate::audio_discovery::vgm::TICKS_PER_SECOND)
    ));
    ui.small(if log.sn_playback.is_some() {
        "PSG preview and audio export are available for one recorded pass. Original source and decoded VGM exports are also available."
    } else {
        "This log can be inspected and preserved. Its chip configuration or commands are not supported by the PSG player."
    });
    span_button(ui, workspace, log.source, "Original source file");
    ui.small(format!(
        "Logical data: {:?} · {} bytes · SHA-256 {}",
        log.logical.address_space, log.logical.byte_len, log.logical.sha256
    ));
    ui.small(format!(
        "Logical header +{:06X} · {} bytes; commands +{:06X} · {} bytes",
        log.header.offset, log.header.byte_len, log.commands.offset, log.commands.byte_len
    ));
    if log.encoding == crate::audio_discovery::vgm::VgmEncoding::Gzip {
        ui.small("Logical offsets address the decompressed VGM. The Hex view shows original compressed source bytes.");
    }
    if let Some(offset) = log.loop_offset {
        ui.small(format!(
            "Logical loop +{offset:06X} · {} samples",
            log.loop_samples
        ));
    }
    for chip in &log.chips {
        ui.small(format!(
            "{} · clock field 0x{:08X}",
            chip.name, chip.raw_clock
        ));
    }
    egui::CollapsingHeader::new("Command counts").show(ui, |ui| {
        for (opcode, count) in &log.command_histogram {
            ui.small(format!("0x{opcode:02X}: {count}"));
        }
    });
    for warning in &log.warnings {
        ui.small(format!("{warning:?}"));
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn draw_cdda_details(
    ui: &mut egui::Ui,
    track: &crate::audio_discovery::cdda::CdAudioTrack,
) {
    ui.label(format!("CD audio track {:02}", track.number));
    ui.small(format!(
        "Index 1 LBA {} through {} · {} sectors · {}",
        track.index1_lba,
        track.end_lba,
        track.sectors,
        cdda_duration(track.pcm_frames)
    ));
    match track.pregap_start_lba {
        Some(start) => ui.small(format!(
            "Stored pregap LBA {start}..{} is verified but omitted from exported PCM.",
            track.index1_lba
        )),
        None => ui.small("No retained pregap before index 1."),
    };
    ui.small("Source: loaded CD disc. Its provenance and identity appear in the scan report; CD sectors do not map to the loaded byte view.");
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn cdda_duration(frames: u64) -> String {
    let seconds = frames / u64::from(crate::audio_discovery::cdda::CDDA_SAMPLE_RATE);
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

pub(super) fn draw_nsq_details(ui: &mut egui::Ui, song: &crate::audio_discovery::nsq::NsqSong) {
    ui.label(format!("{} · {} notes", song.title, song.notes));
    ui.small(format!(
        "{} instruments · {} samples · {} frames",
        song.instruments, song.samples, song.duration_frames
    ));
    ui.small("Audio preview uses the original NSQ sequencer and NPF sample bank.");
    for warning in &song.warnings {
        ui.small(warning);
    }
}

pub(super) fn draw_descriptor_midi_details(
    ui: &mut egui::Ui,
    song: &crate::audio_discovery::descriptor_midi::DescriptorMidiSong,
) {
    ui.label(format!(
        "{} · {} channels · {} tracks",
        song.title, song.channels, song.tracks
    ));
    ui.small(format!(
        "{} notes · {} instruments · {} samples",
        song.notes, song.instruments, song.samples
    ));
    ui.small("MIDI export preserves the original sequence; audio preview uses the original driver and bank.");
    for warning in &song.warnings {
        ui.small(warning);
    }
}

pub(super) fn draw_aas_details(ui: &mut egui::Ui, song: &crate::audio_discovery::aas::AasSong) {
    ui.label(format!(
        "{} · {} channels · {} orders",
        song.title, song.channels, song.orders
    ));
    ui.small(format!(
        "{} patterns · {} notes · {} samples",
        song.patterns, song.notes, song.samples
    ));
    for warning in &song.warnings {
        ui.small(warning);
    }
}

pub(super) fn draw_musyx_details(
    ui: &mut egui::Ui,
    song: &crate::audio_discovery::musyx::MusyxSong,
) {
    ui.label(format!(
        "{} · {} channels · {} BPM",
        song.title, song.channels, song.tempo
    ));
    ui.small(format!(
        "{} patterns · {} notes · {} samples",
        song.patterns, song.notes, song.samples
    ));
    for warning in &song.warnings {
        ui.small(warning);
    }
}

pub(super) fn draw_gb_native_details(
    ui: &mut egui::Ui,
    song: &crate::audio_discovery::gb_native::GbNativeSong,
) {
    ui.label(format!("{} · {} channels", song.title, song.channels.len()));
    let kind = if song.loop_start_frame.is_some() {
        "Looped music"
    } else {
        "Finite cue"
    };
    let hardware = match song.native.timing {
        zeff_audio_discovery::gb_native::GbNativeTiming::Dmg => "DMG",
        zeff_audio_discovery::gb_native::GbNativeTiming::CgbDouble => "CGB double-speed",
    };
    ui.small(format!("{kind} · original {hardware} playback"));
    for warning in &song.warnings {
        ui.small(warning);
    }
}

pub(super) fn draw_nes_native_details(
    ui: &mut egui::Ui,
    song: &crate::audio_discovery::nes_native::NesNativeSong,
) {
    ui.label(format!("{} · {} channels", song.title, song.channels.len()));
    ui.small(format!(
        "{} · mapper {} · NTSC",
        song.profile, song.native.mapper
    ));
    ui.small("Original driver playback; mapped commands are preserved without MIDI conversion.");
    for warning in &song.warnings {
        ui.small(warning);
    }
}

pub(super) fn draw_aas_stream_details(
    ui: &mut egui::Ui,
    song: &crate::audio_discovery::aas_stream::AasStreamSong,
) {
    ui.label(format!(
        "{} · {} encoded bytes",
        song.title, song.encoded_data.byte_len
    ));
    ui.small("Looped music stream with original decoder playback.");
    for warning in &song.warnings {
        ui.small(warning);
    }
}

pub(super) fn draw_aas_pcm_details(
    ui: &mut egui::Ui,
    song: &crate::audio_discovery::aas_pcm::AasPcmSong,
) {
    ui.label(&song.title);
    ui.small(format!(
        "{} Hz · {} sample bytes",
        song.sample_rate, song.sample_data.byte_len
    ));
    if let Some(start) = song.loop_start {
        ui.small(format!("Repeats from sample {start}."));
    } else {
        ui.small("One-shot sound cue.");
    }
    for warning in &song.warnings {
        ui.small(warning);
    }
}

pub(super) fn draw_gbass_details(
    ui: &mut egui::Ui,
    song: &crate::audio_discovery::gbass::GbassSong,
) {
    ui.label(format!("{} · {} channels", song.title, song.channels));
    ui.small(format!(
        "{} instrument entries · {} sample headers",
        song.instruments, song.samples
    ));
    ui.small("Original driver playback; individual instrument relationships are not yet mapped.");
    for warning in &song.warnings {
        ui.small(warning);
    }
}

pub(super) fn draw_radriver_details(
    ui: &mut egui::Ui,
    song: &crate::audio_discovery::radriver::RadriverSong,
) {
    ui.label(&song.title);
    ui.small(format!(
        "{} samples · {} mixer channels · {} Hz",
        song.samples.len(),
        song.native.channels,
        song.native.sample_rate
    ));
    for warning in &song.warnings {
        ui.small(warning);
    }
}

pub(super) fn draw_krawall_details(
    ui: &mut egui::Ui,
    song: &crate::audio_discovery::krawall::KrawallSong,
) {
    ui.label(format!(
        "{} · {} channels · start order {}",
        song.title, song.channels, song.start_order
    ));
    ui.small(format!(
        "{} patterns · {} instruments · {} samples",
        song.pattern_count, song.instrument_count, song.sample_count
    ));
    for warning in &song.warnings {
        ui.small(warning);
    }
}

pub(super) fn draw_gax_native_details(
    ui: &mut egui::Ui,
    song: &crate::audio_discovery::gax_native::GaxNativeSong,
) {
    ui.label(format!(
        "{} · {} · {} channels",
        song.title, song.native.version, song.channels
    ));
    ui.small("Original driver playback; detailed instrument mapping is not yet available.");
    for warning in &song.warnings {
        ui.small(warning);
    }
}

pub(super) fn draw_engine_software_details(
    ui: &mut egui::Ui,
    song: &crate::audio_discovery::engine_software::EngineSoftwareSong,
) {
    ui.label(format!(
        "{} · {} channels · bank +{:06X}",
        song.title, song.channels, song.bank.effective_offset
    ));
    ui.small("Packed tracker patterns and sample instruments; XM playback is approximate.");
    for warning in &song.warnings {
        ui.small(warning);
    }
}
