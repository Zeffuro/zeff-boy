use std::collections::{BTreeMap, btree_map::Entry};
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use zeff_emu_common::replay::{
    ReplayCheckpoint, ReplayColecoControllerFrame, ReplayFirmwareManifest, ReplayJoypadFrame,
    ReplayLoadLimits, ReplayMetadata, ReplayPlayer, ReplayRecorder, ReplayStartMetadata,
    ReplayZapperFrame,
};

use super::model::{
    MAX_CAMERA_ASSET_BYTES, MAX_PROJECT_FRAMES, MAX_START_STATE_BYTES, MAX_TOTAL_ASSET_BYTES,
    TasBranch, TasCameraInput, TasColecoControllerInput, TasControllerInput, TasDigest,
    TasExternalIdentity, TasFirmwareIdentity, TasInputFrame, TasInputSpan, TasProject,
    TasProjectIdentity, TasVerificationCheckpoint, TasVerificationProvenance, TasZapperInput,
};

const ZRPL_MAGIC: &[u8; 4] = b"ZRPL";
const CURRENT_ZRPL_VERSION: u32 = 3;
const MAX_ZRPL_FILE_BYTES: u64 = 96 * 1024 * 1024;
const CURRENT_ZRPL_METADATA_VERSION: u32 = 3;
const MAX_ZRPL_METADATA_BYTES: usize = 8 * 1024 * 1024;
const MAX_ZRPL_CONVERSION_FRAMES: u64 = 1_000_000;
const ZRPL_V2_FRAME_FIXED_BYTES: usize = 24;
const ZRPL_FRAME_FIXED_BYTES: usize = 27;
const ZRPL_CAMERA_REPEAT_SENTINEL: u32 = u32::MAX;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TasZrplImportWitness {
    pub project_id: String,
    pub identity: TasProjectIdentity,
}

