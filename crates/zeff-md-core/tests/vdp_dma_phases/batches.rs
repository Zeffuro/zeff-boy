use super::support::*;
use zeff_md_core::{VdpDmaError, VdpMemory};

pub fn all_increment_batch_layouts() {
    all_increment_batch_layouts_with(&mut batch);
}

pub fn all_increment_batch_layouts_with(run: &mut Batch<'_>) {
    let step: &mut base::Step<'_> = &mut base::transfer;
    for increment in 0..=255u8 {
        for (mode, code, operation, expected) in [
            (0x80, 0x21, Operation::Fill(0xabcd), [0xab; 3]),
            (0xc0, 0x30, Operation::Copy, [0x34, 0x12, 0x78]),
        ] {
            let mut memory = VdpMemory::default();
            word(&mut memory, 0, 0x1234);
            word(&mut memory, 2, 0x5678);
            let pointer = memory.vram().as_ptr();
            let mut dma = image(3, 0, mode);
            let mut command = command(0x2000, code);
            let (mut scalar, mut scalar_command, mut scalar_memory) =
                (dma, command, memory.clone());
            for _ in 0..3 {
                step(
                    &mut scalar,
                    &mut scalar_command,
                    increment,
                    &mut scalar_memory,
                    operation,
                )
                .unwrap();
            }
            assert_eq!(
                run(&mut dma, &mut command, increment, operation, 3, &mut memory),
                Ok(3)
            );
            for (index, &data) in expected.iter().enumerate() {
                let address = (0x2000 + index * usize::from(increment)) ^ 1;
                assert_eq!(
                    memory.vram()[address],
                    if increment == 0 { expected[2] } else { data }
                );
            }
            counters(&dma, 0, 3, mode);
            assert_eq!(
                command,
                super::support::command(0x2000 + 3 * u32::from(increment), code & !0x20)
            );
            assert_eq!(memory.vram().as_ptr(), pointer);
            assert_eq!(
                (dma, command, &memory),
                (scalar, scalar_command, &scalar_memory)
            );
        }
    }
}

pub fn overlap_and_chunks() {
    overlap_and_chunks_with(&mut batch);
}

pub fn overlap_and_chunks_with(run: &mut Batch<'_>) {
    for (source, destination, expected) in [
        (0x100, 0x101, [0x20, 0x20, 0x20, 0x20, 0x50, 0x20]),
        (0x101, 0x100, [0x40, 0x10, 0x60, 0x30, 0x50, 0x60]),
        (0x100, 0x102, [0x10, 0x20, 0x10, 0x20, 0x10, 0x20]),
        (0x102, 0x100, [0x30, 0x40, 0x50, 0x60, 0x50, 0x60]),
        (0x100, 0x100, [0x10, 0x20, 0x30, 0x40, 0x50, 0x60]),
    ] {
        for chunks in [[4, 0], [1, 3], [2, 2], [3, 1]] {
            let mut memory = VdpMemory::default();
            word(&mut memory, 0x100, 0x1020);
            word(&mut memory, 0x102, 0x3040);
            word(&mut memory, 0x104, 0x5060);
            let mut dma = image(4, source, 0xc0);
            let mut command = command(destination, 0x30);
            for count in chunks {
                assert_eq!(
                    run(
                        &mut dma,
                        &mut command,
                        1,
                        Operation::Copy,
                        count,
                        &mut memory
                    ),
                    Ok(count)
                );
            }
            assert_eq!(&memory.vram()[0x100..0x106], &expected);
            counters(&dma, 0, source + 4, 0xc0);
            assert_eq!(command.address, destination + 4);
            assert_eq!(command.code, 0x10);
        }
    }
    let mut memory = VdpMemory::default();
    word(&mut memory, 0xfffe, 0x2211);
    word(&mut memory, 0, 0x4433);
    let mut dma = image(3, 0xfffe, 0xc0);
    let mut command = command(0x100, 0x30);
    assert_eq!(
        run(&mut dma, &mut command, 2, Operation::Copy, 3, &mut memory),
        Ok(3)
    );
    assert_eq!(&memory.vram()[0x100..0x106], &[0, 0x11, 0, 0x22, 0, 0x33]);
    counters(&dma, 0, 1, 0xc0);
}

pub fn batch_capacity_and_failures() {
    batch_capacity_and_failures_with(&mut batch);
}

