use super::*;
use crate::sound_trace::physical::{Occurrence, Stamp, WorkTag};

pub(super) struct Interval {
    pub start: Stamp,
    end: Stamp,
    logical: u16,
    bank: u16,
    value: u8,
}
pub(super) struct Expected {
    tag: WorkTag,
    before: u8,
    interval: usize,
}
fn stamp(companion: &Companion) -> Stamp {
    Stamp {
        half: companion.half,
        pass: companion.pass,
    }
}

pub(super) fn intervals(witness: &work_witness::Witness) -> Vec<Interval> {
    let log = witness.companion_log.as_ref().unwrap();
    assert!(!log.is_empty());
    assert!(!log[0].pins.write && !log.last().unwrap().pins.write);
    let mut previous = None;
    let mut current: Option<(Stamp, u16, u16, u8)> = None;
    let mut intervals = Vec::new();
    for companion in log {
        let at = stamp(companion);
        assert!(previous.is_none_or(|old| old < at));
        previous = Some(at);
        if companion.pins.shared() && companion.pins.write {
            let source = (
                companion.pins.address.unwrap(),
                companion.bank,
                companion.pins.data.unwrap(),
            );
            if let Some((_, address, bank, value)) = current {
                assert_eq!(source, (address, bank, value));
            } else {
                current = Some((at, source.0, source.1, source.2));
            }
        } else if let Some((start, logical, bank, value)) = current.take() {
            intervals.push(Interval {
                start,
                end: at,
                logical,
                bank,
                value,
            });
        }
    }
    assert!(current.is_none());
    intervals
}

pub(super) fn expected(witness: &work_witness::Witness, intervals: &[Interval]) -> Vec<Expected> {
    let mut commits = witness.commits.iter();
    let mut previous = None;
    let mut expected = Vec::new();
    for pulse in &witness.pulses {
        let at = Stamp {
            half: pulse.half,
            pass: pulse.pass,
        };
        assert!(previous.is_none_or(|old| old < at));
        previous = Some(at);
        assert_eq!(pulse.pass, 1);
        assert!(pulse.row_strobe);
        let lanes: Vec<_> = [1, 0]
            .into_iter()
            .filter(|&lane| pulse.write_strobes[lane])
            .collect();
        assert_eq!(lanes.len(), pulse.effects.len());
        let sound = pulse.released;
        let interval = if sound {
            assert!(
                pulse.grant_tristate && pulse.companion.pins.shared() && pulse.companion.pins.write
            );
            assert!(pulse.sound_data_drive && pulse.sound_low_address);
            assert!(
                !pulse.main_address_selected
                    && !pulse.main_data_selected
                    && !pulse.protocol_drive
                    && !pulse.protocol
                    && !pulse.cartridge_drive
            );
            let found: Vec<_> = intervals
                .iter()
                .enumerate()
                .filter(|(_, i)| i.start <= at && at < i.end)
                .collect();
            assert_eq!(found.len(), 1);
            let (index, source) = found[0];
            assert_eq!(
                (
                    pulse.companion.pins.address.unwrap(),
                    pulse.companion.bank,
                    pulse.companion.pins.data.unwrap()
                ),
                (source.logical, source.bank, source.value)
            );
            Some(index)
        } else {
            assert!(pulse.main_address_selected && pulse.main_data_selected);
            None
        };
        for (ordinal, (&lane, &(address, value))) in lanes.iter().zip(&pulse.effects).enumerate() {
            let actual_address = if lane == 0 { pulse.high } else { pulse.low };
            let actual_value = pulse.data.to_be_bytes()[lane];
            assert_eq!((address, value), (actual_address, actual_value));
            let commit = commits.next().unwrap();
            assert_eq!(
                (commit.half, commit.ordinal, commit.address, commit.value),
                (pulse.half, ordinal, address, value)
            );
            if let Some(interval) = interval {
                assert!(!pulse.ram_read_lanes[lane]);
                expected.push(Expected {
                    tag: WorkTag {
                        stamp: at,
                        ordinal,
                        lane,
                        logical: pulse.companion.pins.address.unwrap(),
                        bank: pulse.companion.bank,
                        address,
                        value,
                    },
                    before: commit.before,
                    interval,
                });
            }
        }
    }
    assert!(commits.next().is_none());
    expected
}

