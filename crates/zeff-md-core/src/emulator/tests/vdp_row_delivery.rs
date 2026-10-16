use super::*;
use crate::{VdpCaptureError, VdpLineError, VdpRenderError, VdpScanline};

const COLOR: u16 = 0x0246;
const REGION: Region = Region::NtscUs;

pub(super) fn fixture(concurrent: bool, opcode: u16) -> Emulator {
    let mut machine = super::vdp_line::seed(concurrent, opcode);
    super::vdp_line::palette_command(&mut machine);
    machine.cpu.registers.d[0] = COLOR.into();
    machine
}

pub(super) fn advance(machine: &mut Emulator, ticks: u64) -> Result<(), BusError> {
    if machine.concurrent_sound {
        ScheduledBus {
            bus: &mut machine.bus,
            cpu: &mut machine.sound_cpu,
            schedule: &mut machine.sound_schedule,
        }
        .advance_machine(ticks)
    } else {
        machine.bus.advance_machine(ticks)
    }
}

pub fn cpu_ordering_with(
    mut queue: impl FnMut(&mut Emulator, MasterTicks) -> Result<(), VdpCaptureError>,
    mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>,
    mut advance: impl FnMut(&mut Emulator, u64) -> Result<(), BusError>,
    mut take: impl FnMut(&mut Emulator) -> Option<Result<VdpScanline, VdpLineError>>,
) {
    for concurrent in [false, true] {
        for deadline in [878, 879, 1010] {
            let mut machine = fixture(concurrent, 0x3080);
            let mut ordinary = machine.clone();
            queue(&mut machine, MasterTicks::new(deadline)).unwrap();
            let mut replay = machine.clone();
            let result = step(&mut machine).unwrap();
            assert_eq!(step(&mut replay), Ok(result.clone()));
            assert_eq!(step(&mut ordinary), Ok(result.clone()));
            assert_eq!(machine, replay);
            assert_eq!(result.elapsed.get(), 56);
            let captured = if deadline > machine.bus.now.get() {
                assert_eq!(take(&mut machine), None);
                let ticks = deadline - machine.bus.now.get();
                advance(&mut machine, ticks).unwrap();
                advance(&mut ordinary, ticks).unwrap();
                take(&mut machine).unwrap()
            } else {
                take(&mut machine).unwrap()
            };
            assert_eq!(machine, ordinary);
            match deadline {
                878 | 1010 => {
                    let line = captured.unwrap();
                    assert_eq!(line.at(), MasterTicks::new(deadline));
                    assert_eq!(line.row(), 0);
                    let color = if deadline == 878 { 0 } else { COLOR };
                    assert_eq!(line.colors(), &[color; 256]);
                    let ticks = 1131 - machine.bus.now.get();
                    advance(&mut machine, ticks).unwrap();
                    assert_eq!(machine.vdp().memory().cram()[0], COLOR);
                    assert_eq!(line.colors(), &[color; 256]);
                }
                879 => assert_eq!(
                    captured,
                    Err(VdpLineError::Render(VdpRenderError::PendingLineInputs))
                ),
                _ => unreachable!(),
            }
        }
    }
}

pub fn request_ownership_with(
    mut queue: impl FnMut(&mut Emulator, MasterTicks) -> Result<(), VdpCaptureError>,
    mut advance: impl FnMut(&mut Emulator, u64) -> Result<(), BusError>,
    mut take: impl FnMut(&mut Emulator) -> Option<Result<VdpScanline, VdpLineError>>,
) {
    for concurrent in [false, true] {
        let mut machine = fixture(concurrent, 0x3080);
        let empty = machine.clone();
        assert_eq!(take(&mut machine), None);
        for at in [849, 850] {
            assert_eq!(
                queue(&mut machine, MasterTicks::new(at)),
                Err(VdpCaptureError::DeadlineNotFuture {
                    current: MasterTicks::new(850),
                    requested: MasterTicks::new(at),
                })
            );
            assert_eq!(machine, empty);
        }
        queue(&mut machine, MasterTicks::new(851)).unwrap();
        let pending = machine.clone();
        assert_eq!(
            queue(&mut machine, MasterTicks::new(852)),
            Err(VdpCaptureError::Busy)
        );
        assert_eq!(take(&mut machine), None);
        assert_eq!(machine, pending);
        advance(&mut machine, 1).unwrap();
        let completed = machine.clone();
        assert_eq!(
            queue(&mut machine, MasterTicks::new(852)),
            Err(VdpCaptureError::Busy)
        );
        assert_eq!(machine, completed);
        let mut copy = completed.clone();
        let retained = take(&mut copy).unwrap().unwrap();
        assert_eq!(machine, completed);
        assert_eq!(take(&mut machine), Some(Ok(retained.clone())));
        assert_eq!(machine, copy);
        assert_eq!(take(&mut machine), None);
        queue(&mut machine, MasterTicks::new(853)).unwrap();
        let mut partitioned = machine.clone();
        advance(&mut machine, 2).unwrap();
        advance(&mut partitioned, 1).unwrap();
        assert_eq!(take(&mut partitioned), None);
        advance(&mut partitioned, 1).unwrap();
        assert_eq!(take(&mut machine), take(&mut partitioned));
        assert_eq!(machine, partitioned);
        assert_eq!(retained.at(), MasterTicks::new(851));
        assert_eq!(retained.colors(), &[0; 256]);
    }
}

