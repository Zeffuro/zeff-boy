use super::*;
use crate::{ControlCommand, VdpLineError, VdpRenderError, VdpScanline, VdpVerticalTiming};

const REGION: Region = Region::NtscUs;
const START: u64 = 1501;
const LENGTH: u8 = 17;
const NEXT: u16 = 0x4e71;
const LINE_TICKS: u64 = 3420;
const IRQ_EDGE_TICKS: u64 = 2660;

fn tick(at: u64) -> MasterTicks {
    MasterTicks::new(at)
}

fn register(machine: &mut Emulator, index: u8, value: u8) {
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

fn fixture(concurrent: bool, copy: bool, address: u32, increment: u8) -> Emulator {
    let mut machine = super::vdp_line::seed(concurrent, NEXT);
    register(&mut machine, 15, 0);
    machine.bus.advance_machine(1).unwrap();
    machine.bus.vdp.write_control(0x4400, REGION).unwrap();
    machine.bus.vdp.write_control(0, REGION).unwrap();
    machine.bus.vdp.write_data(0x1234, REGION).unwrap();
    machine
        .bus
        .advance_machine(1331 - machine.bus.now.get())
        .unwrap();
    super::vdp_dma::configure(
        &mut machine,
        LENGTH,
        if copy { 0xc0 } else { 0x80 },
        increment,
    );
    register(&mut machine, 22, 4);
    machine
        .bus
        .advance_machine(START - machine.bus.now.get())
        .unwrap();
    machine
        .bus
        .vdp
        .resume_dma_from_known_phase(
            ControlCommand {
                address,
                code: if copy { 0x30 } else { 0x21 },
            },
            if copy { None } else { Some(0xabcd) },
            REGION,
        )
        .unwrap();
    machine
}

fn slots(machine: &Emulator, count: usize) -> Vec<MasterTicks> {
    let calendar = VdpVerticalTiming::at_counters_zero(MasterTicks::ZERO);
    let mut at = machine.vdp().now();
    (0..count)
        .map(|_| {
            at = calendar
                .next_memory_slot(at, REGION, machine.vdp().registers())
                .unwrap();
            at
        })
        .collect()
}

pub fn cpu_interval_with(mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>) {
    for concurrent in [false, true] {
        let mut machine = super::machine(&[0xc0c1, NEXT, NEXT]);
        machine.concurrent_sound = concurrent;
        machine.bus.vdp = Vdp::bootstrap_mode5();
        machine.bus.now = MasterTicks::ZERO;
        machine.bus.audio = crate::hardware::audio::Audio::default();
        super::vdp_dma::configure(&mut machine, LENGTH, 0x80, 0);
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
                    code: 0x21,
                },
                Some(0xabcd),
                REGION,
            )
            .unwrap();
        machine.cpu.registers.d[0] = 3;
        machine.cpu.registers.d[1] = 0xffff;
        let mut clone = machine.clone();
        let pointer = machine.vdp().memory().vram().as_ptr();
        let result = step(&mut machine).unwrap();
        assert_eq!(result.elapsed.get(), 70 * MAIN_CPU_DIVISOR);
        assert_eq!(step(&mut clone), Ok(result));
        assert_eq!(machine, clone);
        assert_eq!(machine.cpu.registers.d[0], 3 * 0xffff);
        assert_eq!(machine.vdp().registers().registers()[19], Some(0));
        assert_eq!(machine.vdp().memory().vram()[0x101], 0xab);
        assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
    }
}

pub fn capture_rollback_with(
    mut capture: impl FnMut(&mut Vdp, MasterTicks, Region) -> Result<VdpScanline, VdpLineError>,
) {
    for concurrent in [false, true] {
        for address in [0x400, 0x2000] {
            for increment in [0, 1] {
                let mut machine = fixture(concurrent, false, address, increment);
                let opportunities = slots(&machine, LENGTH as usize);
                let before = machine.clone();
                let pointer = machine.vdp().memory().vram().as_ptr();
                assert_eq!(
                    capture(&mut machine.bus.vdp, opportunities[8], REGION),
                    Err(VdpLineError::Render(VdpRenderError::PendingLineInputs))
                );
                assert_eq!(machine, before);
                assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
                let result = capture(&mut machine.bus.vdp, *opportunities.last().unwrap(), REGION);
                if address == 0x2000 {
                    assert_eq!(
                        result,
                        Err(VdpLineError::Render(VdpRenderError::UnknownSpriteCache(0)))
                    );
                    assert_eq!(machine, before);
                } else {
                    let line = result.unwrap();
                    assert_eq!(line.colors(), &[0; 256]);
                    assert_eq!(line.at(), *opportunities.last().unwrap());
                    assert_eq!(machine.vdp().registers().registers()[19], Some(0));
                }
                assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
            }
        }
    }
}

