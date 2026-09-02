use anyhow::Result;

use super::TasControlCoordinator;
use crate::emu_backend::ActiveSystem;
use crate::emu_thread::{EmuCommand, TasExecutionProfile, TasLoadedProfileObservation};
use crate::tas_project::{TasDigest, TasExternalIdentity, TasProjectIdentity};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) struct TasReadinessKey {
    pub(in crate::app) worker_generation: u64,
    pub(in crate::app) profile: TasExecutionProfile,
    pub(in crate::app) project_content_sha256: TasDigest,
    pub(in crate::app) configured_sample_rate: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TasReadinessRequest {
    id: u64,
    key: TasReadinessKey,
}

impl TasControlCoordinator {
    pub(in crate::app) fn begin_readiness_observation(
        &mut self,
        key: TasReadinessKey,
    ) -> Result<Option<EmuCommand>> {
        if self.readiness_key == Some(key)
            || self
                .pending_readiness_request
                .is_some_and(|request| request.key == key)
        {
            return Ok(None);
        }
        self.readiness_key = None;
        self.readiness_report = None;
        let request_id = self.next_readiness_request_id;
        self.next_readiness_request_id = request_id
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("TAS readiness request IDs are exhausted"))?;
        self.pending_readiness_request = Some(TasReadinessRequest {
            id: request_id,
            key,
        });
        Ok(Some(EmuCommand::InspectTasReadiness {
            request_id,
            profile: key.profile,
        }))
    }

    pub(in crate::app) fn accept_readiness_observation(
        &mut self,
        worker_generation: u64,
        request_id: u64,
        identity: &TasProjectIdentity,
        observation: &TasLoadedProfileObservation,
    ) -> bool {
        let Some(request) = self.pending_readiness_request else {
            return false;
        };
        if request.id != request_id
            || request.key.worker_generation != worker_generation
            || request.key.profile != observation.profile
        {
            return false;
        }
        self.pending_readiness_request = None;
        self.readiness_report = Some(evaluate(
            worker_generation,
            identity,
            observation,
            request.key.configured_sample_rate,
        ));
        self.readiness_key = Some(request.key);
        true
    }

    pub(in crate::app) fn readiness_report(&self) -> Option<&TasReadinessReport> {
        self.readiness_report.as_ref()
    }

    pub(in crate::app) fn clear_readiness(&mut self) {
        self.pending_readiness_request = None;
        self.readiness_key = None;
        self.readiness_report = None;
    }

    pub(in crate::app) fn readiness_pending(&self) -> bool {
        self.pending_readiness_request.is_some()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum TasReadinessStatus {
    Ready,
    ReloadRequired,
    Incompatible,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::app) enum TasReadinessCode {
    System,
    CoreIdentity,
    LoadProvenance,
    DirectSource,
    SourceMedia,
    EffectiveMedia,
    Mods,
    PersistentState,
    InitialInput,
    SampleRate,
    Firmware,
    Hardware,
    Controllers,
    RemovableMedia,
    Cheats,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::app) enum TasReadinessResource {
    RunningCore,
    LoadedMedia,
    LoadedProfile,
    ControllerTopology,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum TasReadinessRepair {
    None,
    ReloadLoadedGame,
    LoadMatchingGame,
    ResolveProfileMismatch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::app) enum TasReadinessValue {
    Text(String),
    Digest(TasDigest),
    Boolean(bool),
    SampleRateProfile { initial: u32, current: u32 },
    SampleRate(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::app) struct TasReadinessConditionId {
    pub(in crate::app) worker_generation: u64,
    pub(in crate::app) code: TasReadinessCode,
    pub(in crate::app) resource: TasReadinessResource,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::app) struct TasReadinessCheck {
    pub(in crate::app) id: TasReadinessConditionId,
    pub(in crate::app) expected: TasReadinessValue,
    pub(in crate::app) loaded: Option<TasReadinessValue>,
    pub(in crate::app) configured: Option<TasReadinessValue>,
    pub(in crate::app) status: TasReadinessStatus,
    pub(in crate::app) repair: TasReadinessRepair,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::app) struct TasReadinessReport {
    pub(in crate::app) worker_generation: u64,
    pub(in crate::app) profile: TasExecutionProfile,
    pub(in crate::app) status: TasReadinessStatus,
    pub(in crate::app) checks: Vec<TasReadinessCheck>,
}

pub(in crate::app) fn evaluate(
    worker_generation: u64,
    identity: &TasProjectIdentity,
    observation: &TasLoadedProfileObservation,
    configured_sample_rate: u32,
) -> TasReadinessReport {
    let mut checks = Vec::with_capacity(15);
    let expected_system = expected_system(observation.profile);
    push_boolean(
        &mut checks,
        worker_generation,
        (TasReadinessCode::System, TasReadinessResource::RunningCore),
        observation.system == expected_system,
        (
            TasReadinessValue::Text(identity.system.clone()),
            Some(TasReadinessValue::Text(
                observation.system.code().to_owned(),
            )),
        ),
        TasReadinessRepair::LoadMatchingGame,
    );
    push_boolean(
        &mut checks,
        worker_generation,
        (
            TasReadinessCode::CoreIdentity,
            TasReadinessResource::RunningCore,
        ),
        observation.identity_metadata_matches,
        (TasReadinessValue::Text(identity.core_family.clone()), None),
        TasReadinessRepair::LoadMatchingGame,
    );
    push_boolean(
        &mut checks,
        worker_generation,
        (
            TasReadinessCode::LoadProvenance,
            TasReadinessResource::LoadedProfile,
        ),
        observation.load_provenance_available,
        (
            TasReadinessValue::Boolean(true),
            Some(TasReadinessValue::Boolean(
                observation.load_provenance_available,
            )),
        ),
        TasReadinessRepair::ReloadLoadedGame,
    );
    let persistent_state_matches = if identity.rtc_state != TasExternalIdentity::Absent {
        Some(
            observation
                .project_owned_persistence
                .and_then(crate::emu_thread::TasPersistenceContract::rtc_identities)
                == Some((identity.persistent_state, identity.rtc_state)),
        )
    } else {
        match identity.persistent_state {
            TasExternalIdentity::Absent => observation.persistent_state_absent,
            TasExternalIdentity::ExternalSha256(expected) => Some(
                observation
                    .project_owned_persistence
                    .and_then(|persistence| persistence.initial_sha256())
                    == Some(expected),
            ),
        }
    };
    push_optional_boolean(
        &mut checks,
        worker_generation,
        TasReadinessCode::DirectSource,
        TasReadinessResource::LoadedMedia,
        observation.direct_source,
        TasReadinessRepair::LoadMatchingGame,
    );
    push_digest(
        &mut checks,
        worker_generation,
        TasReadinessCode::SourceMedia,
        identity.source_media_sha256,
        observation.source_media_sha256,
    );
    push_digest(
        &mut checks,
        worker_generation,
        TasReadinessCode::EffectiveMedia,
        identity.effective_media_sha256,
        observation.effective_media_sha256,
    );
    push_optional_boolean(
        &mut checks,
        worker_generation,
        TasReadinessCode::Mods,
        TasReadinessResource::LoadedProfile,
        observation.mods_absent,
        TasReadinessRepair::ResolveProfileMismatch,
    );
    push_optional_boolean(
        &mut checks,
        worker_generation,
        TasReadinessCode::PersistentState,
        TasReadinessResource::LoadedProfile,
        persistent_state_matches,
        if matches!(
            identity.persistent_state,
            TasExternalIdentity::ExternalSha256(_)
        ) {
            TasReadinessRepair::ReloadLoadedGame
        } else {
            TasReadinessRepair::ResolveProfileMismatch
        },
    );
    push_optional_boolean(
        &mut checks,
        worker_generation,
        TasReadinessCode::InitialInput,
        TasReadinessResource::LoadedProfile,
        observation.initial_input_neutral,
        TasReadinessRepair::ReloadLoadedGame,
    );

    let required_sample_rate = 48_000;
    let load_sample_rate_matches = match observation.profile {
        TasExecutionProfile::DirectNesCartridge => observation
            .configured_at_load_sample_rate
            .is_none_or(|rate| rate == required_sample_rate),
        TasExecutionProfile::DirectFdsDisk => {
            observation.configured_at_load_sample_rate == Some(required_sample_rate)
        }
        TasExecutionProfile::DirectGbCartridgeDmg | TasExecutionProfile::DirectGbCartridgeCgb => {
            observation.configured_at_load_sample_rate.is_none()
        }
        TasExecutionProfile::DirectColecoCartridge => {
            observation.configured_at_load_sample_rate.is_none()
        }
        TasExecutionProfile::DirectSmsCartridge => {
            observation.configured_at_load_sample_rate.is_none()
        }
        TasExecutionProfile::DirectGameGearCartridge => {
            observation.configured_at_load_sample_rate.is_none()
        }
        TasExecutionProfile::DirectGbaCartridge => {
            observation.configured_at_load_sample_rate == Some(required_sample_rate)
        }
        TasExecutionProfile::DirectSg1000Cartridge => {
            observation.configured_at_load_sample_rate.is_none()
        }
        TasExecutionProfile::DirectWsCartridge => {
            observation.configured_at_load_sample_rate.is_none()
        }
        TasExecutionProfile::DirectPceHuCard
        | TasExecutionProfile::DirectPceSixButtonHuCard
        | TasExecutionProfile::DirectPceCd => {
            observation.configured_at_load_sample_rate == Some(required_sample_rate)
        }
    };
    let sample_rate_matches = load_sample_rate_matches
        && observation.initial_sample_rate == Some(required_sample_rate)
        && observation.current_sample_rate == Some(required_sample_rate);
    let sample_status = if sample_rate_matches {
        TasReadinessStatus::Ready
    } else if configured_sample_rate == required_sample_rate {
        TasReadinessStatus::ReloadRequired
    } else {
        TasReadinessStatus::Incompatible
    };
    checks.push(TasReadinessCheck {
        id: TasReadinessConditionId {
            worker_generation,
            code: TasReadinessCode::SampleRate,
            resource: TasReadinessResource::LoadedProfile,
        },
        expected: TasReadinessValue::SampleRate(required_sample_rate),
        loaded: observation
            .initial_sample_rate
            .zip(observation.current_sample_rate)
            .map(|(initial, current)| TasReadinessValue::SampleRateProfile { initial, current }),
        configured: Some(TasReadinessValue::SampleRate(configured_sample_rate)),
        status: sample_status,
        repair: if sample_rate_matches {
            TasReadinessRepair::None
        } else if sample_status == TasReadinessStatus::ReloadRequired {
            TasReadinessRepair::ReloadLoadedGame
        } else {
            TasReadinessRepair::ResolveProfileMismatch
        },
    });

    let removable_media_matches = if observation.profile == TasExecutionProfile::DirectPceCd {
        !observation.removable_media_absent
    } else {
        observation.removable_media_absent
    };
    for (code, resource, matches, repair) in [
        (
            TasReadinessCode::Firmware,
            TasReadinessResource::LoadedProfile,
            observation.firmware_profile_matches,
            TasReadinessRepair::ResolveProfileMismatch,
        ),
        (
            TasReadinessCode::Hardware,
            TasReadinessResource::LoadedProfile,
            observation.hardware_profile_matches,
            TasReadinessRepair::ResolveProfileMismatch,
        ),
        (
            TasReadinessCode::Controllers,
            TasReadinessResource::ControllerTopology,
            observation.controller_profile_matches,
            TasReadinessRepair::ResolveProfileMismatch,
        ),
        (
            TasReadinessCode::RemovableMedia,
            TasReadinessResource::LoadedMedia,
            removable_media_matches,
            TasReadinessRepair::ResolveProfileMismatch,
        ),
        (
            TasReadinessCode::Cheats,
            TasReadinessResource::LoadedProfile,
            observation.cheats_absent,
            TasReadinessRepair::ResolveProfileMismatch,
        ),
    ] {
        push_boolean(
            &mut checks,
            worker_generation,
            (code, resource),
            matches,
            (
                TasReadinessValue::Boolean(true),
                Some(TasReadinessValue::Boolean(matches)),
            ),
            repair,
        );
    }

    let status = if checks
        .iter()
        .any(|check| check.status == TasReadinessStatus::Incompatible)
    {
        TasReadinessStatus::Incompatible
    } else if checks
        .iter()
        .any(|check| check.status == TasReadinessStatus::ReloadRequired)
    {
        TasReadinessStatus::ReloadRequired
    } else {
        TasReadinessStatus::Ready
    };
    TasReadinessReport {
        worker_generation,
        profile: observation.profile,
        status,
        checks,
    }
}

#[cfg(test)]
pub(in crate::app) fn evaluate_for_test(
    worker_generation: u64,
    identity: &TasProjectIdentity,
    observation: &TasLoadedProfileObservation,
    configured_sample_rate: u32,
) -> TasReadinessReport {
    evaluate(
        worker_generation,
        identity,
        observation,
        configured_sample_rate,
    )
}

fn expected_system(profile: TasExecutionProfile) -> ActiveSystem {
    match profile {
        TasExecutionProfile::DirectNesCartridge | TasExecutionProfile::DirectFdsDisk => {
            ActiveSystem::Nes
        }
        TasExecutionProfile::DirectGbCartridgeDmg | TasExecutionProfile::DirectGbCartridgeCgb => {
            ActiveSystem::GameBoy
        }
        TasExecutionProfile::DirectColecoCartridge => ActiveSystem::Coleco,
        TasExecutionProfile::DirectSmsCartridge => ActiveSystem::MasterSystem,
        TasExecutionProfile::DirectGameGearCartridge => ActiveSystem::GameGear,
        TasExecutionProfile::DirectGbaCartridge => ActiveSystem::GameBoyAdvance,
        TasExecutionProfile::DirectSg1000Cartridge => ActiveSystem::Sg1000,
        TasExecutionProfile::DirectWsCartridge => ActiveSystem::WonderSwan,
        TasExecutionProfile::DirectPceHuCard
        | TasExecutionProfile::DirectPceSixButtonHuCard
        | TasExecutionProfile::DirectPceCd => ActiveSystem::Pce,
    }
}

fn push_optional_boolean(
    checks: &mut Vec<TasReadinessCheck>,
    worker_generation: u64,
    code: TasReadinessCode,
    resource: TasReadinessResource,
    loaded: Option<bool>,
    repair: TasReadinessRepair,
) {
    if let Some(loaded) = loaded {
        push_boolean(
            checks,
            worker_generation,
            (code, resource),
            loaded,
            (
                TasReadinessValue::Boolean(true),
                Some(TasReadinessValue::Boolean(loaded)),
            ),
            repair,
        );
    }
}

fn push_digest(
    checks: &mut Vec<TasReadinessCheck>,
    worker_generation: u64,
    code: TasReadinessCode,
    expected: TasDigest,
    loaded: Option<TasDigest>,
) {
    let Some(loaded) = loaded else {
        return;
    };
    let matches = loaded == expected;
    push_boolean(
        checks,
        worker_generation,
        (code, TasReadinessResource::LoadedMedia),
        matches,
        (
            TasReadinessValue::Digest(expected),
            Some(TasReadinessValue::Digest(loaded)),
        ),
        TasReadinessRepair::LoadMatchingGame,
    );
}

fn push_boolean(
    checks: &mut Vec<TasReadinessCheck>,
    worker_generation: u64,
    identity: (TasReadinessCode, TasReadinessResource),
    matches: bool,
    values: (TasReadinessValue, Option<TasReadinessValue>),
    repair: TasReadinessRepair,
) {
    let (code, resource) = identity;
    let (expected, loaded) = values;
    checks.push(TasReadinessCheck {
        id: TasReadinessConditionId {
            worker_generation,
            code,
            resource,
        },
        expected,
        loaded,
        configured: None,
        status: if matches {
            TasReadinessStatus::Ready
        } else if repair == TasReadinessRepair::ReloadLoadedGame {
            TasReadinessStatus::ReloadRequired
        } else {
            TasReadinessStatus::Incompatible
        },
        repair: if matches {
            TasReadinessRepair::None
        } else {
            repair
        },
    });
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use anyhow::Result;

    use super::*;
    use crate::emu_backend::loader::DirectNesTasExecutionLoader;

    fn fixture() -> Result<(crate::tas_project::TasProject, TasLoadedProfileObservation)> {
        let directory = crate::test_support::test_directory("tas-readiness")?;
        let source_path = directory.path().join("game.nes");
        std::fs::write(&source_path, crate::test_support::build_nes_test_rom())?;
        let project = DirectNesTasExecutionLoader::new(source_path, Vec::new()).create_project()?;
        let identity = project.identity();
        let observation = TasLoadedProfileObservation {
            profile: TasExecutionProfile::DirectNesCartridge,
            system: ActiveSystem::Nes,
            identity_metadata_matches: true,
            load_provenance_available: true,
            direct_source: Some(true),
            source_media_sha256: Some(identity.source_media_sha256),
            effective_media_sha256: Some(identity.effective_media_sha256),
            mods_absent: Some(true),
            persistent_state_absent: Some(true),
            project_owned_persistence: None,
            initial_input_neutral: Some(true),
            configured_at_load_sample_rate: None,
            initial_sample_rate: Some(48_000),
            current_sample_rate: Some(48_000),
            firmware_profile_matches: true,
            hardware_profile_matches: true,
            controller_profile_matches: true,
            removable_media_absent: true,
            cheats_absent: true,
        };
        Ok((project, observation))
    }

    #[test]
    fn exact_profile_is_ready() -> Result<()> {
        let (project, observation) = fixture()?;
        let report = evaluate(7, project.identity(), &observation, 48_000);

        assert_eq!(report.status, TasReadinessStatus::Ready);
        assert!(
            report
                .checks
                .iter()
                .all(|check| check.status == TasReadinessStatus::Ready)
        );
        Ok(())
    }

    #[test]
    fn coleco_profile_uses_the_direct_default_rate_readiness_rule() -> Result<()> {
        let (project, mut observation) = fixture()?;
        let mut identity = project.identity().clone();
        identity.system = ActiveSystem::Coleco.code().to_owned();
        observation.profile = TasExecutionProfile::DirectColecoCartridge;
        observation.system = ActiveSystem::Coleco;
        let report = evaluate(7, &identity, &observation, 48_000);

        assert_eq!(report.status, TasReadinessStatus::Ready);
        assert_eq!(report.profile, TasExecutionProfile::DirectColecoCartridge);
        assert!(
            report
                .checks
                .iter()
                .all(|check| check.status == TasReadinessStatus::Ready)
        );
        Ok(())
    }

    #[test]
    fn master_system_profile_uses_the_direct_default_rate_readiness_rule() -> Result<()> {
        let (project, mut observation) = fixture()?;
        let mut identity = project.identity().clone();
        identity.system = ActiveSystem::MasterSystem.code().to_owned();
        observation.profile = TasExecutionProfile::DirectSmsCartridge;
        observation.system = ActiveSystem::MasterSystem;
        let report = evaluate(7, &identity, &observation, 48_000);

        assert_eq!(report.status, TasReadinessStatus::Ready);
        assert_eq!(report.profile, TasExecutionProfile::DirectSmsCartridge);
        assert!(
            report
                .checks
                .iter()
                .all(|check| check.status == TasReadinessStatus::Ready)
        );
        Ok(())
    }

    #[test]
    fn sg1000_profile_uses_the_direct_default_rate_readiness_rule() -> Result<()> {
        let (project, mut observation) = fixture()?;
        let mut identity = project.identity().clone();
        identity.system = ActiveSystem::Sg1000.code().to_owned();
        observation.profile = TasExecutionProfile::DirectSg1000Cartridge;
        observation.system = ActiveSystem::Sg1000;
        let report = evaluate(7, &identity, &observation, 48_000);

        assert_eq!(report.status, TasReadinessStatus::Ready);
        assert_eq!(report.profile, TasExecutionProfile::DirectSg1000Cartridge);
        assert!(
            report
                .checks
                .iter()
                .all(|check| check.status == TasReadinessStatus::Ready)
        );
        Ok(())
    }

    #[test]
    fn game_gear_profile_uses_the_direct_default_rate_readiness_rule() -> Result<()> {
        let (project, mut observation) = fixture()?;
        let mut identity = project.identity().clone();
        identity.system = ActiveSystem::GameGear.code().to_owned();
        observation.profile = TasExecutionProfile::DirectGameGearCartridge;
        observation.system = ActiveSystem::GameGear;
        let report = evaluate(7, &identity, &observation, 48_000);

        assert_eq!(report.status, TasReadinessStatus::Ready);
        assert_eq!(report.profile, TasExecutionProfile::DirectGameGearCartridge);
        assert!(
            report
                .checks
                .iter()
                .all(|check| check.status == TasReadinessStatus::Ready)
        );
        Ok(())
    }

    #[test]
    fn wonderswan_profile_uses_the_direct_default_rate_readiness_rule() -> Result<()> {
        let (project, mut observation) = fixture()?;
        let mut identity = project.identity().clone();
        identity.system = ActiveSystem::WonderSwan.code().to_owned();
        observation.profile = TasExecutionProfile::DirectWsCartridge;
        observation.system = ActiveSystem::WonderSwan;
        let report = evaluate(7, &identity, &observation, 48_000);

        assert_eq!(report.status, TasReadinessStatus::Ready);
        assert_eq!(report.profile, TasExecutionProfile::DirectWsCartridge);
        assert!(
            report
                .checks
                .iter()
                .all(|check| check.status == TasReadinessStatus::Ready)
        );
        Ok(())
    }

    #[test]
    fn construction_rate_mismatch_with_matching_setting_requires_reload() -> Result<()> {
        let (project, mut observation) = fixture()?;
        observation.configured_at_load_sample_rate = Some(44_100);
        observation.initial_sample_rate = Some(44_100);
        let report = evaluate(7, project.identity(), &observation, 48_000);
        let check = report
            .checks
            .iter()
            .find(|check| check.id.code == TasReadinessCode::SampleRate)
            .unwrap();

        assert_eq!(report.status, TasReadinessStatus::ReloadRequired);
        assert_eq!(check.status, TasReadinessStatus::ReloadRequired);
        assert_eq!(
            check.loaded,
            Some(TasReadinessValue::SampleRateProfile {
                initial: 44_100,
                current: 48_000
            })
        );
        assert_eq!(
            check.configured,
            Some(TasReadinessValue::SampleRate(48_000))
        );
        assert_eq!(check.repair, TasReadinessRepair::ReloadLoadedGame);
        Ok(())
    }

    #[test]
    fn hard_mismatch_wins_over_reload_and_reports_all_conditions() -> Result<()> {
        let (project, mut observation) = fixture()?;
        observation.initial_sample_rate = Some(44_100);
        observation.source_media_sha256 = Some(TasDigest([9; 32]));
        let report = evaluate(7, project.identity(), &observation, 48_000);

        assert_eq!(report.status, TasReadinessStatus::Incompatible);
        assert!(report.checks.iter().any(|check| {
            check.id.code == TasReadinessCode::SampleRate
                && check.status == TasReadinessStatus::ReloadRequired
        }));
        assert!(report.checks.iter().any(|check| {
            check.id.code == TasReadinessCode::SourceMedia
                && check.status == TasReadinessStatus::Incompatible
        }));
        Ok(())
    }

    #[test]
    fn missing_provenance_is_reloadable_without_unknown_media_cascade() -> Result<()> {
        let (project, mut observation) = fixture()?;
        observation.load_provenance_available = false;
        observation.direct_source = None;
        observation.source_media_sha256 = None;
        observation.mods_absent = None;
        observation.persistent_state_absent = None;
        observation.initial_input_neutral = None;
        observation.initial_sample_rate = None;
        observation.current_sample_rate = None;
        let report = evaluate(7, project.identity(), &observation, 48_000);

        assert_eq!(report.status, TasReadinessStatus::ReloadRequired);
        assert!(report.checks.iter().any(|check| {
            check.id.code == TasReadinessCode::LoadProvenance
                && check.status == TasReadinessStatus::ReloadRequired
        }));
        assert!(
            !report
                .checks
                .iter()
                .any(|check| check.id.code == TasReadinessCode::SourceMedia)
        );
        Ok(())
    }

    #[test]
    fn non_direct_source_requires_matching_media_action() -> Result<()> {
        let (project, mut observation) = fixture()?;
        observation.direct_source = Some(false);
        let report = evaluate(7, project.identity(), &observation, 48_000);
        let check = report
            .checks
            .iter()
            .find(|check| check.id.code == TasReadinessCode::DirectSource)
            .unwrap();

        assert_eq!(report.status, TasReadinessStatus::Incompatible);
        assert_eq!(check.status, TasReadinessStatus::Incompatible);
        assert_eq!(check.repair, TasReadinessRepair::LoadMatchingGame);
        Ok(())
    }

    #[test]
    fn condition_identity_is_stable_within_worker_generation() -> Result<()> {
        let (project, observation) = fixture()?;
        let first = evaluate(7, project.identity(), &observation, 48_000);
        let repeated = evaluate(7, project.identity(), &observation, 48_000);
        let replacement = evaluate(8, project.identity(), &observation, 48_000);
        let ids = first
            .checks
            .iter()
            .map(|check| check.id)
            .collect::<BTreeSet<_>>();

        assert_eq!(first, repeated);
        assert_eq!(ids.len(), first.checks.len());
        assert!(
            first
                .checks
                .iter()
                .zip(&replacement.checks)
                .all(|(old, new)| {
                    old.id.code == new.id.code
                        && old.id.resource == new.id.resource
                        && old.id.worker_generation != new.id.worker_generation
                })
        );
        Ok(())
    }
}
