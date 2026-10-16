use zeff_emu_common::time::MasterTicks;
use zeff_md_core::{
    ControlCommand, ControlWrite, Region, Vdp, VdpControl, VdpDmaError, VdpPortError,
    VdpRegisterError, VdpScheduleError, VdpWriteError,
};

const INTERNAL_TICKS: u64 = 20;
const HORIZONTAL_EDGE: u64 = 2660;
const MODE_REGISTER: u8 = 1;
const SIZE_REGISTER: u8 = 12;
const RELOAD_REGISTER: u8 = 10;
const DMA_SOURCE_HIGH_REGISTER: u8 = 23;
const MODE5: u8 = 4;
const DMA_ENABLE: u8 = 0x10;
const REGISTER_TAG: u16 = 0x8000;

type Write = Result<ControlWrite, VdpPortError>;

fn tick(value: u64) -> MasterTicks {
    MasterTicks::new(value)
}
fn register_word(index: u8, value: u8) -> u16 {
    REGISTER_TAG | (u16::from(index) << 8) | u16::from(value)
}
fn at(value: u64) -> Vdp {
    let mut device = Vdp::bootstrap_mode5();
    device.advance_to(tick(value), Region::NtscUs).unwrap();
    device
}
fn schedule(error: VdpRegisterError) -> VdpPortError {
    VdpPortError::Schedule(error.into())
}

pub fn settled_command_fields_with(mut write: impl FnMut(&mut Vdp, u16, Region) -> Write) {
    for region in [Region::NtscUs, Region::NtscJapan] {
        for code in [1u8, 3, 5, 0x11, 0x13, 0x15] {
            let low_code = code & 3;
            for low_address in [0u16, 0x1234, 0x3fff] {
                for high_address in 0u16..8 {
                    let mut device = at(1);
                    let first = low_address | (u16::from(low_code) << 14);
                    assert_eq!(
                        write(&mut device, first, region),
                        Ok(ControlWrite::FirstHalf {
                            address_low: low_address,
                            code_low: low_code,
                        })
                    );
                    assert_eq!(device.control().pending_word(), Some(first));
                    assert_eq!(
                        device.command(),
                        ControlCommand {
                            address: u32::from(low_address),
                            code: low_code | 0x10,
                        }
                    );
                    let second = high_address | (u16::from(code & 0x3c) << 2) | 0xff08;
                    let raw = ControlCommand {
                        address: u32::from(low_address) | (u32::from(high_address) << 14),
                        code,
                    };
                    assert_eq!(
                        write(&mut device, second, region),
                        Ok(ControlWrite::Command(raw))
                    );
                    assert_eq!(device.command(), raw);
                    assert_eq!(device.control().pending_word(), None);
                    assert_eq!(device.registers().pending(), None);
                    let next = 0x4001;
                    write(&mut device, next, region).unwrap();
                    assert_eq!(
                        device.command(),
                        ControlCommand {
                            address: (raw.address & 0x1c000) | 1,
                            code: (raw.code & 0x3c) | 0x11,
                        }
                    );
                }
            }
        }
    }
}
pub fn settled_command_fields() {
    settled_command_fields_with(Vdp::write_control);
}

pub fn delayed_register_publication_with(mut write: impl FnMut(&mut Vdp, u16, Region) -> Write) {
    for index in 0u8..24 {
        let value = match index {
            MODE_REGISTER => MODE5 | 0x60,
            SIZE_REGISTER => 0x10,
            DMA_SOURCE_HIGH_REGISTER => 0x80,
            _ => 0x55,
        };
        let mut device = at(1);
        write(&mut device, 0x5234, Region::NtscUs).unwrap();
        write(&mut device, 0x0047, Region::NtscUs).unwrap();
        let before = *device.registers();
        let word = register_word(index, value);
        assert_eq!(
            write(&mut device, word, Region::NtscUs),
            Ok(ControlWrite::Register { index, value })
        );
        assert_eq!(
            device.command(),
            ControlCommand {
                address: 0x1c000 | u32::from(word & 0x3fff),
                code: 0x12,
            }
        );
        assert_eq!(device.control().pending_word(), None);
        assert_eq!(device.registers().registers(), before.registers());
        let pending = device.registers().pending().unwrap();
        assert_eq!(pending.index, index);
        assert_eq!(pending.value, value);
        assert_eq!(pending.publish_at, tick(INTERNAL_TICKS));
        device
            .advance_to(tick(INTERNAL_TICKS - 1), Region::NtscUs)
            .unwrap();
        assert_eq!(device.registers().registers(), before.registers());
        device
            .advance_to(pending.publish_at, Region::NtscUs)
            .unwrap();
        assert_eq!(
            device.registers().registers()[usize::from(index)],
            Some(value)
        );
        assert_eq!(device.registers().pending(), None);
    }
}
pub fn delayed_register_publication() {
    delayed_register_publication_with(Vdp::write_control);
}

