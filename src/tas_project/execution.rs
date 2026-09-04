#![cfg(not(target_arch = "wasm32"))]
#![allow(dead_code)]

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use zeff_emu_common::replay::{
    POCKET_CAMERA_FRAME_BYTES, ReplayEvent, ReplayJoypadFrame, ReplayZapperFrame,
};

use crate::emu_backend::EmuBackend;
use crate::replay_execution::apply_immediate_replay_event;

use super::editor_session::TasEditorSession;
use super::model::{
    TasBranch, TasCameraInput, TasDigest, TasInputFrame, TasProject, TasProjectIdentity,
    TasSeekCacheIdentity, TasVerificationProvenance,
};
use super::verification::TasExecutionSession;

pub(crate) trait TasEditorExecutionProvider {
    fn load_editor_engine(&self, project: &TasProject) -> Result<TasEditorExecutionEngine>;
}

pub(crate) enum TasEditorExecutionAttachment {
    Available(Box<dyn TasEditorExecutionProvider>),
    Unavailable(TasEditorExecutionUnavailableReason),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TasEditorExecutionUnavailableReason {
    NoRunningEmulator,
    UnsupportedSystem(String),
    UnsupportedMedia(String),
}

impl std::fmt::Display for TasEditorExecutionUnavailableReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoRunningEmulator => formatter.write_str("no emulator is running"),
            Self::UnsupportedSystem(system) => {
                write!(formatter, "{system} does not have a TAS execution profile")
            }
            Self::UnsupportedMedia(reason) => formatter.write_str(reason),
        }
    }
}