pub fn publication_with(
    mut queue: impl FnMut(&mut Emulator, MasterTicks) -> Result<(), VdpCaptureError>,
    mut advance: impl FnMut(&mut Emulator, u64) -> Result<(), BusError>,
    mut take: impl FnMut(&mut Emulator) -> Option<Result<VdpScanline, VdpLineError>>,
) {
    for concurrent in [false, true] {
        let mut seed = fixture(concurrent, 0x3080);
        advance(&mut seed, 1).unwrap();
        seed.bus.vdp.write_data(COLOR, REGION).unwrap();
        advance(&mut seed, 160).unwrap();
        seed.bus.vdp.write_data(0x0eee, REGION).unwrap();
        advance(&mut seed, 320).unwrap();
        let write = seed.queue_vdp_register(7, 1).unwrap();
        assert_eq!(write.publish_at, MasterTicks::new(1340));
        for at in [1339, 1340, 1341] {
            let mut machine = seed.clone();
            let mut ordinary = machine.clone();
            queue(&mut machine, MasterTicks::new(at)).unwrap();
            let mut partitioned = machine.clone();
            let ticks = 1351 - machine.bus.now.get();
            advance(&mut machine, ticks).unwrap();
            advance(&mut ordinary, ticks).unwrap();
            for _ in 0..ticks {
                advance(&mut partitioned, 1).unwrap();
            }
            let line = take(&mut machine).unwrap().unwrap();
            assert_eq!(line.at(), MasterTicks::new(at));
            assert_eq!(
                line.colors(),
                &[if at < 1340 { COLOR } else { 0x0eee }; 256]
            );
            assert_eq!(take(&mut partitioned), Some(Ok(line)));
            assert_eq!(machine, partitioned);
            assert_eq!(machine, ordinary);
        }
    }
}

pub fn wait_and_stop_with(
    mut queue: impl FnMut(&mut Emulator, MasterTicks) -> Result<(), VdpCaptureError>,
    mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>,
    mut advance: impl FnMut(&mut Emulator, u64) -> Result<(), BusError>,
    mut take: impl FnMut(&mut Emulator) -> Option<Result<VdpScanline, VdpLineError>>,
) {
    for live_sound in [false, true] {
        let mut machine = fixture(true, 0x2080);
        advance(&mut machine, 1).unwrap();
        for color in [COLOR, 0x0240, 0x000e, 0x0aaa] {
            machine.bus.vdp.write_data(color, REGION).unwrap();
        }
        if live_sound {
            super::vdp_data::sound_hold(&mut machine, 870);
        }
        let mut ordinary = machine.clone();
        queue(&mut machine, MasterTicks::new(1010)).unwrap();
        let result = step(&mut machine).unwrap();
        assert_eq!(step(&mut ordinary), Ok(result.clone()));
        assert_eq!(result.elapsed.get(), 73 * MAIN_CPU_DIVISOR);
        assert_eq!(
            take(&mut machine),
            Some(Err(VdpLineError::Render(VdpRenderError::PendingLineInputs)))
        );
        assert_eq!(machine, ordinary);
        queue(&mut machine, MasterTicks::new(2130)).unwrap();
        let ticks = 2131 - machine.bus.now.get();
        advance(&mut machine, ticks).unwrap();
        advance(&mut ordinary, ticks).unwrap();
        let row = take(&mut machine).unwrap().unwrap();
        assert_eq!(row.at(), MasterTicks::new(2130));
        assert_eq!(row.colors(), &[COLOR; 256]);
        assert_eq!(machine, ordinary);
        if live_sound {
            assert_eq!(machine.peek(0xff8000), Some(0x5a));
            assert!(machine.sound_bus_granted());
        }
    }
    for concurrent in [false, true] {
        let mut machine =
            super::vdp_stable_display::fixture(112, &[0x2080, 0x4e72, 0x2700, 0x4e71], concurrent);
        machine.cpu.registers.d[0] = 0;
        step(&mut machine).unwrap();
        let ticks = 850 - machine.bus.now.get();
        advance(&mut machine, ticks).unwrap();
        step(&mut machine).unwrap();
        let deadline = MasterTicks::new(machine.bus.now.get() + 1);
        let mut ordinary = machine.clone();
        queue(&mut machine, deadline).unwrap();
        let result = step(&mut machine).unwrap();
        assert_eq!(result.cpu, TimedOutcome::Stopped);
        assert_eq!(step(&mut ordinary), Ok(result));
        let row = take(&mut machine).unwrap().unwrap();
        assert_eq!(row.at(), deadline);
        assert_eq!(row.colors(), &[0; 256]);
        assert_eq!(machine, ordinary);
    }
}

