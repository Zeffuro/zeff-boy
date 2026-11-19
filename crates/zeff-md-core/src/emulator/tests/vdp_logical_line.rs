use super::*;
use crate::{VdpLineError, VdpRenderError, VdpScanline};

const REGION: Region = Region::NtscUs;

pub fn cpu_and_reset_with(
    mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>,
    mut prepare: impl FnMut(&Emulator, u16) -> Result<VdpScanline, VdpLineError>,
    mut restart: impl FnMut(&mut Emulator) -> Result<(), TimingFault<BusError>>,
    mut reset: impl FnMut(&mut Emulator),
) {
    for concurrent in [false, true] {
        for row in [0, 223] {
            let mut machine = super::vdp_line::seed(concurrent, 0x3080);
            let saved = machine.clone();
            let initial = prepare(&machine, row).unwrap();
            assert_eq!(initial.row(), row);
            assert_eq!(initial.at(), MasterTicks::new(850));
            assert_eq!(initial.colors(), &[0; 256]);
            assert_eq!(machine, saved);
            let pointer = machine.vdp().memory().vram().as_ptr();
            super::vdp_line::palette_command(&mut machine);
            machine.cpu.registers.d[0] = 0x0246;
            step(&mut machine).unwrap();
            let pending = machine.clone();
            assert_eq!(
                prepare(&machine, row),
                Err(VdpLineError::Render(VdpRenderError::PendingLineInputs))
            );
            assert_eq!(machine, pending);
            machine
                .bus
                .advance_machine(1010 - machine.bus.now.get())
                .unwrap();
            let line = prepare(&machine, row).unwrap();
            assert_eq!(line.colors(), &[0x0246; 256]);
            assert_eq!(line.row(), row);
            assert_eq!(line.at(), machine.bus.now);
            machine.bus.cartridge.initial.pc = 0xc0000e;
            let before = machine.clone();
            assert!(restart(&mut machine).is_err());
            assert_eq!(machine, before);
            assert_eq!(prepare(&machine, row), Ok(line.clone()));
            machine.bus.cartridge.initial.pc = 0x100;
            restart(&mut machine).unwrap();
            assert_eq!(prepare(&machine, row).unwrap().colors(), line.colors());
            reset(&mut machine);
            let powered = machine.clone();
            assert!(prepare(&machine, row).is_err());
            assert_eq!(machine, powered);
            assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
            assert_eq!(initial.colors(), &[0; 256]);
            assert_eq!(line.colors(), &[0x0246; 256]);
        }
    }
}

#[test]
fn cpu_and_reset() {
    cpu_and_reset_with(
        Emulator::step_instruction,
        |machine, row| machine.vdp().prepare_logical_line(row, REGION),
        Emulator::reset_cpu,
        Reset::reset,
    );
}
