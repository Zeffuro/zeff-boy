use std::panic::{AssertUnwindSafe, catch_unwind};

use sha2::{Digest, Sha256};
use zeff_emu_common::media::MediaSlotSnapshot;
use zeff_emu_common::system::CoreFamily;
use zeff_emu_common::time::{MachineTiming, TimingSnapshot};

use super::fixtures::{
    build_coleco_backend, build_gb_backend, build_gba_backend, build_nes_backend,
    build_pce_backend, build_sms_backend, build_ws_backend,
};
use crate::emu_backend::{ActiveSystem, EmuBackend};

const REPEATED_CHECKPOINTS: usize = 8;
const REPLAY_FRAMES: usize = 5;
const SPECULATIVE_FRAMES: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RunAheadSupport {
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IneligibilityReason {
    FeatureDisabled,
    DetachedPanicHangContainmentUnavailable,
    NoSpeculativeWorker,
    SramDiskWriteIsolationUnproven,
    RecoveryGenerationIsolationUnproven,
    UiPersistenceIsolationUnproven,
    ReplayIsolationUnproven,
    RemoteIsolationUnproven,
    LinkDevice,
    Rtc,
    LiveCamera,
    HostSensor,
    Printer,
    Rumble,
    LightGun,
    Mouse,
    RemovableMedia,
    CdMedia,
    ObservedRestoreFramebufferMismatch,
    ObservedRestoreFrameCountMismatch,
    ObservedAudioCadenceMismatch,
}

struct EligibilityResult {
    core_family: CoreFamily,
    fixture_system: ActiveSystem,
    support: RunAheadSupport,
    reasons: &'static [IneligibilityReason],
}

struct FrameObservation {
    exact_state: Vec<u8>,
    framebuffer: Vec<u8>,
    audio: Vec<f32>,
    frame_count: u64,
    timing: TimingSnapshot,
    program_counter: u64,
    battery_hash: [u8; 32],
    rumble: bool,
    printer_jobs: Vec<zeff_gb_core::hardware::GameBoyPrinterJob>,
    media: Option<MediaSlotSnapshot>,
}

#[derive(Clone, Copy)]
struct InputState {
    buttons: u8,
    dpad: u8,
}

struct CoreCase {
    eligibility: EligibilityResult,
    build: fn() -> EmuBackend,
    expected_local_failures: [&'static [FailureClass]; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FailureClass {
    ExactState,
    Framebuffer,
    AudioCadence,
    FrameCount,
    Timing,
    ProgramCounter,
    Battery,
    Rumble,
    Printer,
    Media,
    Operational,
    Panic,
}

struct GateFailure {
    classes: Vec<FailureClass>,
    detail: String,
}

struct GateResult {
    name: &'static str,
    failure: Option<GateFailure>,
}

struct ConformanceReport {
    core_family: CoreFamily,
    gates: Vec<GateResult>,
}

#[test]
fn every_core_family_has_a_disabled_local_conformance_result() {
    let cases = core_cases();
    assert_eq!(cases.len(), 7);
    let reports = cases.iter().map(run_conformance).collect::<Vec<_>>();

    for (case, report) in cases.iter().zip(&reports) {
        eprintln!("{}", report.summary());
        assert_eq!(case.eligibility.support, RunAheadSupport::Unsupported);
        assert_eq!(report.core_family, case.eligibility.core_family);
        assert_eq!(report.gates.len(), 3);
        for (gate, expected) in report.gates.iter().zip(case.expected_local_failures) {
            assert_eq!(
                gate.failure_classes(),
                expected,
                "run-ahead {} failure taxonomy changed for {:?}: {}",
                gate.name,
                case.eligibility.core_family,
                gate.summary()
            );
            for class in gate.failure_classes() {
                if let Some(reason) = class.ineligibility_reason() {
                    assert!(
                        case.eligibility.reasons.contains(&reason),
                        "run-ahead {:?} records {class:?} without {reason:?}",
                        case.eligibility.core_family
                    );
                }
            }
        }
        if matches!(
            case.eligibility.core_family,
            CoreFamily::GameBoyAdvance | CoreFamily::Sega8
        ) {
            assert!(
                case.eligibility
                    .reasons
                    .contains(&IneligibilityReason::FeatureDisabled)
            );
        } else {
            for reason in [
                IneligibilityReason::NoSpeculativeWorker,
                IneligibilityReason::SramDiskWriteIsolationUnproven,
                IneligibilityReason::RecoveryGenerationIsolationUnproven,
                IneligibilityReason::UiPersistenceIsolationUnproven,
                IneligibilityReason::ReplayIsolationUnproven,
                IneligibilityReason::RemoteIsolationUnproven,
            ] {
                assert!(case.eligibility.reasons.contains(&reason));
            }
        }
    }
}

#[test]
fn game_boy_v13_observes_all_local_conformance_gates() {
    let case = core_cases()
        .iter()
        .find(|case| case.eligibility.core_family == CoreFamily::GameBoy)
        .unwrap();
    let report = run_conformance(case);

    eprintln!("{}", report.summary());
    assert_eq!(report.gates.len(), 3);
    for gate in &report.gates {
        assert!(
            !gate.failure_classes().contains(&FailureClass::Panic)
                && !gate.failure_classes().contains(&FailureClass::Operational),
            "{}",
            gate.summary()
        );
    }
}

#[test]
fn nes_v11_observes_all_local_conformance_gates() {
    let case = core_cases()
        .iter()
        .find(|case| case.eligibility.core_family == CoreFamily::Nes)
        .unwrap();
    let report = run_conformance(case);

    eprintln!("{}", report.summary());
    assert_eq!(report.gates.len(), 3);
    for gate in &report.gates {
        assert!(
            !gate.failure_classes().contains(&FailureClass::Panic)
                && !gate.failure_classes().contains(&FailureClass::Operational),
            "{}",
            gate.summary()
        );
    }
}

#[test]
fn nondeterministic_and_host_owned_paths_are_explicitly_ineligible() {
    let cases = core_cases();
    let reasons = |family| {
        cases
            .iter()
            .find(|case| case.eligibility.core_family == family)
            .expect("core family should have an eligibility result")
            .eligibility
            .reasons
    };

    let gb = reasons(CoreFamily::GameBoy);
    for reason in [
        IneligibilityReason::LinkDevice,
        IneligibilityReason::Rtc,
        IneligibilityReason::LiveCamera,
        IneligibilityReason::HostSensor,
        IneligibilityReason::Printer,
        IneligibilityReason::Rumble,
        IneligibilityReason::ObservedAudioCadenceMismatch,
    ] {
        assert!(gb.contains(&reason));
    }
    assert_eq!(
        reasons(CoreFamily::GameBoyAdvance),
        &[
            IneligibilityReason::FeatureDisabled,
            IneligibilityReason::DetachedPanicHangContainmentUnavailable,
            IneligibilityReason::ObservedAudioCadenceMismatch,
        ]
    );
    let nes = reasons(CoreFamily::Nes);
    assert!(nes.contains(&IneligibilityReason::LightGun));
    assert!(nes.contains(&IneligibilityReason::RemovableMedia));
    assert!(nes.contains(&IneligibilityReason::ObservedAudioCadenceMismatch));
    assert!(
        reasons(CoreFamily::ColecoVision)
            .contains(&IneligibilityReason::ObservedAudioCadenceMismatch)
    );
    let pce = reasons(CoreFamily::PcEngine);
    assert!(pce.contains(&IneligibilityReason::Mouse));
    assert!(pce.contains(&IneligibilityReason::CdMedia));
    assert!(pce.contains(&IneligibilityReason::RemovableMedia));
    let sega8 = cases
        .iter()
        .find(|case| case.eligibility.core_family == CoreFamily::Sega8)
        .expect("Sega8 should have an eligibility row");
    assert_eq!(sega8.eligibility.fixture_system, ActiveSystem::MasterSystem);
    assert_eq!(sega8.eligibility.support, RunAheadSupport::Unsupported);
    assert_eq!(
        sega8.eligibility.reasons,
        &[
            IneligibilityReason::FeatureDisabled,
            IneligibilityReason::DetachedPanicHangContainmentUnavailable,
        ]
    );
    let ws = reasons(CoreFamily::WonderSwan);
    assert!(ws.contains(&IneligibilityReason::LinkDevice));
    assert!(ws.contains(&IneligibilityReason::ObservedAudioCadenceMismatch));
}

impl ConformanceReport {
    fn summary(&self) -> String {
        let gates = self
            .gates
            .iter()
            .map(GateResult::summary)
            .collect::<Vec<_>>()
            .join("; ");
        format!("run-ahead {:?}: Unsupported; {gates}", self.core_family)
    }
}

impl GateResult {
    fn failure_classes(&self) -> &[FailureClass] {
        self.failure
            .as_ref()
            .map_or(&[], |failure| failure.classes.as_slice())
    }

    fn summary(&self) -> String {
        match &self.failure {
            Some(failure) => format!(
                "{}=FAIL({:?}: {})",
                self.name, failure.classes, failure.detail
            ),
            None => format!("{}=PASS", self.name),
        }
    }
}

impl FailureClass {
    fn ineligibility_reason(self) -> Option<IneligibilityReason> {
        match self {
            Self::Framebuffer => Some(IneligibilityReason::ObservedRestoreFramebufferMismatch),
            Self::FrameCount => Some(IneligibilityReason::ObservedRestoreFrameCountMismatch),
            Self::AudioCadence => Some(IneligibilityReason::ObservedAudioCadenceMismatch),
            _ => None,
        }
    }
}

impl GateFailure {
    fn semantic(class: FailureClass, detail: impl Into<String>) -> Self {
        Self {
            classes: vec![class],
            detail: detail.into(),
        }
    }

    fn operational(detail: impl Into<String>) -> Self {
        Self {
            classes: vec![FailureClass::Operational],
            detail: detail.into(),
        }
    }

    fn with_context(mut self, context: impl std::fmt::Display) -> Self {
        self.detail = format!("{context}: {}", self.detail);
        self
    }
}

fn run_conformance(case: &CoreCase) -> ConformanceReport {
    ConformanceReport {
        core_family: case.eligibility.core_family,
        gates: vec![
            run_gate("repeated_capture_restore", || {
                check_repeated_capture_restore(case)
            }),
            run_gate("speculative_restore_next_frame", || {
                check_speculative_restore_next_frame(case)
            }),
            run_gate("failed_restore_containment", || {
                check_failed_restore_is_contained(case)
            }),
        ],
    }
}

fn run_gate(name: &'static str, check: impl FnOnce() -> Result<(), GateFailure>) -> GateResult {
    let failure = match catch_unwind(AssertUnwindSafe(check)) {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(error),
        Err(payload) => Some(GateFailure {
            classes: vec![FailureClass::Panic],
            detail: panic_message(payload),
        }),
    };
    GateResult { name, failure }
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    let message = if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic".to_owned()
    };
    let mut chars = message.chars();
    let prefix = chars.by_ref().take(240).collect::<String>();
    if chars.next().is_some() {
        format!("{prefix}...")
    } else {
        message
    }
}

fn check_repeated_capture_restore(case: &CoreCase) -> Result<(), GateFailure> {
    let mut backend = build_checked_backend(case)?;

    for checkpoint in 0..REPEATED_CHECKPOINTS {
        let checkpoint_input = input_for(checkpoint);
        apply_input(&mut backend, checkpoint_input);
        backend.step_frame();
        clear_host_outputs(&mut backend)?;
        let saved = backend.encode_state_bytes().map_err(|error| {
            GateFailure::operational(format!("checkpoint {checkpoint} capture: {error}"))
        })?;
        let checkpoint_observation = observe_frame(&mut backend)?;
        let first = run_observed_frames(&mut backend, checkpoint, REPLAY_FRAMES)?;

        restore_with_input(&mut backend, saved, checkpoint_input)
            .map_err(|error| error.with_context(format!("checkpoint {checkpoint} restore")))?;
        compare_observations(&checkpoint_observation, &observe_frame(&mut backend)?).map_err(
            |error| error.with_context(format!("checkpoint {checkpoint} restore boundary")),
        )?;
        let second = run_observed_frames(&mut backend, checkpoint, REPLAY_FRAMES)?;
        compare_trajectories(&first, &second)
            .map_err(|error| error.with_context(format!("checkpoint {checkpoint} trajectory")))?;
    }
    Ok(())
}

fn check_speculative_restore_next_frame(case: &CoreCase) -> Result<(), GateFailure> {
    let mut primary = build_checked_backend(case)?;
    let mut control = build_checked_backend(case)?;
    let mut checkpoint_input = input_for(0);

    for frame in 0..3 {
        checkpoint_input = input_for(frame);
        apply_input(&mut primary, checkpoint_input);
        primary.step_frame();
        clear_host_outputs(&mut primary)?;
    }
    let checkpoint = primary
        .encode_state_bytes()
        .map_err(|error| GateFailure::operational(format!("primary capture: {error}")))?;
    let checkpoint_observation = observe_frame(&mut primary)?;
    restore_with_input(&mut control, checkpoint.clone(), checkpoint_input)
        .map_err(|error| error.with_context("secondary transfer"))?;
    compare_observations(&checkpoint_observation, &observe_frame(&mut control)?)
        .map_err(|error| error.with_context("secondary transfer boundary"))?;

    let before_battery = primary.battery_component_hash();
    let before_rumble = primary.rumble_active();
    let before_media = primary.media_slot_snapshot();
    for frame in 0..SPECULATIVE_FRAMES {
        apply_input(&mut primary, input_for(100 + frame));
        primary.step_frame();
        let mut discarded_audio = Vec::new();
        primary.drain_audio_samples_into(&mut discarded_audio);
        if !primary.take_game_boy_printer_jobs().is_empty() {
            return Err(GateFailure::semantic(
                FailureClass::Printer,
                format!("speculative frame {frame}: printer job escaped"),
            ));
        }
        if primary.media_slot_snapshot() != before_media {
            return Err(GateFailure::semantic(
                FailureClass::Media,
                format!("speculative frame {frame}: media changed"),
            ));
        }
    }
    restore_with_input(&mut primary, checkpoint, checkpoint_input)
        .map_err(|error| error.with_context("speculative rollback"))?;
    compare_observations(&checkpoint_observation, &observe_frame(&mut primary)?)
        .map_err(|error| error.with_context("speculative rollback boundary"))?;
    if primary.battery_component_hash() != before_battery {
        return Err(GateFailure::semantic(
            FailureClass::Battery,
            "speculative rollback changed battery hash",
        ));
    }
    if primary.rumble_active() != before_rumble {
        return Err(GateFailure::semantic(
            FailureClass::Rumble,
            "speculative rollback changed rumble",
        ));
    }
    if primary.media_slot_snapshot() != before_media {
        return Err(GateFailure::semantic(
            FailureClass::Media,
            "speculative rollback changed media",
        ));
    }
    if !primary.take_game_boy_printer_jobs().is_empty() {
        return Err(GateFailure::semantic(
            FailureClass::Printer,
            "speculative rollback retained printer jobs",
        ));
    }

    let committed_input = input_for(500);
    apply_input(&mut primary, committed_input);
    apply_input(&mut control, committed_input);
    primary.step_frame();
    control.step_frame();
    compare_observations(&observe_frame(&mut control)?, &observe_frame(&mut primary)?)
        .map_err(|error| error.with_context("committed next frame"))
}

fn check_failed_restore_is_contained(case: &CoreCase) -> Result<(), GateFailure> {
    let mut subject = build_checked_backend(case)?;
    let mut control = build_checked_backend(case)?;
    let mut checkpoint_input = input_for(0);

    for frame in 0..3 {
        checkpoint_input = input_for(frame);
        apply_input(&mut subject, checkpoint_input);
        subject.step_frame();
        clear_host_outputs(&mut subject)?;
    }
    let checkpoint = subject.encode_state_bytes().map_err(|error| {
        GateFailure::operational(format!("failure checkpoint capture: {error}"))
    })?;
    let checkpoint_observation = observe_frame(&mut subject)?;
    restore_with_input(&mut control, checkpoint.clone(), checkpoint_input)
        .map_err(|error| error.with_context("failure control restore"))?;
    compare_observations(&checkpoint_observation, &observe_frame(&mut control)?)
        .map_err(|error| error.with_context("failure control boundary"))?;

    let before = observe_without_draining(&subject)?;
    let mut malformed = checkpoint;
    malformed.push(0xA5);
    if subject.load_state_from_bytes(malformed).is_ok() {
        return Err(GateFailure::operational(
            "trailing state payload was accepted",
        ));
    }
    compare_observations(&before, &observe_without_draining(&subject)?)
        .map_err(|error| error.with_context("failed restore changed session"))?;

    let committed_input = input_for(700);
    apply_input(&mut subject, committed_input);
    apply_input(&mut control, committed_input);
    subject.step_frame();
    control.step_frame();
    compare_observations(&observe_frame(&mut control)?, &observe_frame(&mut subject)?)
        .map_err(|error| error.with_context("failed restore changed next frame"))
}

fn build_checked_backend(case: &CoreCase) -> Result<EmuBackend, GateFailure> {
    let backend = (case.build)();
    if backend.system() != case.eligibility.fixture_system {
        return Err(GateFailure::operational(format!(
            "fixture system expected {:?}, got {:?}",
            case.eligibility.fixture_system,
            backend.system()
        )));
    }
    if backend.core_family() != case.eligibility.core_family {
        return Err(GateFailure::operational(format!(
            "fixture family expected {:?}, got {:?}",
            case.eligibility.core_family,
            backend.core_family()
        )));
    }
    Ok(backend)
}

fn restore_with_input(
    backend: &mut EmuBackend,
    state: Vec<u8>,
    input: InputState,
) -> Result<(), GateFailure> {
    backend
        .load_state_from_bytes(state)
        .map_err(|error| GateFailure::operational(error.to_string()))?;
    apply_input(backend, input);
    Ok(())
}

fn run_observed_frames(
    backend: &mut EmuBackend,
    input_base: usize,
    count: usize,
) -> Result<Vec<FrameObservation>, GateFailure> {
    (0..count)
        .map(|frame| {
            apply_input(backend, input_for(input_base * REPLAY_FRAMES + frame));
            backend.step_frame();
            observe_frame(backend)
        })
        .collect()
}

fn observe_frame(backend: &mut EmuBackend) -> Result<FrameObservation, GateFailure> {
    let mut observation = observe_without_draining(backend)?;
    backend.drain_audio_samples_into(&mut observation.audio);
    observation.printer_jobs = backend.take_game_boy_printer_jobs();
    Ok(observation)
}

fn observe_without_draining(backend: &EmuBackend) -> Result<FrameObservation, GateFailure> {
    Ok(FrameObservation {
        exact_state: backend.encode_state_bytes().map_err(|error| {
            GateFailure::operational(format!("exact state observation: {error}"))
        })?,
        framebuffer: backend.framebuffer().to_vec(),
        audio: Vec::new(),
        frame_count: backend.frame_count(),
        timing: backend.timing_snapshot(),
        program_counter: program_counter(backend),
        battery_hash: backend.battery_component_hash(),
        rumble: backend.rumble_active(),
        printer_jobs: Vec::new(),
        media: backend.media_slot_snapshot(),
    })
}

fn compare_trajectories(
    expected: &[FrameObservation],
    actual: &[FrameObservation],
) -> Result<(), GateFailure> {
    if expected.len() != actual.len() {
        return Err(GateFailure::operational(format!(
            "trajectory length {} != {}",
            expected.len(),
            actual.len()
        )));
    }
    for (index, (expected, actual)) in expected.iter().zip(actual).enumerate() {
        compare_observations(expected, actual)
            .map_err(|error| error.with_context(format!("frame {index}")))?;
    }
    Ok(())
}

fn compare_observations(
    expected: &FrameObservation,
    actual: &FrameObservation,
) -> Result<(), GateFailure> {
    let mut classes = Vec::new();
    let mut differences = Vec::new();
    if expected.exact_state != actual.exact_state {
        classes.push(FailureClass::ExactState);
        differences.push(format!(
            "state {}; {}",
            bytes_summary(&expected.exact_state, &actual.exact_state),
            first_byte_difference(&expected.exact_state, &actual.exact_state)
        ));
    }
    if expected.framebuffer != actual.framebuffer {
        classes.push(FailureClass::Framebuffer);
        differences.push(format!(
            "framebuffer {}; {}",
            bytes_summary(&expected.framebuffer, &actual.framebuffer),
            first_byte_difference(&expected.framebuffer, &actual.framebuffer)
        ));
    }
    if expected.audio != actual.audio {
        classes.push(FailureClass::AudioCadence);
        differences.push(audio_difference(&expected.audio, &actual.audio));
    }
    if expected.frame_count != actual.frame_count {
        classes.push(FailureClass::FrameCount);
        differences.push(format!(
            "frame_count {} != {}",
            expected.frame_count, actual.frame_count
        ));
    }
    if expected.timing != actual.timing {
        classes.push(FailureClass::Timing);
        differences.push(format!(
            "timing {} != {}",
            timing_summary(expected.timing),
            timing_summary(actual.timing)
        ));
    }
    if expected.program_counter != actual.program_counter {
        classes.push(FailureClass::ProgramCounter);
        differences.push(format!(
            "PC {:X} != {:X}",
            expected.program_counter, actual.program_counter
        ));
    }
    if expected.battery_hash != actual.battery_hash {
        classes.push(FailureClass::Battery);
        differences.push(format!(
            "battery {} != {}",
            digest_prefix(&expected.battery_hash),
            digest_prefix(&actual.battery_hash)
        ));
    }
    if expected.rumble != actual.rumble {
        classes.push(FailureClass::Rumble);
        differences.push(format!("rumble {} != {}", expected.rumble, actual.rumble));
    }
    if expected.printer_jobs != actual.printer_jobs {
        classes.push(FailureClass::Printer);
        differences.push(format!(
            "printer {} != {}",
            printer_summary(&expected.printer_jobs),
            printer_summary(&actual.printer_jobs)
        ));
    }
    if expected.media != actual.media {
        classes.push(FailureClass::Media);
        differences.push(format!(
            "media {} != {}",
            media_summary(expected.media.as_ref()),
            media_summary(actual.media.as_ref())
        ));
    }

    if differences.is_empty() {
        Ok(())
    } else {
        Err(GateFailure {
            classes,
            detail: differences.join(", "),
        })
    }
}

fn bytes_summary(expected: &[u8], actual: &[u8]) -> String {
    format!(
        "len {}/{} sha {}/{}",
        expected.len(),
        actual.len(),
        sha256_prefix(expected),
        sha256_prefix(actual)
    )
}

fn first_byte_difference(expected: &[u8], actual: &[u8]) -> String {
    match expected
        .iter()
        .zip(actual)
        .position(|(left, right)| left != right)
    {
        Some(index) => format!(
            "first_diff {index}: {:02X}/{:02X}",
            expected[index], actual[index]
        ),
        None => format!("first_diff {}: length", expected.len().min(actual.len())),
    }
}

fn audio_difference(expected: &[f32], actual: &[f32]) -> String {
    let first = expected
        .iter()
        .zip(actual)
        .position(|(left, right)| left.to_bits() != right.to_bits());
    let first = match first {
        Some(index) => format!(
            "{index}: {:08X}/{:08X}",
            expected[index].to_bits(),
            actual[index].to_bits()
        ),
        None => format!("{}: length", expected.len().min(actual.len())),
    };
    format!(
        "audio len {}/{} sha {}/{} first_diff {first}",
        expected.len(),
        actual.len(),
        audio_sha256_prefix(expected),
        audio_sha256_prefix(actual)
    )
}

fn sha256_prefix(bytes: &[u8]) -> String {
    let digest: [u8; 32] = Sha256::digest(bytes).into();
    digest_prefix(&digest)
}

fn audio_sha256_prefix(samples: &[f32]) -> String {
    let mut hasher = Sha256::new();
    for sample in samples {
        hasher.update(sample.to_bits().to_le_bytes());
    }
    let digest: [u8; 32] = hasher.finalize().into();
    digest_prefix(&digest)
}

fn digest_prefix(digest: &[u8; 32]) -> String {
    digest[..8]
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect()
}

fn timing_summary(timing: TimingSnapshot) -> String {
    format!(
        "{}@{}/{}Hz",
        timing.now(),
        timing.rate().numerator_hz(),
        timing.rate().denominator()
    )
}

fn printer_summary(jobs: &[zeff_gb_core::hardware::GameBoyPrinterJob]) -> String {
    match jobs.first() {
        Some(job) => format!(
            "{} first(h={}, copies={}, pixels={}:{})",
            jobs.len(),
            job.height,
            job.copies,
            job.pixels.len(),
            sha256_prefix(&job.pixels)
        ),
        None => "0".to_owned(),
    }
}

fn media_summary(media: Option<&MediaSlotSnapshot>) -> String {
    match media {
        Some(media) => format!(
            "slot={} inserted={} side={:?} writes={} sides={}",
            media.state.slot.as_ref(),
            media.inserted(),
            media.state.side,
            media.state.mutation_counter,
            media.side_count
        ),
        None => "none".to_owned(),
    }
}

fn clear_host_outputs(backend: &mut EmuBackend) -> Result<(), GateFailure> {
    let mut discarded_audio = Vec::new();
    backend.drain_audio_samples_into(&mut discarded_audio);
    if backend.take_game_boy_printer_jobs().is_empty() {
        Ok(())
    } else {
        Err(GateFailure::semantic(
            FailureClass::Printer,
            "fixture emitted printer job before checkpoint",
        ))
    }
}

fn input_for(frame: usize) -> InputState {
    const BUTTONS: [u8; 4] = [0, 1, 2, 4];
    const DPAD: [u8; 5] = [0, 1, 2, 4, 8];
    InputState {
        buttons: BUTTONS[frame % BUTTONS.len()],
        dpad: DPAD[frame % DPAD.len()],
    }
}

fn apply_input(backend: &mut EmuBackend, input: InputState) {
    backend.set_input(input.buttons, input.dpad);
}

fn program_counter(backend: &EmuBackend) -> u64 {
    match backend {
        EmuBackend::Gb(backend) => u64::from(backend.emu.cpu_pc()),
        EmuBackend::Gba(backend) => u64::from(backend.emu.cpu_pc()),
        EmuBackend::Nes(backend) => u64::from(backend.emu.cpu_pc()),
        EmuBackend::Coleco(backend) => u64::from(backend.emu.cpu().regs().pc),
        EmuBackend::Pce(backend) => u64::from(backend.debug_cpu_snapshot().registers().pc),
        EmuBackend::Sega8(backend) => u64::from(backend.emu.cpu().regs().pc),
        EmuBackend::Ws(backend) => u64::from(backend.emu.cpu_pc()),
    }
}

fn core_cases() -> &'static [CoreCase] {
    const PASS: &[FailureClass] = &[];
    const AUDIO_CADENCE: &[FailureClass] = &[FailureClass::AudioCadence];
    const GB: &[IneligibilityReason] = &[
        IneligibilityReason::NoSpeculativeWorker,
        IneligibilityReason::SramDiskWriteIsolationUnproven,
        IneligibilityReason::RecoveryGenerationIsolationUnproven,
        IneligibilityReason::UiPersistenceIsolationUnproven,
        IneligibilityReason::ReplayIsolationUnproven,
        IneligibilityReason::RemoteIsolationUnproven,
        IneligibilityReason::LinkDevice,
        IneligibilityReason::Rtc,
        IneligibilityReason::LiveCamera,
        IneligibilityReason::HostSensor,
        IneligibilityReason::Printer,
        IneligibilityReason::Rumble,
        IneligibilityReason::ObservedAudioCadenceMismatch,
    ];
    const GBA: &[IneligibilityReason] = &[
        IneligibilityReason::FeatureDisabled,
        IneligibilityReason::DetachedPanicHangContainmentUnavailable,
        IneligibilityReason::ObservedAudioCadenceMismatch,
    ];
    const NES: &[IneligibilityReason] = &[
        IneligibilityReason::NoSpeculativeWorker,
        IneligibilityReason::SramDiskWriteIsolationUnproven,
        IneligibilityReason::RecoveryGenerationIsolationUnproven,
        IneligibilityReason::UiPersistenceIsolationUnproven,
        IneligibilityReason::ReplayIsolationUnproven,
        IneligibilityReason::RemoteIsolationUnproven,
        IneligibilityReason::LightGun,
        IneligibilityReason::RemovableMedia,
        IneligibilityReason::ObservedAudioCadenceMismatch,
    ];
    const PCE: &[IneligibilityReason] = &[
        IneligibilityReason::NoSpeculativeWorker,
        IneligibilityReason::SramDiskWriteIsolationUnproven,
        IneligibilityReason::RecoveryGenerationIsolationUnproven,
        IneligibilityReason::UiPersistenceIsolationUnproven,
        IneligibilityReason::ReplayIsolationUnproven,
        IneligibilityReason::RemoteIsolationUnproven,
        IneligibilityReason::Mouse,
        IneligibilityReason::RemovableMedia,
        IneligibilityReason::CdMedia,
    ];
    const SEGA8: &[IneligibilityReason] = &[
        IneligibilityReason::FeatureDisabled,
        IneligibilityReason::DetachedPanicHangContainmentUnavailable,
    ];
    const COLECO: &[IneligibilityReason] = &[
        IneligibilityReason::NoSpeculativeWorker,
        IneligibilityReason::SramDiskWriteIsolationUnproven,
        IneligibilityReason::RecoveryGenerationIsolationUnproven,
        IneligibilityReason::UiPersistenceIsolationUnproven,
        IneligibilityReason::ReplayIsolationUnproven,
        IneligibilityReason::RemoteIsolationUnproven,
        IneligibilityReason::ObservedAudioCadenceMismatch,
    ];
    const WS: &[IneligibilityReason] = &[
        IneligibilityReason::NoSpeculativeWorker,
        IneligibilityReason::SramDiskWriteIsolationUnproven,
        IneligibilityReason::RecoveryGenerationIsolationUnproven,
        IneligibilityReason::UiPersistenceIsolationUnproven,
        IneligibilityReason::ReplayIsolationUnproven,
        IneligibilityReason::RemoteIsolationUnproven,
        IneligibilityReason::LinkDevice,
        IneligibilityReason::ObservedAudioCadenceMismatch,
    ];
    const CASES: &[CoreCase] = &[
        CoreCase {
            eligibility: EligibilityResult {
                core_family: CoreFamily::GameBoy,
                fixture_system: ActiveSystem::GameBoy,
                support: RunAheadSupport::Unsupported,
                reasons: GB,
            },
            build: build_gb_backend,
            expected_local_failures: [AUDIO_CADENCE, PASS, AUDIO_CADENCE],
        },
        CoreCase {
            eligibility: EligibilityResult {
                core_family: CoreFamily::GameBoyAdvance,
                fixture_system: ActiveSystem::GameBoyAdvance,
                support: RunAheadSupport::Unsupported,
                reasons: GBA,
            },
            build: build_gba_backend,
            expected_local_failures: [PASS, PASS, AUDIO_CADENCE],
        },
        CoreCase {
            eligibility: EligibilityResult {
                core_family: CoreFamily::Nes,
                fixture_system: ActiveSystem::Nes,
                support: RunAheadSupport::Unsupported,
                reasons: NES,
            },
            build: build_nes_backend,
            expected_local_failures: [AUDIO_CADENCE, PASS, AUDIO_CADENCE],
        },
        CoreCase {
            eligibility: EligibilityResult {
                core_family: CoreFamily::ColecoVision,
                fixture_system: ActiveSystem::Coleco,
                support: RunAheadSupport::Unsupported,
                reasons: COLECO,
            },
            build: build_coleco_backend,
            expected_local_failures: [AUDIO_CADENCE, PASS, AUDIO_CADENCE],
        },
        CoreCase {
            eligibility: EligibilityResult {
                core_family: CoreFamily::PcEngine,
                fixture_system: ActiveSystem::Pce,
                support: RunAheadSupport::Unsupported,
                reasons: PCE,
            },
            build: build_pce_backend,
            expected_local_failures: [PASS, PASS, PASS],
        },
        CoreCase {
            eligibility: EligibilityResult {
                core_family: CoreFamily::Sega8,
                fixture_system: ActiveSystem::MasterSystem,
                support: RunAheadSupport::Unsupported,
                reasons: SEGA8,
            },
            build: build_sms_backend,
            expected_local_failures: [PASS, PASS, PASS],
        },
        CoreCase {
            eligibility: EligibilityResult {
                core_family: CoreFamily::WonderSwan,
                fixture_system: ActiveSystem::WonderSwan,
                support: RunAheadSupport::Unsupported,
                reasons: WS,
            },
            build: build_ws_backend,
            expected_local_failures: [AUDIO_CADENCE, PASS, AUDIO_CADENCE],
        },
    ];
    CASES
}
