use std::collections::BTreeMap;

use super::{AssetKind as Kind, Builder, Relation, Result};
use crate::{SongCandidate, ToneInventory};

pub(super) fn mp2k(b: &mut Builder<'_>, song: &SongCandidate) -> Result<()> {
    b.graph.mp2k_evidence = Some(super::Mp2kEvidence {
        engine: song.engine,
        confidence: song.confidence,
        evidence: song.evidence.clone(),
    });
    let root = b.root(Kind::Song, "MP2k song", Some(song.header.into()))?;
    for entry in &song.table_entries {
        b.charge()?;
        let node = b.node(
            Kind::SongTableEntry,
            &format!("Song table selector {}", entry.index),
            Some(entry.entry.into()),
        )?;
        b.edge(node, root, Relation::Selects { index: entry.index })?;
    }
    let mut instruments = BTreeMap::new();
    for instrument in &song.instruments {
        b.charge()?;
        let node = tone(b, &instrument.tone)?;
        b.edge(
            root,
            node,
            Relation::UsesInstrument {
                index: u32::from(instrument.voice),
            },
        )?;
        instruments.insert(instrument.voice, node);
        if let Some(map) = instrument.key_map {
            b.child(
                node,
                Kind::KeyMap,
                "Instrument key map",
                Some(map.into()),
                Relation::References,
            )?;
        }
        for region in &instrument.regions {
            b.charge()?;
            let child = if let Some(tone_data) = &region.tone {
                tone(b, tone_data)?
            } else {
                b.node(
                    Kind::Unresolved,
                    "Referenced instrument region was not resolved",
                    None,
                )?
            };
            b.edge(
                node,
                child,
                Relation::MapsKeys {
                    first: region.key_start,
                    last: region.key_end,
                },
            )?;
        }
        if matches!(instrument.kind, 0x40 | 0x80) {
            b.unresolved(
                node,
                "Key regions cover observed notes only; other keys are not mapped",
            )?;
        }
    }
    for (index, track) in song.tracks.iter().enumerate() {
        b.charge()?;
        let node = b.child(
            root,
            Kind::Channel,
            &format!("Track {}", index + 1),
            None,
            Relation::Contains,
        )?;
        for span in &track.spans {
            b.charge()?;
            b.child(
                node,
                Kind::SequenceData,
                "Visited sequence bytes",
                Some((*span).into()),
                Relation::References,
            )?;
        }
        for voice in &track.voices {
            b.charge()?;
            let instrument = if let Some(&node) = instruments.get(voice) {
                node
            } else {
                b.node(
                    Kind::Unresolved,
                    &format!("Instrument {voice} is not mapped"),
                    None,
                )?
            };
            b.edge(
                node,
                instrument,
                Relation::UsesInstrument {
                    index: u32::from(*voice),
                },
            )?;
        }
        if track.termination == crate::TrackTermination::Unresolved {
            b.unresolved(
                node,
                "Sequence interpretation stopped before resolving its termination",
            )?;
        }
    }
    if !song.warnings.is_empty() {
        b.unresolved(
            root,
            "See the song inventory for interpretation warnings and unsupported behavior",
        )?;
    }
    Ok(())
}

fn tone(b: &mut Builder<'_>, tone: &ToneInventory) -> Result<u32> {
    let node = b.node(
        Kind::Instrument,
        &format!("Tone type {:02X}", tone.kind),
        Some(tone.descriptor.into()),
    )?;
    if !b.expanded.insert(node) {
        return Ok(node);
    }
    if let Some(sample) = tone.sample {
        let sample_node = b.child(
            node,
            Kind::Sample,
            "PCM sample",
            Some(sample.header.into()),
            Relation::UsesSample { slot: 0 },
        )?;
        b.child(
            sample_node,
            Kind::SampleData,
            "Encoded sample bytes",
            Some(sample.data.into()),
            Relation::Contains,
        )?;
        b.graph.nodes[sample_node as usize].sample = Some(sample);
    } else if tone.synthesis.is_none() && (tone.kind & 7) == 0 && !matches!(tone.kind, 0x40 | 0x80)
    {
        if let Some(header) = tone.sample_header {
            b.child(
                node,
                Kind::Sample,
                "Sample header",
                Some(header.into()),
                Relation::References,
            )?;
        }
        b.unresolved(
            node,
            "Sample is empty, unsupported, or unresolved in this inventory",
        )?;
    }
    if let Some(wave) = tone.waveform {
        b.child(
            node,
            Kind::Waveform,
            "PSG waveform",
            Some(wave.into()),
            Relation::References,
        )?;
    }
    if let Some(recipe) = &tone.synthesis {
        let synth = b.child(
            node,
            Kind::Synthesis,
            recipe.profile,
            Some(recipe.header.into()),
            Relation::References,
        )?;
        b.child(
            synth,
            Kind::Metadata,
            "Synthesis parameters",
            Some(recipe.parameters.into()),
            Relation::Contains,
        )?;
        for span in recipe.engine_evidence {
            b.charge()?;
            b.child(
                synth,
                Kind::Program,
                "Verified synthesis signature",
                Some(span.into()),
                Relation::References,
            )?;
        }
    }
    Ok(node)
}