pub fn staged_writes_with(
    mut queue: impl FnMut(&mut Emulator, MasterTicks) -> Result<(), VdpCaptureError>,
    mut advance: impl FnMut(&mut Emulator, u64) -> Result<(), BusError>,
    mut take: impl FnMut(&mut Emulator) -> Option<Result<VdpScanline, VdpLineError>>,
) {
    for concurrent in [false, true] {
        let mut machine = fixture(concurrent, 0x3080);
        advance(&mut machine, 1).unwrap();
        let register = machine.queue_vdp_register(15, 0).unwrap();
        advance(&mut machine, register.publish_at.get() + 1 - 851).unwrap();
        for (address, code, data) in [
            (0x2000, 1, 0x1234),
            (0x2001, 1, 0x5678),
            (0x2000, 0x11, 0x9abc),
            (0x2001, 0x11, 0),
        ] {
            machine
                .bus
                .vdp
                .write_control((address & 0x3fff) | ((code & 3) << 14), REGION)
                .unwrap();
            machine
                .bus
                .vdp
                .write_control((code >> 2) << 4, REGION)
                .unwrap();
            machine.bus.vdp.write_data(data, REGION).unwrap();
        }
        machine.bus.vdp.write_control(0x2000, REGION).unwrap();
        machine.bus.vdp.write_control(0, REGION).unwrap();
        let ready = machine.vdp().data_read_wait(REGION).unwrap().unwrap();
        assert_eq!(ready, MasterTicks::new(2890));
        let mut ordinary = machine.clone();
        queue(&mut machine, MasterTicks::new(1330)).unwrap();
        let mut partitioned = machine.clone();
        let pointer = machine.vdp().memory().vram().as_ptr();
        let begin = machine.bus.now.get();
        advance(&mut machine, 2891 - begin).unwrap();
        advance(&mut ordinary, 2891 - begin).unwrap();
        advance(&mut partitioned, 1010 - begin).unwrap();
        assert_eq!(take(&mut partitioned), None);
        advance(&mut partitioned, 320).unwrap();
        advance(&mut partitioned, 1561).unwrap();
        assert_eq!(machine, partitioned);
        let error = Some(Err(VdpLineError::Render(VdpRenderError::PendingLineInputs)));
        assert_eq!(take(&mut machine), error);
        assert_eq!(take(&mut partitioned), error);
        assert_eq!(machine, ordinary);
        assert_eq!(machine, partitioned);
        assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
        assert_eq!(&machine.vdp().memory().vram()[0x2000..0x2002], &[0, 0]);
        assert_eq!(machine.vdp().read_cache().buffered_word(), Some(0));
        assert!(machine.vdp().read_cache().is_ready());
        queue(&mut machine, MasterTicks::new(2892)).unwrap();
        advance(&mut machine, 1).unwrap();
        assert_eq!(
            take(&mut machine),
            Some(Err(VdpLineError::Render(
                VdpRenderError::UnknownSpriteCache(0)
            )))
        );
    }
}

#[test]
fn cpu_ordering() {
    cpu_ordering_with(
        Emulator::queue_vdp_capture,
        Emulator::step_instruction,
        advance,
        Emulator::take_vdp_capture,
    );
}

#[test]
fn request_ownership() {
    request_ownership_with(
        Emulator::queue_vdp_capture,
        advance,
        Emulator::take_vdp_capture,
    );
}

#[test]
fn publication() {
    publication_with(
        Emulator::queue_vdp_capture,
        advance,
        Emulator::take_vdp_capture,
    );
}

#[test]
fn wait_and_stop() {
    wait_and_stop_with(
        Emulator::queue_vdp_capture,
        Emulator::step_instruction,
        advance,
        Emulator::take_vdp_capture,
    );
}

#[test]
fn staged_writes() {
    staged_writes_with(
        Emulator::queue_vdp_capture,
        advance,
        Emulator::take_vdp_capture,
    );
}