pub fn pending_clone_and_bootstrap_with(mut write: impl FnMut(&mut Vdp, u16, Region) -> Write) {
    let bootstrap = Vdp::bootstrap_mode5();
    assert_eq!(*bootstrap.control(), VdpControl::default());
    assert_eq!(
        bootstrap.command(),
        ControlCommand {
            address: 0,
            code: 0
        }
    );
    let mut device = at(1);
    write(&mut device, 0x4123, Region::NtscUs).unwrap();
    let mut cloned = device.clone();
    let reconstructed =
        Vdp::from_known_state(*device.interrupts(), *device.registers(), Region::NtscUs).unwrap();
    assert_eq!(reconstructed.command(), bootstrap.command());
    assert_eq!(reconstructed.control(), bootstrap.control());
    device
        .advance_to(tick(INTERNAL_TICKS - 1), Region::NtscUs)
        .unwrap();
    assert_eq!(device.control().pending_word(), Some(0x4123));
    let word = register_word(RELOAD_REGISTER, 0x55);
    let command = ControlCommand {
        address: 0x14123,
        code: 0x15,
    };
    assert_eq!(
        write(&mut device, word, Region::NtscUs),
        Ok(ControlWrite::Command(command))
    );
    assert_eq!(device.command(), command);
    assert_eq!(device.control().pending_word(), None);
    assert_eq!(device.registers().pending(), None);
    assert_eq!(cloned.control().pending_word(), Some(0x4123));
    write(&mut cloned, 0, Region::NtscUs).unwrap();
    assert_eq!(
        cloned.command(),
        ControlCommand {
            address: 0x123,
            code: 1
        }
    );
    assert_ne!(device, cloned);
    let reconstructed =
        Vdp::from_known_state(*device.interrupts(), *device.registers(), Region::NtscUs).unwrap();
    assert_eq!(reconstructed.command(), bootstrap.command());
    assert_eq!(reconstructed.control(), bootstrap.control());
    assert_eq!(Vdp::bootstrap_mode5(), bootstrap);
}
pub fn pending_clone_and_bootstrap() {
    pending_clone_and_bootstrap_with(Vdp::write_control);
}

