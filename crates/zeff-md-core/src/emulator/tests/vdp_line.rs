use super::*;
use crate::{VdpLineError, VdpRenderError, VdpScanline};

const NEXT: u16 = 0x4e71;
const REGION: Region = Region::NtscUs;

pub(super) fn seed(concurrent: bool, opcode: u16) -> Emulator {
    let mut machine =
        super::vdp_stable_display::fixture(112, &[0x2080, opcode, NEXT, NEXT], concurrent);
    machine.cpu.registers.d[0] = 0;
    machine.step_instruction().unwrap();
    machine
        .bus
        .advance_machine(850 - machine.bus.now.get())
        .unwrap();
    machine
}

pub(super) fn palette_command(machine: &mut Emulator) {
    machine.bus.vdp.write_control(0xc000, REGION).unwrap();
    machine.bus.vdp.write_control(0, REGION).unwrap();
}

pub fn cpu_palette_publication_with(
    mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>,
    mut snapshot: impl FnMut(&Emulator) -> Result<VdpScanline, VdpLineError>,
) {
    for concurrent in [false, true] {
        for (opcode, value, color) in [(0x3080, 0x0246, 0x0246), (0x1080, 0x00ea, 0x0aea)] {
            let mut machine = seed(concurrent, opcode);
            let before = snapshot(&machine).unwrap();
            assert_eq!(before.row(), 0);
            assert_eq!(before.at(), MasterTicks::new(850));
            assert_eq!(before.colors(), &[0; 256]);
            palette_command(&mut machine);
            machine.cpu.registers.d[0] = value;
            let mut copy = machine.clone();
            let result = step(&mut machine).unwrap();
            assert_eq!(step(&mut copy), Ok(result.clone()));
            assert_eq!(machine, copy);
            assert_eq!(result.elapsed.get(), 8 * MAIN_CPU_DIVISOR);
            assert_eq!(
                snapshot(&machine),
                Err(VdpLineError::Render(VdpRenderError::PendingLineInputs))
            );
            machine
                .bus
                .advance_machine(1011 - machine.bus.now.get())
                .unwrap();
            let captured = snapshot(&machine).unwrap();
            assert_eq!(captured.colors(), &[color; 256]);
            let write = machine.bus.vdp.queue_register(7, 1, REGION).unwrap();
            machine
                .bus
                .advance_machine(write.publish_at.get() - 1 - machine.bus.now.get())
                .unwrap();
            assert_eq!(snapshot(&machine).unwrap().colors(), captured.colors());
            machine.bus.advance_machine(1).unwrap();
            assert_eq!(snapshot(&machine).unwrap().colors(), &[0; 256]);
            assert_eq!(captured.colors(), &[color; 256]);
            assert_eq!(before.colors(), &[0; 256]);
        }
    }
}

pub fn fifo_and_sound_with(
    mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>,
    mut snapshot: impl FnMut(&Emulator) -> Result<VdpScanline, VdpLineError>,
) {
    for live_sound in [false, true] {
        let mut machine = seed(true, 0x2080);
        machine.bus.advance_machine(1).unwrap();
        palette_command(&mut machine);
        for color in [0x0246, 0x0240, 0x000e, 0x0aaa] {
            machine.bus.vdp.write_data(color, REGION).unwrap();
        }
        if live_sound {
            super::vdp_data::sound_hold(&mut machine, 870);
        }
        let mut copy = machine.clone();
        let result = step(&mut machine).unwrap();
        assert_eq!(step(&mut copy), Ok(result.clone()));
        assert_eq!(machine, copy);
        assert_eq!(result.elapsed.get(), 73 * MAIN_CPU_DIVISOR);
        assert_eq!(machine.bus.now.get(), 1362);
        assert_eq!(
            snapshot(&machine),
            Err(VdpLineError::Render(VdpRenderError::PendingLineInputs))
        );
        machine
            .bus
            .advance_machine(2131 - machine.bus.now.get())
            .unwrap();
        let line = snapshot(&machine).unwrap();
        assert_eq!(line.colors(), &[0x0246; 256]);
        assert_eq!(line.at(), machine.vdp().now());
        if live_sound {
            assert_eq!(machine.peek(0xff8000), Some(0x5a));
            assert!(machine.sound_bus_granted());
            assert!(machine.sound_schedule.pending.is_none());
        }
    }
}

pub fn reset_snapshot_domains_with(
    mut restart: impl FnMut(&mut Emulator) -> Result<(), TimingFault<BusError>>,
    mut power_on: impl FnMut(&mut Emulator),
    mut snapshot: impl FnMut(&Emulator) -> Result<VdpScanline, VdpLineError>,
) {
    for concurrent in [false, true] {
        let mut machine = seed(concurrent, NEXT);
        machine
            .bus
            .advance_machine(989 - machine.bus.now.get())
            .unwrap();
        let before = snapshot(&machine).unwrap();
        palette_command(&mut machine);
        machine.bus.vdp.write_data(0x0246, REGION).unwrap();
        machine.bus.cartridge.initial.pc = 0xc0000e;
        let saved = machine.clone();
        let pointer = machine.vdp().memory().vram().as_ptr();
        let mut progressed = machine.clone();
        progressed.bus.advance_machine(28).unwrap();
        assert_eq!(snapshot(&progressed).unwrap().colors(), &[0x0246; 256]);
        assert!(restart(&mut machine).is_err());
        assert_eq!(machine, saved);
        assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
        assert_eq!(before.colors(), &[0; 256]);
        machine.bus.cartridge.initial.pc = 0x100;
        restart(&mut machine).unwrap();
        assert_eq!(snapshot(&machine).unwrap().colors(), &[0x0246; 256]);
        power_on(&mut machine);
        assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
        assert_eq!(machine.vdp().memory().cram()[0], 0);
        assert!(snapshot(&machine).is_err());
        assert_eq!(before.colors(), &[0; 256]);
    }
}

#[test]
fn cpu_palette_publication() {
    cpu_palette_publication_with(Emulator::step_instruction, |machine| {
        machine.vdp().prepare_line(REGION)
    });
}
#[test]
fn fifo_and_sound() {
    fifo_and_sound_with(Emulator::step_instruction, |machine| {
        machine.vdp().prepare_line(REGION)
    });
}
#[test]
fn reset_snapshot_domains() {
    reset_snapshot_domains_with(Emulator::reset_cpu, Reset::reset, |machine| {
        machine.vdp().prepare_line(REGION)
    });
}
