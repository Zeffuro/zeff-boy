use super::*;
use zeff_m68k::timing::PrefetchCpu;
use zeff_m68k::timing::physical::ResetQualification;
use zeff_m68k::timing::reset_boundary::ExternalResetBoundaryError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdmissionError {
    Unqualified,
    InputSerial,
    Cpu(ExternalResetBoundaryError),
    Bus(BusError),
    Sound(SoundFault),
    MainRmw,
    SoundRmwCheckpoint,
    ResetOutput,
    AudioBatchActive,
    CartridgeOwner,
    SoundLatches,
    SoundEpoch,
    SoundContinuation,
}

pub(crate) fn admit(
    cpu: &PrefetchCpu,
    bus: &Bus,
    schedule: &SoundSchedule,
    qualification: ResetQualification,
) -> Result<CpuClocks, AdmissionError> {
    if qualification != ResetQualification::QualifiedSafeAssertionAndRelease {
        return Err(AdmissionError::Unqualified);
    }
    cpu.check_external_reset_ready_boundary()
        .map_err(AdmissionError::Cpu)?;
    bus.check_main_clock(cpu.clock())
        .map_err(AdmissionError::Bus)?;
    if let Some(fault) = schedule.fault {
        return Err(AdmissionError::Sound(fault));
    }
    if bus.rmw.is_some() {
        return Err(AdmissionError::MainRmw);
    }
    if schedule.rmw_checkpoint.is_some() {
        return Err(AdmissionError::SoundRmwCheckpoint);
    }
    if bus.reset_output {
        return Err(AdmissionError::ResetOutput);
    }
    Ok(cpu.clock())
}
