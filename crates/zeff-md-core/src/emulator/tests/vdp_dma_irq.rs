use super::*;
use crate::{
    ControlCommand, VdpInterruptError, VdpLineError, VdpScanline, VdpScheduleError, VdpTimingError,
};

const REGION: Region = Region::NtscUs;
const TIE: u64 = 224 * 3420 + 10;
const NEXT: u16 = 0x4e71;
const MODE: u8 = 0x24;

fn tick(at: u64) -> MasterTicks {
    MasterTicks::new(at)
}

fn fixture(concurrent: bool, copy: bool, count: u8, start: u64, increment: u8) -> Emulator {
    let mut machine = super::machine(&[NEXT; 8]);
    machine.concurrent_sound = concurrent;
    machine.bus.vdp = Vdp::bootstrap_mode5();
    machine.bus.now = MasterTicks::ZERO;
    machine.bus.audio = crate::hardware::audio::Audio::default();
    super::vdp_dma::configure(&mut machine, count, if copy { 0xc0 } else { 0x80 }, 0);
    machine.bus.vdp.write_control(0x4000, REGION).unwrap();
    machine.bus.vdp.write_control(0, REGION).unwrap();
    machine.bus.vdp.write_data(0x1234, REGION).unwrap();
    machine
        .bus
        .advance_machine(201 - machine.bus.now.get())
        .unwrap();
    for (index, value) in [(15, increment), (1, MODE)] {
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
    machine
        .bus
        .advance_machine(start - machine.bus.now.get())
        .unwrap();
    machine
        .bus
        .vdp
        .resume_dma_from_known_phase(
            ControlCommand {
                address: 0x100,
                code: if copy { 0x30 } else { 0x21 },
            },
            if copy { None } else { Some(0xabcd) },
            REGION,
        )
        .unwrap();
    machine
}

pub fn cpu_crossing_with(
    mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>,
    mut restart: impl FnMut(&mut Emulator) -> Result<(), TimingFault<BusError>>,
    mut reset: impl FnMut(&mut Emulator),
) {
    for concurrent in [false, true] {
        for copy in [false, true] {
            let mut machine = fixture(concurrent, copy, if copy { 1 } else { 2 }, TIE - 21, 1);
            assert_eq!(machine.vdp().interrupt_level(), 0);
            assert!(!machine.vdp().interrupts().vertical_pending());
            let pointer = machine.vdp().memory().vram().as_ptr();
            let mut clone = machine.clone();
            let result = step(&mut machine).unwrap();
            assert_eq!(result.elapsed.get(), 4 * MAIN_CPU_DIVISOR);
            assert_eq!(step(&mut clone), Ok(result));
            assert_eq!(machine, clone);
            assert_eq!(machine.bus.now, tick(TIE + 7));
            assert!(machine.vdp().interrupts().vertical_pending());
            assert_eq!(machine.vdp().interrupt_level(), 6);
            assert_eq!(machine.vdp().registers().registers()[19], Some(0));
            assert_eq!(
                &machine.vdp().memory().vram()[0x100..0x102],
                if copy { &[0, 0x34] } else { &[0xab, 0xab] }
            );
            assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
            machine.bus.cartridge.initial.pc = 0xc00000;
            let before = machine.clone();
            assert!(restart(&mut machine).is_err());
            assert_eq!(machine, before);
            assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
            machine.bus.cartridge.initial.pc = 0x100;
            restart(&mut machine).unwrap();
            assert!(machine.vdp().interrupts().vertical_pending());
            reset(&mut machine);
            assert!(!machine.vdp().interrupts().vertical_pending());
            assert_eq!(machine.vdp().memory().vram()[0x101], 0);
            assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
        }
    }
}

pub fn capture_delivery_with(
    mut capture: impl FnMut(&mut Vdp, MasterTicks, Region) -> Result<VdpScanline, VdpLineError>,
    mut advance: impl FnMut(&mut Emulator, u64) -> Result<(), BusError>,
) {
    for concurrent in [false, true] {
        for copy in [false, true] {
            let start = TIE - if copy { 381 } else { 201 };
            let mut machine = fixture(concurrent, copy, 17, start, 0);
            let before = machine.clone();
            let pointer = machine.vdp().memory().vram().as_ptr();
            let error = Err(VdpLineError::Schedule(VdpScheduleError::Interrupt(
                VdpInterruptError::Timing(VdpTimingError::UnsupportedMode {
                    index: 1,
                    value: MODE,
                }),
            )));
            assert_eq!(capture(&mut machine.bus.vdp, tick(TIE), REGION), error);
            assert_eq!(machine, before);
            assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
            let mut ordinary = machine.clone();
            let mut split = machine.clone();
            machine.queue_vdp_capture(tick(TIE)).unwrap();
            split.queue_vdp_capture(tick(TIE)).unwrap();
            let end = TIE + 401;
            advance(&mut machine, end - start).unwrap();
            advance(&mut ordinary, end - start).unwrap();
            advance(&mut split, TIE - 1 - start).unwrap();
            assert_eq!(split.take_vdp_capture(), None);
            advance(&mut split, 1).unwrap();
            assert_eq!(split.take_vdp_capture(), Some(error.clone()));
            advance(&mut split, end - TIE).unwrap();
            assert_eq!(machine.take_vdp_capture(), Some(error));
            assert_eq!(machine, ordinary);
            assert_eq!(machine, split);
            assert!(machine.vdp().interrupts().vertical_pending());
            assert_eq!(machine.vdp().interrupt_level(), 6);
            assert_eq!(machine.vdp().registers().registers()[19], Some(0));
            assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
        }
    }
}

pub fn interrupt_entry_with(
    mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>,
) {
    const IRQ_TIE: u64 = TIE + 1000;
    for concurrent in [false, true] {
        for copy in [false, true] {
            let count = if copy { 9 } else { 17 };
            let mut machine = super::vdp::ready(IRQ_TIE - 4000, concurrent);
            super::vdp_dma::configure(&mut machine, count, if copy { 0xc0 } else { 0x80 }, 0);
            machine.bus.advance_machine(1).unwrap();
            machine.bus.vdp.write_control(0x4000, REGION).unwrap();
            machine.bus.vdp.write_control(0, REGION).unwrap();
            machine.bus.vdp.write_data(0x1234, REGION).unwrap();
            machine
                .bus
                .advance_machine(IRQ_TIE - 80 - machine.bus.now.get())
                .unwrap();
            machine
                .bus
                .vdp
                .resume_dma_from_known_phase(
                    ControlCommand {
                        address: 0x100,
                        code: if copy { 0x30 } else { 0x21 },
                    },
                    if copy { None } else { Some(0xabcd) },
                    REGION,
                )
                .unwrap();
            let mut clone = machine.clone();
            let pointer = machine.vdp().memory().vram().as_ptr();
            let result = step(&mut machine).unwrap();
            assert_eq!(step(&mut clone), Ok(result.clone()));
            super::vdp::entry(&machine, &result, 4, 44);
            assert_eq!(machine, clone);
            assert!(machine.vdp().interrupts().horizontal_pending());
            assert!(!machine.vdp().interrupts().vertical_pending());
            assert_eq!(machine.vdp().interrupt_level(), 4);
            assert_eq!(machine.vdp().registers().registers()[19], Some(2));
            for _ in 0..2 {
                let result = step(&mut machine).unwrap();
                assert_eq!(step(&mut clone), Ok(result));
            }
            assert_eq!(machine, clone);
            assert_eq!(machine.vdp().registers().registers()[19], Some(0));
            assert_eq!(
                machine.vdp().memory().vram()[0x101],
                if copy { 0 } else { 0xab }
            );
            assert!(!machine.vdp().interrupts().vertical_pending());
            assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
        }
    }
}

#[test]
fn cpu_crossing() {
    cpu_crossing_with(
        Emulator::step_instruction,
        Emulator::reset_cpu,
        Reset::reset,
    );
}
#[test]
fn capture_delivery() {
    capture_delivery_with(Vdp::capture_line_at, super::vdp_row_delivery::advance);
}
#[test]
fn interrupt_entry() {
    interrupt_entry_with(Emulator::step_instruction);
}