impl TasProject {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn import_zrpl(path: &Path, witness: TasZrplImportWitness) -> Result<Self> {
        Self::import_zrpl_with_witness(path, |_| Ok(witness))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn import_zrpl_with_witness(
        path: &Path,
        witness: impl FnOnce(&[u8]) -> Result<TasZrplImportWitness>,
    ) -> Result<Self> {
        require_zrpl_path(path)?;
        let source_bytes = read_zrpl_bounded(path)?;
        preflight_current_zrpl(&source_bytes)?;
        let mut player = ReplayPlayer::decode_bounded(&source_bytes, zrpl_load_limits())
            .with_context(|| format!("failed to decode replay {}", path.display()))?;
        let metadata = player.metadata().clone();
        let witness = witness(player.save_state())?;
        require_zrpl_representable_identity(&witness.identity)?;
        let coleco_topology = uses_coleco_controller_topology(&witness.identity);
        if coleco_topology && player.version() != 3 {
            bail!("ColecoVision TAS import requires ZRPL v3 controller input records");
        }
        if player.version() == 3 && !coleco_topology {
            bail!("ZRPL v3 controller input records require the ColecoVision TAS topology");
        }
        if player.uses_coleco_input() && !coleco_topology {
            bail!("replay contains ColecoVision controller input for an incompatible TAS topology");
        }
        validate_import_witness(&witness.identity, player.save_state(), &metadata)?;

        let frame_count = u64::try_from(player.total_frames())
            .context("replay frame count does not fit the TAS project model")?;
        if frame_count > MAX_PROJECT_FRAMES {
            bail!("replay exceeds the {MAX_PROJECT_FRAMES}-frame TAS project limit");
        }
        if frame_count > MAX_ZRPL_CONVERSION_FRAMES {
            bail!("replay exceeds the {MAX_ZRPL_CONVERSION_FRAMES}-frame conversion limit");
        }

        let mut assets = BTreeMap::new();
        let mut input_spans = Vec::new();
        let mut open_span: Option<TasInputSpan> = None;
        for frame in 0..frame_count {
            let replay_frame = player
                .next_joypad_frame()
                .ok_or_else(|| anyhow::anyhow!("replay ended before its declared frame count"))?;
            let input = import_input_frame(replay_frame, &mut assets)?;
            match (&mut open_span, input == TasInputFrame::default()) {
                (Some(span), false) if span.input == input => {
                    span.length = span
                        .length
                        .checked_add(1)
                        .ok_or_else(|| anyhow::anyhow!("TAS input span length overflow"))?;
                }
                (Some(_), _) => {
                    input_spans.push(open_span.take().expect("open span exists"));
                    if input != TasInputFrame::default() {
                        open_span = Some(TasInputSpan {
                            start: frame,
                            length: 1,
                            input,
                        });
                    }
                }
                (None, false) => {
                    open_span = Some(TasInputSpan {
                        start: frame,
                        length: 1,
                        input,
                    });
                }
                (None, true) => {}
            }
        }
        if let Some(span) = open_span {
            input_spans.push(span);
        }

        let replay_start = ReplayStartMetadata {
            game_boy_link_state: metadata.game_boy_link_start_state,
            game_boy_link_tick: metadata.game_boy_link_start_tick,
            wonder_swan_link_tick: metadata.wonder_swan_link_start_tick,
            game_boy_link_coordinator_state: metadata.game_boy_link_coordinator_start_state,
        };
        let has_verification =
            !metadata.checkpoints.is_empty() || metadata.final_state_sha256.is_some();
        let verification_checkpoints = metadata
            .checkpoints
            .iter()
            .map(|checkpoint| TasVerificationCheckpoint {
                cursor: checkpoint.frame,
                state_sha256: TasDigest(checkpoint.state_sha256),
            })
            .collect();
        let mut project = Self {
            project_id: witness.project_id,
            source_replay_sha256: Some(TasDigest::from_bytes(&source_bytes)),
            identity: witness.identity,
            start_state: player.save_state().to_vec(),
            replay_start,
            edit_generation: 0,
            rerecord_count: 0,
            active_branch_id: "main".to_owned(),
            project_comment: String::new(),
            branches: vec![TasBranch {
                id: "main".to_owned(),
                name: "Main".to_owned(),
                comment: String::new(),
                parent: None,
                frame_count,
                input_spans,
                events: metadata.events,
                verification: None,
            }],
            markers: Vec::new(),
            annotations: Vec::new(),
            assets,
        };
        project.validate()?;
        if has_verification {
            project.branches[0].verification = Some(TasVerificationProvenance {
                branch_movie_sha256: project.branch_movie_sha256("main")?,
                checkpoints: verification_checkpoints,
                final_state_sha256: metadata.final_state_sha256.map(TasDigest),
            });
            project.validate()?;
        }
        Ok(project)
    }

    #[cfg(test)]
    pub(super) fn export_zrpl_without_execution_for_test(
        &self,
        branch_id: &str,
        path: &Path,
    ) -> Result<PathBuf> {
        require_zrpl_path(path)?;
        if path.exists() {
            bail!("refusing to overwrite existing replay {}", path.display());
        }

        let (bytes, expected) = self.compile_zrpl(branch_id)?;
        crate::platform::write_new_file_atomically_validated(path, &bytes, |temp_file| {
            let actual = decode_zrpl_file_bounded(temp_file)
                .context("temporary replay failed strict validation")?;
            validate_compiled_replay(&actual, &expected)
        })
        .with_context(|| format!("failed to atomically publish replay {}", path.display()))?;
        Ok(path.to_path_buf())
    }

    #[cfg(test)]
    fn compile_zrpl(&self, branch_id: &str) -> Result<(Vec<u8>, CompiledReplay)> {
        let branch = self
            .branch(branch_id)
            .ok_or_else(|| anyhow::anyhow!("unknown TAS branch {branch_id:?}"))?;
        if branch.verification.is_some() && !self.verification_is_current(branch_id)? {
            bail!("TAS branch {branch_id:?} has stale verification provenance");
        }
        self.compile_zrpl_with_provenance(branch_id, branch.verification.as_ref())
    }

