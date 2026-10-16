use super::support::*;
use zeff_md_core::{VdpScheduleError, VdpVerticalTiming};

const LONG_LENGTH: u16 = 33;

fn operation(fill: Option<u16>) -> (u8, u8, usize) {
    if fill.is_some() {
        (0xbf, 0x31, 1)
    } else {
        (0xff, 0x30, 2)
    }
}

fn partitioned(run: &mut Run<'_>, source: u16, destination: u32, increment: u8, fill: Option<u16>) {
    let (mode, code, stride) = operation(fill);
    let mut one = fixture(LONG_LENGTH, source, mode, increment);
    let mut split = one.clone();
    let before = one.memory().clone();
    let pointer = one.memory().vram().as_ptr();
    let mut expected = [0; 65536];
    expected.copy_from_slice(before.vram());
    for index in 0..LONG_LENGTH {
        let from = source.wrapping_add(index) as usize ^ 1;
        let to = (destination as usize + usize::from(index) * usize::from(increment)) ^ 1;
        expected[to] = fill.map_or(expected[from], |word| (word >> 8) as u8);
    }
    seed(run, &mut one, destination, code, fill);
    seed(run, &mut split, destination, code, fill);
    let opportunities = usize::from(LONG_LENGTH) * stride;
    advance(run, &mut one, slot(opportunities));
    for index in 1..=opportunities {
        advance(run, &mut split, slot(index));
        let completed = (index / stride) as u16;
        counters(
            &split,
            LONG_LENGTH - completed,
            source.wrapping_add(completed),
            mode,
        );
    }
    assert_eq!(one, split);
    assert_eq!(one.memory().vram(), &expected);
    assert_eq!(one.memory().vram().as_ptr(), pointer);
    assert_eq!(one.memory().cram(), before.cram());
    assert_eq!(one.memory().vsram(), before.vsram());
    counters(&one, 0, source.wrapping_add(LONG_LENGTH), mode);
    assert_eq!(
        one.command(),
        command(
            destination + u32::from(LONG_LENGTH) * u32::from(increment),
            code & !0x20,
        )
    );
    busy_status(run, &mut one, false);
}

pub fn long_partitions_with(run: &mut Run<'_>) {
    for increment in [0, 1, 2, 127] {
        for fill in [Some(0x90ff), None] {
            partitioned(run, 0, 0x2000, increment, fill);
        }
    }
}

pub fn long_overlap_and_source_wrap_with(run: &mut Run<'_>) {
    for (source, destination) in [
        (0x100, 0x101),
        (0x101, 0x100),
        (0x100, 0x102),
        (0x102, 0x100),
        (0x100, 0x100),
    ] {
        partitioned(run, source, destination, 1, None);
    }
    for fill in [Some(0x90ff), None] {
        partitioned(run, 0xfff8, 0x2000, 1, fill);
    }
}

