use super::*;

#[test]
pub fn real_cpu_register_write_crosses_counter_edge() {
    real_cpu_register_write_crosses_counter_edge_with(Emulator::step_instruction);
}

pub fn real_cpu_register_write_crosses_counter_edge_with(
    mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>,
) {
    for concurrent in [false, true] {
        for (h40, edge) in [(false, 2660), (true, 2640)] {
            let mut machine = super::machine(&[0x3080, 0x4e71, 0x4e71]);
            machine.concurrent_sound = concurrent;
            machine.cpu.registers.a[0] = 0xc00004;
            machine.cpu.registers.d[0] = 0x8d2e;
            let mut vdp = Vdp::bootstrap_mode5();
            if h40 {
                vdp.write_control(0x8c81, Region::NtscUs).unwrap();
                vdp.advance_to(MasterTicks::new(20), Region::NtscUs)
                    .unwrap();
            }
            vdp.advance_to(MasterTicks::new(edge - 29), Region::NtscUs)
                .unwrap();
            machine.bus.vdp = vdp;
            machine.bus.now = MasterTicks::new(edge - 29);
            let mut copy = machine.clone();
            let outcome = step(&mut machine).unwrap();
            assert_eq!(step(&mut copy), Ok(outcome.clone()));
            assert_eq!(machine, copy);
            assert_eq!(outcome.elapsed.get(), 8 * MAIN_CPU_DIVISOR);
            assert_eq!(machine.bus.now, MasterTicks::new(edge + 27));
            assert_eq!(machine.vdp().registers().registers()[13], Some(0x2e));
            assert!(machine.vdp().interrupts().horizontal_pending());
            assert!(machine.vdp().registers().pending().is_none());
        }
    }
}