    pub(super) fn compile_zrpl_with_provenance(
        &self,
        branch_id: &str,
        verification: Option<&TasVerificationProvenance>,
    ) -> Result<(Vec<u8>, CompiledReplay)> {
        self.validate()?;
        require_zrpl_representable_identity(&self.identity)?;
        let branch = self
            .branch(branch_id)
            .ok_or_else(|| anyhow::anyhow!("unknown TAS branch {branch_id:?}"))?;
        let coleco_topology = uses_coleco_controller_topology(&self.identity);
        if !coleco_topology
            && branch
                .input_spans()
                .iter()
                .any(|span| span.input.coleco != [TasColecoControllerInput::default(); 2])
        {
            bail!("TAS branch contains ColecoVision controller input for an incompatible topology");
        }
        if branch.frame_count > MAX_ZRPL_CONVERSION_FRAMES {
            bail!("TAS replay conversion exceeds the {MAX_ZRPL_CONVERSION_FRAMES}-frame limit");
        }
        if let Some(verification) = verification
            && verification.branch_movie_sha256 != self.branch_movie_sha256(branch_id)?
        {
            bail!("TAS verification provenance does not match branch {branch_id:?}");
        }
        for event in &branch.events {
            let required_frames = event
                .required_frame_count()
                .ok_or_else(|| anyhow::anyhow!("replay event frame overflow"))?;
            if required_frames > branch.frame_count {
                bail!("TAS branch contains an event that requires an additional replay frame");
            }
        }

        let identity = self.canonical_identity();
        let metadata = ReplayMetadata {
            system: Some(identity.system),
            core_family: Some(identity.core_family),
            rom_sha256: Some(identity.effective_media_sha256.0),
            firmware: identity
                .firmware
                .iter()
                .map(export_firmware_identity)
                .collect(),
            events: branch.events.clone(),
            cheat_sha256: match identity.cheats {
                TasExternalIdentity::Absent => None,
                TasExternalIdentity::ExternalSha256(digest) => Some(digest.0),
            },
            final_state_sha256: verification
                .and_then(|verification| verification.final_state_sha256)
                .map(|digest| digest.0),
            game_boy_link_start_state: self.replay_start.game_boy_link_state,
            game_boy_link_start_tick: self.replay_start.game_boy_link_tick,
            wonder_swan_link_start_tick: self.replay_start.wonder_swan_link_tick,
            checkpoints: verification
                .map(|verification| {
                    verification
                        .checkpoints
                        .iter()
                        .map(|checkpoint| ReplayCheckpoint {
                            frame: checkpoint.cursor,
                            state_sha256: checkpoint.state_sha256.0,
                        })
                        .collect()
                })
                .unwrap_or_default(),
            game_boy_link_coordinator_start_state: self
                .replay_start
                .game_boy_link_coordinator_state,
        };

        let frame_capacity = usize::try_from(branch.frame_count)
            .context("TAS branch frame count does not fit this platform")?;
        let mut frames = Vec::with_capacity(frame_capacity);
        let mut decoded_camera_bytes = 0usize;
        for frame in 0..branch.frame_count {
            let frame = export_input_frame(branch.input_at(frame), &self.assets)?;
            if let Some(camera) = &frame.camera_frame {
                decoded_camera_bytes = decoded_camera_bytes
                    .checked_add(camera.len())
                    .ok_or_else(|| anyhow::anyhow!("replay camera data size overflow"))?;
                if decoded_camera_bytes > MAX_TOTAL_ASSET_BYTES {
                    bail!(
                        "TAS replay conversion exceeds the {MAX_TOTAL_ASSET_BYTES}-byte decoded camera limit"
                    );
                }
            }
            frames.push(frame);
        }
        let expected = CompiledReplay {
            start_state: self.start_state.clone(),
            metadata: metadata.clone(),
            frames: frames.clone(),
        };
        let mut recorder =
            ReplayRecorder::new_with_metadata(PathBuf::new(), self.start_state.clone(), metadata);
        if coleco_topology {
            recorder.enable_coleco_input_format();
        }
        for frame in frames {
            recorder.record_joypad_frame(frame);
        }
        let bytes = recorder.into_bytes()?;
        preflight_current_zrpl(&bytes)?;
        Ok((bytes, expected))
    }
}

fn require_zrpl_representable_identity(identity: &TasProjectIdentity) -> Result<()> {
    if identity.system == "coleco" && !uses_coleco_controller_topology(identity) {
        bail!("ColecoVision TAS replay requires two standard controller/keypad devices");
    }
    Ok(())
}

fn uses_coleco_controller_topology(identity: &TasProjectIdentity) -> bool {
    identity.system == "coleco"
        && identity.devices.len() == 2
        && identity.devices.iter().enumerate().all(|(index, device)| {
            device.port == format!("p{}", index + 1)
                && device.device == "coleco-standard-controller-keypad"
        })
}

fn read_zrpl_bounded(path: &Path) -> Result<Vec<u8>> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("failed to open replay {}", path.display()))?;
    let mut source_bytes = Vec::new();
    file.take(MAX_ZRPL_FILE_BYTES + 1)
        .read_to_end(&mut source_bytes)
        .with_context(|| format!("failed to read replay {}", path.display()))?;
    if source_bytes.len() as u64 > MAX_ZRPL_FILE_BYTES {
        bail!("replay exceeds the {MAX_ZRPL_FILE_BYTES}-byte TAS import limit");
    }
    Ok(source_bytes)
}

