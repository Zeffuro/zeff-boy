use super::*;
use crate::{ControlCommand, VdpLineError, VdpRenderError, VdpScanline};

const REGION: Region = Region::NtscUs;
const NEXT: u16 = 0x4e71;

fn tick(at: u64) -> MasterTicks {
    MasterTicks::new(at)
}

pub(super) fn configure(machine: &mut Emulator, count: u8, mode: u8, increment: u8) {
    for (index, value) in [
        (15, increment),
        (19, count),
        (20, 0),
        (21, 0),
        (22, 0),
        (23, mode),
    ] {
        machine.bus.advance_machine(1).unwrap();
        let write = machine
            .bus
            .vdp
            .queue_register(index, value, REGION)
            .unwrap();
        machine
            .bus
            .advance_machine(write.publish_at.get() - machine.bus.now.get())
            .unwrap();
    }
}

fn copy_machine(concurrent: bool) -> Emulator {
    let mut machine = super::machine(&[NEXT; 8]);
    machine.concurrent_sound = concurrent;
    machine.bus.vdp = Vdp::bootstrap_mode5();
    machine.bus.now = MasterTicks::ZERO;
    machine.bus.audio = crate::hardware::audio::Audio::default();
    configure(&mut machine, 2, 0xc0, 2);
    machine.bus.vdp.write_control(0x4000, REGION).unwrap();
    machine.bus.vdp.write_control(0, REGION).unwrap();
    machine.bus.vdp.write_data(0x1234, REGION).unwrap();
    machine
        .bus
        .advance_machine(201 - machine.bus.now.get())
        .unwrap();
    machine.bus.advance_machine(1).unwrap();
    let write = machine.bus.vdp.queue_register(15, 1, REGION).unwrap();
    machine
        .bus
        .advance_machine(write.publish_at.get() - machine.bus.now.get())
        .unwrap();
    machine
        .bus
        .advance_machine(1001 - machine.bus.now.get())
        .unwrap();
    machine
        .bus
        .vdp
        .resume_dma_from_known_phase(
            ControlCommand {
                address: 0x100,
                code: 0x30,
            },
            None,
            REGION,
        )
        .unwrap();
    machine
}

fn fill_machine(concurrent: bool, address: u32) -> Emulator {
    let mut machine = super::vdp_line::seed(concurrent, NEXT);
    configure(&mut machine, 2, 0x80, 0);
    assert_eq!(machine.bus.now.get(), 960);
    machine
        .bus
        .vdp
        .resume_dma_from_known_phase(
            ControlCommand {
                address,
                code: 0x21,
            },
            Some(0xabcd),
            REGION,
        )
        .unwrap();
    machine
}

pub fn cpu_clones_with(mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>) {
    for concurrent in [false, true] {
        let mut machine = copy_machine(concurrent);
        let pointer = machine.vdp().memory().vram().as_ptr();
        let mut clone = machine.clone();
        for (at, length, bytes) in [
            (1029, 2, [0, 0]),
            (1057, 1, [0, 0x34]),
            (1085, 0, [0x12, 0x34]),
        ] {
            let result = step(&mut machine).unwrap();
            assert_eq!(result.elapsed.get(), 28);
            assert_eq!(step(&mut clone), Ok(result));
            assert_eq!(machine, clone);
            assert_eq!(machine.bus.now.get(), at);
            assert_eq!(machine.vdp().registers().registers()[19], Some(length));
            assert_eq!(&machine.vdp().memory().vram()[0x100..0x102], &bytes);
            assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
        }
        assert_eq!(
            machine.vdp().command(),
            ControlCommand {
                address: 0x102,
                code: 0x10
            }
        );
    }
}

pub fn capture_and_delivery_with(
    mut capture: impl FnMut(&mut Vdp, MasterTicks, Region) -> Result<VdpScanline, VdpLineError>,
    mut advance: impl FnMut(&mut Emulator, u64) -> Result<(), BusError>,
) {
    for concurrent in [false, true] {
        for address in [0x400, 0x2000] {
            let mut machine = fill_machine(concurrent, address);
            let saved = machine.clone();
            let pointer = machine.vdp().memory().vram().as_ptr();
            assert_eq!(
                capture(&mut machine.bus.vdp, tick(1010), REGION),
                Err(VdpLineError::Render(VdpRenderError::PendingLineInputs))
            );
            assert_eq!(machine, saved);
            assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
            if address == 0x2000 {
                assert_eq!(
                    capture(&mut machine.bus.vdp, tick(1330), REGION),
                    Err(VdpLineError::Render(VdpRenderError::UnknownSpriteCache(0)))
                );
                assert_eq!(machine, saved);
            } else {
                let line = capture(&mut machine.bus.vdp, tick(1330), REGION).unwrap();
                assert_eq!(line.colors(), &[0; 256]);
                assert_eq!(line.at(), tick(1330));
                assert_eq!(machine.vdp().memory().vram()[0x401], 0xab);
            }
            for deadline in [1009, 1010, 1330] {
                let mut machine = saved.clone();
                let mut ordinary = machine.clone();
                machine.queue_vdp_capture(tick(deadline)).unwrap();
                advance(&mut machine, 470).unwrap();
                advance(&mut ordinary, 470).unwrap();
                let result = machine.take_vdp_capture().unwrap();
                if deadline < 1330 {
                    assert_eq!(
                        result,
                        Err(VdpLineError::Render(VdpRenderError::PendingLineInputs))
                    );
                } else if address == 0x2000 {
                    assert_eq!(
                        result,
                        Err(VdpLineError::Render(VdpRenderError::UnknownSpriteCache(0)))
                    );
                } else {
                    assert_eq!(result.unwrap().colors(), &[0; 256]);
                }
                assert_eq!(machine, ordinary);
                assert_eq!(machine.vdp().memory().vram()[address as usize ^ 1], 0xab);
            }
        }
    }
}

