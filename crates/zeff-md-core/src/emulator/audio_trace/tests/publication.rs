use super::*;
use crate::hardware::audio::Audio;
use crate::hardware::audio::trace::{main_origin, sound_origin, trace_chip};
use crate::hardware::bus::ownership::{Requester, Reservation};
use crate::hardware::sound::SoundMemory;
use crate::hardware::sound::schedule::{PendingSound, SoundInterval, SoundSchedule};

fn audio(region: Region) -> Audio {
    let mut audio = Audio::new(region);
    audio.trace = audio
        .trace
        .prepare(
            16,
            region.master_hz() as u32,
            trace_chip(region),
            AudioTraceTiming::BusServiceBoundary,
        )
        .unwrap();
    audio
}

fn memory(program: &[u8]) -> SoundMemory {
    let mut memory = SoundMemory::default();
    memory.set_reset(false);
    memory.ram[..program.len()].copy_from_slice(program);
    memory
}

#[test]
fn sound_store_pre_tie_post_deadline_and_main_context_are_exact() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let cartridge = fixture(region, true, &[0x60fe]).bus.cartridge;
        let mut memory = memory(&[0x3e, 0x85, 0x32, 0x11, 0x7f, 0x76]);
        let mut cpu = zeff_z80::Cpu::new();
        let mut schedule = SoundSchedule::default();
        let mut audio = audio(region);
        let main = main_origin(0x100, &cartridge);
        audio.trace_origin = Some(main);
        schedule.advance(
            &mut cpu,
            &mut memory,
            &cartridge,
            &mut [],
            SoundInterval::new(0..299, &mut audio),
        );
        assert_eq!(schedule.pending.as_ref().unwrap().deadline, 300);
        assert!(audio.clone().trace.finish(299).unwrap().events.is_empty());
        schedule.advance(
            &mut cpu,
            &mut memory,
            &cartridge,
            &mut [],
            SoundInterval::new(299..300, &mut audio),
        );
        assert_eq!(audio.trace_origin, Some(main));
        let trace = audio.clone().trace.finish(300).unwrap();
        assert_eq!(trace.events.len(), 1);
        assert_eq!(trace.events[0].cycle, 300);
        assert!(matches!(
            trace.events[0].write,
            MdTraceWrite::Psg {
                origin: MdTraceOrigin {
                    writer: MdTraceWriter::Z80,
                    source: MdTraceSource::SoundRam { .. },
                    ..
                },
                value: 0x85
            }
        ));
        schedule.advance(
            &mut cpu,
            &mut memory,
            &cartridge,
            &mut [],
            SoundInterval::new(300..301, &mut audio),
        );
        assert_eq!(audio.trace.finish(301).unwrap().events, trace.events);
        assert_eq!(schedule.fault, None);
    }
}

#[test]
fn banked_store_uses_reserved_final_deadline_and_never_records_preparation() {
    let cartridge = fixture(Region::NtscUs, true, &[0x60fe]).bus.cartridge;
    let mut memory = memory(&[0x3e, 0x9f, 0x32, 0x11, 0x80, 0x76]);
    memory.bank = 0x180;
    let mut cpu = zeff_z80::Cpu::new();
    let mut audio = audio(Region::NtscUs);
    PendingSound::prepare(&cpu, &memory, &cartridge, 0)
        .unwrap()
        .publish(&mut cpu, &mut memory, &mut [], &mut audio);
    let mut pending = PendingSound::prepare(&cpu, &memory, &cartridge, 105).unwrap();
    pending.trace_origin = Some(sound_origin(cpu.regs().pc, memory.bank, &cartridge));
    assert_eq!(pending.deadline, 300);
    pending
        .reserve_after(&[Reservation {
            requester: Requester::VDP,
            ticks: 105..1500,
        }])
        .unwrap();
    assert_eq!(pending.deadline, 1695);
    assert!(
        audio
            .clone()
            .trace
            .finish(pending.deadline)
            .unwrap()
            .events
            .is_empty()
    );
    let deadline = pending.deadline;
    pending.publish(&mut cpu, &mut memory, &mut [], &mut audio);
    let trace = audio.trace.finish(deadline).unwrap();
    trace.validate_complete().unwrap();
    assert_eq!(trace.events.len(), 1);
    assert_eq!(trace.events[0].cycle, deadline);
    assert_eq!(
        trace.events[0].write,
        MdTraceWrite::Psg {
            origin: MdTraceOrigin {
                writer: MdTraceWriter::Z80,
                boundary_pc: 2,
                source: MdTraceSource::SoundRam { offset: 2 }
            },
            value: 0x9f
        }
    );
}

