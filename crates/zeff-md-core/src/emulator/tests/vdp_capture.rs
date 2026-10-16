use super::*;
use crate::{VdpLineError, VdpRenderError, VdpScanline};

const REGION: Region = Region::NtscUs;

pub fn cpu_capture_with(
    mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>,
    mut capture: impl FnMut(&mut Emulator) -> Result<VdpScanline, VdpLineError>,
) {
    for concurrent in [false, true] {
        let mut machine = super::vdp_line::seed(concurrent, 0x3080);
        let before = machine.clone();
        let initial = capture(&mut machine).unwrap();
        assert_eq!(machine, before);
        super::vdp_line::palette_command(&mut machine);
        machine.cpu.registers.d[0] = 0x0246;
        step(&mut machine).unwrap();
        let pending = machine.clone();
        let pointer = machine.vdp().memory().vram().as_ptr();
        assert_eq!(
            capture(&mut machine),
            Err(VdpLineError::Render(VdpRenderError::PendingLineInputs))
        );
        assert_eq!(machine, pending);
        assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
        machine
            .bus
            .advance_machine(1010 - machine.bus.now.get())
            .unwrap();
        let settled = machine.clone();
        let line = capture(&mut machine).unwrap();
        assert_eq!(line.at(), MasterTicks::new(1010));
        assert_eq!(line.colors(), &[0x0246; 256]);
        assert_eq!(machine, settled);
        assert_eq!(line, machine.vdp().prepare_line(REGION).unwrap());
        assert_eq!(initial.colors(), &[0; 256]);
    }
}

pub fn reset_capture_with(
    mut restart: impl FnMut(&mut Emulator) -> Result<(), TimingFault<BusError>>,
    mut reset: impl FnMut(&mut Emulator),
    mut capture: impl FnMut(&mut Emulator) -> Result<VdpScanline, VdpLineError>,
) {
    for concurrent in [false, true] {
        let mut machine = super::vdp_line::seed(concurrent, 0x4e71);
        let initial = capture(&mut machine).unwrap();
        machine.bus.advance_machine(139).unwrap();
        super::vdp_line::palette_command(&mut machine);
        machine.bus.vdp.write_data(0x0246, REGION).unwrap();
        machine.bus.cartridge.initial.pc = 0xc0000e;
        let saved = machine.clone();
        let pointer = machine.vdp().memory().vram().as_ptr();
        assert!(restart(&mut machine).is_err());
        assert_eq!(machine, saved);
        assert!(capture(&mut machine).is_err());
        assert_eq!(machine, saved);
        machine.bus.cartridge.initial.pc = 0x100;
        restart(&mut machine).unwrap();
        let retained = capture(&mut machine).unwrap();
        assert_eq!(retained.colors(), &[0x0246; 256]);
        reset(&mut machine);
        let powered = machine.clone();
        assert!(capture(&mut machine).is_err());
        assert_eq!(machine, powered);
        assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
        assert_eq!(initial.colors(), &[0; 256]);
        assert_eq!(retained.colors(), &[0x0246; 256]);
    }
}

fn capture(machine: &mut Emulator) -> Result<VdpScanline, VdpLineError> {
    machine.bus.vdp.capture_line_at(machine.bus.now, REGION)
}

#[test]
fn cpu_capture() {
    cpu_capture_with(Emulator::step_instruction, capture);
}

#[test]
fn reset_capture() {
    reset_capture_with(Emulator::reset_cpu, Reset::reset, capture);
}
