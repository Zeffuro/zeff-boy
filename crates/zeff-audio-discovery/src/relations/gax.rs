use std::collections::BTreeMap;

use super::{AssetKind as Kind, Builder, GraphStop, Relation, Result};
use crate::gax::GaxSong;

pub(super) fn project(b: &mut Builder<'_>, song: &GaxSong) -> Result<()> {
    b.graph.profile = Some(super::label(&song.version));
    let root = b.root(Kind::Song, "GAX song", Some(song.header.into()))?;
    let mut samples = BTreeMap::new();
    for sample in &song.samples {
        b.charge()?;
        let node = b.node(
            Kind::Sample,
            &format!("Sample {}", sample.index),
            Some(sample.header.into()),
        )?;
        b.child(
            node,
            Kind::SampleData,
            "Unsigned PCM bytes",
            Some(sample.data.into()),
            Relation::Contains,
        )?;
        samples.insert(sample.index, node);
    }
    let mut instruments = BTreeMap::new();
    for instrument in &song.instruments {
        b.charge()?;
        let node = b.node(
            Kind::Instrument,
            &format!("Instrument {}", instrument.index),
            Some(instrument.descriptor.into()),
        )?;
        b.edge(
            root,
            node,
            Relation::UsesInstrument {
                index: u32::from(instrument.index),
            },
        )?;
        instruments.insert(instrument.index, node);
        for (slot, setting) in instrument.sample_settings.iter().enumerate() {
            b.charge()?;
            b.child(
                node,
                Kind::Metadata,
                &format!("Sample slot {} settings", slot + 1),
                Some(setting.source.into()),
                Relation::Contains,
            )?;
            let sample = instrument
                .sample_indices
                .get(slot)
                .and_then(|index| samples.get(index))
                .copied();
            let sample = match sample {
                Some(node) => node,
                None => b.node(Kind::Unresolved, "Declared sample slot is not mapped", None)?,
            };
            b.edge(
                node,
                sample,
                Relation::DeclaresSampleSlot {
                    slot: slot as u32 + 1,
                },
            )?;
        }
        if instrument.blank {
            continue;
        }
        b.child(
            node,
            Kind::SequenceData,
            "Instrument pattern",
            Some(instrument.rows_span.into()),
            Relation::References,
        )?;
        b.child(
            node,
            Kind::Envelope,
            "Volume envelope",
            Some(instrument.envelope.source.into()),
            Relation::References,
        )?;
        for row in &instrument.rows {
            b.charge()?;
            let sample = row
                .sample_slot
                .checked_sub(1)
                .and_then(|slot| instrument.sample_indices.get(usize::from(slot)))
                .and_then(|index| samples.get(index))
                .copied();
            let sample = match sample {
                Some(node) => node,
                None => b.node(
                    Kind::Unresolved,
                    "Sample selection needs prior state or an unmapped sample",
                    None,
                )?,
            };
            b.edge(
                node,
                sample,
                Relation::UsesSample {
                    slot: u32::from(row.sample_slot),
                },
            )?;
        }
    }
    let mut patterns = Vec::new();
    for (index, pattern) in song.patterns.iter().enumerate() {
        b.charge()?;
        let node = b.node(
            Kind::Pattern,
            &format!("Pattern {index}"),
            Some(pattern.source.into()),
        )?;
        patterns.push(node);
        for row in &pattern.rows {
            b.charge()?;
            if let Some(index @ 1..=255) = row.instrument {
                let instrument = match instruments.get(&index) {
                    Some(&node) => node,
                    None => b.node(
                        Kind::Unresolved,
                        &format!("Instrument {index} is not mapped"),
                        None,
                    )?,
                };
                b.edge(
                    node,
                    instrument,
                    Relation::UsesInstrument {
                        index: u32::from(index),
                    },
                )?;
            }
        }
    }
    for (index, channel) in song.channels.iter().enumerate() {
        b.charge()?;
        let node = b.child(
            root,
            Kind::Channel,
            &format!("Channel {}", index + 1),
            None,
            Relation::Contains,
        )?;
        let table = b.child(
            node,
            Kind::OrderTable,
            "Order table",
            Some(channel.order_table.into()),
            Relation::References,
        )?;
        for (index, order) in channel.orders.iter().enumerate() {
            b.charge()?;
            let relative = u32::try_from(index)
                .ok()
                .and_then(|index| index.checked_mul(4))
                .ok_or(GraphStop::InvalidLocation)?;
            if relative
                .checked_add(4)
                .is_none_or(|end| end > channel.order_table.byte_len)
            {
                return Err(GraphStop::InvalidLocation);
            }
            let offset = channel
                .order_table
                .effective_offset
                .checked_add(relative)
                .ok_or(GraphStop::InvalidLocation)?;
            let span = crate::SourceSpan {
                effective_offset: offset,
                byte_len: 4,
                canonical_cpu_address: Some(
                    0x0800_0000u32
                        .checked_add(offset)
                        .ok_or(GraphStop::InvalidLocation)?,
                ),
            };
            let entry = b.child(
                table,
                Kind::Order,
                &format!("Order {index} · transpose {}", order.transpose),
                Some(span.into()),
                Relation::Contains,
            )?;
            let pattern = match patterns.get(usize::from(order.pattern)) {
                Some(&node) => node,
                None => b.node(
                    Kind::Unresolved,
                    "Order refers to an unmapped pattern",
                    None,
                )?,
            };
            b.edge(
                entry,
                pattern,
                Relation::Selects {
                    index: u32::from(order.pattern),
                },
            )?;
        }
    }
    if !song.warnings.is_empty() {
        b.unresolved(
            root,
            "See the GAX inventory for unsupported commands and projection limits",
        )?;
    }
    Ok(())
}