pub fn zero_length_continuation_with(run: &mut Run<'_>) {
    const COLLISION: u64 = 224 * 3420 + 10;
    let mut slots = Vec::new();
    let mut at = START;
    while slot_after(at) < COLLISION {
        at = slot_after(at);
        slots.push(at);
    }
    assert_eq!(slots.len(), 37135);
    for fill in [Some(0x90ff), None] {
        let (mode, code, stride) = operation(fill);
        let mut one = fixture(0, 0xffff, mode, 0);
        let before = one.memory().clone();
        let pointer = one.memory().vram().as_ptr();
        let completed = (slots.len() / stride) as u16;
        let mut expected = [0; 65536];
        expected.copy_from_slice(before.vram());
        for index in 0..completed {
            let source = 0xffffu16.wrapping_add(index) as usize ^ 1;
            expected[0x2001] = fill.map_or(expected[source], |word| (word >> 8) as u8);
        }
        seed(run, &mut one, 0x2000, code, fill);
        let seeded = one.clone();
        let requested = slot(65536 * stride);
        let error = Error::Schedule(VdpScheduleError::CoincidentMemory(tick(COLLISION)));
        reject(run, &mut one, Op::Acknowledge(COLLISION), error);
        advance(run, &mut one, COLLISION - 1);
        counters(
            &one,
            0u16.wrapping_sub(completed),
            0xffffu16.wrapping_add(completed),
            mode,
        );
        assert_eq!(one.command(), command(0x2000, code));
        assert_eq!(one.memory().vram(), &expected);
        assert_eq!(one.memory().vram().as_ptr(), pointer);
        assert_eq!(one.memory().cram(), before.cram());
        assert_eq!(one.memory().vsram(), before.vsram());
        for chunk in [9, 16, 33, 997] {
            let mut split = seeded.clone();
            for endpoints in slots.chunks(chunk) {
                advance(run, &mut split, *endpoints.last().unwrap());
            }
            advance(run, &mut split, COLLISION - 1);
            assert_eq!(one, split);
        }
        if fill.is_none() {
            assert_eq!(slots.len() % stride, 1);
            let mut before_read = seeded.clone();
            advance(run, &mut before_read, slots[slots.len() - 2]);
            assert_eq!(before_read.memory(), one.memory());
            counters(
                &before_read,
                0u16.wrapping_sub(completed),
                0xffffu16.wrapping_add(completed),
                mode,
            );
            advance(run, &mut before_read, COLLISION - 1);
            assert_eq!(before_read, one);
        }
        busy_status(run, &mut one, true);
        reject(run, &mut one, Op::Acknowledge(COLLISION), error);
        let mut complete = seeded;
        advance(run, &mut complete, requested);
        advance(run, &mut one, requested);
        assert_eq!(one, complete);
        counters(&one, 0, 0xffff, mode);
        assert_eq!(one.command(), command(0x2000, code & !0x20));
        expected[0x2001] = if fill.is_some() { 0x90 } else { 0xcd };
        assert_eq!(one.memory().vram(), &expected);
        assert_eq!(one.memory().vram().as_ptr(), pointer);
        busy_status(run, &mut one, false);
    }
}
pub fn late_carry_and_iack_with(run: &mut Run<'_>) {
    for fill in [Some(0x90ff), None] {
        let (mode, code, stride) = operation(fill);
        let mut carry = fixture(20, 0, mode, 1);
        seed(run, &mut carry, 0x3ff6, code, fill);
        let mut completed = carry.clone();
        advance(run, &mut completed, slot(20 * stride));
        advance(run, &mut carry, slot(9 * stride));
        counters(&carry, 11, 9, mode);
        assert_eq!(carry.command(), command(0x3fff, code));
        advance(run, &mut carry, slot(20 * stride));
        assert_eq!(carry, completed);
        counters(&carry, 0, 20, mode);
        assert_eq!(carry.command(), command(0x400a, code & !0x20));
        let mut device = fixture(LONG_LENGTH, 0, mode, 1);
        seed(run, &mut device, 0x2000, code, fill);
        let mut expected = device.clone();
        advance(run, &mut expected, 2661);
        let last_write = slot(usize::from(LONG_LENGTH) * stride);
        reject(
            run,
            &mut device,
            Op::Acknowledge(last_write),
            Error::Schedule(VdpScheduleError::CoincidentMemory(tick(last_write))),
        );
        reject(
            run,
            &mut device,
            Op::Acknowledge(2660),
            Error::Schedule(VdpScheduleError::CoincidentAcknowledge(tick(2660))),
        );
        assert_eq!(run(&mut device, Op::Acknowledge(2661)), Ok(Value::Level(0)));
        assert_eq!(device, expected);
        counters(&device, 0, LONG_LENGTH, mode);
        busy_status(run, &mut device, false);
    }
}

pub fn long_active_field_margin_with(run: &mut Run<'_>) {
    for fill in [Some(0x90ff), None] {
        let (mode, code, stride) = operation(fill);
        let mut device = known(0, 220 * 3420 + START, 0x44, None, true);
        for (index, value) in [(15, 0), (19, 0), (23, mode)] {
            register(&mut device, index, value);
        }
        let calendar = VdpVerticalTiming::at_counters_zero(tick(0));
        let mut at = device.now();
        let mut prefix = at;
        for _ in 0..9 * stride {
            at = calendar
                .next_memory_slot(at, REGION, device.registers())
                .unwrap();
            prefix = at;
        }
        assert!(prefix.get() < 222 * 3420);
        seed(run, &mut device, 0x2000, code, fill);
        advance(run, &mut device, prefix.get());
        counters(&device, 0u16.wrapping_sub(9), 9, mode);
        assert_eq!(device.command(), command(0x2000, code));
        advance(run, &mut device, 222 * 3420);
        assert_eq!(device.now(), tick(222 * 3420));
        assert_eq!(device.command(), command(0x2000, code));
        assert_eq!(
            device.memory().vram()[0x2001],
            if fill.is_some() { 0x90 } else { 0 }
        );
    }
}