pub fn dma_gate_and_rejection_with(mut write: impl FnMut(&mut Vdp, u16, Region) -> Write) {
    let mut device = at(1);
    write(
        &mut device,
        register_word(MODE_REGISTER, MODE5 | DMA_ENABLE),
        Region::NtscUs,
    )
    .unwrap();
    let publication = device.registers().pending().unwrap().publish_at;
    write(&mut device, 0x4234, Region::NtscUs).unwrap();
    let raw = ControlCommand {
        address: 0x1c234,
        code: 0x21,
    };
    assert_eq!(
        write(&mut device, 0x0087, Region::NtscUs),
        Ok(ControlWrite::Command(raw))
    );
    assert_eq!(device.command(), ControlCommand { code: 1, ..raw });
    device.advance_to(publication, Region::NtscUs).unwrap();
    write(&mut device, 0x4234, Region::NtscUs).unwrap();
    let pending = device.clone();
    for second in [0x0087, 0x00f7] {
        let target = ControlCommand {
            code: if second == 0x0087 { 0x21 } else { 0x3d },
            ..raw
        };
        let mut candidate = pending.clone();
        if second == 0x0087 {
            assert_eq!(
                write(&mut candidate, second, Region::NtscUs),
                Ok(ControlWrite::Command(target))
            );
            assert_eq!(candidate.command(), target);
            assert_eq!(candidate.control().pending_word(), None);
            let armed = candidate.clone();
            assert_eq!(
                candidate.advance_to(tick(INTERNAL_TICKS * 2), Region::NtscUs),
                Err(VdpScheduleError::Dma(VdpDmaError::MissingSource))
            );
            assert_eq!(candidate, armed);
        } else {
            assert_eq!(
                write(&mut candidate, second, Region::NtscUs),
                Err(VdpPortError::Dma(VdpDmaError::UnsupportedCommand(target)))
            );
            assert_eq!(candidate, pending);
            assert_eq!(candidate.control().pending_word(), Some(0x4234));
        }
    }
    assert_eq!(
        write(&mut device, 7, Region::NtscUs),
        Ok(ControlWrite::Command(ControlCommand { code: 1, ..raw }))
    );
    assert_eq!(device.control().pending_word(), None);
    device
        .advance_to(tick(INTERNAL_TICKS + 1), Region::NtscUs)
        .unwrap();
    write(
        &mut device,
        register_word(MODE_REGISTER, MODE5),
        Region::NtscUs,
    )
    .unwrap();
    let publication = device.registers().pending().unwrap().publish_at;
    write(&mut device, 0x4234, Region::NtscUs).unwrap();
    let before_disable = device.clone();
    assert_eq!(
        write(&mut device, 0x0087, Region::NtscUs),
        Err(VdpPortError::Dma(VdpDmaError::PendingInputs))
    );
    assert_eq!(device, before_disable);
    device.advance_to(publication, Region::NtscUs).unwrap();
    assert_eq!(
        write(&mut device, 0x0087, Region::NtscUs),
        Ok(ControlWrite::Command(raw))
    );
    assert_eq!(device.command(), ControlCommand { code: 1, ..raw });
}
pub fn dma_gate_and_rejection() {
    dma_gate_and_rejection_with(Vdp::write_control);
}

pub fn register_fault_atomicity_with(mut write: impl FnMut(&mut Vdp, u16, Region) -> Write) {
    let initial = at(1);
    for index in 24..32 {
        let mut device = initial.clone();
        assert_eq!(
            write(&mut device, register_word(index, 0x55), Region::NtscUs),
            Err(schedule(VdpRegisterError::UnsupportedRegister(index)))
        );
        assert_eq!(device, initial);
    }
    for value in 0..0x80 {
        let mut device = initial.clone();
        assert_eq!(
            write(
                &mut device,
                register_word(DMA_SOURCE_HIGH_REGISTER, value),
                Region::NtscUs
            ),
            Ok(ControlWrite::Register {
                index: DMA_SOURCE_HIGH_REGISTER,
                value,
            })
        );
        assert_eq!(
            device.registers().registers(),
            initial.registers().registers()
        );
        let pending = device.registers().pending().unwrap();
        assert_eq!(
            (pending.index, pending.value),
            (DMA_SOURCE_HIGH_REGISTER, value)
        );
        device
            .advance_to(pending.publish_at, Region::NtscUs)
            .unwrap();
        assert_eq!(
            device.registers().registers()[usize::from(DMA_SOURCE_HIGH_REGISTER)],
            Some(value)
        );
        assert_eq!(device.registers().pending(), None);
    }
    for (index, mask, expected) in [(MODE_REGISTER, MODE5 | 8, MODE5), (SIZE_REGISTER, 0x87, 0)] {
        for value in 0..=u8::MAX {
            if value & mask == expected
                || (index == SIZE_REGISTER && matches!(value & mask, 6 | 0x81 | 0x87))
            {
                continue;
            }
            let mut device = initial.clone();
            assert_eq!(
                write(&mut device, register_word(index, value), Region::NtscUs),
                Err(schedule(VdpRegisterError::UnsupportedModeChange {
                    index,
                    value
                }))
            );
            assert_eq!(device, initial);
        }
    }
    let mut device = Vdp::bootstrap_mode5();
    assert_eq!(
        write(
            &mut device,
            register_word(RELOAD_REGISTER, 7),
            Region::NtscUs
        ),
        Ok(ControlWrite::Register {
            index: RELOAD_REGISTER,
            value: 7
        })
    );
    assert_eq!(
        device.registers().pending().unwrap().publish_at,
        tick(INTERNAL_TICKS)
    );
    assert_eq!(
        device.registers().registers()[usize::from(RELOAD_REGISTER)],
        Some(0)
    );
    device = initial.clone();
    write(
        &mut device,
        register_word(RELOAD_REGISTER, 7),
        Region::NtscUs,
    )
    .unwrap();
    let before = device.clone();
    let publication = device.registers().pending().unwrap().publish_at;
    assert_eq!(
        write(&mut device, register_word(0, 0x10), Region::NtscUs),
        Err(schedule(VdpRegisterError::Busy {
            publish_at: publication
        }))
    );
    assert_eq!(device, before);
    write(&mut device, 0x4001, Region::NtscUs).unwrap();
    write(&mut device, 0, Region::NtscUs).unwrap();
    assert_eq!(device.registers().pending(), before.registers().pending());
    let mut device = at(HORIZONTAL_EDGE - 1);
    write(
        &mut device,
        register_word(RELOAD_REGISTER, 7),
        Region::NtscUs,
    )
    .unwrap();
    device
        .advance_to(tick(HORIZONTAL_EDGE), Region::NtscUs)
        .unwrap();
    assert_eq!(
        device.registers().registers()[usize::from(RELOAD_REGISTER)],
        Some(7)
    );
    assert_eq!(device.interrupts().line_counter(), 0);
    assert!(device.interrupts().horizontal_pending());
    let mut device = at(u64::MAX);
    let before = device.clone();
    assert_eq!(
        write(
            &mut device,
            register_word(RELOAD_REGISTER, 7),
            Region::NtscUs
        ),
        Err(schedule(VdpRegisterError::ClockOverflow))
    );
    assert_eq!(device, before);
}
pub fn register_fault_atomicity() {
    register_fault_atomicity_with(Vdp::write_control);
}