pub(super) fn zrpl_load_limits() -> ReplayLoadLimits {
    ReplayLoadLimits {
        max_file_bytes: MAX_ZRPL_FILE_BYTES,
        max_metadata_bytes: MAX_ZRPL_METADATA_BYTES,
        max_state_bytes: MAX_START_STATE_BYTES,
        max_frames: MAX_ZRPL_CONVERSION_FRAMES as usize,
        max_decoded_camera_bytes: MAX_TOTAL_ASSET_BYTES,
    }
}

pub(super) fn decode_zrpl_file_bounded(file: &mut std::fs::File) -> Result<ReplayPlayer> {
    file.rewind()?;
    let mut bytes = Vec::new();
    file.take(MAX_ZRPL_FILE_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_ZRPL_FILE_BYTES {
        bail!("replay exceeds the {MAX_ZRPL_FILE_BYTES}-byte TAS conversion limit");
    }
    ReplayPlayer::decode_bounded(&bytes, zrpl_load_limits())
}

pub(super) struct CompiledReplay {
    start_state: Vec<u8>,
    metadata: ReplayMetadata,
    frames: Vec<ReplayJoypadFrame>,
}

#[cfg(not(target_arch = "wasm32"))]
fn preflight_current_zrpl(bytes: &[u8]) -> Result<()> {
    if bytes.len() as u64 > MAX_ZRPL_FILE_BYTES {
        bail!("replay exceeds the {MAX_ZRPL_FILE_BYTES}-byte TAS conversion limit");
    }
    let header = bytes
        .get(..8)
        .ok_or_else(|| anyhow::anyhow!("replay header is truncated"))?;
    if &header[..4] != ZRPL_MAGIC {
        bail!("not a valid replay file");
    }
    let version = u32::from_le_bytes(header[4..8].try_into().expect("version is four bytes"));
    if !matches!(version, 2 | CURRENT_ZRPL_VERSION) {
        bail!("TAS import requires canonical ZRPL v{CURRENT_ZRPL_VERSION}, found v{version}");
    }

    let mut offset = 8usize;
    let metadata_len = read_zrpl_u32(bytes, &mut offset, "metadata length")? as usize;
    if metadata_len > MAX_ZRPL_METADATA_BYTES {
        bail!("replay metadata exceeds the {MAX_ZRPL_METADATA_BYTES}-byte conversion limit");
    }
    let metadata = read_zrpl_bytes(bytes, &mut offset, metadata_len, "metadata")?;
    let metadata_version = metadata
        .get(..4)
        .map(|version| u32::from_le_bytes(version.try_into().expect("version is four bytes")))
        .ok_or_else(|| anyhow::anyhow!("replay metadata is truncated"))?;
    if metadata_version != CURRENT_ZRPL_METADATA_VERSION {
        bail!(
            "TAS import requires replay metadata v{CURRENT_ZRPL_METADATA_VERSION}, found v{metadata_version}"
        );
    }

    let state_len = read_zrpl_u32(bytes, &mut offset, "starting-state length")? as usize;
    if state_len > MAX_START_STATE_BYTES {
        bail!("replay starting state exceeds the {MAX_START_STATE_BYTES}-byte conversion limit");
    }
    read_zrpl_bytes(bytes, &mut offset, state_len, "starting state")?;

    let frame_count = read_zrpl_u32(bytes, &mut offset, "frame count")? as u64;
    if frame_count > MAX_ZRPL_CONVERSION_FRAMES {
        bail!("replay exceeds the {MAX_ZRPL_CONVERSION_FRAMES}-frame conversion limit");
    }
    let mut previous_camera_len = None;
    let mut decoded_camera_bytes = 0usize;
    for frame in 0..frame_count {
        let fixed_bytes = if version == CURRENT_ZRPL_VERSION {
            ZRPL_FRAME_FIXED_BYTES
        } else {
            ZRPL_V2_FRAME_FIXED_BYTES
        };
        read_zrpl_bytes(bytes, &mut offset, fixed_bytes, "input frame")?;
        let camera_len = read_zrpl_u32(bytes, &mut offset, "camera length")?;
        let decoded_len = match camera_len {
            0 => 0,
            ZRPL_CAMERA_REPEAT_SENTINEL => previous_camera_len.ok_or_else(|| {
                anyhow::anyhow!("replay camera frame {frame} repeats before any camera frame")
            })?,
            len => {
                let len = len as usize;
                if len > MAX_CAMERA_ASSET_BYTES {
                    bail!(
                        "replay camera frame exceeds the {MAX_CAMERA_ASSET_BYTES}-byte conversion limit"
                    );
                }
                read_zrpl_bytes(bytes, &mut offset, len, "camera frame")?;
                previous_camera_len = Some(len);
                len
            }
        };
        decoded_camera_bytes = decoded_camera_bytes
            .checked_add(decoded_len)
            .ok_or_else(|| anyhow::anyhow!("replay camera data size overflow"))?;
        if decoded_camera_bytes > MAX_TOTAL_ASSET_BYTES {
            bail!(
                "replay exceeds the {MAX_TOTAL_ASSET_BYTES}-byte decoded camera conversion limit"
            );
        }
    }
    if offset != bytes.len() {
        bail!("replay input stream has trailing bytes");
    }
    Ok(())
}

fn read_zrpl_u32(bytes: &[u8], offset: &mut usize, name: &str) -> Result<u32> {
    let value = read_zrpl_bytes(bytes, offset, 4, name)?;
    Ok(u32::from_le_bytes(
        value.try_into().expect("u32 is four bytes"),
    ))
}

fn read_zrpl_bytes<'a>(
    bytes: &'a [u8],
    offset: &mut usize,
    len: usize,
    name: &str,
) -> Result<&'a [u8]> {
    let end = offset
        .checked_add(len)
        .ok_or_else(|| anyhow::anyhow!("replay {name} offset overflow"))?;
    let value = bytes
        .get(*offset..end)
        .ok_or_else(|| anyhow::anyhow!("replay {name} is truncated"))?;
    *offset = end;
    Ok(value)
}