pub fn copy_delivery_with(mut advance: impl FnMut(&mut Emulator, u64) -> Result<(), BusError>) {
    for concurrent in [false, true] {
        let mut fixture = super::vdp_line::seed(concurrent, NEXT);
        fixture.bus.advance_machine(1).unwrap();
        fixture.bus.vdp.write_control(0x4400, REGION).unwrap();
        fixture.bus.vdp.write_control(0, REGION).unwrap();
        fixture.bus.vdp.write_data(0x1234, REGION).unwrap();
        fixture
            .bus
            .advance_machine(1331 - fixture.bus.now.get())
            .unwrap();
        configure(&mut fixture, 2, 0xc0, 1);
        fixture.bus.advance_machine(1).unwrap();
        let write = fixture.bus.vdp.queue_register(22, 4, REGION).unwrap();
        fixture
            .bus
            .advance_machine(write.publish_at.get() - fixture.bus.now.get())
            .unwrap();
        assert_eq!(fixture.bus.now.get(), 1460);
        fixture
            .bus
            .vdp
            .resume_dma_from_known_phase(
                ControlCommand {
                    address: 0x100,
                    code: 0x30,
                },
                None,
                REGION,
            )
            .unwrap();
        for deadline in [1490, 1650, 1970, 2130] {
            let mut machine = fixture.clone();
            let mut ordinary = machine.clone();
            machine.queue_vdp_capture(tick(deadline)).unwrap();
            advance(&mut machine, 770).unwrap();
            advance(&mut ordinary, 770).unwrap();
            let result = machine.take_vdp_capture().unwrap();
            if deadline < 2130 {
                assert_eq!(
                    result,
                    Err(VdpLineError::Render(VdpRenderError::PendingLineInputs))
                );
            } else {
                assert_eq!(result.unwrap().colors(), &[0; 256]);
            }
            assert_eq!(machine, ordinary);
            assert_eq!(&machine.vdp().memory().vram()[0x100..0x102], &[0x12, 0x34]);
            assert_eq!(machine.vdp().registers().registers()[19], Some(0));
        }
    }
}

pub fn reset_and_horizon_with(
    mut restart: impl FnMut(&mut Emulator) -> Result<(), TimingFault<BusError>>,
    mut reset: impl FnMut(&mut Emulator),
    mut advance: impl FnMut(&mut Emulator, u64) -> Result<(), BusError>,
) {
    for concurrent in [false, true] {
        let mut machine = copy_machine(concurrent);
        advance(&mut machine, 9).unwrap();
        let saved = machine.clone();
        let pointer = machine.vdp().memory().vram().as_ptr();
        machine.bus.cartridge.initial.pc = 0xc00000;
        let failed = machine.clone();
        assert!(restart(&mut machine).is_err());
        assert_eq!(machine, failed);
        assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
        machine.bus.cartridge.initial.pc = 0x100;
        restart(&mut machine).unwrap();
        assert_eq!(&machine.vdp().memory().vram()[0x100..0x102], &[0, 0x34]);
        assert_eq!(machine.vdp().registers().registers()[19], Some(1));
        reset(&mut machine);
        assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
        assert_eq!(machine.vdp().registers().registers()[19], Some(0));
        assert_eq!(machine.vdp().memory().vram()[0x101], 0);
        assert!(machine.bus.vdp.write_control(0x4000, REGION).is_ok());

        let mut machine = saved;
        machine.bus.vdp = Vdp::bootstrap_mode5();
        machine.bus.now = MasterTicks::ZERO;
        machine.bus.audio = crate::hardware::audio::Audio::default();
        configure(&mut machine, 10, 0x80, 1);
        machine
            .bus
            .advance_machine(1001 - machine.bus.now.get())
            .unwrap();
        machine
            .bus
            .vdp
            .resume_dma_from_known_phase(
                ControlCommand {
                    address: 0x3ff6,
                    code: 0x21,
                },
                Some(0xabcd),
                REGION,
            )
            .unwrap();
        machine.queue_vdp_capture(tick(1010)).unwrap();
        let mut split = machine.clone();
        assert_eq!(advance(&mut machine, 209), Ok(()));
        advance(&mut split, 9).unwrap();
        advance(&mut split, 200).unwrap();
        assert_eq!(machine, split);
        assert_eq!(
            machine.vdp().command(),
            ControlCommand {
                address: 0x4000,
                code: 1
            }
        );
        assert_eq!(machine.vdp().registers().registers()[19], Some(0));
        for address in 0x3ff6..0x4000usize {
            assert_eq!(machine.vdp().memory().vram()[address ^ 1], 0xab);
        }
        assert!(machine.take_vdp_capture().unwrap().is_err());
    }
}

#[test]
fn cpu_clones() {
    cpu_clones_with(Emulator::step_instruction);
}
#[test]
fn capture_and_delivery() {
    capture_and_delivery_with(Vdp::capture_line_at, super::vdp_row_delivery::advance);
}

#[test]
fn copy_delivery() {
    copy_delivery_with(super::vdp_row_delivery::advance);
}
#[test]
fn reset_and_horizon() {
    reset_and_horizon_with(
        Emulator::reset_cpu,
        Reset::reset,
        super::vdp_row_delivery::advance,
    );
}