#[test]
fn live_opn2_read_replay_does_not_emit_phantom_or_duplicate_writes() {
    let cartridge = fixture(Region::NtscUs, true, &[0x60fe]).bus.cartridge;
    let mut memory = memory(&[0x3a, 0, 0x40, 0x32, 0x11, 0x7f, 0x76]);
    let mut cpu = zeff_z80::Cpu::new();
    let mut schedule = SoundSchedule::default();
    let mut audio = audio(Region::NtscUs);
    audio.trace_origin = Some(main_origin(0x100, &cartridge));
    audio.write_opn2(0, 0, 0x2a);
    schedule.advance(
        &mut cpu,
        &mut memory,
        &cartridge,
        &mut [],
        SoundInterval::new(0..151, &mut audio),
    );
    assert_eq!(schedule.pending.as_ref().unwrap().deadline, 195);
    assert_eq!(audio.clone().trace.finish(151).unwrap().events.len(), 1);
    audio.write_opn2(160, 1, 0x90);
    schedule.advance(
        &mut cpu,
        &mut memory,
        &cartridge,
        &mut [],
        SoundInterval::new(160..195, &mut audio),
    );
    assert_eq!(cpu.regs().a, 0x80);
    assert_eq!(audio.clone().trace.finish(195).unwrap().events.len(), 2);
    schedule.advance(
        &mut cpu,
        &mut memory,
        &cartridge,
        &mut [],
        SoundInterval::new(195..390, &mut audio),
    );
    let trace = audio.trace.finish(390).unwrap();
    trace.validate_complete().unwrap();
    assert_eq!(
        trace
            .events
            .iter()
            .map(|event| event.cycle)
            .collect::<Vec<_>>(),
        [0, 160, 390]
    );
    assert!(matches!(
        trace.events[2].write,
        MdTraceWrite::Psg {
            origin: MdTraceOrigin {
                writer: MdTraceWriter::Z80,
                ..
            },
            value: 0x80
        }
    ));
    assert_eq!(schedule.fault, None);
}

#[test]
fn main_transfer_tied_to_sound_publication_retains_sound_then_main_order() {
    use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, FunctionCode};
    use zeff_m68k::timing::ClockedBus;
    let mut machine = fixture(Region::NtscUs, true, &[0x60fe]);
    machine.reset_and_begin_audio_trace(8).unwrap();
    machine.bus.sound.set_reset(false);
    machine.bus.sound.ram[..6].copy_from_slice(&[0x3e, 0x85, 0x32, 0x11, 0x7f, 0x76]);
    let cartridge = &machine.bus.cartridge;
    PendingSound::prepare(&machine.sound_cpu, &machine.bus.sound, cartridge, 56)
        .unwrap()
        .publish(
            &mut machine.sound_cpu,
            &mut machine.bus.sound,
            &mut [],
            &mut machine.bus.audio,
        );
    let mut pending =
        PendingSound::prepare(&machine.sound_cpu, &machine.bus.sound, cartridge, 225).unwrap();
    assert_eq!(pending.deadline, 420);
    pending.trace_origin = Some(MdTraceOrigin {
        writer: MdTraceWriter::Z80,
        boundary_pc: 2,
        source: MdTraceSource::SoundRam { offset: 2 },
    });
    machine.sound_schedule.pending = Some(pending);
    machine.bus.advance_machine(336).unwrap();
    assert_eq!(machine.bus.now.get(), 392);
    machine.begin_main_trace();
    let clock = machine.bus.cpu_clock;
    let access = BusAccess {
        logical_address: 0xc00011,
        bus_address: 0xc00010,
        function_code: FunctionCode::SupervisorData,
        kind: AccessKind::Data,
        lanes: ByteLanes::Lower,
    };
    crate::hardware::bus::scheduled::ScheduledBus {
        bus: &mut machine.bus,
        cpu: &mut machine.sound_cpu,
        schedule: &mut machine.sound_schedule,
    }
    .write_cycle(access, 0x9f, clock)
    .result
    .unwrap();
    let trace = machine.finish_audio_trace().unwrap().trace;
    trace.validate_complete().unwrap();
    assert_eq!(
        trace.events.iter().map(|e| e.cycle).collect::<Vec<_>>(),
        [420, 420]
    );
    assert_eq!(
        trace.events[0].write,
        MdTraceWrite::Psg {
            origin: MdTraceOrigin {
                writer: MdTraceWriter::Z80,
                boundary_pc: 2,
                source: MdTraceSource::SoundRam { offset: 2 }
            },
            value: 0x85
        }
    );
    assert_eq!(
        trace.events[1].write,
        MdTraceWrite::Psg {
            origin: MdTraceOrigin {
                writer: MdTraceWriter::M68000,
                boundary_pc: 0x100,
                source: MdTraceSource::EffectiveRom { offset: 0x100 }
            },
            value: 0x9f
        }
    );
}