pub(super) fn require_zrpl_path(path: &Path) -> Result<()> {
    if !path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("zrpl"))
    {
        bail!("replay files must use the .zrpl extension");
    }
    Ok(())
}

fn validate_import_witness(
    identity: &TasProjectIdentity,
    start_state: &[u8],
    metadata: &ReplayMetadata,
) -> Result<()> {
    let system = metadata
        .system
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("replay is missing canonical system identity"))?;
    if system != identity.system {
        bail!("replay system does not match the TAS identity witness");
    }
    let core_family = metadata
        .core_family
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("replay is missing canonical core-family identity"))?;
    if core_family != identity.core_family {
        bail!("replay core family does not match the TAS identity witness");
    }
    let rom_sha256 = metadata
        .rom_sha256
        .ok_or_else(|| anyhow::anyhow!("replay is missing canonical media identity"))?;
    if rom_sha256 != identity.effective_media_sha256.0 {
        bail!("replay media does not match the TAS identity witness");
    }
    if TasDigest::from_bytes(start_state) != identity.start_state_sha256 {
        bail!("replay starting state does not match the TAS identity witness");
    }

    let expected_cheats = match identity.cheats {
        TasExternalIdentity::Absent => None,
        TasExternalIdentity::ExternalSha256(digest) => Some(digest.0),
    };
    if metadata.cheat_sha256 != expected_cheats {
        bail!("replay cheats do not match the TAS identity witness");
    }

    let expected_firmware = canonical_replay_firmware(
        identity
            .firmware
            .iter()
            .map(export_firmware_identity)
            .collect(),
    )?;
    let actual_firmware = canonical_replay_firmware(metadata.firmware.clone())?;
    if actual_firmware != expected_firmware {
        bail!("replay firmware does not match the TAS identity witness");
    }
    Ok(())
}

