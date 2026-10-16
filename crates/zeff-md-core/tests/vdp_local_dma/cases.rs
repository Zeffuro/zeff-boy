use super::support::*;
use zeff_md_core::{VdpDmaWrite, VdpMemory};

pub fn fill_trigger_and_lanes() {
    fill_trigger_and_lanes_with(&mut transfer);
}

pub fn fill_trigger_and_lanes_with(step: &mut Step<'_>) {
    for (address, increment, expected, next) in [
        (16, 2, [0x12, 0x34, 0, 0x12, 0, 0], 20),
        (17, 2, [0x34, 0x12, 0x12, 0, 0, 0], 21),
        (16, 1, [0x12, 0x34, 0, 0, 0, 0], 18),
        (17, 1, [0x34, 0x12, 0, 0x12, 0, 0], 19),
        (16, 0, [0x12, 0x12, 0, 0, 0, 0], 16),
        (17, 0, [0x12, 0x12, 0, 0, 0, 0], 17),
    ] {
        let mut memory = VdpMemory::default();
        let pointer = memory.vram().as_ptr();
        word(&mut memory, address, 0x1234);
        let mut dma = image(1, 0x00ff, 0xbf);
        let mut command = command(address + u32::from(increment), 0x21);
        counters(&dma, 1, 0x00ff, 0xbf);
        assert_eq!(
            step(
                &mut dma,
                &mut command,
                increment,
                &mut memory,
                Operation::Fill(0x1234)
            )
            .unwrap(),
            Some(VdpDmaWrite {
                address: (address + u32::from(increment)) as u16 ^ 1,
                data: 0x12,
            })
        );
        assert_eq!(&memory.vram()[16..22], &expected);
        assert_eq!(command, super::support::command(next, 1));
        counters(&dma, 0, 0x0100, 0xbf);
        assert_eq!(memory.vram().as_ptr(), pointer);
        assert!(
            memory
                .cram()
                .iter()
                .chain(memory.vsram())
                .all(|&value| value == 0)
        );
    }
}

pub fn all_increments_and_live_fill_data() {
    all_increments_and_live_fill_data_with(&mut transfer);
}

pub fn all_increments_and_live_fill_data_with(step: &mut Step<'_>) {
    for increment in 0..=255u8 {
        let mut memory = VdpMemory::default();
        let mut dma = image(2, 0x7ffe, 0x80);
        let mut command = command(0x2000, 0x31);
        let first = step(
            &mut dma,
            &mut command,
            increment,
            &mut memory,
            Operation::Fill(0xab45),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            first,
            VdpDmaWrite {
                address: 0x2001,
                data: 0xab
            }
        );
        let second = step(
            &mut dma,
            &mut command,
            increment,
            &mut memory,
            Operation::Fill(0xcd67),
        )
        .unwrap()
        .unwrap();
        assert_eq!(second.address, (0x2000 + u16::from(increment)) ^ 1);
        assert_eq!(second.data, 0xcd);
        assert_eq!(memory.vram()[usize::from(second.address)], 0xcd);
        assert_eq!(
            memory.vram()[0x2001],
            if increment == 0 { 0xcd } else { 0xab }
        );
        assert_eq!(command.address, 0x2000 + 2 * u32::from(increment));
        assert_eq!(command.code, 0x11);
        counters(&dma, 0, 0x8000, 0x80);
    }
    let mut memory = VdpMemory::default();
    let mut dma = image(3, 0xffff, 0xa5);
    let mut command = command(0x100, 0x21);
    for (increment, data, address) in [(1, 0xabc1, 0x101), (4, 0xdef2, 0x100), (0, 0x98e3, 0x104)] {
        assert_eq!(
            step(
                &mut dma,
                &mut command,
                increment,
                &mut memory,
                Operation::Fill(data)
            )
            .unwrap(),
            Some(VdpDmaWrite {
                address,
                data: (data >> 8) as u8
            })
        );
    }
    assert_eq!(&memory.vram()[0x100..0x106], &[0xde, 0xab, 0, 0, 0x98, 0]);
    counters(&dma, 0, 2, 0xa5);
    assert_eq!(command, super::support::command(0x105, 1));
}

