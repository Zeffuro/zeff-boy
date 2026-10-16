use super::*;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, FunctionCode};
use zeff_m68k::timing::CycleResult;

const REGION: Region = Region::NtscUs;
const HV: u32 = 0xc00008;
const NEXT: u16 = 0x4e71;

fn access(address: u32, lanes: ByteLanes) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & !1,
        function_code: FunctionCode::SupervisorData,
        kind: AccessKind::Data,
        lanes,
    }
}

fn fixture(concurrent: bool, opcode: u16, now: u64) -> Emulator {
    let mut machine = super::machine(&[opcode, NEXT, NEXT]);
    machine.concurrent_sound = concurrent;
    machine.cpu.registers.a[0] = HV;
    machine
        .bus
        .advance_machine(now - machine.bus.now.get())
        .unwrap();
    machine.bus.vdp.write_control(0x4567, REGION).unwrap();
    machine
}

pub fn cpu_lanes_and_mirrors_with(
    mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>,
) {
    for concurrent in [false, true] {
        for address in [HV, HV + 2, HV + 4, HV + 6, 0xd8ff08, 0xd8ff0e] {
            for (opcode, lane, expected) in [(0x3010, 0, 0x01e9), (0x1010, 0, 1), (0x1010, 1, 0xe9)]
            {
                let mut machine = fixture(concurrent, opcode, 2932);
                machine.cpu.registers.a[0] = address + lane;
                let mut clone = machine.clone();
                let command = machine.vdp().command();
                let result = step(&mut machine).unwrap();
                assert_eq!(step(&mut clone), Ok(result.clone()));
                assert_eq!(machine, clone);
                assert_eq!(machine.registers().d[0], expected);
                assert_eq!(result.elapsed.get(), 8 * MAIN_CPU_DIVISOR);
                assert_eq!(machine.vdp().control().pending_word(), Some(0x4567));
                assert_eq!(machine.vdp().command(), command);
            }
        }
    }
}

pub fn transfer_samples_and_longword_with(
    mut read: impl FnMut(&mut Emulator, BusAccess) -> CycleResult<u16, BusError>,
    mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>,
) {
    for concurrent in [false, true] {
        for base in [HV, 0xd8ff08] {
            for offset in 0..8 {
                let lanes = if offset & 1 == 0 {
                    ByteLanes::Upper
                } else {
                    ByteLanes::Lower
                };
                let mut machine = fixture(concurrent, NEXT, 2922);
                machine.bus.prefetch_upper = None;
                let command = machine.vdp().command();
                let first = read(&mut machine, access(base + offset, lanes));
                assert_eq!(first.result, Ok(0x0193));
                assert_eq!(first.wait_clocks, 0);
                assert_eq!(machine.bus.now, MasterTicks::new(2950));
                let second = read(&mut machine, access(base + offset, ByteLanes::Both));
                assert_eq!(second.result, Ok(0x01e9));
                assert_eq!(second.wait_clocks, 0);
                assert_eq!(machine.bus.now, MasterTicks::new(2978));
                assert_eq!(machine.vdp().control().pending_word(), Some(0x4567));
                assert_eq!(machine.vdp().command(), command);
            }
        }
        for (now, expected) in [(2922, 0x019301e9), (2631, 0x00840186)] {
            let mut machine = fixture(concurrent, 0x2010, now);
            let mut clone = machine.clone();
            let result = step(&mut machine).unwrap();
            assert_eq!(step(&mut clone), Ok(result));
            assert_eq!(machine, clone);
            assert_eq!(machine.registers().d[0], expected);
            assert_eq!(machine.vdp().control().pending_word(), Some(0x4567));
        }
    }
}

pub fn reset_latch_with(
    mut restart: impl FnMut(&mut Emulator) -> Result<(), TimingFault<BusError>>,
    mut power_on: impl FnMut(&mut Emulator),
) {
    for concurrent in [false, true] {
        let mut machine = fixture(concurrent, NEXT, 101);
        machine.bus.vdp.read_status(REGION).unwrap();
        let write = machine.bus.vdp.queue_register(0, 2, REGION).unwrap();
        machine
            .bus
            .advance_machine(write.publish_at.get() - machine.bus.now.get())
            .unwrap();
        assert_eq!(machine.vdp().read_hv(REGION), Ok(6));
        machine
            .bus
            .advance_machine(3000 - machine.bus.now.get())
            .unwrap();
        let pointer = machine.vdp().memory().vram().as_ptr();
        restart(&mut machine).unwrap();
        assert_eq!(machine.vdp().read_hv(REGION), Ok(6));
        assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
        power_on(&mut machine);
        assert_eq!(machine.vdp().registers().registers()[0], Some(0));
        assert_eq!(machine.vdp().read_hv(REGION), Ok(2));
        assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
        assert_eq!(machine.bus.now, MasterTicks::new(56));
    }
}

#[test]
fn cpu_lanes_and_mirrors() {
    cpu_lanes_and_mirrors_with(Emulator::step_instruction);
}

#[test]
fn transfer_samples_and_longword() {
    transfer_samples_and_longword_with(
        super::vdp_status_port::transfer,
        Emulator::step_instruction,
    );
}

#[test]
fn reset_latch() {
    reset_latch_with(Emulator::reset_cpu, Reset::reset);
}