fn canonical_replay_firmware(
    mut firmware: Vec<ReplayFirmwareManifest>,
) -> Result<Vec<ReplayFirmwareManifest>> {
    firmware.sort_by(|left, right| replay_firmware_id(left).cmp(replay_firmware_id(right)));
    if firmware
        .windows(2)
        .any(|pair| replay_firmware_id(&pair[0]) == replay_firmware_id(&pair[1]))
    {
        bail!("replay contains duplicate firmware identities");
    }
    Ok(firmware)
}

fn replay_firmware_id(firmware: &ReplayFirmwareManifest) -> &str {
    match firmware {
        ReplayFirmwareManifest::External { firmware_id, .. }
        | ReplayFirmwareManifest::Hle { firmware_id, .. }
        | ReplayFirmwareManifest::BuiltinOpenSource { firmware_id, .. }
        | ReplayFirmwareManifest::Skipped { firmware_id, .. } => firmware_id,
    }
}

fn export_firmware_identity(firmware: &TasFirmwareIdentity) -> ReplayFirmwareManifest {
    match firmware {
        TasFirmwareIdentity::External {
            firmware_id,
            variant,
            sha256,
        } => ReplayFirmwareManifest::External {
            firmware_id: firmware_id.clone(),
            variant: variant.clone(),
            sha256: sha256.0,
        },
        TasFirmwareIdentity::Hle {
            firmware_id,
            implementation,
            compatibility_version,
        } => ReplayFirmwareManifest::Hle {
            firmware_id: firmware_id.clone(),
            implementation: implementation.clone(),
            compatibility_version: *compatibility_version,
        },
        TasFirmwareIdentity::BuiltinOpenSource {
            firmware_id,
            implementation,
            compatibility_version,
            sha256,
        } => ReplayFirmwareManifest::BuiltinOpenSource {
            firmware_id: firmware_id.clone(),
            implementation: implementation.clone(),
            compatibility_version: *compatibility_version,
            sha256: sha256.0,
        },
        TasFirmwareIdentity::Skipped {
            firmware_id,
            compatibility_version,
        } => ReplayFirmwareManifest::Skipped {
            firmware_id: firmware_id.clone(),
            compatibility_version: *compatibility_version,
        },
    }
}

fn import_input_frame(
    frame: ReplayJoypadFrame,
    assets: &mut BTreeMap<TasDigest, Vec<u8>>,
) -> Result<TasInputFrame> {
    let camera = match frame.camera_frame {
        None => TasCameraInput::None,
        Some(bytes) => {
            if bytes.is_empty() {
                bail!("replay contains an unrepresentable empty camera frame");
            }
            let digest = TasDigest::from_bytes(&bytes);
            match assets.entry(digest) {
                Entry::Vacant(entry) => {
                    entry.insert(bytes);
                }
                Entry::Occupied(entry) if entry.get() != &bytes => {
                    bail!("TAS camera asset SHA-256 collision");
                }
                Entry::Occupied(_) => {}
            }
            TasCameraInput::Blob(digest)
        }
    };
    Ok(TasInputFrame {
        players: [
            TasControllerInput {
                buttons: frame.buttons,
                dpad: frame.dpad,
            },
            TasControllerInput {
                buttons: frame.buttons_p2,
                dpad: frame.dpad_p2,
            },
            TasControllerInput {
                buttons: frame.buttons_p3,
                dpad: frame.dpad_p3,
            },
            TasControllerInput {
                buttons: frame.buttons_p4,
                dpad: frame.dpad_p4,
            },
            TasControllerInput {
                buttons: frame.buttons_p5,
                dpad: frame.dpad_p5,
            },
        ],
        coleco: frame.coleco.map(import_coleco_controller),
        zapper: TasZapperInput {
            enabled: frame.zapper.enabled,
            trigger: frame.zapper.trigger,
            hit: frame.zapper.hit,
            screen_pos: frame.zapper.screen_pos.map(|(x, y)| [x, y]),
        },
        tilt_x_bits: frame.host_tilt.0.to_bits(),
        tilt_y_bits: frame.host_tilt.1.to_bits(),
        camera,
    })
}

