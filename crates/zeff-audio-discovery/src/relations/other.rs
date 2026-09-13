use super::{AssetKind as Kind, AssetLocation, Builder, Relation, Result, label};
use crate::{catalog::SongRef, tracker::FileSpan, vgm::VgmAddressSpace};

mod native;

pub(super) fn project(b: &mut Builder<'_>, song: SongRef<'_>) -> Result<()> {
    match song {
        SongRef::GbNative(_)
        | SongRef::NesNative(_)
        | SongRef::SegaPsg(_)
        | SongRef::AasStream(_)
        | SongRef::AasPcm(_)
        | SongRef::Gbass(_) => native::project(b, song)?,
        SongRef::Radriver(song) => {
            let root = b.root(Kind::Song, &song.title, Some(song.header.into()))?;
            b.child(
                root,
                Kind::Container,
                "RADriver bank",
                Some(song.native.bank.into()),
                Relation::References,
            )?;
            let (block_kind, block_label) = match song.kind {
                crate::radriver::RadriverSongKind::Effect => {
                    (Kind::Container, "Effect sample table")
                }
                crate::radriver::RadriverSongKind::CompressedMusic => {
                    b.child(
                        root,
                        Kind::Container,
                        "Music selectors",
                        Some(song.root.into()),
                        Relation::References,
                    )?;
                    (Kind::SequenceData, "Compressed music block table")
                }
            };
            for (span, kind, name) in [
                (song.order, Kind::OrderTable, "Music order"),
                (song.blocks, block_kind, block_label),
            ] {
                if let Some(span) = span {
                    b.child(root, kind, name, Some(span.into()), Relation::References)?;
                }
            }
            for (index, sample) in song.samples.iter().enumerate() {
                let node = b.child(
                    root,
                    Kind::Sample,
                    &format!("Sample {index}"),
                    Some(sample.header.into()),
                    Relation::References,
                )?;
                b.child(
                    node,
                    Kind::SampleData,
                    "Encoded sample data",
                    Some(sample.data.into()),
                    Relation::References,
                )?;
            }
            mapped(b, root, song.mapped_spans.iter().copied().map(Into::into))?;
            b.unresolved(root, "Native sample decoding and sequencing are not projected to MIDI or instrument programs")?;
        }
        SongRef::Nsq(song) => {
            let root = b.root(Kind::Song, &song.title, Some(song.header.into()))?;
            b.child(
                root,
                Kind::Container,
                "Song selectors",
                Some(song.root.into()),
                Relation::References,
            )?;
            b.child(
                root,
                Kind::SequenceData,
                "NSQ sequence",
                Some(song.sequence.into()),
                Relation::References,
            )?;
            b.child(
                root,
                Kind::Container,
                "NPF instruments",
                Some(song.bank.into()),
                Relation::References,
            )?;
            mapped(b, root, song.mapped_spans.iter().copied().map(Into::into))?;
            b.unresolved(root, "Referenced sample files are mapped; individual instrument relationships are not retained")?;
        }
        SongRef::DescriptorMidi(song) => {
            let root = b.root(Kind::Song, &song.title, Some(song.header.into()))?;
            b.child(
                root,
                Kind::Container,
                "Sparse song table",
                Some(song.root.into()),
                Relation::References,
            )?;
            b.child(
                root,
                Kind::SequenceData,
                "Original MIDI",
                Some(song.midi.into()),
                Relation::References,
            )?;
            mapped(b, root, song.mapped_spans.iter().copied().map(Into::into))?;
            b.unresolved(root, "Referenced bank and sample backing are mapped; individual instrument relationships are not retained")?;
        }
        SongRef::Aas(song) => {
            let root = b.root(Kind::Song, &song.title, Some(song.header.into()))?;
            b.child(
                root,
                Kind::Container,
                "Apex Audio System bank",
                Some(song.root.into()),
                Relation::References,
            )?;
            mapped(b, root, song.mapped_spans.iter().copied().map(Into::into))?;
            b.unresolved(root, "Mapped sequences, patterns and samples; detailed instrument relationships are not retained")?;
        }
        SongRef::Musyx(song) => {
            let root = b.root(Kind::Song, &song.title, Some(song.header.into()))?;
            b.child(
                root,
                Kind::Container,
                "MusyX bank",
                Some(song.root.into()),
                Relation::References,
            )?;
            mapped(b, root, song.mapped_spans.iter().copied().map(Into::into))?;
            b.unresolved(root, "Original driver executes macros and keymaps; detailed relationships are not retained")?;
        }
        SongRef::Krawall(song) => {
            b.graph.profile = Some(label(song.profile));
            let root = b.root(Kind::Song, &song.title, Some(song.header.into()))?;
            mapped(b, root, song.mapped_spans.iter().copied().map(Into::into))?;
            b.unresolved(root, "Mapped module, instrument and sample data; individual relationship edges are not retained")?;
        }
        SongRef::GaxNative(song) => {
            b.graph.profile = Some(label(&song.native.version));
            let root = b.root(Kind::Song, &song.title, Some(song.header.into()))?;
            mapped(b, root, song.mapped_spans.iter().copied().map(Into::into))?;
            b.unresolved(root, "Original driver playback; detailed instrument and sample relationships are not retained")?;
        }
        SongRef::EngineSoftware(song) => {
            let root = b.root(Kind::Song, &song.title, Some(song.header.into()))?;
            b.child(
                root,
                Kind::Container,
                "Engine Software bank",
                Some(song.bank.into()),
                Relation::References,
            )?;
            mapped(
                b,
                root,
                song.mapped_spans
                    .iter()
                    .filter(|span| span.byte_len != 0)
                    .copied()
                    .map(Into::into),
            )?;
            b.unresolved(
                root,
                "XM playback approximates native envelopes, effects and mixing",
            )?;
        }
        SongRef::Gb(song) => {
            b.graph.profile = Some(label(song.profile));
            let root = b.root(Kind::Song, "GB song", Some(song.header.into()))?;
            selector(b, root, song.table_entry.into(), u32::from(song.index))?;
            for channel in &song.channels {
                b.charge()?;
                let node = b.child(
                    root,
                    Kind::Channel,
                    &format!("Channel {} entry", channel.number),
                    Some(channel.entry.into()),
                    Relation::Contains,
                )?;
                if channel.termination == crate::gb_music::GbTermination::Unresolved {
                    b.unresolved(node, "Channel interpretation has unresolved behavior")?;
                }
            }
            mapped(b, root, song.mapped_spans.iter().copied().map(Into::into))?;
            b.unresolved(
                root,
                "Instrument and waveform relationships are not retained by this detector",
            )?;
        }
        SongRef::Nes(song) => {
            b.graph.profile = Some(label(song.profile));
            let root = b.root(Kind::Song, "NES song", Some(song.header.into()))?;
            selector(b, root, song.table_entry.into(), u32::from(song.index))?;
            for channel in &song.channels {
                b.charge()?;
                let node = b.child(
                    root,
                    Kind::Channel,
                    &format!("Initial channel {} entry", channel.number),
                    Some(channel.entry.into()),
                    Relation::Contains,
                )?;
                if channel.termination == crate::nes_music::NesTermination::Unresolved {
                    b.unresolved(node, "Channel interpretation has unresolved behavior")?;
                }
            }
            for (index, section) in song.sections.iter().enumerate() {
                b.charge()?;
                let node = b.child(
                    root,
                    Kind::Section,
                    &format!(
                        "Section visit {index} · frames {}–{}",
                        section.start_frame, section.end_frame
                    ),
                    None,
                    Relation::Contains,
                )?;
                b.child(
                    node,
                    Kind::SequenceData,
                    "Section header",
                    Some(section.header.into()),
                    Relation::References,
                )?;
                b.child(
                    node,
                    Kind::SongTableEntry,
                    "Section table entry",
                    Some(section.table_entry.into()),
                    Relation::References,
                )?;
            }
            mapped(b, root, song.mapped_spans.iter().copied().map(Into::into))?;
            b.unresolved(
                root,
                "Instrument relationships and later-section channel entries are not retained",
            )?;
        }
        SongRef::Natsume(song) => {
            b.graph.profile = Some(label(song.profile));
            let root = b.root(Kind::Song, "Natsume song", Some(song.header.into()))?;
            selector(b, root, song.table_entry.into(), u32::from(song.index))?;
            for channel in &song.channels {
                b.charge()?;
                b.child(
                    root,
                    Kind::Channel,
                    &format!(
                        "Channel {} entry · hardware kind {}",
                        channel.number, channel.hardware_kind
                    ),
                    Some(channel.entry.into()),
                    Relation::Contains,
                )?;
            }
            mapped(b, root, song.mapped_spans.iter().copied().map(Into::into))?;
            b.unresolved(
                root,
                "Instrument and sample-bank relationships are not mapped",
            )?;
        }
        SongRef::Module(module) => {
            let root = b.root(
                Kind::Module,
                module.format.label(),
                Some(module.span.into()),
            )?;
            b.unresolved(root, &format!("{} orders, {} patterns, {} instruments, {} samples; individual asset ranges are not retained", module.orders, module.patterns, module.instruments, module.samples))?;
            if let crate::tracker::ModuleSource::Standalone { trailing_bytes } = module.source
                && trailing_bytes > 0
            {
                let offset = module
                    .span
                    .offset
                    .checked_add(module.span.byte_len)
                    .ok_or(super::GraphStop::InvalidLocation)?;
                b.child(
                    root,
                    Kind::Metadata,
                    "Preserved trailing bytes",
                    Some(
                        FileSpan {
                            offset,
                            byte_len: trailing_bytes,
                        }
                        .into(),
                    ),
                    Relation::References,
                )?;
            }
        }
        SongRef::Vgm(log) => vgm(b, log)?,
        SongRef::Rip(rip) => {
            let root = b.root(Kind::Container, rip.format.label(), Some(rip.source.into()))?;
            b.child(
                root,
                Kind::Metadata,
                "Container header",
                Some(rip.header.into()),
                Relation::Contains,
            )?;
            let program = b.child(
                root,
                Kind::Program,
                "Preserved program",
                Some(rip.program.into()),
                Relation::Contains,
            )?;
            for (name, entry) in [("Init", rip.init), ("Play", rip.play)] {
                b.charge()?;
                if entry.initial_source_offset.is_some_and(|offset| {
                    offset < rip.program.offset
                        || u64::from(offset)
                            >= u64::from(rip.program.offset) + u64::from(rip.program.byte_len)
                }) {
                    return Err(super::GraphStop::InvalidLocation);
                }
                let location = entry.initial_source_offset.map(|offset| {
                    FileSpan {
                        offset,
                        byte_len: 1,
                    }
                    .into()
                });
                let node = b.child(
                    program,
                    Kind::EntryPoint,
                    &format!("{name} at CPU ${:04X}", entry.cpu_address),
                    location,
                    Relation::Declares,
                )?;
                if location.is_none() {
                    b.unresolved(
                        node,
                        "No source byte is mapped at this address in the initial bank state",
                    )?;
                }
            }
            if let Some(span) = rip.opaque_metadata {
                b.child(
                    root,
                    Kind::Metadata,
                    "Opaque container metadata",
                    Some(span.into()),
                    Relation::Contains,
                )?;
            }
            b.unresolved(root, &format!("{} declared songs; native sequence, instrument and sample graphs are not mapped", rip.song_count))?;
        }
        #[cfg(not(target_arch = "wasm32"))]
        SongRef::Cdda(track) => {
            b.root(
                Kind::CdAudio,
                &format!("CD track {:02}", track.number),
                Some(AssetLocation::DiscTrack {
                    number: track.number,
                    index1_lba: track.index1_lba,
                    end_lba: track.end_lba,
                    pcm_frames: track.pcm_frames,
                }),
            )?;
        }
        SongRef::Mp2k(_) | SongRef::Gax(_) => {
            unreachable!("GBA graph adapters dispatch separately")
        }
    }
    Ok(())
}

