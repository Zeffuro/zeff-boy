use super::*;
use crate::{VdpCaptureError, VdpLineError, VdpRenderError, VdpScanline};
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, FunctionCode};
use zeff_m68k::timing::ClockedBus;

const REGION: Region = Region::NtscUs;
const NEXT: u16 = 0x4e71;

fn tick(at: u64) -> MasterTicks {
    MasterTicks::new(at)
}

pub fn reset_delivery_with(
    mut restart: impl FnMut(&mut Emulator) -> Result<(), TimingFault<BusError>>,
    mut power_on: impl FnMut(&mut Emulator),
    mut advance: impl FnMut(&mut Emulator, u64) -> Result<(), BusError>,
    mut take: impl FnMut(&mut Emulator) -> Option<Result<VdpScanline, VdpLineError>>,
) {
    for concurrent in [false, true] {
        for offset in [10, 100] {
            let mut machine = super::vdp_row_delivery::fixture(concurrent, NEXT);
            let initial = machine.vdp().prepare_line(REGION).unwrap();
            let pointer = machine.vdp().memory().vram().as_ptr();
            let deadline = machine.bus.now.get() + offset;
            machine.queue_vdp_capture(tick(deadline)).unwrap();
            restart(&mut machine).unwrap();
            assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
            if offset > PRIME_CLOCKS * MAIN_CPU_DIVISOR {
                assert_eq!(take(&mut machine), None);
                assert_eq!(
                    machine.queue_vdp_capture(tick(deadline + 1)),
                    Err(VdpCaptureError::Busy)
                );
                let remaining = deadline - machine.bus.now.get();
                advance(&mut machine, remaining).unwrap();
            }
            let retained = take(&mut machine).unwrap().unwrap();
            assert_eq!(retained.at(), tick(deadline));
            assert_eq!(retained.colors(), &[0; 256]);
            machine.queue_vdp_capture(tick(deadline + 100)).unwrap();
            power_on(&mut machine);
            assert_eq!(take(&mut machine), None);
            assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
            assert_eq!(initial.colors(), &[0; 256]);
            assert_eq!(retained.colors(), &[0; 256]);
        }

        for completed in [false, true] {
            let mut machine = super::vdp_row_delivery::fixture(concurrent, NEXT);
            let pointer = machine.vdp().memory().vram().as_ptr();
            machine.queue_vdp_capture(tick(860)).unwrap();
            if completed {
                advance(&mut machine, 10).unwrap();
            }
            machine.bus.cartridge.initial.pc = 0xc0000e;
            let saved = machine.clone();
            assert!(restart(&mut machine).is_err());
            assert_eq!(machine, saved);
            assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
            assert_eq!(
                machine.queue_vdp_capture(tick(machine.bus.now.get() + 100)),
                Err(VdpCaptureError::Busy)
            );
            if !completed {
                assert_eq!(take(&mut machine), None);
                advance(&mut machine, 10).unwrap();
            }
            let row = take(&mut machine).unwrap().unwrap();
            assert_eq!(row.at(), tick(860));
            assert_eq!(row.colors(), &[0; 256]);
            machine.bus.cartridge.initial.pc = 0x100;
            machine
                .queue_vdp_capture(tick(machine.bus.now.get() + 1))
                .unwrap();
            advance(&mut machine, 1).unwrap();
            restart(&mut machine).unwrap();
            assert!(take(&mut machine).unwrap().is_ok());
            machine
                .queue_vdp_capture(tick(machine.bus.now.get() + 1))
                .unwrap();
            advance(&mut machine, 1).unwrap();
            power_on(&mut machine);
            assert_eq!(take(&mut machine), None);
            assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
            assert_eq!(row.colors(), &[0; 256]);
            machine
                .queue_vdp_capture(tick(machine.bus.now.get() + 1))
                .unwrap();
        }
    }
}

