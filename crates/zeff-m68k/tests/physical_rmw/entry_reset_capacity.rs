use super::*;
use bulk_device::{Device, Event, Stage};
use bulk_model::Spec;
use device::Kind;

fn offset(events: &[Event], stage: Stage, at: usize) -> u64 {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Transfer(s, _, _, _, clock, _, _, _) if *s == stage => Some(*clock),
            _ => None,
        })
        .nth(at)
        .unwrap()
}

fn idle_offset(events: &[Event], clocks: u32) -> u64 {
    events
        .iter()
        .find_map(|event| match event {
            Event::Idle(clock, n) if *n == clocks => Some(*clock),
            _ => None,
        })
        .unwrap()
}

fn entry_device(s: Spec, at: usize, idle: Option<u32>) -> Device {
    let (_, mut b) = s.seed();
    b.report = Some((
        at,
        Kind::Boundary,
        if idle == Some(4) { u32::MAX } else { 8 },
    ));
    if idle == Some(2) {
        b.entry_failure = Some((9, Kind::Complete, u32::MAX));
    }
    b
}

pub(super) fn reserves() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for (s, primary) in bulk_model::triggers() {
        let b = entry_device(s, primary, None);
        let p = bulk_plan::plan(s, &b);
        let full = bulk_oracle::predict(s, &b, &p);
        for at in 0..11 {
            let elapsed = offset(&full.events, Stage::Entry, at);
            for remaining in [
                u64::from(u32::MAX) - 1,
                u64::from(u32::MAX),
                u64::from(u32::MAX) + 1,
            ] {
                let mut shifted = s;
                shifted.start = u64::MAX - remaining - elapsed;
                let b = entry_device(shifted, primary, None);
                count += 1;
                bad += u32::from(!bulk_oracle::run(shifted, b));
            }
        }
    }
    for s in bulk_reset::seeds() {
        for state in 0..5 {
            let (_, mut b, mut e, _) = bulk_reset::origin(s, state);
            bulk_reset::prepare(&mut b, &mut e, 0, 0x90000, 0x7000);
            bulk_entry_oracle::reset(&mut e, &b, 0).unwrap();
            for at in 0..6 {
                let elapsed = offset(&e.events, Stage::Entry, at);
                for remaining in [
                    u64::from(u32::MAX) - 1,
                    u64::from(u32::MAX),
                    u64::from(u32::MAX) + 1,
                ] {
                    let (mut cpu, mut bus, mut expected, mut good) = bulk_reset::origin(s, state);
                    let release = u64::MAX - remaining - elapsed;
                    bulk_reset::prepare(&mut bus, &mut expected, release, 0x90000, 0x7000);
                    good &= bulk_reset::execute(&mut cpu, &mut bus, expected, release);
                    count += 1;
                    bad += u32::from(!good);
                }
            }
        }
    }
    (count, bad)
}

pub(super) fn idles() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for (s, primary) in bulk_model::triggers() {
        for idle in [4, 2] {
            let b = entry_device(s, primary, Some(idle));
            let p = bulk_plan::plan(s, &b);
            let full = bulk_oracle::predict(s, &b, &p);
            let elapsed = idle_offset(&full.events, idle);
            for remaining in [u64::from(idle - 1), u64::from(idle), u64::from(idle + 1)] {
                let mut shifted = s;
                shifted.start = u64::MAX - remaining - elapsed;
                let b = entry_device(shifted, primary, Some(idle));
                count += 1;
                bad += u32::from(!bulk_oracle::run(shifted, b));
            }
        }
    }
    for s in bulk_reset::seeds() {
        for state in 0..5 {
            for idle in [14, 2] {
                let (_, mut b, mut e, _) = bulk_reset::origin(s, state);
                bulk_reset::prepare(&mut b, &mut e, 0, 0x90000, 0x7000);
                if idle == 2 {
                    b.entry_failure = Some((4, Kind::Complete, u32::MAX));
                }
                bulk_entry_oracle::reset(&mut e, &b, 0).unwrap();
                let elapsed = idle_offset(&e.events, idle);
                for remaining in [u64::from(idle - 1), u64::from(idle), u64::from(idle + 1)] {
                    let (mut cpu, mut bus, mut expected, mut good) = bulk_reset::origin(s, state);
                    let release = u64::MAX - remaining - elapsed;
                    bulk_reset::prepare(&mut bus, &mut expected, release, 0x90000, 0x7000);
                    if idle == 2 {
                        bus.entry_failure = Some((4, Kind::Complete, u32::MAX));
                    }
                    good &= bulk_reset::execute(&mut cpu, &mut bus, expected, release);
                    count += 1;
                    bad += u32::from(!good);
                }
            }
        }
    }
    (count, bad)
}