fn selector(b: &mut Builder<'_>, target: u32, location: AssetLocation, index: u32) -> Result<()> {
    let entry = b.node(
        Kind::SongTableEntry,
        &format!("Selector {index}"),
        Some(location),
    )?;
    b.edge(entry, target, Relation::Selects { index })
}

fn mapped(
    b: &mut Builder<'_>,
    root: u32,
    spans: impl Iterator<Item = AssetLocation>,
) -> Result<()> {
    for span in spans {
        b.charge()?;
        b.child(
            root,
            Kind::MappedData,
            "Mapped song read set",
            Some(span),
            Relation::References,
        )?;
    }
    Ok(())
}

fn vgm(b: &mut Builder<'_>, log: &crate::vgm::VgmLog) -> Result<()> {
    let expected_space = match log.encoding {
        crate::vgm::VgmEncoding::Raw => VgmAddressSpace::SourceFile,
        crate::vgm::VgmEncoding::Gzip => VgmAddressSpace::DecompressedVgm,
    };
    if log.logical.address_space != expected_space
        || log.header.address_space != expected_space
        || log.commands.address_space != expected_space
        || log
            .gd3
            .is_some_and(|span| span.address_space != expected_space)
    {
        return Err(super::GraphStop::InvalidLocation);
    }
    b.graph.logical_source = Some(crate::vgm::LogicalIdentity {
        address_space: log.logical.address_space,
        byte_len: log.logical.byte_len,
        sha256: label(&log.logical.sha256),
    });
    let root = b.root(Kind::RegisterLog, "VGM source", Some(log.source.into()))?;
    let logical = if log.logical.address_space == VgmAddressSpace::DecompressedVgm {
        b.child(
            root,
            Kind::LogicalImage,
            "Decompressed VGM",
            Some(AssetLocation::LogicalVgm {
                offset: 0,
                byte_len: log.logical.byte_len,
            }),
            Relation::DecodesTo,
        )?
    } else {
        root
    };
    for (kind, name, span) in [
        (Kind::Metadata, "VGM header", log.header),
        (Kind::SequenceData, "Register command stream", log.commands),
    ] {
        b.charge()?;
        b.child(
            logical,
            kind,
            name,
            Some(vgm_location(span)),
            Relation::Contains,
        )?;
    }
    if let Some(span) = log.gd3 {
        b.child(
            logical,
            Kind::Metadata,
            "GD3 metadata",
            Some(vgm_location(span)),
            Relation::Contains,
        )?;
    }
    for chip in &log.chips {
        b.charge()?;
        b.child(root, Kind::Chip, chip.name, None, Relation::Declares)?;
    }
    b.unresolved(
        root,
        "Command-to-chip routing, instrument semantics and native sequences are not mapped",
    )?;
    Ok(())
}

fn vgm_location(span: crate::vgm::VgmSpan) -> AssetLocation {
    match span.address_space {
        VgmAddressSpace::SourceFile => FileSpan {
            offset: span.offset,
            byte_len: span.byte_len,
        }
        .into(),
        VgmAddressSpace::DecompressedVgm => AssetLocation::LogicalVgm {
            offset: span.offset,
            byte_len: span.byte_len,
        },
    }
}
