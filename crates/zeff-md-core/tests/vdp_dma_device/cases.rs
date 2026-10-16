use super::support::*;
use zeff_md_core::{VdpDmaError, VdpPortError, VdpRenderError};

pub fn fill_slots_with(run: &mut Run<'_>) {
    for parity in [0, 1] {
        let mut device = fixture(2, 0x12ff, 0xa5, 1);
        let before = device.memory().clone();
        let pointer = device.memory().vram().as_ptr();
        seed(run, &mut device, 0x2000 + parity, 0x31, Some(0xabcd));
        counters(&device, 2, 0x12ff, 0xa5);
        assert_eq!(device.memory(), &before);
        busy_status(run, &mut device, true);
        advance(run, &mut device, 1009);
        assert_eq!(device.memory(), &before);
        counters(&device, 2, 0x12ff, 0xa5);
        advance(run, &mut device, 1010);
        assert_eq!(device.memory().vram()[(0x2000 + parity) as usize ^ 1], 0xab);
        assert_eq!(device.memory().vram()[(0x2001 + parity) as usize ^ 1], 0);
        counters(&device, 1, 0x1300, 0xa5);
        assert_eq!(device.command(), command(0x2001 + parity, 0x31));
        busy_status(run, &mut device, true);
        advance(run, &mut device, 1029);
        counters(&device, 1, 0x1300, 0xa5);
        advance(run, &mut device, 1030);
        assert_eq!(device.memory().vram()[(0x2001 + parity) as usize ^ 1], 0xab);
        counters(&device, 0, 0x1301, 0xa5);
        assert_eq!(device.command(), command(0x2002 + parity, 0x11));
        busy_status(run, &mut device, false);
        assert_eq!(device.memory().vram().as_ptr(), pointer);
        assert_eq!(device.memory().cram(), before.cram());
        assert_eq!(device.memory().vsram(), before.vsram());
    }
}

pub fn copy_slots_and_clones_with(run: &mut Run<'_>) {
    for source in [0, 1] {
        for parity in [0, 1] {
            let mut device = fixture(2, source, 0xe5, 1);
            let memory = device.memory().clone();
            let pointer = device.memory().vram().as_ptr();
            let bytes = if source == 0 {
                [0x34, 0x12]
            } else {
                [0x12, 0x78]
            };
            seed(run, &mut device, 0x2000 + parity, 0x30, None);
            let registers = *device.registers();
            advance(run, &mut device, 1010);
            assert_eq!(device.memory(), &memory);
            assert_eq!(device.registers().registers(), registers.registers());
            assert_eq!(device.command(), command(0x2000 + parity, 0x30));
            counters(&device, 2, source, 0xe5);
            let mut cloned_read = device.clone();
            busy_status(run, &mut device, true);
            advance(run, &mut device, 1029);
            assert_eq!(device.memory(), &memory);
            advance(run, &mut device, 1030);
            assert_eq!(
                device.memory().vram()[(0x2000 + parity) as usize ^ 1],
                bytes[0]
            );
            counters(&device, 1, source + 1, 0xe5);
            assert_eq!(device.command(), command(0x2001 + parity, 0x30));
            let written = device.memory().clone();
            advance(run, &mut device, 1050);
            assert_eq!(device.memory(), &written);
            counters(&device, 1, source + 1, 0xe5);
            advance(run, &mut device, 1069);
            assert_eq!(device.memory(), &written);
            advance(run, &mut device, 1070);
            advance(run, &mut cloned_read, 1070);
            assert_eq!(device, cloned_read);
            assert_eq!(
                device.memory().vram()[(0x2001 + parity) as usize ^ 1],
                bytes[1]
            );
            counters(&device, 0, source + 2, 0xe5);
            assert_eq!(device.command(), command(0x2002 + parity, 0x10));
            busy_status(run, &mut device, false);
            assert_eq!(device.memory().vram().as_ptr(), pointer);
        }
    }
}

