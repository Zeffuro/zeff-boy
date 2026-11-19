use super::support::*;
use zeff_md_core::{Vdp, VdpDmaError, VdpDmaRegisters, VdpMemory, VdpRegisterPipeline};

pub fn published_register_images() {
    published_register_images_with(&mut VdpDmaRegisters::from_registers);
}

pub fn published_register_images_with(
    create: &mut impl FnMut(&VdpRegisterPipeline) -> Result<VdpDmaRegisters, VdpDmaError>,
) {
    let mut registers = VdpRegisterPipeline::at_h32_edge(tick(0));
    for index in 19..=23 {
        let saved = registers;
        assert_eq!(create(&registers), Err(VdpDmaError::UnknownRegister(index)));
        assert_eq!(registers, saved);
        publish(&mut registers, index, if index == 23 { 0x80 } else { 0 });
    }
    for mode in 0..=255u8 {
        let registers = super::support::registers(0x1234, 0xabcd, mode);
        let saved = registers;
        let result = create(&registers);
        counters(&result.unwrap(), 0x1234, 0xabcd, mode);
        assert_eq!(registers, saved);
    }
    for index in 19..=23 {
        let mut registers = super::support::registers(2, 0x100, 0x80);
        registers
            .advance_to(tick(registers.now().get() + 1))
            .unwrap();
        let write = registers.queue_register(index, 0xff).unwrap();
        let saved = registers;
        assert_eq!(create(&registers), Err(VdpDmaError::PendingRegister(write)));
        assert_eq!(registers, saved);
        registers.advance_to(write.publish_at).unwrap();
        assert!(create(&registers).is_ok());
    }
    let mut registers = super::support::registers(2, 0x100, 0x80);
    let saved = create(&registers).unwrap();
    publish(&mut registers, 19, 7);
    counters(&saved, 2, 0x100, 0x80);
    counters(&create(&registers).unwrap(), 7, 0x100, 0x80);
    registers
        .advance_to(tick(registers.now().get() + 1))
        .unwrap();
    registers.queue_register(7, 3).unwrap();
    assert!(create(&registers).is_ok());
}

pub fn transfer_faults_are_atomic() {
    transfer_faults_are_atomic_with(&mut transfer);
}

pub fn transfer_faults_are_atomic_with(step: &mut Step<'_>) {
    for (mode, code, operation) in [
        (0x80, 0x21, Operation::Fill(0x1234)),
        (0xc0, 0x30, Operation::Copy),
    ] {
        for (address, increment, error) in [
            (
                0x20000,
                1,
                VdpDmaError::UnsupportedCommand(command(0x20000, code)),
            ),
            (
                u32::MAX,
                255,
                VdpDmaError::UnsupportedCommand(command(u32::MAX, code)),
            ),
        ] {
            let mut memory = VdpMemory::default();
            word(&mut memory, 0, 0xabcd);
            let mut dma = image(2, 0xffff, mode);
            let mut command = command(address, code);
            let saved = (dma, command, memory.clone());
            let pointer = memory.vram().as_ptr();
            assert_eq!(
                step(&mut dma, &mut command, increment, &mut memory, operation),
                Err(error)
            );
            assert_eq!((dma, command, &memory), (saved.0, saved.1, &saved.2));
            assert_eq!(memory.vram().as_ptr(), pointer);
        }
        for invalid in [0, 3, 5, 0x20, 0x23, 0x25, 0x28, 0x40, 0x61, 0xff] {
            let mut memory = VdpMemory::default();
            let mut dma = image(1, 0, mode);
            let mut command = command(0, invalid);
            let saved = (dma, command, memory.clone());
            assert_eq!(
                step(&mut dma, &mut command, 0, &mut memory, operation),
                Err(VdpDmaError::UnsupportedCommand(command))
            );
            assert_eq!((dma, command, &memory), (saved.0, saved.1, &saved.2));
        }
        let mut memory = VdpMemory::default();
        let mut dma = image(1, 0, mode);
        let mut command = command(0x3fff, code);
        assert!(
            step(&mut dma, &mut command, 0, &mut memory, operation)
                .unwrap()
                .is_some()
        );
        let saved = (dma, command, memory.clone());
        assert_eq!(
            step(&mut dma, &mut command, 255, &mut memory, operation),
            Ok(None)
        );
        assert_eq!((dma, command, &memory), (saved.0, saved.1, &saved.2));
    }
    for (mode, code, operation) in [
        (0x80, 0x30, Operation::Copy),
        (0xc0, 0x21, Operation::Fill(0xabcd)),
    ] {
        let mut dma = image(1, 0, mode);
        let mut command = command(0, code);
        let mut memory = VdpMemory::default();
        let saved = (dma, command, memory.clone());
        assert_eq!(
            step(&mut dma, &mut command, 1, &mut memory, operation),
            Err(VdpDmaError::WrongOperation(mode))
        );
        assert_eq!((dma, command, &memory), (saved.0, saved.1, &saved.2));
    }
    let mut dma = image(2, 0, 0x80);
    let mut command = command(0x3ffe, 0x21);
    let mut memory = VdpMemory::default();
    step(
        &mut dma,
        &mut command,
        1,
        &mut memory,
        Operation::Fill(0xabcd),
    )
    .unwrap();
    assert!(
        step(
            &mut dma,
            &mut command,
            1,
            &mut memory,
            Operation::Fill(0x1234)
        )
        .unwrap()
        .is_some()
    );
    assert_eq!(command.address, 0x4000);
    counters(&dma, 0, 2, 0x80);
    assert_eq!(memory.vram()[0x3fff], 0xab);
    assert_eq!(memory.vram()[0x3ffe], 0x12);
}

pub fn device_dma_guards_remain() {
    let region = zeff_md_core::Region::NtscUs;
    for mode in [0x80, 0xc0] {
        let mut device = Vdp::bootstrap_mode5();
        device.advance_to(tick(1), region).unwrap();
        device.queue_register(1, 0x14, region).unwrap();
        device.advance_to(tick(20), region).unwrap();
        device.advance_to(tick(21), region).unwrap();
        device.queue_register(23, mode, region).unwrap();
        device.advance_to(tick(40), region).unwrap();
        device
            .write_control(if mode == 0x80 { 0x4000 } else { 0 }, region)
            .unwrap();
        let command = command(0, if mode == 0x80 { 0x21 } else { 0x30 });
        assert_eq!(
            device.write_control(if mode == 0x80 { 0x80 } else { 0xc0 }, region),
            Ok(zeff_md_core::ControlWrite::Command(command))
        );
        assert_eq!(device.command(), command);
        assert_eq!(device.read_status(region).unwrap() & 2, 2);
    }
}
