use super::admission::{AdmissionError, admit};
use super::*;
use zeff_emu_common::audio_trace::AudioTraceInvalidation;
use zeff_m68k::timing::PrefetchCpu;
use zeff_m68k::timing::physical::{
    EntryFault, EntryStep, ExternalResetBoundary, PhysicalEntryState, PhysicalInspection,
    ResetQualification,
};
use zeff_m68k::timing::reset_boundary::ExternalResetBoundaryError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Rejection {
    Admission(AdmissionError),
    AudioBatchActive,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CompletionError {
    Constructor,
    Primitive,
    Adapter,
    ClockFault,
    Sound(SoundFault),
    ClockGeometry,
    Finalizer(ExternalResetBoundaryError),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Terminal {
    pub cpu_clock: CpuClocks,
    pub bus_clock: CpuClocks,
    pub master: MasterTicks,
    pub vdp: MasterTicks,
    pub stopped: bool,
    pub physical: PhysicalEntryState,
    pub inspection: PhysicalInspection,
    pub readiness: Result<(), ExternalResetBoundaryError>,
    pub clock_fault: bool,
    pub sound_fault: Option<SoundFault>,
}

impl Terminal {
    pub fn capture(cpu: &PrefetchCpu, bus: &Bus, schedule: &SoundSchedule) -> Self {
        Self {
            cpu_clock: cpu.clock(),
            bus_clock: bus.cpu_clock,
            master: bus.now,
            vdp: bus.vdp.now(),
            stopped: cpu.stopped(),
            physical: cpu.physical_entry_state(),
            inspection: cpu.physical_inspection(),
            readiness: cpu.check_external_reset_ready_boundary(),
            clock_fault: bus.clock_fault,
            sound_fault: schedule.fault,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Report {
    pub origin: Terminal,
    pub entry: Option<Result<EntryStep, EntryFault<ResetBusError>>>,
    pub adapter: Option<ResetBusError>,
    pub idle: Option<IdleFailure>,
    pub completion: Result<(), CompletionError>,
    pub finalization: Option<Result<(), ExternalResetBoundaryError>>,
    pub elapsed_master: Option<u64>,
    pub elapsed_cpu: Option<u64>,
    pub elapsed_bus: Option<u64>,
    pub terminal: Terminal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    Rejected(Rejection),
    Entered(Box<Report>),
}

fn consistent(
    report: &Report,
    cpu: &PrefetchCpu,
    bus: &Bus,
    schedule: &SoundSchedule,
) -> Result<(), CompletionError> {
    let Some(entry) = &report.entry else {
        return Err(CompletionError::Constructor);
    };
    let step = entry.as_ref().map_err(|_| CompletionError::Primitive)?;
    if report.adapter.is_some() || report.idle.is_some() {
        return Err(CompletionError::Adapter);
    }
    if bus.clock_fault {
        return Err(CompletionError::ClockFault);
    }
    if let Some(fault) = schedule.fault {
        return Err(CompletionError::Sound(fault));
    }
    let master = step
        .clocks
        .checked_mul(MAIN_CPU_DIVISOR)
        .ok_or(CompletionError::ClockGeometry)?;
    if cpu.clock() != bus.cpu_clock
        || report.elapsed_cpu != Some(step.clocks)
        || report.elapsed_bus != Some(step.clocks)
        || report.elapsed_master != Some(master)
        || bus.now != bus.vdp.now()
    {
        return Err(CompletionError::ClockGeometry);
    }
    Ok(())
}

pub(crate) fn run<C: SoundCapture>(
    cpu: &mut PrefetchCpu,
    bus: &mut Bus,
    sound: (&mut Cpu, &mut SoundSchedule),
    concurrent: bool,
    qualification: ResetQualification,
    capture: &mut C,
) -> Outcome {
    let (sound, schedule) = sound;
    let release = match admit(cpu, bus, schedule, qualification) {
        Ok(clock) => clock,
        Err(error) => return Outcome::Rejected(Rejection::Admission(error)),
    };
    if bus.audio.batch_active() {
        return Outcome::Rejected(Rejection::AudioBatchActive);
    }
    let origin = Terminal::capture(cpu, bus, schedule);
    bus.audio
        .invalidate_trace(AudioTraceInvalidation::ExternalMutation);
    schedule.finish_epoch_field(sound, &bus.sound, capture);
    cpu.interrupt_input().set_level(bus.interrupt_level());
    if concurrent {
        bus.audio.begin_batch();
    }
    let target = if concurrent {
        Target::Scheduled(ScheduledBus {
            capture: &mut *capture,
            bus,
            cpu: sound,
            schedule,
        })
    } else {
        Target::Direct(bus)
    };
    let (entry, adapter, idle) = match ResetEntry::new(target) {
        Ok(mut entry) => {
            let result = cpu.external_warm_reset(
                &mut entry,
                ExternalResetBoundary {
                    sampled_release: release,
                    qualification,
                },
            );
            (Some(result), entry.failure, entry.idle_failure)
        }
        Err(error) => (None, Some(error), None),
    };
    schedule.finish_epoch_field(sound, &bus.sound, capture);
    if concurrent {
        bus.audio.finish_batch();
    }
    bus.audio.trace_origin = None;
    let mut report = Report {
        entry,
        adapter,
        idle,
        completion: Ok(()),
        finalization: None,
        elapsed_master: bus.now.get().checked_sub(origin.master.get()),
        elapsed_cpu: cpu.clock().0.checked_sub(origin.cpu_clock.0),
        elapsed_bus: bus.cpu_clock.0.checked_sub(origin.bus_clock.0),
        origin,
        terminal: Terminal::capture(cpu, bus, schedule),
    };
    report.completion = consistent(&report, cpu, bus, schedule);
    if report.completion.is_ok() {
        let finalized = cpu.finish_external_reset_boundary();
        report.finalization = Some(finalized);
        report.completion = finalized.map_err(CompletionError::Finalizer);
    }
    report.terminal = Terminal::capture(cpu, bus, schedule);
    Outcome::Entered(Box::new(report))
}
