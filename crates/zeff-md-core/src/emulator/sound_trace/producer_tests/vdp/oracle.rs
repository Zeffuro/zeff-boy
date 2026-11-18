use super::*;
use crate::sound_trace::physical::vdp::raw::{Driver, Event as Raw, Record};
use crate::sound_trace::physical::vdp::{Kind, Primitive, Slot};
use crate::sound_trace::physical::{Shape, Stamp};

struct Interval {
    start: Stamp,
    end: Stamp,
    logical: u16,
    bank: u16,
    value: u8,
}
struct Expected {
    at: Stamp,
    interval: usize,
    input: Primitive,
    kind: Kind,
}
fn intervals(raw: &[Record]) -> Vec<Interval> {
    let mut active = None;
    let mut found = Vec::new();
    let mut last = None;
    for record in raw {
        let Raw::Companion(companion) = record.event else {
            continue;
        };
        assert!(last.is_none_or(|old| old < record.at));
        last = Some(record.at);
        if companion.pins.shared() && companion.pins.write {
            let source = (
                companion.pins.address.unwrap(),
                companion.bank,
                companion.pins.data.unwrap(),
            );
            if let Some((_, logical, bank, value)) = active {
                assert_eq!(source, (logical, bank, value));
            } else {
                active = Some((record.at, source.0, source.1, source.2));
            }
        } else if let Some((start, logical, bank, value)) = active.take() {
            found.push(Interval {
                start,
                end: record.at,
                logical,
                bank,
                value,
            });
        }
    }
    assert!(active.is_none());
    found
}
fn chain(raw: &[Record], end: usize, interval: &Interval, input: Primitive) {
    let capture = (0..end)
        .rev()
        .find(|&index| matches!(raw[index].event, Raw::Capture { .. }))
        .unwrap();
    let Raw::Capture { dma, address, pins } = raw[capture].event else {
        unreachable!()
    };
    assert!(!dma);
    assert_eq!(address, input.address);
    assert_eq!(
        pins.address,
        0xc00000 | (u32::from(interval.logical & 255) & !1)
    );
    assert_eq!(pins.address, input.bus_address);
    assert_eq!(pins.word, u16::from(interval.value) * 0x101);
    assert_eq!(pins.word, input.word);
    assert!(!pins.controls[0] && pins.controls[1]);
    assert_eq!([pins.controls[2], pins.controls[3]], input.lanes);
    let resolve = (0..capture)
        .rev()
        .find(|&index| matches!(raw[index].event, Raw::Resolve { .. }))
        .unwrap();
    let Raw::Resolve {
        released,
        held_strobes,
        held_read,
        pins: resolved,
    } = raw[resolve].event
    else {
        unreachable!()
    };
    assert!(released && !held_strobes && !held_read);
    assert_eq!(pins, resolved);
    assert!(
        !raw[resolve + 1..capture]
            .iter()
            .any(|r| matches!(r.event, Raw::Readback))
    );
    let outputs = (0..resolve)
        .rev()
        .find(|&index| matches!(raw[index].event, Raw::Outputs { .. }))
        .unwrap();
    let arbiter = (0..outputs)
        .rev()
        .find(|&index| matches!(raw[index].event, Raw::Arbiter { .. }))
        .unwrap();
    let Raw::Arbiter {
        logical,
        bank,
        write,
        levels,
        ..
    } = raw[arbiter].event
    else {
        unreachable!()
    };
    assert_eq!(
        (logical, bank, write),
        (interval.logical, interval.bank, true)
    );
    assert!(interval.start <= raw[arbiter].at && raw[arbiter].at < interval.end);
    let Raw::Outputs { levels: copied } = raw[outputs].event else {
        unreachable!()
    };
    assert_eq!(copied, levels);
    assert_eq!(
        [!pins.controls[1], !pins.controls[2], !pins.controls[3]],
        levels
    );
    let data = (0..resolve).rev().find(|&index| matches!(raw[index].event, Raw::Drive { slot: Slot::Data, mask, .. } if mask != 0)).unwrap();
    let Raw::Drive {
        driver,
        mask,
        value,
        ..
    } = raw[data].event
    else {
        unreachable!()
    };
    assert_eq!(
        (driver, mask, value),
        (Driver::SoundData, 0xffff, u32::from(pins.word))
    );
    assert!(interval.start <= raw[data].at && raw[data].at < interval.end);
    let mut address_mask = 0;
    for record in raw[..resolve].iter().rev() {
        if record.at < interval.start {
            break;
        }
        if let Raw::Drive {
            driver,
            slot: Slot::Address,
            mask,
            value,
        } = record.event
        {
            let relevant = mask & !address_mask;
            if relevant == 0 {
                continue;
            }
            assert!(matches!(driver, Driver::SoundLow | Driver::SoundHigh));
            assert_eq!(value & relevant, pins.address & relevant);
            address_mask |= mask;
            if address_mask == 0xfffffe {
                break;
            }
        }
    }
    assert_eq!(address_mask, 0xfffffe);
}
#[derive(Debug, PartialEq, Eq)]
struct UnexplainedAck {
    at: Stamp,
    primitive: u8,
}
fn expected(raw: &[Record], intervals: &[Interval]) -> Result<Vec<Expected>, UnexplainedAck> {
    let mut expected = Vec::new();
    let mut pending_psg_strobe = vec![false; intervals.len()];
    for (index, record) in raw.iter().enumerate() {
        let Raw::Primitive(input) = record.event else {
            continue;
        };
        if !input.ack[0] && !input.ack[1] && !input.psg[0] {
            continue;
        }
        let Some((interval, source)) = intervals
            .iter()
            .enumerate()
            .find(|(_, source)| source.start <= record.at && record.at < source.end)
        else {
            return Err(UnexplainedAck {
                at: record.at,
                primitive: input.ordinal,
            });
        };
        let mut kinds = Vec::new();
        if input.ack[0] {
            assert!(input.main[0] && input.intake[0]);
            assert_eq!(input.load, Some((input.word, [true, false])));
            kinds.push(Kind::Data);
        }
        if input.ack[1] {
            assert!(input.main[0] && (input.intake[1] || input.intake[2]));
            if input.intake[1] {
                kinds.push(Kind::ControlFirst);
            }
            if input.intake[2] {
                kinds.push(Kind::ControlSecond);
            }
        }
        if input.psg[0] {
            assert_eq!(input.main, [false; 3]);
            assert_eq!(input.intake, [false; 3]);
            assert_eq!(input.ack, [false; 3]);
            assert_eq!(input.psg, [true, false]);
            assert!(input.load.is_none());
            assert_eq!(source.logical & 0xff81, 0x7f00);
            if input.lanes == [false, false] {
                // PSG can acknowledge address select before a data strobe.
                chain(raw, index, source, input);
                pending_psg_strobe[interval] = true;
            } else {
                assert_eq!(input.lanes, [true, false]);
                pending_psg_strobe[interval] = false;
                kinds.push(Kind::DisconnectedPsg);
            }
        }
        for kind in kinds {
            chain(raw, index, source, input);
            expected.push(Expected {
                at: record.at,
                interval,
                input,
                kind,
            });
        }
    }
    assert!(pending_psg_strobe.into_iter().all(|pending| !pending));
    Ok(expected)
}
pub(super) fn exact(raw: &[Record], events: &[Event]) -> Vec<(u16, Kind, usize)> {
    let intervals = intervals(raw);
    let expected = expected(raw, &intervals)
        .unwrap_or_else(|gap| panic!("unproved raw ACK outside active SOUND interval: {gap:?}"));
    let actual: Vec<_> = events
        .iter()
        .filter_map(|event| match event.fact {
            Fact::VdpAccepted(receipt) => Some((event, receipt)),
            _ => None,
        })
        .collect();
    assert_eq!(actual.len(), expected.len());
    let mut counts = vec![0; intervals.len()];
    let mut kinds = vec![None; intervals.len()];
    let mut ids = Vec::new();
    for (expected, (event, receipt)) in expected.iter().zip(actual) {
        let interval = &intervals[expected.interval];
        assert_eq!(
            (
                event.stamp,
                receipt.primitive,
                receipt.kind,
                receipt.address,
                receipt.bus_address,
                receipt.word,
                receipt.lanes
            ),
            (
                expected.at,
                expected.input.ordinal,
                expected.kind,
                expected.input.address << 1,
                expected.input.bus_address,
                expected.input.word,
                expected.input.lanes
            )
        );
        let activation = events
            .iter()
            .filter(|event| {
                event.stamp < interval.start && matches!(event.fact, Fact::Activated(_))
            })
            .max_by_key(|event| event.stamp)
            .unwrap();
        let Fact::Activated(binding) = activation.fact else {
            unreachable!()
        };
        assert_eq!(
            (binding.shape, binding.address, binding.bank),
            (Shape::Write, interval.logical, interval.bank)
        );
        assert_eq!(receipt.binding, binding);
        assert_eq!(receipt.source, activation.stamp);
        assert_eq!(event.occurrence, activation.occurrence);
        assert!(event.occurrence.is_some());
        if expected.interval > 0 {
            assert!(activation.stamp >= intervals[expected.interval - 1].end);
        }
        if let Some(kind) = kinds[expected.interval] {
            assert_eq!(kind, receipt.kind);
        } else {
            kinds[expected.interval] = Some(receipt.kind);
            ids.push(event.occurrence);
        }
        counts[expected.interval] += 1;
    }
    assert!(counts.iter().all(|count| *count >= 2));
    for (index, id) in ids.iter().enumerate() {
        assert!(!ids[..index].contains(id));
    }
    assert!(expected.iter().any(|expected| expected.input.ordinal == 1));
    intervals
        .into_iter()
        .zip(kinds)
        .zip(counts)
        .map(|((interval, kind), count)| (interval.logical, kind.unwrap(), count))
        .collect()
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn precise_sound_vdp_raw_oracle_rejects_unowned_ack_outside_source_intervals() {
    let quiet = Primitive {
        ordinal: 1,
        address: 0x600000,
        bus_address: 0xc00000,
        word: 0x5a5a,
        lanes: [true, false],
        main: [false; 3],
        intake: [false; 3],
        ack: [false; 3],
        psg: [false; 2],
        load: None,
    };
    let at = Stamp { half: 100, pass: 1 };
    let intervals = [Interval {
        start: Stamp { half: 90, pass: 0 },
        end: at,
        logical: 0x7f00,
        bank: 0,
        value: 0x5a,
    }];
    let record = |input| {
        [Record {
            at,
            event: Raw::Primitive(input),
        }]
    };
    assert!(expected(&record(quiet), &intervals).unwrap().is_empty());
    for kind in 0..3 {
        let mut input = quiet;
        match kind {
            0 => {
                input.main[0] = true;
                input.intake[0] = true;
                input.ack[0] = true;
                input.load = Some((input.word, [true, false]));
            }
            1 => {
                input.main[0] = true;
                input.intake[1] = true;
                input.ack[1] = true;
            }
            _ => {
                input.psg[0] = true;
            }
        }
        match expected(&record(input), &intervals) {
            Err(gap) => assert_eq!(gap, UnexplainedAck { at, primitive: 1 }),
            Ok(_) => panic!("unexplained raw ACK kind {kind} was omitted"),
        }
    }
}