pub(crate) const MAX_EDITOR_SEEK_EXECUTION_FRAMES: u64 = 600;
const MAX_EDITOR_SEEK_CACHE_RESTORE_ATTEMPTS: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TasEditorFramebuffer {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl TasEditorFramebuffer {
    pub(crate) fn from_rgba(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self> {
        let expected = u64::from(width)
            .checked_mul(u64::from(height))
            .and_then(|pixels| pixels.checked_mul(4))
            .and_then(|bytes| usize::try_from(bytes).ok())
            .ok_or_else(|| anyhow::anyhow!("TAS framebuffer dimensions overflow"))?;
        anyhow::ensure!(
            rgba.len() == expected,
            "TAS framebuffer size does not match its dimensions"
        );
        Ok(Self {
            width,
            height,
            rgba,
        })
    }

    pub(crate) fn width(&self) -> u32 {
        self.width
    }

    pub(crate) fn height(&self) -> u32 {
        self.height
    }

    pub(crate) fn rgba(&self) -> &[u8] {
        &self.rgba
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TasEditorExecutionOutcome {
    pub(crate) requested_cursor: u64,
    pub(crate) cursor: u64,
    pub(crate) restored_cache_cursor: Option<u64>,
    pub(crate) executed_frames: u64,
    pub(crate) events_applied: usize,
    pub(crate) framebuffer: TasEditorFramebuffer,
    pub(crate) cache_store_error: Option<String>,
}

impl TasEditorExecutionOutcome {
    pub(crate) fn remaining_frames(&self) -> u64 {
        self.requested_cursor - self.cursor
    }

    pub(crate) fn reached_target(&self) -> bool {
        self.cursor == self.requested_cursor
    }
}

pub(crate) struct TasEditorExecutionEngine {
    backend: EmuBackend,
    identity: TasProjectIdentity,
    scope_validator: Option<TasEditorExecutionScopeValidator>,
    seek_identity_scope: Option<(TasDigest, String)>,
    seek_identity_memo: BTreeMap<u64, TasSeekCacheIdentity>,
}

pub(crate) type TasEditorExecutionScopeValidator = fn(&TasProject, &str) -> Result<()>;

impl TasEditorExecutionEngine {
    pub(crate) fn attach(
        project: &TasProject,
        session: TasExecutionSession,
        scope_validator: TasEditorExecutionScopeValidator,
    ) -> Result<Self> {
        project.validate_execution_identity(session.identity(), "editor execution attachment")?;
        let mut engine = Self::new(session);
        engine.scope_validator = Some(scope_validator);
        Ok(engine)
    }

    pub(crate) fn new(session: TasExecutionSession) -> Self {
        let (backend, identity) = session.into_parts();
        Self {
            backend,
            identity,
            scope_validator: None,
            seek_identity_scope: None,
            seek_identity_memo: BTreeMap::new(),
        }
    }

    pub(crate) fn validate_editor_session(&self, editor: &TasEditorSession) -> Result<()> {
        let project = editor.project();
        project.validate_execution_identity(&self.identity, "editor execution session")?;
        let branch_id = editor.selected_branch_id();
        if let Some(validate_scope) = self.scope_validator {
            validate_scope(project, branch_id)
                .context("selected TAS branch is outside the attached editor execution profile")?;
        }
        let branch = project
            .branch(branch_id)
            .expect("validated editor selection must name a branch");
        ensure_single_endpoint_profile(project, branch)
    }

    pub(crate) fn backend(&self) -> &EmuBackend {
        &self.backend
    }

    pub(crate) fn into_backend(self) -> EmuBackend {
        self.backend
    }

    pub(crate) fn seek(
        &mut self,
        editor: &mut TasEditorSession,
        target_cursor: u64,
    ) -> Result<TasEditorExecutionOutcome> {
        self.seek_with_frame_budget(editor, target_cursor, MAX_EDITOR_SEEK_EXECUTION_FRAMES)
    }

    fn seek_with_frame_budget(
        &mut self,
        editor: &mut TasEditorSession,
        target_cursor: u64,
        frame_budget: u64,
    ) -> Result<TasEditorExecutionOutcome> {
        if frame_budget == 0 {
            bail!("TAS editor execution frame budget must be nonzero");
        }
        self.validate_editor_session(editor)?;
        let project = editor.project();
        let branch_id = editor.selected_branch_id().to_owned();
        let branch = project
            .branch(&branch_id)
            .expect("validated editor selection must name a branch");
        if target_cursor > branch.frame_count() {
            bail!("TAS editor seek target is past selected branch end");
        }
        let (start_cursor, restored_cache_cursor) =
            self.restore_newest_state(editor, target_cursor, branch.verification())?;
        let reached_cursor = target_cursor.min(start_cursor.saturating_add(frame_budget));
        let mut events_applied = 0usize;
        let mut event_index = branch
            .events()
            .partition_point(|event| event.frame() < start_cursor);
        for cursor in start_cursor..reached_cursor {
            while let Some(event) = branch.events().get(event_index)
                && event.frame() == cursor
            {
                events_applied +=
                    usize::from(apply_immediate_replay_event(&mut self.backend, event)?);
                event_index += 1;
            }
            let input = branch.input_at(cursor);
            if self.backend.system() == crate::emu_backend::ActiveSystem::Coleco {
                self.backend.apply_coleco_tas_input(input.coleco)?;
            } else {
                let frame = materialize_input(project, input)?;
                self.backend.apply_replay_input(&frame);
            }
            let frame_count_before = self.backend.frame_count();
            self.backend.step_frame();
            if self.backend.frame_count() == frame_count_before {
                bail!("TAS editor execution made no frame progress");
            }
            validate_project_checkpoint(branch.verification(), cursor + 1, &self.backend)?;
        }

        // Cache before terminal events so resuming applies them exactly once.
        let state = self
            .backend
            .encode_state_bytes()
            .context("failed to encode exact TAS editor cursor state")?;
        if reached_cursor == target_cursor && reached_cursor == branch.frame_count() {
            while let Some(event) = branch.events().get(event_index)
                && event.frame() == reached_cursor
            {
                events_applied +=
                    usize::from(apply_immediate_replay_event(&mut self.backend, event)?);
                event_index += 1;
            }
        }
        let framebuffer = capture_framebuffer(&self.backend)?;
        editor.set_cursor(reached_cursor)?;
        let cache_store_error = self
            .store_memoized_seek_state(editor, &state)
            .err()
            .map(|error| error.to_string());

        Ok(TasEditorExecutionOutcome {
            requested_cursor: target_cursor,
            cursor: reached_cursor,
            restored_cache_cursor,
            executed_frames: reached_cursor - start_cursor,
            events_applied,
            framebuffer,
            cache_store_error,
        })
    }

    fn restore_newest_state(
        &mut self,
        editor: &TasEditorSession,
        target_cursor: u64,
        verification: Option<&TasVerificationProvenance>,
    ) -> Result<(u64, Option<u64>)> {
        let mut cache_ceiling = Some(target_cursor);
        for _ in 0..MAX_EDITOR_SEEK_CACHE_RESTORE_ATTEMPTS {
            let Some(ceiling) = cache_ceiling else {
                break;
            };
            let Some((cursor, state)) = self.load_memoized_seek_state(editor, ceiling)? else {
                break;
            };
            if self.backend.load_state_from_bytes(state).is_ok() {
                self.restore_materialized_camera_input(editor, cursor);
                if validate_project_checkpoint(verification, cursor, &self.backend).is_ok() {
                    return Ok((cursor, Some(cursor)));
                }
            }
            cache_ceiling = cursor.checked_sub(1);
        }

        self.backend
            .load_state_from_bytes(editor.project().start_state().to_vec())
            .context("failed to restore exact TAS project start state")?;
        self.restore_materialized_camera_input(editor, 0);
        validate_project_checkpoint(verification, 0, &self.backend)?;
        Ok((0, None))
    }

    fn load_memoized_seek_state(
        &mut self,
        editor: &TasEditorSession,
        target_cursor: u64,
    ) -> Result<Option<(u64, Vec<u8>)>> {
        let scope = (
            editor.project_content_sha256(),
            editor.selected_branch_id().to_owned(),
        );
        if self.seek_identity_scope.as_ref() != Some(&scope) {
            self.seek_identity_scope = Some(scope.clone());
            self.seek_identity_memo.clear();
        }
        let project = editor.project();
        let branch_id = scope.1;
        let memo = &mut self.seek_identity_memo;
        editor
            .seek_cache()
            .load_newest_matching(target_cursor, |cursor| {
                if let Some(identity) = memo.get(&cursor) {
                    return Ok(identity.clone());
                }
                let identity = project.seek_cache_identity(&branch_id, cursor)?;
                memo.insert(cursor, identity.clone());
                Ok(identity)
            })
    }

    fn store_memoized_seek_state(&mut self, editor: &TasEditorSession, state: &[u8]) -> Result<()> {
        let scope = (
            editor.project_content_sha256(),
            editor.selected_branch_id().to_owned(),
        );
        if self.seek_identity_scope.as_ref() != Some(&scope) {
            self.seek_identity_scope = Some(scope.clone());
            self.seek_identity_memo.clear();
        }
        let cursor = editor.cursor();
        let identity = match self.seek_identity_memo.get(&cursor) {
            Some(identity) => identity.clone(),
            None => {
                let identity = editor.project().seek_cache_identity(&scope.1, cursor)?;
                self.seek_identity_memo.insert(cursor, identity.clone());
                identity
            }
        };
        editor.seek_cache().store(&identity, state)
    }

    fn restore_materialized_camera_input(&mut self, editor: &TasEditorSession, cursor: u64) {
        if !self.backend.is_pocket_camera() {
            return;
        }
        let branch = editor.selected_branch();
        let latest = branch.input_spans().iter().rev().find_map(|span| {
            (span.start < cursor)
                .then_some(span.input.camera)
                .and_then(|camera| match camera {
                    TasCameraInput::None => None,
                    TasCameraInput::Blob(digest) => Some(digest),
                })
        });
        let frame = match latest {
            Some(digest) => editor
                .project()
                .assets()
                .get(&digest)
                .expect("validated TAS camera asset must exist")
                .clone(),
            None => vec![0xFF; POCKET_CAMERA_FRAME_BYTES],
        };
        self.backend.set_replay_camera_frame(&frame);
    }
}

fn ensure_single_endpoint_profile(project: &TasProject, branch: &TasBranch) -> Result<()> {
    let replay_start = project.replay_start();
    if replay_start.game_boy_link_state.is_some()
        || replay_start.game_boy_link_tick.is_some()
        || replay_start.game_boy_link_coordinator_state.is_some()
        || replay_start.wonder_swan_link_tick.is_some()
        || branch.events().iter().any(|event| {
            matches!(
                event,
                ReplayEvent::GameBoyLink { .. }
                    | ReplayEvent::GameBoyLinkState { .. }
                    | ReplayEvent::GameBoyLinkStateAtTick { .. }
                    | ReplayEvent::WonderSwanLink { .. }
            )
        })
    {
        bail!(
            "native TAS editor execution currently supports one emulator endpoint only; synchronized Game Boy and WonderSwan link domains are unsupported"
        );
    }
    Ok(())
}

fn materialize_input(project: &TasProject, frame: TasInputFrame) -> Result<ReplayJoypadFrame> {
    let camera_frame = match frame.camera {
        TasCameraInput::None => None,
        TasCameraInput::Blob(digest) => Some(
            project
                .assets()
                .get(&digest)
                .ok_or_else(|| anyhow::anyhow!("TAS input references a missing camera asset"))?
                .clone(),
        ),
    };
    Ok(ReplayJoypadFrame {
        buttons: frame.players[0].buttons,
        dpad: frame.players[0].dpad,
        buttons_p2: frame.players[1].buttons,
        dpad_p2: frame.players[1].dpad,
        buttons_p3: frame.players[2].buttons,
        dpad_p3: frame.players[2].dpad,
        buttons_p4: frame.players[3].buttons,
        dpad_p4: frame.players[3].dpad,
        buttons_p5: frame.players[4].buttons,
        dpad_p5: frame.players[4].dpad,
        zapper: ReplayZapperFrame {
            enabled: frame.zapper.enabled,
            trigger: frame.zapper.trigger,
            hit: frame.zapper.hit,
            screen_pos: frame.zapper.screen_pos.map(|[x, y]| (x, y)),
        },
        host_tilt: (
            f32::from_bits(frame.tilt_x_bits),
            f32::from_bits(frame.tilt_y_bits),
        ),
        camera_frame,
        coleco: Default::default(),
    })
}

fn validate_project_checkpoint(
    verification: Option<&TasVerificationProvenance>,
    cursor: u64,
    backend: &EmuBackend,
) -> Result<()> {
    let Some(verification) = verification else {
        return Ok(());
    };
    let first = verification
        .checkpoints
        .partition_point(|checkpoint| checkpoint.cursor < cursor);
    for checkpoint in verification.checkpoints[first..]
        .iter()
        .take_while(|checkpoint| checkpoint.cursor == cursor)
    {
        let actual =
            super::model::TasDigest::from_bytes(&backend.encode_replay_hash_state_bytes()?);
        if actual != checkpoint.state_sha256 {
            bail!(
                "TAS editor execution diverged at checkpoint cursor {cursor}: expected {}, got {}",
                const_hex::encode(checkpoint.state_sha256.0),
                const_hex::encode(actual.0)
            );
        }
    }
    Ok(())
}

fn capture_framebuffer(backend: &EmuBackend) -> Result<TasEditorFramebuffer> {
    let (width, height) = backend.system().screen_size();
    let rgba = backend.framebuffer().to_vec();
    if rgba.len() != backend.system().framebuffer_len() {
        bail!(
            "private TAS framebuffer has {} bytes; expected {} for {}x{} RGBA",
            rgba.len(),
            backend.system().framebuffer_len(),
            width,
            height
        );
    }
    TasEditorFramebuffer::from_rgba(width, height, rgba)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    use zeff_emu_common::media::MediaEvent;
    use zeff_emu_common::replay::{
        ReplayEvent, ReplayFirmwareManifest, ReplayStartMetadata, ReplayWonderSwanLinkEvent,
    };

    use super::*;
    use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};
    use crate::tas_project::{
        TasAutosaveConfig, TasAutosaveStore, TasControllerInput, TasDeviceIdentity, TasDigest,
        TasExternalIdentity, TasFirmwareIdentity, TasInitialBranch, TasInputFrame, TasInputSpan,
        TasProject, TasProjectIdentity, TasSeekStateCache, TasZapperInput,
    };

    fn nes_backend() -> EmuBackend {
        let rom = crate::test_support::build_nes_test_rom();
        let emu = zeff_nes_core::emulator::Emulator::new(&rom, 44_100.0)
            .expect("synthetic NES emulator should initialize");
        EmuBackend::from_nes(emu, PathBuf::from("synthetic.nes"))
    }

    fn fds_backend() -> EmuBackend {
        static BIOS: [u8; zeff_nes_core::hardware::cartridge::mappers::FDS_BIOS_SIZE] =
            [0xFF; zeff_nes_core::hardware::cartridge::mappers::FDS_BIOS_SIZE];
        let side_size = zeff_nes_core::hardware::cartridge::mappers::FDS_SIDE_SIZE;
        let mut disk = vec![0xA1; side_size];
        disk.resize(side_size * 2, 0xB2);
        load_backend_from_rom_source(
            ActiveSystem::Nes,
            Path::new("synthetic.fds"),
            Path::new("synthetic.fds"),
            Some(disk),
            BackendLoadConfig {
                fds_bios_override: Some(&BIOS),
                ..BackendLoadConfig::default()
            },
        )
        .expect("synthetic FDS backend should initialize")
        .backend
    }

    fn project_for_backend(
        backend: &EmuBackend,
        frame_count: u64,
        input_spans: Vec<TasInputSpan>,
        events: Vec<ReplayEvent>,
    ) -> (TasProject, TasProjectIdentity) {
        let start_state = backend
            .encode_state_bytes()
            .expect("synthetic backend should encode state");
        let metadata = backend.replay_metadata();
        let effective_media_sha256 = TasDigest(
            metadata
                .rom_sha256
                .expect("synthetic backend should expose its media hash"),
        );
        let identity = TasProjectIdentity {
            system: metadata
                .system
                .expect("synthetic backend should expose its system"),
            core_family: metadata
                .core_family
                .expect("synthetic backend should expose its core family"),
            determinism_abi: "synthetic-editor-execution-v1".to_owned(),
            source_media_sha256: effective_media_sha256,
            effective_media_sha256,
            patches: Vec::new(),
            firmware: metadata
                .firmware
                .iter()
                .map(tas_firmware_identity)
                .collect(),
            devices: (1..=5)
                .map(|player| TasDeviceIdentity {
                    port: format!("p{player}"),
                    device: "materialized-test-input".to_owned(),
                    configuration_sha256: TasDigest([player; 32]),
                })
                .collect(),
            sync_config_sha256: TasDigest([0x51; 32]),
            persistent_state: TasExternalIdentity::Absent,
            rtc_state: TasExternalIdentity::Absent,
            sensor_state: TasExternalIdentity::Absent,
            cheats: TasExternalIdentity::Absent,
            state_format_compatibility_id: "synthetic-native-state-v1".to_owned(),
            start_state_sha256: TasDigest::from_bytes(&start_state),
        };
        let project = TasProject::new(
            "synthetic-editor-execution",
            identity.clone(),
            start_state,
            ReplayStartMetadata::default(),
            TasInitialBranch {
                id: "main".to_owned(),
                name: "Main".to_owned(),
                frame_count,
                input_spans,
                events,
            },
            BTreeMap::new(),
        )
        .expect("synthetic TAS project should validate");
        (project, identity)
    }

    fn editor(project: TasProject, root: &Path, name: &str) -> TasEditorSession {
        let manual_path = root.join(format!("{name}.ztas"));
        let autosaves =
            TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default())
                .unwrap();
        let cache = TasSeekStateCache::open(root.join(format!("{name}-seek-cache"))).unwrap();
        TasEditorSession::new(project, manual_path, autosaves, cache).unwrap()
    }

    fn engine(backend: EmuBackend, identity: &TasProjectIdentity) -> TasEditorExecutionEngine {
        TasEditorExecutionEngine::new(TasExecutionSession::new(backend, identity.clone()))
    }

    #[test]
    fn exact_start_execution_is_repeatable_and_ignores_prior_backend_input() {
        let root = crate::test_support::test_directory("tas-editor-execution-start").unwrap();
        let backend = nes_backend();
        let input = TasInputFrame {
            players: [
                TasControllerInput {
                    buttons: 0x01,
                    dpad: 0x10,
                },
                TasControllerInput {
                    buttons: 0x02,
                    dpad: 0x20,
                },
                TasControllerInput {
                    buttons: 0x04,
                    dpad: 0x40,
                },
                TasControllerInput {
                    buttons: 0x08,
                    dpad: 0x80,
                },
                TasControllerInput {
                    buttons: 0x10,
                    dpad: 0x01,
                },
            ],
            zapper: TasZapperInput {
                enabled: true,
                trigger: true,
                hit: false,
                screen_pos: Some([40, 50]),
            },
            ..TasInputFrame::default()
        };
        let (project, identity) = project_for_backend(
            &backend,
            8,
            vec![TasInputSpan {
                start: 0,
                length: 8,
                input,
            }],
            Vec::new(),
        );

        let mut first_editor = editor(project.clone(), root.path(), "first");
        let mut first = engine(backend, &identity);
        let first_outcome = first.seek(&mut first_editor, 8).unwrap();
        let expected = first.backend().encode_state_bytes().unwrap();
        let expected_framebuffer = first.backend().framebuffer().to_vec();
        assert_eq!(first_outcome.requested_cursor, 8);
        assert!(first_outcome.reached_target());
        assert_eq!(first_outcome.executed_frames, 8);
        assert_eq!(first_outcome.restored_cache_cursor, None);
        assert_eq!(first_outcome.framebuffer.width(), 256);
        assert_eq!(first_outcome.framebuffer.height(), 240);
        assert_eq!(first_outcome.framebuffer.rgba(), expected_framebuffer);
        assert_eq!(
            first_editor.load_seek_state().unwrap(),
            Some(expected.clone())
        );

        let mut dirty_backend = nes_backend();
        dirty_backend.set_input(0xFF, 0xFF);
        dirty_backend.set_input_p2(0xFF, 0xFF);
        dirty_backend.step_frame();
        let mut second_editor = editor(project, root.path(), "second");
        let mut second = engine(dirty_backend, &identity);
        let second_outcome = second.seek(&mut second_editor, 8).unwrap();
        assert_eq!(second_outcome.executed_frames, 8);
        assert_eq!(second.backend().encode_state_bytes().unwrap(), expected);
    }

    #[test]
    fn distant_seek_is_bounded_and_resumes_from_its_exact_intermediate_cache() {
        let root = crate::test_support::test_directory("tas-editor-execution-budget").unwrap();
        let backend = nes_backend();
        let (project, identity) = project_for_backend(&backend, 1_000_001, Vec::new(), Vec::new());
        let mut editor = editor(project, root.path(), "bounded");
        let mut executor = engine(backend, &identity);

        let first = executor
            .seek_with_frame_budget(&mut editor, 900_000, 2)
            .unwrap();
        assert_eq!(first.requested_cursor, 900_000);
        assert_eq!(first.cursor, 2);
        assert_eq!(first.executed_frames, 2);
        assert_eq!(first.remaining_frames(), 899_998);
        assert!(!first.reached_target());
        assert_eq!(editor.cursor(), 2);

        let second = executor
            .seek_with_frame_budget(&mut editor, 900_000, 2)
            .unwrap();
        assert_eq!(second.restored_cache_cursor, Some(2));
        assert_eq!(second.cursor, 4);
        assert_eq!(second.executed_frames, 2);
        assert_eq!(editor.cursor(), 4);
    }

    #[test]
    fn seek_uses_newest_prefix_eligible_cache_and_replays_only_the_changed_suffix() {
        let root = crate::test_support::test_directory("tas-editor-execution-cache").unwrap();
        let backend = nes_backend();
        let (project, identity) = project_for_backend(
            &backend,
            8,
            vec![TasInputSpan {
                start: 0,
                length: 8,
                input: TasInputFrame {
                    players: [
                        TasControllerInput {
                            buttons: 1,
                            dpad: 0,
                        },
                        TasControllerInput::default(),
                        TasControllerInput::default(),
                        TasControllerInput::default(),
                        TasControllerInput::default(),
                    ],
                    ..TasInputFrame::default()
                },
            }],
            Vec::new(),
        );
        let mut cached_editor = editor(project, root.path(), "cached");
        let mut executor = engine(backend, &identity);
        executor.seek(&mut cached_editor, 4).unwrap();
        executor.seek(&mut cached_editor, 6).unwrap();
        executor.seek(&mut cached_editor, 8).unwrap();

        cached_editor
            .edit_transaction(|edit| {
                edit.set_input_range(
                    "main",
                    6,
                    1,
                    TasInputFrame {
                        players: [
                            TasControllerInput {
                                buttons: 2,
                                dpad: 0,
                            },
                            TasControllerInput::default(),
                            TasControllerInput::default(),
                            TasControllerInput::default(),
                            TasControllerInput::default(),
                        ],
                        ..TasInputFrame::default()
                    },
                )
            })
            .unwrap();
        let outcome = executor.seek(&mut cached_editor, 8).unwrap();
        assert_eq!(outcome.restored_cache_cursor, Some(6));
        assert_eq!(outcome.executed_frames, 2);

        cached_editor.set_cursor(6).unwrap();
        cached_editor
            .edit_transaction(|edit| {
                edit.set_input_range(
                    "main",
                    6,
                    1,
                    TasInputFrame {
                        players: [
                            TasControllerInput {
                                buttons: 3,
                                dpad: 0,
                            },
                            TasControllerInput::default(),
                            TasControllerInput::default(),
                            TasControllerInput::default(),
                            TasControllerInput::default(),
                        ],
                        ..TasInputFrame::default()
                    },
                )
            })
            .unwrap();
        cached_editor
            .store_seek_state(b"valid cache envelope, invalid emulator state")
            .unwrap();
        let fallback = executor.seek(&mut cached_editor, 8).unwrap();
        assert_eq!(fallback.restored_cache_cursor, Some(4));
        assert_eq!(fallback.executed_frames, 4);
        let actual = executor.backend().encode_state_bytes().unwrap();

        let mut reference_editor =
            editor(cached_editor.project().clone(), root.path(), "reference");
        let mut reference = engine(nes_backend(), &identity);
        reference.seek(&mut reference_editor, 8).unwrap();
        assert_eq!(reference.backend().encode_state_bytes().unwrap(), actual);
    }

    #[test]
    fn immediate_fds_and_media_events_share_canonical_replay_order() {
        let root = crate::test_support::test_directory("tas-editor-execution-events").unwrap();
        let backend = fds_backend();
        let slot = backend.media_slot_snapshot().unwrap().state.slot;
        let (project, identity) = project_for_backend(
            &backend,
            2,
            Vec::new(),
            vec![
                ReplayEvent::FdsDiskSide { frame: 0, side: 1 },
                ReplayEvent::Media {
                    frame: 1,
                    sequence: 0,
                    event: MediaEvent::SelectSide { slot, side: 0 },
                },
                ReplayEvent::FdsDiskSide { frame: 2, side: 1 },
            ],
        );
        let mut editor = editor(project, root.path(), "events");
        let mut executor = engine(backend, &identity);
        let outcome = executor.seek(&mut editor, 2).unwrap();

        assert_eq!(outcome.events_applied, 3);
        assert_eq!(executor.backend().fds_disk_side(), Some(1));
        let cached_state = editor.load_seek_state().unwrap().unwrap();
        let mut cached_backend = fds_backend();
        cached_backend.load_state_from_bytes(cached_state).unwrap();
        assert_eq!(cached_backend.fds_disk_side(), Some(0));
    }

    #[test]
    fn synchronized_link_domains_are_rejected_without_moving_the_editor_cursor() {
        let root = crate::test_support::test_directory("tas-editor-execution-link").unwrap();
        let backend = nes_backend();
        let (project, identity) = project_for_backend(
            &backend,
            1,
            Vec::new(),
            vec![ReplayEvent::WonderSwanLink {
                frame: 0,
                session_cycle: 1,
                event: ReplayWonderSwanLinkEvent::RemoteByte {
                    generation: 1,
                    baud_bps: 9_600,
                    byte: 0x42,
                },
            }],
        );
        let mut editor = editor(project, root.path(), "link");
        let mut executor = engine(backend, &identity);
        let error = executor.seek(&mut editor, 1).unwrap_err();

        assert!(error.to_string().contains("one emulator endpoint only"));
        assert_eq!(editor.cursor(), 0);
        assert!(editor.load_seek_state().unwrap().is_none());
    }

    fn tas_firmware_identity(manifest: &ReplayFirmwareManifest) -> TasFirmwareIdentity {
        match manifest {
            ReplayFirmwareManifest::External {
                firmware_id,
                variant,
                sha256,
            } => TasFirmwareIdentity::External {
                firmware_id: firmware_id.clone(),
                variant: variant.clone(),
                sha256: TasDigest(*sha256),
            },
            ReplayFirmwareManifest::Hle {
                firmware_id,
                implementation,
                compatibility_version,
            } => TasFirmwareIdentity::Hle {
                firmware_id: firmware_id.clone(),
                implementation: implementation.clone(),
                compatibility_version: *compatibility_version,
            },
            ReplayFirmwareManifest::BuiltinOpenSource {
                firmware_id,
                implementation,
                compatibility_version,
                sha256,
            } => TasFirmwareIdentity::BuiltinOpenSource {
                firmware_id: firmware_id.clone(),
                implementation: implementation.clone(),
                compatibility_version: *compatibility_version,
                sha256: TasDigest(*sha256),
            },
            ReplayFirmwareManifest::Skipped {
                firmware_id,
                compatibility_version,
            } => TasFirmwareIdentity::Skipped {
                firmware_id: firmware_id.clone(),
                compatibility_version: *compatibility_version,
            },
        }
    }
}