pub fn batch_capacity_and_failures_with(run: &mut Batch<'_>) {
    for (mode, code, operation) in [
        (0x80, 0x21, Operation::Fill(0xabcd)),
        (0xc0, 0x30, Operation::Copy),
    ] {
        let mut memory = VdpMemory::default();
        word(&mut memory, 0, 0x1234);
        let pointer = memory.vram().as_ptr();
        let mut dma = image(9, 0, mode);
        let mut command = command(0x100, code);
        let saved = (dma, command, memory.clone());
        assert_eq!(
            run(&mut dma, &mut command, 1, operation, 9, &mut memory),
            Err(VdpDmaError::BatchFull)
        );
        assert_eq!((dma, command, &memory), (saved.0, saved.1, &saved.2));
        assert_eq!(
            run(&mut dma, &mut command, 1, operation, 8, &mut memory),
            Ok(8)
        );
        assert_eq!(
            &memory.vram()[0x100..0x108],
            if mode == 0x80 {
                &[0xab; 8]
            } else {
                &[0x12, 0x34, 0, 0, 0, 0, 0, 0]
            }
        );
        counters(&dma, 1, 8, mode);
        let before_last = memory.clone();
        assert_eq!(
            run(&mut dma, &mut command, 1, operation, 8, &mut memory),
            Ok(1)
        );
        counters(&dma, 0, 9, mode);
        assert_eq!(command.address, 0x109);
        assert_eq!(
            &memory.vram()[0x100..0x108],
            &before_last.vram()[0x100..0x108]
        );
        assert_eq!(memory.vram().as_ptr(), pointer);

        let mut dma = image(10, 0, mode);
        let mut command = super::support::command(0x3ff8, code);
        assert_eq!(
            run(&mut dma, &mut command, 1, operation, 8, &mut memory),
            Ok(8)
        );
        counters(&dma, 2, 8, mode);
        assert_eq!(command.address, 0x4000);
        assert_eq!(
            run(&mut dma, &mut command, 1, operation, 1, &mut memory),
            Ok(1)
        );
        counters(&dma, 1, 9, mode);
        assert_eq!(command.address, 0x4001);
        for (mut dma, mut command, error) in [
            (
                image(2, 0, mode),
                super::support::command(0x100, 0x20),
                VdpDmaError::UnsupportedCommand(super::support::command(0x100, 0x20)),
            ),
            (
                image(2, 0, mode),
                super::support::command(0x20000, code),
                VdpDmaError::UnsupportedCommand(super::support::command(0x20000, code)),
            ),
            (
                image(2, 0, if mode == 0x80 { 0xc0 } else { 0x80 }),
                super::support::command(0x100, code),
                VdpDmaError::WrongOperation(if mode == 0x80 { 0xc0 } else { 0x80 }),
            ),
        ] {
            let saved = (dma, command, memory.clone());
            assert_eq!(
                run(&mut dma, &mut command, 1, operation, 1, &mut memory),
                Err(error)
            );
            assert_eq!((dma, command, &memory), (saved.0, saved.1, &saved.2));
        }
    }
    let mut memory = VdpMemory::default();
    word(&mut memory, 0, 0x1234);
    let mut dma = image(9, 0, 0xc0);
    let mut command = command(0x100, 0x30);
    let copy = dma.read_copy_byte(&command, 1, &memory).unwrap().unwrap();
    assert_eq!(
        run(&mut dma, &mut command, 1, Operation::Copy, 9, &mut memory),
        Err(VdpDmaError::BatchFull)
    );
    word(&mut memory, 0, 0xabcd);
    assert_eq!(
        dma.write_copy_byte(&mut command, 1, copy, &mut memory)
            .unwrap()
            .data,
        0x34
    );
    assert_eq!(memory.vram()[0x101], 0x34);
}

pub fn batch_completion_and_zero_count() {
    batch_completion_and_zero_count_with(&mut batch);
}

pub fn batch_completion_and_zero_count_with(run: &mut Batch<'_>) {
    for (mode, code, operation) in [
        (0x80, 0x21, Operation::Fill(0xabcd)),
        (0xc0, 0x30, Operation::Copy),
    ] {
        for chunks in [[u32::MAX, 0], [17, 65519]] {
            let mut memory = VdpMemory::default();
            word(&mut memory, 0, 0x1234);
            let pointer = memory.vram().as_ptr();
            let mut dma = image(0, 0xffff, mode);
            let mut command = command(0x2000, code);
            let mut completed = 0;
            for count in chunks {
                let result = run(&mut dma, &mut command, 0, operation, count, &mut memory).unwrap();
                assert_eq!(result, count.min(65536 - completed));
                completed += result;
            }
            assert_eq!(completed, 65536);
            counters(&dma, 0, 0xffff, mode);
            assert_eq!(command, super::support::command(0x2000, code & !0x20));
            assert_eq!(memory.vram()[0x2001], if mode == 0x80 { 0xab } else { 0 });
            let saved = (dma, command, memory.clone());
            assert_eq!(
                run(
                    &mut dma,
                    &mut command,
                    255,
                    operation,
                    u32::MAX,
                    &mut memory
                ),
                Ok(0)
            );
            assert_eq!((dma, command, &memory), (saved.0, saved.1, &saved.2));
            assert_eq!(memory.vram().as_ptr(), pointer);
        }
        let mut memory = VdpMemory::default();
        let mut dma = image(1, 0, mode);
        let mut command = command(0x3fff, 0xff);
        let saved = (dma, command, memory.clone());
        assert_eq!(
            run(&mut dma, &mut command, 255, operation, 0, &mut memory),
            Ok(0)
        );
        assert_eq!((dma, command, &memory), (saved.0, saved.1, &saved.2));
    }
}