pub fn all_increments_with(run: &mut Run<'_>) {
    for increment in 0..=255u8 {
        for fill in [Some(0xabcd), None] {
            let (mode, code) = if fill.is_some() {
                (0x80, 0x21)
            } else {
                (0xc0, 0x30)
            };
            let mut one = fixture(3, 0, mode, increment);
            let mut split = one.clone();
            let mut expected = [0; 65536];
            expected.copy_from_slice(one.memory().vram());
            for index in 0..3 {
                let destination = (0x2000 + index * usize::from(increment)) ^ 1;
                expected[destination] = fill.map_or(expected[index ^ 1], |_| 0xab);
            }
            let pointer = one.memory().vram().as_ptr();
            seed(run, &mut one, 0x2000, code, fill);
            seed(run, &mut split, 0x2000, code, fill);
            let opportunities = if fill.is_some() { 3 } else { 6 };
            advance(run, &mut one, slot(opportunities));
            for index in 1..=opportunities {
                advance(run, &mut split, slot(index));
                let completed = if fill.is_some() { index } else { index / 2 };
                counters(&split, 3 - completed as u16, completed as u16, mode);
            }
            assert_eq!(one, split);
            assert_eq!(one.memory().vram(), &expected);
            assert_eq!(one.memory().vram().as_ptr(), pointer);
            counters(&one, 0, 3, mode);
            assert_eq!(
                one.command(),
                command(0x2000 + 3 * u32::from(increment), code & !0x20)
            );
            if increment == 0 {
                assert_eq!(
                    one.memory().vram()[0x2001],
                    if fill.is_some() { 0xab } else { 0x78 }
                );
            } else {
                for (index, data) in [0x34, 0x12, 0x78].into_iter().enumerate() {
                    assert_eq!(
                        one.memory().vram()[(0x2000 + index * usize::from(increment)) ^ 1],
                        fill.map_or(data, |_| 0xab)
                    );
                }
            }
        }
    }
}

pub fn overlap_and_partitions_with(run: &mut Run<'_>) {
    for (source, destination, bytes) in [
        (0x100, 0x101, [0x20, 0x20, 0x20, 0x20, 0x50, 0x20]),
        (0x101, 0x100, [0x40, 0x10, 0x60, 0x30, 0x50, 0x60]),
        (0x100, 0x102, [0x10, 0x20, 0x10, 0x20, 0x10, 0x20]),
        (0x102, 0x100, [0x30, 0x40, 0x50, 0x60, 0x50, 0x60]),
        (0x100, 0x100, [0x10, 0x20, 0x30, 0x40, 0x50, 0x60]),
    ] {
        let base = fixture(4, source, 0xc0, 1);
        let mut expected = [0; 65536];
        expected.copy_from_slice(base.memory().vram());
        expected[0x100..0x106].copy_from_slice(&bytes);
        let mut one = base.clone();
        seed(run, &mut one, destination, 0x30, None);
        advance(run, &mut one, slot(8));
        assert_eq!(one.memory().vram(), &expected);
        for first in 0..=4 {
            let mut split = base.clone();
            seed(run, &mut split, destination, 0x30, None);
            advance(run, &mut split, slot(first * 2));
            advance(run, &mut split, slot(8));
            assert_eq!(split, one);
        }
        for first in [1, 3, 5, 7] {
            let mut split = base.clone();
            seed(run, &mut split, destination, 0x30, None);
            advance(run, &mut split, slot(first));
            counters(
                &split,
                4 - (first / 2) as u16,
                source + (first / 2) as u16,
                0xc0,
            );
            advance(run, &mut split, slot(8));
            assert_eq!(split, one);
        }
        counters(&one, 0, source + 4, 0xc0);
    }
}