pub fn row_delivery_with(mut advance: impl FnMut(&mut Emulator, u64) -> Result<(), BusError>) {
    for concurrent in [false, true] {
        for copy in [false, true] {
            let fixture = fixture(concurrent, copy, if copy { 0x401 } else { 0x400 }, 1);
            let opportunities = slots(&fixture, LENGTH as usize * if copy { 2 } else { 1 });
            let end = opportunities.last().unwrap().get() + 1;
            for deadline in [
                opportunities[0],
                opportunities[8],
                opportunities[16],
                *opportunities.last().unwrap(),
                tick(end),
            ] {
                let mut machine = fixture.clone();
                let mut ordinary = fixture.clone();
                let mut split = fixture.clone();
                machine.queue_vdp_capture(deadline).unwrap();
                split.queue_vdp_capture(deadline).unwrap();
                advance(&mut machine, end - START).unwrap();
                advance(&mut ordinary, end - START).unwrap();
                for at in &opportunities {
                    let delta = at.get() - split.bus.now.get();
                    advance(&mut split, delta).unwrap();
                }
                advance(&mut split, 1).unwrap();
                let captured = machine.take_vdp_capture().unwrap();
                if deadline < *opportunities.last().unwrap() {
                    assert_eq!(
                        captured,
                        Err(VdpLineError::Render(VdpRenderError::PendingLineInputs))
                    );
                } else {
                    let line = captured.as_ref().unwrap();
                    assert_eq!(line.colors(), &[0; 256]);
                    assert_eq!(line.at(), deadline);
                }
                assert_eq!(split.take_vdp_capture(), Some(captured));
                assert_eq!(machine, split);
                assert_eq!(machine, ordinary);
                if copy {
                    assert_eq!(&machine.vdp().memory().vram()[0x400..0x412], &[0x34; 18]);
                }
            }
        }
    }
}

pub fn capture_iack_endpoint_with(
    mut advance: impl FnMut(
        &mut Vdp,
        MasterTicks,
        MasterTicks,
        bool,
        Region,
    ) -> Result<Result<VdpScanline, VdpLineError>, crate::VdpScheduleError>,
) {
    for copy in [false, true] {
        let fixture = fixture(false, copy, 0x100, 1);
        let end = slots(&fixture, LENGTH as usize * if copy { 2 } else { 1 })
            .last()
            .unwrap()
            .get();
        for deadline in [tick(end + 1), tick(end + 2)] {
            let mut device = fixture.bus.vdp.clone();
            let mut ordinary = device.clone();
            ordinary.acknowledge_to(deadline, REGION).unwrap();
            let line = advance(&mut device, deadline, deadline, true, REGION)
                .unwrap()
                .unwrap();
            assert_eq!(line.at(), deadline);
            assert_eq!(device, ordinary);
        }
        let mut device = fixture.bus.vdp;
        let before = device.clone();
        let at = tick((end / LINE_TICKS + 1) * LINE_TICKS + IRQ_EDGE_TICKS);
        assert_eq!(
            advance(&mut device, at, tick(1650), true, REGION),
            Err(crate::VdpScheduleError::CoincidentAcknowledge(at))
        );
        assert_eq!(device, before);
    }
}

pub fn retained_restart_with(
    mut advance: impl FnMut(&mut Emulator, u64) -> Result<(), BusError>,
    mut restart: impl FnMut(&mut Emulator) -> Result<(), TimingFault<BusError>>,
    mut reset: impl FnMut(&mut Emulator),
) {
    for concurrent in [false, true] {
        let mut machine = fixture(concurrent, true, 0x401, 1);
        let opportunities = slots(&machine, LENGTH as usize * 2);
        advance(&mut machine, opportunities[18].get() - START).unwrap();
        assert_eq!(machine.vdp().registers().registers()[19], Some(LENGTH - 9));
        let pointer = machine.vdp().memory().vram().as_ptr();
        machine.bus.cartridge.initial.pc = 0xc00000;
        let before = machine.clone();
        assert!(restart(&mut machine).is_err());
        assert_eq!(machine, before);
        assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
        machine.bus.cartridge.initial.pc = 0x100;
        restart(&mut machine).unwrap();
        let mut clone = machine.clone();
        let delta = opportunities.last().unwrap().get() - machine.bus.now.get();
        advance(&mut machine, delta).unwrap();
        advance(&mut clone, delta).unwrap();
        assert_eq!(machine, clone);
        assert_eq!(&machine.vdp().memory().vram()[0x400..0x412], &[0x34; 18]);
        assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
        reset(&mut machine);
        assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
        assert_eq!(machine.vdp().memory().vram()[0x401], 0);
        assert_eq!(machine.vdp().registers().registers()[19], Some(0));
        assert!(machine.bus.vdp.write_control(0x4000, REGION).is_ok());
    }
}

#[test]
fn cpu_interval() {
    cpu_interval_with(Emulator::step_instruction);
}
#[test]
fn capture_rollback() {
    capture_rollback_with(Vdp::capture_line_at);
}
#[test]
fn row_delivery() {
    row_delivery_with(super::vdp_row_delivery::advance);
}
#[test]
fn capture_iack_endpoint() {
    capture_iack_endpoint_with(|device, at, capture, acknowledge, region| {
        device.advance_with_line_from(at, capture, acknowledge, region, None)
    });
}
#[test]
fn retained_restart() {
    retained_restart_with(
        super::vdp_row_delivery::advance,
        Emulator::reset_cpu,
        Reset::reset,
    );
}