pub fn copy_lanes_and_source_wrap() {
    copy_lanes_and_source_wrap_with(&mut transfer);
}

pub fn copy_lanes_and_source_wrap_with(step: &mut Step<'_>) {
    for increment in 0..=255u8 {
        let mut memory = VdpMemory::default();
        word(&mut memory, 0, 0x1234);
        let mut dma = image(2, 0, 0xc0);
        let mut command = command(0x2000, 0x30);
        assert_eq!(
            step(
                &mut dma,
                &mut command,
                increment,
                &mut memory,
                Operation::Copy
            )
            .unwrap(),
            Some(VdpDmaWrite {
                address: 0x2001,
                data: 0x34
            })
        );
        let second = step(
            &mut dma,
            &mut command,
            increment,
            &mut memory,
            Operation::Copy,
        )
        .unwrap()
        .unwrap();
        assert_eq!(second.address, (0x2000 + u16::from(increment)) ^ 1);
        assert_eq!(second.data, 0x12);
        assert_eq!(memory.vram()[usize::from(second.address)], 0x12);
        assert_eq!(
            memory.vram()[0x2001],
            if increment == 0 { 0x12 } else { 0x34 }
        );
        assert_eq!(
            command,
            super::support::command(0x2000 + 2 * u32::from(increment), 0x10)
        );
        counters(&dma, 0, 2, 0xc0);
    }
    for (source, destination, expected) in [
        (0, 16, [0, 0x34]),
        (1, 16, [0, 0x12]),
        (0, 17, [0x34, 0]),
        (1, 17, [0x12, 0]),
    ] {
        let mut memory = VdpMemory::default();
        word(&mut memory, 0, 0x1234);
        let mut dma = image(1, source, 0xff);
        let mut command = command(destination, 0x30);
        step(&mut dma, &mut command, 1, &mut memory, Operation::Copy).unwrap();
        assert_eq!(&memory.vram()[16..18], &expected);
        counters(&dma, 0, source + 1, 0xff);
        assert_eq!(command, super::support::command(destination + 1, 0x10));
    }
    let mut memory = VdpMemory::default();
    word(&mut memory, 0xfffe, 0x2211);
    word(&mut memory, 0, 0x4433);
    let mut dma = image(3, 0xfffe, 0xc7);
    let mut command = command(0x20, 0x30);
    for (source, address, data) in [(0xffff, 0x21, 0x11), (0, 0x23, 0x22), (1, 0x25, 0x33)] {
        assert_eq!(
            step(&mut dma, &mut command, 2, &mut memory, Operation::Copy).unwrap(),
            Some(VdpDmaWrite { address, data })
        );
        assert_eq!(dma.source(), source);
    }
    assert_eq!(
        &memory.vram()[0x20..0x28],
        &[0, 0x11, 0, 0x22, 0, 0x33, 0, 0]
    );
    counters(&dma, 0, 1, 0xc7);
    assert_eq!(command, super::support::command(0x26, 0x10));
}

pub fn progressive_overlap() {
    progressive_overlap_with(&mut transfer);
}

pub fn progressive_overlap_with(step: &mut Step<'_>) {
    for (source, destination, expected) in [
        (0x100, 0x101, [0x20, 0x20, 0x20, 0x20, 0x50, 0x20]),
        (0x101, 0x100, [0x40, 0x10, 0x60, 0x30, 0x50, 0x60]),
        (0x100, 0x102, [0x10, 0x20, 0x10, 0x20, 0x10, 0x20]),
        (0x102, 0x100, [0x30, 0x40, 0x50, 0x60, 0x50, 0x60]),
        (0x100, 0x100, [0x10, 0x20, 0x30, 0x40, 0x50, 0x60]),
    ] {
        let mut memory = VdpMemory::default();
        word(&mut memory, 0x100, 0x1020);
        word(&mut memory, 0x102, 0x3040);
        word(&mut memory, 0x104, 0x5060);
        let mut dma = image(4, source, 0xc0);
        let mut command = command(destination, 0x30);
        for _ in 0..4 {
            step(&mut dma, &mut command, 1, &mut memory, Operation::Copy).unwrap();
        }
        assert_eq!(&memory.vram()[0x100..0x106], &expected);
        counters(&dma, 0, source + 4, 0xc0);
    }
    let mut memory = VdpMemory::default();
    word(&mut memory, 0x100, 0x1020);
    word(&mut memory, 0x102, 0x3040);
    let mut dma = image(2, 0x100, 0xc0);
    let mut command = command(0x200, 0x30);
    step(&mut dma, &mut command, 1, &mut memory, Operation::Copy).unwrap();
    word(&mut memory, 0x100, 0x5566);
    step(&mut dma, &mut command, 1, &mut memory, Operation::Copy).unwrap();
    assert_eq!(&memory.vram()[0x200..0x202], &[0x55, 0x20]);
}