pub fn source_wrap_and_zero_length_with(run: &mut Run<'_>) {
    for length in [0, 2] {
        for fill in [Some(0x90ff), None] {
            let (mode, code, opportunities) = if fill.is_some() {
                (0xbf, 0x21, 2)
            } else {
                (0xff, 0x30, 4)
            };
            let mut device = fixture(length, 0xffff, mode, 1);
            seed(run, &mut device, 0x2000, code, fill);
            advance(run, &mut device, slot(opportunities));
            assert_eq!(
                &device.memory().vram()[0x2000..0x2002],
                &if fill.is_some() {
                    [0x90, 0x90]
                } else {
                    [0x34, 0xab]
                }
            );
            counters(&device, length.wrapping_sub(2), 1, mode);
            assert_eq!(
                device.command(),
                command(0x2002, if length == 0 { code } else { code & !0x20 })
            );
            busy_status(run, &mut device, length == 0);
        }
    }
}

pub fn dma_invalidates_cached_word_with(run: &mut Run<'_>) {
    for fill in [Some(0xabcd), None] {
        let (mode, code) = if fill.is_some() {
            (0x80, 0x21)
        } else {
            (0xc0, 0x30)
        };
        for destination in [0, 1, 2, 3, 4, 5, 6, 7, 0x200] {
            let mut device = fixture(1, 0x100, mode, 1);
            assert_eq!(
                device.sprite_cache().unwrap().entry_words(0),
                Ok([Some(0x0234), Some(0x0678)])
            );
            let before = device.memory().clone();
            seed(run, &mut device, destination, code, fill);
            if fill.is_none() {
                advance(run, &mut device, slot(1));
                assert_eq!(device.memory(), &before);
                assert_eq!(
                    device.sprite_cache(),
                    Err(VdpRenderError::PendingSpriteWrites)
                );
            }
            advance(run, &mut device, slot(if fill.is_some() { 1 } else { 2 }));
            let raw = (destination ^ 1) as usize;
            assert_eq!(
                device.memory().vram()[raw],
                if fill.is_some() { 0xab } else { 0x20 }
            );
            let mut cached = [Some(0x0234), Some(0x0678)];
            if destination < 4 {
                cached[(destination / 2) as usize] = None;
            }
            assert_eq!(device.sprite_cache().unwrap().entry_words(0), Ok(cached));
            if destination < 4 {
                assert_eq!(
                    device.sprite_cache().unwrap().select_row(64),
                    Err(VdpRenderError::UnknownSpriteCache(0))
                );
            }
            for (index, &old) in before.vram().iter().enumerate() {
                if index != raw {
                    assert_eq!(device.memory().vram()[index], old);
                }
            }
        }
    }
}

pub fn busy_ports_and_restart_with(run: &mut Run<'_>) {
    for fill in [Some(0xabcd), None] {
        let (mode, code) = if fill.is_some() {
            (0x80, 0x21)
        } else {
            (0xc0, 0x30)
        };
        let mut device = fixture(2, 0, mode, 1);
        seed(run, &mut device, 0x2000, code, fill);
        for opportunity in 0..=2 {
            if opportunity > 0 {
                advance(run, &mut device, slot(opportunity));
            }
            if fill.is_some() && opportunity == 2 {
                break;
            }
            for op in [
                Op::Control(0x8f02),
                Op::Data(0x1234),
                Op::Read,
                Op::ReadWait,
                Op::WriteWait,
                Op::Seed(command(0x2100, code), fill),
            ] {
                reject(
                    run,
                    &mut device,
                    op,
                    Error::Port(VdpPortError::Dma(VdpDmaError::Busy)),
                );
            }
            for index in 0..24 {
                reject(
                    run,
                    &mut device,
                    Op::Register(index, 0),
                    Error::Schedule(zeff_md_core::VdpScheduleError::Dma(VdpDmaError::Busy)),
                );
            }
            busy_status(run, &mut device, true);
        }
        advance(run, &mut device, slot(if fill.is_some() { 2 } else { 4 }));
        busy_status(run, &mut device, false);
        let Value::Register(write) = run(&mut device, Op::Register(7, 9)).unwrap() else {
            panic!("register deadline")
        };
        advance(run, &mut device, write.publish_at.get() + 1);
        assert_eq!(device.registers().registers()[7], Some(9));
    }
}