pub(super) fn exact(
    journal: &Journal,
    intervals: &[Interval],
    expected: &[Expected],
) -> Vec<Option<Occurrence>> {
    let pulses: Vec<_> = journal
        .events()
        .iter()
        .filter_map(|event| match event.fact {
            Fact::WorkPulse(tag) => Some((event, tag)),
            _ => None,
        })
        .collect();
    let commits: Vec<_> = journal
        .events()
        .iter()
        .filter_map(|event| match event.fact {
            Fact::WorkCommitted { tag, before } => Some((event, tag, before)),
            _ => None,
        })
        .collect();
    assert_eq!(pulses.len(), expected.len());
    assert_eq!(commits.len(), expected.len());
    let mut counts = vec![0; intervals.len()];
    let mut identities: Vec<Option<Option<Occurrence>>> = vec![None; intervals.len()];
    for ((expected, (pulse, tag)), (commit, committed, before)) in
        expected.iter().zip(pulses).zip(commits)
    {
        assert_eq!(tag, expected.tag);
        assert_eq!(committed, expected.tag);
        assert_eq!(pulse.stamp, tag.stamp);
        assert_eq!(commit.stamp, tag.stamp);
        assert_eq!(before, expected.before);
        let activation = journal
            .events()
            .iter()
            .filter(|event| event.stamp <= tag.stamp && matches!(event.fact, Fact::Activated(_)))
            .max_by_key(|event| event.stamp)
            .unwrap();
        let Fact::Activated(binding) = activation.fact else {
            unreachable!()
        };
        assert_eq!(
            (binding.shape, binding.address, binding.bank),
            (Shape::Write, tag.logical, tag.bank)
        );
        assert!(activation.stamp < intervals[expected.interval].start);
        if expected.interval > 0 {
            assert!(activation.stamp >= intervals[expected.interval - 1].end);
        }
        assert_eq!(pulse.occurrence, activation.occurrence);
        assert_eq!(commit.occurrence, activation.occurrence);
        if let Some(id) = identities[expected.interval] {
            assert_eq!(id, pulse.occurrence);
        } else {
            identities[expected.interval] = Some(pulse.occurrence);
        }
        counts[expected.interval] += 1;
        assert_eq!(
            (tag.logical, tag.bank, tag.address, tag.value),
            (0x8000, 0x1ff, 0x8000, 0x22)
        );
    }
    assert!(counts.iter().all(|count| *count >= 2));
    identities.into_iter().map(Option::unwrap).collect()
}

pub(super) fn psg_source(
    journal: &Journal,
    interval: &Interval,
    event: &Event,
    receipt: crate::hardware::vdp::owner::board::PsgWrite,
) -> Option<Occurrence> {
    let source = Stamp {
        half: receipt.binding.half,
        pass: receipt.binding.pass,
    };
    assert!(interval.start <= source && source < interval.end);
    assert_eq!(
        (receipt.binding.address, receipt.binding.bank, receipt.value),
        (interval.logical, interval.bank, interval.value)
    );
    let activation = journal
        .events()
        .iter()
        .filter(|e| e.stamp <= source && matches!(e.fact, Fact::Activated(_)))
        .max_by_key(|e| e.stamp)
        .unwrap();
    let Fact::Activated(binding) = activation.fact else {
        unreachable!()
    };
    assert_eq!(
        (binding.shape, binding.address, binding.bank),
        (Shape::Write, interval.logical, interval.bank)
    );
    assert!(activation.stamp < interval.start);
    assert_eq!(event.occurrence, activation.occurrence);
    assert_eq!((receipt.binding.address, receipt.value), (0x7f11, 0x22));
    event.occurrence
}
