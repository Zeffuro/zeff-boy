use super::support::*;
use zeff_md_core::{VdpDmaError, VdpDmaRegisters, VdpDmaWrite, VdpMemory};

pub fn copy_read_write_visibility() {
    copy_read_write_visibility_with(
        &mut VdpDmaRegisters::read_copy_byte,
        &mut VdpDmaRegisters::write_copy_byte,
    );
}

pub fn copy_read_write_visibility_with(read: &mut Read<'_>, write: &mut Write<'_>) {
    for (source, destination, data) in [(0, 16, 0x34), (1, 16, 0x12), (0, 17, 0x34), (1, 17, 0x12)]
    {
        let mut memory = VdpMemory::default();
        word(&mut memory, 0, 0x1234);
        word(&mut memory, 16, 0x5678);
        let mut dma = image(1, source, 0xc7);
        let mut command = command(destination, 0x30);
        let saved = (dma, command, memory.clone());
        let pointer = memory.vram().as_ptr();
        let copy = read(&dma, &command, 2, &memory).unwrap().unwrap();
        assert_eq!(copy.data(), data);
        assert_eq!((dma, command, &memory), (saved.0, saved.1, &saved.2));
        word(&mut memory, 0, 0xabcd);
        word(&mut memory, 16, 0x9abc);
        assert_eq!(
            write(&mut dma, &mut command, 2, copy, &mut memory).unwrap(),
            VdpDmaWrite {
                address: destination as u16 ^ 1,
                data
            }
        );
        assert_eq!(&memory.vram()[0..2], &[0xab, 0xcd]);
        assert_eq!(
            &memory.vram()[16..18],
            &if destination == 16 {
                [0x9a, data]
            } else {
                [data, 0xbc]
            }
        );
        counters(&dma, 0, source + 1, 0xc7);
        assert_eq!(command, super::support::command(destination + 2, 0x10));
        assert_eq!(memory.vram().as_ptr(), pointer);
        assert_eq!(read(&dma, &command, 255, &memory), Ok(None));
    }
}

pub fn phase_clones_and_interleaving() {
    phase_clones_and_interleaving_with(
        &mut VdpDmaRegisters::read_copy_byte,
        &mut VdpDmaRegisters::write_copy_byte,
    );
}

pub fn phase_clones_and_interleaving_with(read: &mut Read<'_>, write: &mut Write<'_>) {
    let mut memory = VdpMemory::default();
    word(&mut memory, 0xfffe, 0x1234);
    word(&mut memory, 0, 0x5678);
    let mut dma = image(2, 0xffff, 0xff);
    let mut command = command(0x100, 0x30);
    let copy = read(&dma, &command, 1, &memory).unwrap().unwrap();
    assert_eq!(copy.data(), 0x12);
    let (mut cloned_dma, mut cloned_command, mut cloned_memory) = (dma, command, memory.clone());
    let pointers = [memory.vram().as_ptr(), cloned_memory.vram().as_ptr()];
    assert_ne!(pointers[0], pointers[1]);
    word(&mut memory, 0xfffe, 0xabcd);
    let first = write(&mut dma, &mut command, 1, copy, &mut memory).unwrap();
    assert_eq!(
        write(
            &mut cloned_dma,
            &mut cloned_command,
            1,
            copy,
            &mut cloned_memory
        )
        .unwrap(),
        first
    );
    assert_eq!(
        first,
        VdpDmaWrite {
            address: 0x101,
            data: 0x12
        }
    );
    counters(&dma, 1, 0, 0xff);
    let copy = read(&dma, &command, 1, &memory).unwrap().unwrap();
    word(&mut memory, 0, 0x9abc);
    assert_eq!(copy.data(), 0x78);
    assert_eq!(
        write(&mut dma, &mut command, 1, copy, &mut memory).unwrap(),
        VdpDmaWrite {
            address: 0x100,
            data: 0x78
        }
    );
    assert_eq!(&memory.vram()[0x100..0x102], &[0x78, 0x12]);
    counters(&dma, 0, 1, 0xff);
    assert_eq!(command, super::support::command(0x102, 0x10));
    assert_eq!(
        [memory.vram().as_ptr(), cloned_memory.vram().as_ptr()],
        pointers
    );
}

pub fn phase_guards() {
    phase_guards_with(
        &mut VdpDmaRegisters::read_copy_byte,
        &mut VdpDmaRegisters::write_copy_byte,
    );
}

pub fn phase_guards_with(read: &mut Read<'_>, write: &mut Write<'_>) {
    let mut memory = VdpMemory::default();
    word(&mut memory, 0, 0x1234);
    let initial = memory.clone();
    let original = image(2, 0, 0xc0);
    let original_command = command(0x100, 0x30);
    let copy = read(&original, &original_command, 2, &memory)
        .unwrap()
        .unwrap();
    for (mut dma, mut command, increment) in [
        (image(1, 0, 0xc0), original_command, 2),
        (image(2, 1, 0xc0), original_command, 2),
        (image(2, 0, 0xc1), original_command, 2),
        (image(2, 0, 0x80), original_command, 2),
        (original, command(0x101, 0x30), 2),
        (original, command(0x100, 0x10), 2),
        (original, original_command, 1),
    ] {
        let saved = (dma, command);
        assert_eq!(
            write(&mut dma, &mut command, increment, copy, &mut memory),
            Err(VdpDmaError::StaleCopy)
        );
        assert_eq!((dma, command), saved);
        assert_eq!(memory, initial);
    }
    for (dma, command, increment, error) in [
        (
            image(1, 0, 0x80),
            original_command,
            2,
            VdpDmaError::WrongOperation(0x80),
        ),
        (
            original,
            command(0x100, 0x20),
            2,
            VdpDmaError::UnsupportedCommand(command(0x100, 0x20)),
        ),
        (
            original,
            command(0x20000, 0x30),
            2,
            VdpDmaError::UnsupportedCommand(command(0x20000, 0x30)),
        ),
        (
            original,
            command(0x20001, 0x30),
            1,
            VdpDmaError::UnsupportedCommand(command(0x20001, 0x30)),
        ),
    ] {
        let saved = (dma, command);
        assert_eq!(read(&dma, &command, increment, &memory), Err(error));
        assert_eq!((dma, command), saved);
        assert_eq!(memory, initial);
    }
    let (mut dma, mut command) = (original, original_command);
    write(&mut dma, &mut command, 2, copy, &mut memory).unwrap();
    let saved = (dma, command, memory.clone());
    assert_eq!(
        write(&mut dma, &mut command, 2, copy, &mut memory),
        Err(VdpDmaError::StaleCopy)
    );
    assert_eq!((dma, command, &memory), (saved.0, saved.1, &saved.2));
}
