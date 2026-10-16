use super::*;

const REGION: Region = Region::NtscUs;
const NEXT: u16 = 0x4e71;

fn fixture(copy: bool, concurrent: bool) -> Emulator {
    let program: &[u16] = if copy {
        &[0x2080, NEXT, NEXT]
    } else {
        &[0x2080, 0x3281, NEXT, NEXT]
    };
    let mut machine = super::machine(program);
    machine.concurrent_sound = concurrent;
    machine.bus.vdp = Vdp::bootstrap_mode5();
    machine.bus.now = MasterTicks::ZERO;
    machine.bus.audio = crate::hardware::audio::Audio::default();
    super::vdp_dma::configure(
        &mut machine,
        if copy { 2 } else { 3 },
        if copy { 0xc0 } else { 0x80 },
        0,
    );
    machine.bus.vdp.write_control(0x4000, REGION).unwrap();
    machine.bus.vdp.write_control(0, REGION).unwrap();
    machine.bus.vdp.write_data(0x1234, REGION).unwrap();
    machine
        .bus
        .advance_machine(201 - machine.bus.now.get())
        .unwrap();
    for (index, value) in [(15, 1), (1, 0x14)] {
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
        .advance_machine(1001 - machine.bus.now.get())
        .unwrap();
    machine.cpu.registers.a[0] = 0xc00004;
    machine.cpu.registers.a[1] = 0xc00000;
    machine.cpu.registers.d[0] = if copy { 0x010000c0 } else { 0x41000080 };
    machine.cpu.registers.d[1] = 0xabcd;
    machine
}

pub fn command_and_trigger_with(
    mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>,
    mut advance: impl FnMut(&mut Emulator, u64) -> Result<(), BusError>,
) {
    for concurrent in [false, true] {
        for copy in [false, true] {
            let mut machine = fixture(copy, concurrent);
            let mut clone = machine.clone();
            let pointer = machine.vdp().memory().vram().as_ptr();
            let result = step(&mut machine).unwrap();
            assert_eq!(step(&mut clone), Ok(result));
            assert_eq!(machine, clone);
            if !copy {
                assert_eq!(machine.vdp().registers().registers()[19], Some(3));
                assert_eq!(&machine.vdp().memory().vram()[0x100..0x104], &[0; 4]);
                assert_eq!(machine.bus.vdp.read_status(REGION).unwrap() & 2, 2);
                let result = step(&mut machine).unwrap();
                assert_eq!(step(&mut clone), Ok(result));
            }
            advance(&mut machine, 1000).unwrap();
            advance(&mut clone, 1000).unwrap();
            assert_eq!(machine, clone);
            assert_eq!(
                &machine.vdp().memory().vram()[0x100..0x104],
                if copy {
                    &[0x12, 0x34, 0, 0]
                } else {
                    &[0xab, 0xcd, 0xab, 0xab]
                }
            );
            assert_eq!(machine.vdp().registers().registers()[19], Some(0));
            assert_eq!(
                machine.vdp().registers().registers()[21],
                Some(if copy { 2 } else { 3 })
            );
            assert_eq!(
                machine.vdp().command().address,
                if copy { 0x102 } else { 0x104 }
            );
            assert_eq!(machine.vdp().command().code, if copy { 0x10 } else { 1 });
            assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
        }
    }
}

pub fn waiting_reset_with(
    mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>,
    mut restart: impl FnMut(&mut Emulator) -> Result<(), TimingFault<BusError>>,
    mut reset: impl FnMut(&mut Emulator),
) {
    for concurrent in [false, true] {
        let mut machine = fixture(false, concurrent);
        step(&mut machine).unwrap();
        let pointer = machine.vdp().memory().vram().as_ptr();
        machine.bus.cartridge.initial.pc = 0xc00000;
        let before = machine.clone();
        assert!(restart(&mut machine).is_err());
        assert_eq!(machine, before);
        assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
        machine.bus.cartridge.initial.pc = 0x100;
        restart(&mut machine).unwrap();
        assert_eq!(machine.bus.vdp.read_status(REGION).unwrap() & 2, 2);
        reset(&mut machine);
        assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
        assert_eq!(machine.bus.vdp.read_status(REGION).unwrap() & 2, 0);
        assert_eq!(machine.vdp().registers().registers()[19], Some(0));
    }
}

#[test]
fn command_and_trigger() {
    command_and_trigger_with(Emulator::step_instruction, super::vdp_row_delivery::advance);
}
#[test]
fn waiting_reset() {
    waiting_reset_with(
        Emulator::step_instruction,
        Emulator::reset_cpu,
        Reset::reset,
    );
}