pub fn delivery_faults_with(
    mut advance: impl FnMut(&mut Emulator, u64) -> Result<(), BusError>,
    mut take: impl FnMut(&mut Emulator) -> Option<Result<VdpScanline, VdpLineError>>,
) {
    for concurrent in [false, true] {
        let mut whole = super::vdp_row_delivery::fixture(concurrent, NEXT);
        whole.bus.vdp.write_control(0x2000, REGION).unwrap();
        whole.bus.vdp.write_control(0, REGION).unwrap();
        let ready = whole.vdp().data_read_wait(REGION).unwrap().unwrap();
        assert!(ready > tick(851));
        whole.queue_vdp_capture(tick(851)).unwrap();
        let mut partitioned = whole.clone();
        let pointer = whole.vdp().memory().vram().as_ptr();
        advance(&mut whole, ready.get() + 1 - 850).unwrap();
        advance(&mut partitioned, 1).unwrap();
        assert_eq!(
            partitioned.queue_vdp_capture(tick(ready.get() + 1)),
            Err(VdpCaptureError::Busy)
        );
        advance(&mut partitioned, ready.get() - 851).unwrap();
        advance(&mut partitioned, 1).unwrap();
        assert_eq!(whole, partitioned);
        assert_eq!(whole.bus.now, tick(ready.get() + 1));
        assert!(whole.vdp().read_cache().is_ready());
        assert_eq!(whole.vdp().memory().vram().as_ptr(), pointer);
        let expected = Some(Err(VdpLineError::Render(VdpRenderError::PendingLineInputs)));
        assert_eq!(take(&mut whole), expected);
        assert_eq!(take(&mut partitioned), expected);
        assert_eq!(whole, partitioned);
        whole.queue_vdp_capture(tick(ready.get() + 2)).unwrap();
        advance(&mut whole, 1).unwrap();
        let row = take(&mut whole).unwrap().unwrap();
        assert_eq!(row.at(), tick(ready.get() + 2));
        assert_eq!(row.colors(), &[0; 256]);

        let mut machine = super::vdp_row_delivery::fixture(concurrent, NEXT);
        let initial = machine.vdp().prepare_line(REGION).unwrap();
        let at = 766_091;
        machine.queue_vdp_capture(tick(at)).unwrap();
        let mut expected_vdp = machine.vdp().clone();
        expected_vdp.advance_to(tick(at + 9), REGION).unwrap();
        advance(&mut machine, at + 9 - 850).unwrap();
        assert_eq!(machine.vdp(), &expected_vdp);
        assert_eq!(machine.bus.now, tick(at + 9));
        assert_eq!(
            take(&mut machine),
            Some(Err(VdpLineError::Schedule(VdpScheduleError::Interrupt(
                crate::VdpInterruptError::Timing(crate::VdpTimingError::UnqualifiedMemoryInterval(
                    tick(at)
                ))
            ))))
        );
        assert_eq!(initial.colors(), &[0; 256]);
        assert_eq!(take(&mut machine), None);
        machine.queue_vdp_capture(tick(at + 10)).unwrap();

        let mut machine = super::vdp_row_delivery::fixture(concurrent, NEXT);
        let write = machine.queue_vdp_register(11, 1).unwrap();
        machine
            .queue_vdp_capture(tick(write.publish_at.get() + 1))
            .unwrap();
        let mut ordinary = machine.clone();
        ordinary.bus.video = Default::default();
        advance(&mut machine, 30).unwrap();
        advance(&mut ordinary, 30).unwrap();
        assert_eq!(
            take(&mut machine),
            Some(Err(VdpLineError::Render(
                VdpRenderError::UnsupportedScrollMode(1)
            )))
        );
        assert_eq!(machine, ordinary);

        let mut machine = super::vdp_row_delivery::fixture(concurrent, NEXT);
        machine.queue_vdp_capture(tick(u64::MAX)).unwrap();
        let saved = machine.clone();
        assert_eq!(
            advance(&mut machine, u64::MAX),
            Err(BusError::ClockOverflow)
        );
        assert_eq!(machine, saved);
        assert_eq!(take(&mut machine), None);
    }
}

pub fn horizon_and_acknowledge_with(
    mut advance: impl FnMut(&mut Emulator, u64) -> Result<(), BusError>,
    mut take: impl FnMut(&mut Emulator) -> Option<Result<VdpScanline, VdpLineError>>,
) {
    for concurrent in [false, true] {
        for capture_at in [2650, 2660] {
            let mut machine = super::vdp_row_delivery::fixture(concurrent, NEXT);
            advance(&mut machine, 2632 - 850).unwrap();
            machine.queue_vdp_capture(tick(capture_at)).unwrap();
            let mut ordinary = machine.clone();
            let pointer = machine.vdp().memory().vram().as_ptr();
            let clock = machine.bus.cpu_clock;
            let response = acknowledge(&mut machine);
            assert_eq!(response.wait_clocks, 0);
            assert_eq!(
                response.result,
                Ok(zeff_m68k::bus::BusResponse::Complete(
                    zeff_m68k::timing::InterruptVector::Autovector
                ))
            );
            advance(&mut ordinary, 28).unwrap();
            assert_eq!(machine.bus.now, tick(2660));
            assert_eq!(machine.vdp().now(), machine.bus.now);
            assert_eq!(machine.bus.cpu_clock.0, clock.0 + 4);
            assert_eq!(machine.vdp(), ordinary.vdp());
            assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
            let row = take(&mut machine).unwrap().unwrap();
            assert_eq!(take(&mut ordinary), Some(Ok(row.clone())));
            assert_eq!(row.at(), tick(capture_at));
            assert_eq!(row.colors(), &[0; 256]);
            assert_eq!(take(&mut machine), None);
            assert_eq!(machine.bus.video, ordinary.bus.video);
        }

        let mut machine = super::vdp_row_delivery::fixture(concurrent, NEXT);
        machine.queue_vdp_capture(tick(878)).unwrap();
        assert!(acknowledge(&mut machine).result.is_ok());
        let row = take(&mut machine).unwrap().unwrap();
        assert_eq!(row.at(), tick(878));
        assert_eq!(row.colors(), &[0; 256]);
        assert_eq!(machine.bus.now, tick(878));
    }
}

fn acknowledge(
    machine: &mut Emulator,
) -> zeff_m68k::timing::CycleResult<
    zeff_m68k::bus::BusResponse<zeff_m68k::timing::InterruptVector>,
    BusError,
> {
    let access = BusAccess {
        logical_address: 0xfffffff8,
        bus_address: 0xfffff8,
        function_code: FunctionCode::CpuSpace,
        kind: AccessKind::InterruptAcknowledge,
        lanes: ByteLanes::Both,
    };
    let clock = machine.bus.cpu_clock;
    if machine.concurrent_sound {
        ScheduledBus {
            bus: &mut machine.bus,
            cpu: &mut machine.sound_cpu,
            schedule: &mut machine.sound_schedule,
        }
        .acknowledge_cycle(access, clock)
    } else {
        machine.bus.acknowledge_cycle(access, clock)
    }
}

fn advance(machine: &mut Emulator, ticks: u64) -> Result<(), BusError> {
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

#[test]
fn reset_delivery() {
    reset_delivery_with(
        Emulator::reset_cpu,
        Reset::reset,
        advance,
        Emulator::take_vdp_capture,
    );
}

#[test]
fn delivery_faults() {
    delivery_faults_with(advance, Emulator::take_vdp_capture);
}

#[test]
fn horizon_and_acknowledge() {
    horizon_and_acknowledge_with(advance, Emulator::take_vdp_capture);
}