pub fn full_count_and_completion() {
    full_count_and_completion_with(&mut transfer);
}

pub fn full_count_and_completion_with(step: &mut Step<'_>) {
    for (mode, code, operation) in [
        (0x80, 0x21, Operation::Fill(0xabcd)),
        (0xc0, 0x30, Operation::Copy),
    ] {
        let mut memory = VdpMemory::default();
        word(&mut memory, 0, 0x1234);
        let pointer = memory.vram().as_ptr();
        let mut dma = image(0, 0xffff, mode);
        let mut command = command(0x2000, code);
        for index in 1..=65536u32 {
            assert!(
                step(&mut dma, &mut command, 0, &mut memory, operation)
                    .unwrap()
                    .is_some()
            );
            assert_eq!(dma.length(), 0u16.wrapping_sub(index as u16));
            assert_eq!(dma.source(), 0xffffu16.wrapping_add(index as u16));
            assert_eq!(command.code & 0x20 != 0, index < 65536);
        }
        let finished = memory.clone();
        assert_eq!(memory.vram()[0x2001], if mode == 0x80 { 0xab } else { 0 });
        assert_eq!(
            step(&mut dma, &mut command, 255, &mut memory, operation),
            Ok(None)
        );
        assert_eq!(memory, finished);
        assert_eq!(memory.vram().as_ptr(), pointer);
        counters(&dma, 0, 0xffff, mode);
        assert_eq!(command, super::support::command(0x2000, code & !0x20));
    }
    for length in [1, 255, 256, 65535] {
        let mut memory = VdpMemory::default();
        let mut dma = image(length, 0, 0x80);
        let mut command = command(0, 0x21);
        for remaining in (0..length).rev() {
            step(
                &mut dma,
                &mut command,
                0,
                &mut memory,
                Operation::Fill(0x5678),
            )
            .unwrap();
            assert_eq!(dma.length(), remaining);
        }
        assert_eq!(memory.vram()[1], 0x56);
        assert_eq!(dma.source(), length);
        assert_eq!(command.code, 1);
    }
}

pub fn clones_and_retained_memory() {
    clones_and_retained_memory_with(&mut transfer);
}

pub fn clones_and_retained_memory_with(step: &mut Step<'_>) {
    let mut first = VdpMemory::default();
    word(&mut first, 0, 0x1234);
    let mut dma = image(3, 0, 0xc0);
    let mut command = command(0x100, 0x30);
    let write = step(&mut dma, &mut command, 2, &mut first, Operation::Copy).unwrap();
    let mut second = first.clone();
    let pointers = [first.vram().as_ptr(), second.vram().as_ptr()];
    assert_ne!(pointers[0], pointers[1]);
    let (mut copied_dma, mut copied_command) = (dma, command);
    for _ in 0..2 {
        assert_eq!(
            step(&mut dma, &mut command, 2, &mut first, Operation::Copy),
            step(
                &mut copied_dma,
                &mut copied_command,
                2,
                &mut second,
                Operation::Copy
            )
        );
    }
    assert_eq!(
        write,
        Some(VdpDmaWrite {
            address: 0x101,
            data: 0x34
        })
    );
    assert_eq!(first, second);
    assert_eq!(dma, copied_dma);
    assert_eq!(command, copied_command);
    assert_eq!([first.vram().as_ptr(), second.vram().as_ptr()], pointers);
    word(&mut second, 0x100, 0x5678);
    assert_eq!(&first.vram()[0x100..0x106], &[0, 0x34, 0, 0x12, 0, 0]);
}