pub fn unsupported_region_atomicity_with(mut write: impl FnMut(&mut Vdp, u16, Region) -> Write) {
    for pending in [false, true] {
        for word in [0x4001, 0, register_word(RELOAD_REGISTER, 7)] {
            let mut device = at(1);
            if pending {
                write(&mut device, 0x4123, Region::NtscUs).unwrap();
            }
            let before = device.clone();
            let mut pal = device.clone();
            let mut ntsc = device.clone();
            let result = write(&mut pal, word, Region::Pal);
            assert_eq!(result, write(&mut ntsc, word, Region::NtscUs));
            assert!(result.is_ok());
            assert_eq!(pal, ntsc);
            assert_eq!(device, before);
        }
    }
}
pub fn unsupported_region_atomicity() {
    unsupported_region_atomicity_with(Vdp::write_control);
}

pub fn unsupported_command_atomicity_with(mut write: impl FnMut(&mut Vdp, u16, Region) -> Write) {
    for region in [Region::NtscUs, Region::NtscJapan] {
        for raw_code in 0u8..64 {
            let code = raw_code & 0x1f;
            if raw_code & 3 == 2 || !matches!(code & 0xf, 0 | 4 | 8 | 0xc) {
                continue;
            }
            let mut device = at(1);
            write(&mut device, 0x5234, region).unwrap();
            write(&mut device, 0x0047, region).unwrap();
            let first = 0x0123 | (u16::from(raw_code & 3) << 14);
            write(&mut device, first, region).unwrap();
            let before = device.clone();
            let command = ControlCommand {
                address: 0x14123,
                code,
            };
            let second = 5 | (u16::from(raw_code & 0x3c) << 2);
            assert_eq!(
                write(&mut device, second, region),
                if matches!(command.code, 4 | 8) {
                    Err(VdpPortError::Schedule(VdpScheduleError::Read(
                        zeff_md_core::VdpReadError::UnknownFifoHistory,
                    )))
                } else if command.code == 0 {
                    Err(VdpPortError::Read(
                        zeff_md_core::VdpReadError::UnsupportedCommand(command),
                    ))
                } else {
                    Err(VdpPortError::Control(VdpWriteError::UnsupportedCommand(
                        command,
                    )))
                }
            );
            assert_eq!(device, before);
            assert_eq!(device.control().pending_word(), Some(first));
        }
    }
}
pub fn unsupported_command_atomicity() {
    unsupported_command_atomicity_with(Vdp::write_control);
}
