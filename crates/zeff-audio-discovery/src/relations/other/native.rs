use super::{Builder, Kind, Relation, Result, label, mapped, selector};
use crate::catalog::SongRef;

pub(super) fn project(b: &mut Builder<'_>, song: SongRef<'_>) -> Result<()> {
    match song {
        SongRef::GbNative(song) => {
            b.graph.profile = Some(label(song.profile));
            let root = b.root(Kind::Song, &song.title, Some(song.header.into()))?;
            selector(b, root, song.table_entry.into(), u32::from(song.raw_index))?;
            b.child(
                root,
                Kind::EntryPoint,
                "Game Boy driver entry",
                Some(song.native.driver.into()),
                Relation::References,
            )?;
            for channel in &song.channels {
                let node = b.child(
                    root,
                    Kind::Channel,
                    &format!("Channel {}", channel.number),
                    Some(channel.entry.into()),
                    Relation::Contains,
                )?;
                b.child(
                    node,
                    Kind::SequenceData,
                    "Sequence entry",
                    Some(channel.sequence.into()),
                    Relation::References,
                )?;
            }
            mapped(b, root, song.mapped_spans.iter().copied().map(Into::into))?;
            b.unresolved(root, "Only qualified native audio selectors are mapped; full soundtrack coverage and MIDI conversion are not established")?;
        }
        SongRef::NesNative(song) => {
            b.graph.profile = Some(label(song.profile));
            let root = b.root(Kind::Song, &song.title, Some(song.header.into()))?;
            selector(b, root, song.table_entry.into(), u32::from(song.raw_index))?;
            b.child(
                root,
                Kind::EntryPoint,
                "NES driver entry",
                Some(song.native.driver.into()),
                Relation::References,
            )?;
            for channel in &song.channels {
                let node = b.child(
                    root,
                    Kind::Channel,
                    &format!("Channel {}", channel.number),
                    Some(channel.entry.into()),
                    Relation::Contains,
                )?;
                b.child(
                    node,
                    Kind::SequenceData,
                    "Sequence entry",
                    Some(channel.sequence.into()),
                    Relation::References,
                )?;
            }
            mapped(b, root, song.mapped_spans.iter().copied().map(Into::into))?;
            b.unresolved(root, "Only qualified native audio selectors are mapped; full soundtrack coverage and MIDI conversion are not established")?;
        }
        SongRef::SegaPsg(song) => {
            b.graph.profile = Some(label(song.profile));
            let root = b.root(Kind::Song, &song.title, Some(song.header.into()))?;
            selector(b, root, song.table_entry.into(), u32::from(song.raw_index))?;
            b.child(
                root,
                Kind::EntryPoint,
                "PSG driver entry",
                Some(song.driver.into()),
                Relation::References,
            )?;
            for channel in &song.channels {
                let node = b.child(
                    root,
                    Kind::Channel,
                    &format!("Channel {}", channel.number),
                    Some(channel.entry.into()),
                    Relation::Contains,
                )?;
                b.child(
                    node,
                    Kind::SequenceData,
                    "Sequence entry",
                    Some(channel.sequence.into()),
                    Relation::References,
                )?;
            }
            mapped(b, root, song.mapped_spans.iter().copied().map(Into::into))?;
            b.unresolved(root, "Mapped audio banks and initial channel entries are preserved; native commands and envelopes are not projected to MIDI")?;
        }
        SongRef::AasPcm(song) => {
            b.graph.profile = Some(label(song.native.profile));
            let root = b.root(Kind::Song, &song.title, Some(song.header.into()))?;
            selector(b, root, song.header.into(), u32::from(song.index))?;
            for (kind, span, name) in [
                (Kind::Container, song.root, "Qualified PCM selectors"),
                (Kind::Container, song.sample_leadin, "Native sample lead-in"),
                (
                    Kind::SampleData,
                    song.sample_data,
                    "Signed 8-bit sample data",
                ),
                (
                    Kind::Container,
                    song.sample_padding,
                    "Native sample padding",
                ),
                (Kind::EntryPoint, song.native.play, "Original PCM player"),
            ] {
                b.child(root, kind, name, Some(span.into()), Relation::References)?;
            }
            mapped(b, root, song.mapped_spans.iter().copied().map(Into::into))?;
            b.unresolved(root, "Sound cues include effects and speech; MIDI and instrument-bank conversion are unavailable")?;
        }
        SongRef::AasStream(song) => {
            b.graph.profile = Some(label(song.native.profile));
            let root = b.root(Kind::Song, &song.title, Some(song.header.into()))?;
            selector(b, root, song.header.into(), u32::from(song.index))?;
            b.child(
                root,
                Kind::Container,
                "Qualified stream selectors",
                Some(song.root.into()),
                Relation::References,
            )?;
            b.child(
                root,
                Kind::SampleData,
                "Encoded music stream",
                Some(song.encoded_data.into()),
                Relation::References,
            )?;
            b.child(
                root,
                Kind::Container,
                "Native decoder lookahead",
                Some(song.decoder_lookahead.into()),
                Relation::References,
            )?;
            mapped(b, root, song.mapped_spans.iter().copied().map(Into::into))?;
            b.unresolved(
                root,
                "Other audio selectors and instrument conversion remain unmapped",
            )?;
        }
        SongRef::Gbass(song) => {
            b.graph.profile = Some(label(song.native.profile));
            let root = b.root(Kind::Song, &song.title, Some(song.header.into()))?;
            selector(b, root, song.header.into(), u32::from(song.index))?;
            if let Some(module) = song.native.module {
                let node = b.child(
                    root,
                    Kind::Module,
                    &format!(
                        "Module {:02}, loaded at 0x{:08X}",
                        module.index, module.load_address
                    ),
                    Some(module.source.into()),
                    Relation::Selects {
                        index: u32::from(module.index),
                    },
                )?;
                b.child(
                    node,
                    Kind::EntryPoint,
                    "Original module loader",
                    Some(module.loader.into()),
                    Relation::References,
                )?;
            }
            for (span, name) in [
                (song.root, "Driver configuration"),
                (song.native.song_table, "Song selectors"),
                (song.native.instrument_table, "Instrument pointers"),
                (song.native.sample_table, "Sample headers"),
            ] {
                b.child(
                    root,
                    Kind::Container,
                    name,
                    Some(span.into()),
                    Relation::References,
                )?;
            }
            for track in &song.tracks {
                let node = b.child(
                    root,
                    Kind::Channel,
                    &format!("Channel {}", track.channel + 1),
                    Some(track.header.into()),
                    Relation::Contains,
                )?;
                b.child(
                    node,
                    Kind::SequenceData,
                    "Sequence entry",
                    Some(track.sequence.into()),
                    Relation::References,
                )?;
            }
            mapped(b, root, song.mapped_spans.iter().copied().map(Into::into))?;
            b.unresolved(
                root,
                "Native sequencing and mixing are not projected to MIDI or instrument programs",
            )?;
        }
        _ => unreachable!("Native graph adapter requires a native song"),
    }
    Ok(())
}