fn export_input_frame(
    frame: TasInputFrame,
    assets: &BTreeMap<TasDigest, Vec<u8>>,
) -> Result<ReplayJoypadFrame> {
    let camera_frame = match frame.camera {
        TasCameraInput::None => None,
        TasCameraInput::Blob(digest) => {
            let bytes = assets
                .get(&digest)
                .ok_or_else(|| anyhow::anyhow!("TAS input references a missing camera asset"))?;
            if bytes.is_empty() {
                bail!("empty TAS camera assets are not representable in ZRPL v2");
            }
            Some(bytes.clone())
        }
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
        coleco: frame.coleco.map(export_coleco_controller),
    })
}

fn import_coleco_controller(frame: ReplayColecoControllerFrame) -> TasColecoControllerInput {
    TasColecoControllerInput {
        up: frame.up,
        right: frame.right,
        down: frame.down,
        left: frame.left,
        left_button: frame.left_button,
        right_button: frame.right_button,
        keypad: match frame.keypad {
            0 => crate::tas_project::TasColecoKeypadKey::None,
            1 => crate::tas_project::TasColecoKeypadKey::Zero,
            2 => crate::tas_project::TasColecoKeypadKey::One,
            3 => crate::tas_project::TasColecoKeypadKey::Two,
            4 => crate::tas_project::TasColecoKeypadKey::Three,
            5 => crate::tas_project::TasColecoKeypadKey::Four,
            6 => crate::tas_project::TasColecoKeypadKey::Five,
            7 => crate::tas_project::TasColecoKeypadKey::Six,
            8 => crate::tas_project::TasColecoKeypadKey::Seven,
            9 => crate::tas_project::TasColecoKeypadKey::Eight,
            10 => crate::tas_project::TasColecoKeypadKey::Nine,
            11 => crate::tas_project::TasColecoKeypadKey::Star,
            12 => crate::tas_project::TasColecoKeypadKey::Pound,
            _ => unreachable!("replay keypad values are validated while decoding"),
        },
    }
}

fn export_coleco_controller(frame: TasColecoControllerInput) -> ReplayColecoControllerFrame {
    ReplayColecoControllerFrame {
        up: frame.up,
        right: frame.right,
        down: frame.down,
        left: frame.left,
        left_button: frame.left_button,
        right_button: frame.right_button,
        keypad: match frame.keypad {
            crate::tas_project::TasColecoKeypadKey::None => 0,
            crate::tas_project::TasColecoKeypadKey::Zero => 1,
            crate::tas_project::TasColecoKeypadKey::One => 2,
            crate::tas_project::TasColecoKeypadKey::Two => 3,
            crate::tas_project::TasColecoKeypadKey::Three => 4,
            crate::tas_project::TasColecoKeypadKey::Four => 5,
            crate::tas_project::TasColecoKeypadKey::Five => 6,
            crate::tas_project::TasColecoKeypadKey::Six => 7,
            crate::tas_project::TasColecoKeypadKey::Seven => 8,
            crate::tas_project::TasColecoKeypadKey::Eight => 9,
            crate::tas_project::TasColecoKeypadKey::Nine => 10,
            crate::tas_project::TasColecoKeypadKey::Star => 11,
            crate::tas_project::TasColecoKeypadKey::Pound => 12,
        },
    }
}

pub(super) fn validate_compiled_replay(
    actual: &ReplayPlayer,
    expected: &CompiledReplay,
) -> Result<()> {
    if actual.save_state() != expected.start_state {
        bail!("compiled replay changed the starting state");
    }
    if actual.metadata() != &expected.metadata {
        bail!("compiled replay changed replay metadata");
    }
    if actual.total_frames() != expected.frames.len()
        || actual.peek_joypad_frames(0, actual.total_frames()) != expected.frames
    {
        bail!("compiled replay changed the input timeline");
    }
    Ok(())
}
