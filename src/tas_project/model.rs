use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use anyhow::{Result, bail};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest as _, Sha256};
use zeff_emu_common::replay::{
    ReplayEvent, ReplayStartMetadata, decode_replay_event_stream, encode_replay_event_stream,
    encode_replay_start_metadata, validate_replay_start_events,
};

pub const FORMAT_VERSION: u32 = 1;
pub const MAX_PROJECT_FRAMES: u64 = 1_000_000_000;
pub const MAX_PROJECT_BRANCHES: usize = 256;
pub const MAX_PROJECT_MARKERS: usize = 100_000;
pub const MAX_PROJECT_ANNOTATIONS: usize = 100_000;
pub const MAX_PROJECT_INPUT_SPANS: usize = 1_000_000;
pub const MAX_PROJECT_ASSETS: usize = 4096;
pub const MAX_CAMERA_ASSET_BYTES: usize = 1024 * 1024;
pub const MAX_TOTAL_ASSET_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_START_STATE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct TasDigest(pub [u8; 32]);

impl TasDigest {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }

    pub fn to_hex(self) -> String {
        const_hex::encode(self.0)
    }
}

impl Serialize for TasDigest {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for TasDigest {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(serde::de::Error::custom(
                "SHA-256 must contain exactly 64 hex digits",
            ));
        }
        let bytes = const_hex::decode(value).map_err(serde::de::Error::custom)?;
        let bytes = bytes
            .try_into()
            .map_err(|_| serde::de::Error::custom("SHA-256 must contain 32 bytes"))?;
        Ok(Self(bytes))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TasProjectIdentity {
    pub system: String,
    pub core_family: String,
    pub determinism_abi: String,
    pub source_media_sha256: TasDigest,
    pub effective_media_sha256: TasDigest,
    pub patches: Vec<TasPatchIdentity>,
    pub firmware: Vec<TasFirmwareIdentity>,
    pub devices: Vec<TasDeviceIdentity>,
    pub sync_config_sha256: TasDigest,
    pub persistent_state: TasExternalIdentity,
    pub rtc_state: TasExternalIdentity,
    pub sensor_state: TasExternalIdentity,
    pub cheats: TasExternalIdentity,
    pub state_format_compatibility_id: String,
    pub start_state_sha256: TasDigest,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TasPatchIdentity {
    pub format: String,
    pub sha256: TasDigest,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TasFirmwareIdentity {
    External {
        firmware_id: String,
        variant: Option<String>,
        sha256: TasDigest,
    },
    Hle {
        firmware_id: String,
        implementation: String,
        compatibility_version: u32,
    },
    BuiltinOpenSource {
        firmware_id: String,
        implementation: String,
        compatibility_version: u32,
        sha256: TasDigest,
    },
    Skipped {
        firmware_id: String,
        compatibility_version: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TasDeviceIdentity {
    pub port: String,
    pub device: String,
    pub configuration_sha256: TasDigest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "sha256", rename_all = "snake_case")]
pub enum TasExternalIdentity {
    Absent,
    ExternalSha256(TasDigest),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TasProject {
    pub(super) project_id: String,
    pub(super) source_replay_sha256: Option<TasDigest>,
    pub(super) identity: TasProjectIdentity,
    pub(super) start_state: Arc<[u8]>,
    pub(super) replay_start: ReplayStartMetadata,
    pub(super) edit_generation: u64,
    pub(super) rerecord_count: u64,
    pub(super) active_branch_id: String,
    pub(super) project_comment: String,
    pub(super) branches: Vec<TasBranch>,
    pub(super) markers: Vec<TasMarker>,
    pub(super) annotations: Vec<TasAnnotation>,
    pub(super) assets: BTreeMap<TasDigest, Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TasBranch {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) comment: String,
    pub(super) parent: Option<TasBranchOrigin>,
    pub(super) frame_count: u64,
    pub(super) input_spans: Vec<TasInputSpan>,
    pub(super) events: Vec<ReplayEvent>,
    pub(super) verification: Option<TasVerificationProvenance>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TasInitialBranch {
    pub id: String,
    pub name: String,
    pub frame_count: u64,
    pub input_spans: Vec<TasInputSpan>,
    pub events: Vec<ReplayEvent>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TasBranchOrigin {
    pub branch_id: String,
    pub branch_movie_sha256: TasDigest,
    pub fork_cursor: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TasControllerInput {
    pub buttons: u8,
    pub dpad: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TasColecoKeypadKey {
    #[default]
    None,
    Zero,
    One,
    Two,
    Three,
    Four,
    Five,
    Six,
    Seven,
    Eight,
    Nine,
    Star,
    Pound,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TasColecoControllerInput {
    pub up: bool,
    pub right: bool,
    pub down: bool,
    pub left: bool,
    pub left_button: bool,
    pub right_button: bool,
    pub keypad: TasColecoKeypadKey,
}

impl TasColecoControllerInput {
    pub const fn is_neutral(self) -> bool {
        !self.up
            && !self.right
            && !self.down
            && !self.left
            && !self.left_button
            && !self.right_button
            && matches!(self.keypad, TasColecoKeypadKey::None)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TasZapperInput {
    pub enabled: bool,
    pub trigger: bool,
    pub hit: bool,
    pub screen_pos: Option<[u16; 2]>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TasCameraInput {
    #[default]
    None,
    Blob(TasDigest),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TasInputFrame {
    pub players: [TasControllerInput; 5],
    #[serde(default, skip_serializing_if = "coleco_inputs_are_neutral")]
    pub coleco: [TasColecoControllerInput; 2],
    pub zapper: TasZapperInput,
    pub tilt_x_bits: u32,
    pub tilt_y_bits: u32,
    pub camera: TasCameraInput,
}

fn coleco_inputs_are_neutral(inputs: &[TasColecoControllerInput; 2]) -> bool {
    inputs.iter().all(|input| input.is_neutral())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TasInputSpan {
    pub start: u64,
    pub length: u64,
    pub input: TasInputFrame,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TasMarker {
    pub id: String,
    pub branch_id: String,
    pub cursor: u64,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TasAnnotation {
    pub id: String,
    pub branch_id: String,
    pub start: u64,
    pub length: u64,
    pub kind: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TasVerificationProvenance {
    pub branch_movie_sha256: TasDigest,
    pub checkpoints: Vec<TasVerificationCheckpoint>,
    pub final_state_sha256: Option<TasDigest>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TasVerificationCheckpoint {
    pub cursor: u64,
    pub state_sha256: TasDigest,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TasSeekCacheIdentity {
    pub cache_format_version: u32,
    pub state_format_compatibility_id: String,
    pub sync_identity_sha256: TasDigest,
    pub branch_prefix_sha256: TasDigest,
    pub cursor: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TasProjectLoadSource {
    Primary,
    Backup,
}

impl TasProject {
    pub fn new(
        project_id: impl Into<String>,
        identity: TasProjectIdentity,
        start_state: Vec<u8>,
        replay_start: ReplayStartMetadata,
        initial_branch: TasInitialBranch,
        assets: BTreeMap<TasDigest, Vec<u8>>,
    ) -> Result<Self> {
        let active_branch_id = initial_branch.id.clone();
        let project = Self {
            project_id: project_id.into(),
            source_replay_sha256: None,
            identity,
            start_state: start_state.into(),
            replay_start,
            edit_generation: 0,
            rerecord_count: 0,
            active_branch_id,
            project_comment: String::new(),
            branches: vec![TasBranch {
                id: initial_branch.id,
                name: initial_branch.name,
                comment: String::new(),
                parent: None,
                frame_count: initial_branch.frame_count,
                input_spans: initial_branch.input_spans,
                events: initial_branch.events,
                verification: None,
            }],
            markers: Vec::new(),
            annotations: Vec::new(),
            assets,
        };
        project.validate()?;
        Ok(project)
    }

    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    pub fn source_replay_sha256(&self) -> Option<TasDigest> {
        self.source_replay_sha256
    }

    pub fn identity(&self) -> &TasProjectIdentity {
        &self.identity
    }

    pub fn start_state(&self) -> &[u8] {
        &self.start_state
    }

    pub fn replay_start(&self) -> &ReplayStartMetadata {
        &self.replay_start
    }

    pub fn edit_generation(&self) -> u64 {
        self.edit_generation
    }

    pub fn rerecord_count(&self) -> u64 {
        self.rerecord_count
    }

    pub fn active_branch_id(&self) -> &str {
        &self.active_branch_id
    }

    pub fn project_comment(&self) -> &str {
        &self.project_comment
    }

    pub fn branches(&self) -> &[TasBranch] {
        &self.branches
    }

    pub fn markers(&self) -> &[TasMarker] {
        &self.markers
    }

    pub fn annotations(&self) -> &[TasAnnotation] {
        &self.annotations
    }

    pub fn assets(&self) -> &BTreeMap<TasDigest, Vec<u8>> {
        &self.assets
    }

    pub fn validate(&self) -> Result<()> {
        self.validate_editor_state()?;
        if TasDigest::from_bytes(&self.start_state) != self.identity.start_state_sha256 {
            bail!("TAS starting state SHA-256 does not match project identity");
        }
        Ok(())
    }

    pub(super) fn validate_editor_state(&self) -> Result<()> {
        validate_id(&self.project_id, "project ID")?;
        validate_identity(&self.identity)?;
        if self.start_state.len() > MAX_START_STATE_BYTES {
            bail!("TAS starting state exceeds {MAX_START_STATE_BYTES} bytes");
        }
        encode_replay_start_metadata(&self.replay_start)?;
        if self.branches.is_empty() || self.branches.len() > MAX_PROJECT_BRANCHES {
            bail!("TAS project must contain 1..={MAX_PROJECT_BRANCHES} branches");
        }
        if self.markers.len() > MAX_PROJECT_MARKERS {
            bail!("TAS project exceeds {MAX_PROJECT_MARKERS} markers");
        }
        if self.annotations.len() > MAX_PROJECT_ANNOTATIONS {
            bail!("TAS project exceeds {MAX_PROJECT_ANNOTATIONS} annotations");
        }
        if self.assets.len() > MAX_PROJECT_ASSETS {
            bail!("TAS project exceeds {MAX_PROJECT_ASSETS} assets");
        }
        validate_text(&self.project_comment, "project comment", 64 * 1024)?;

        let mut branch_ids = HashSet::new();
        let mut input_span_count = 0usize;
        for branch in &self.branches {
            validate_id(&branch.id, "branch ID")?;
            validate_text(&branch.name, "branch name", 256)?;
            validate_text(&branch.comment, "branch comment", 64 * 1024)?;
            if !branch_ids.insert(branch.id.as_str()) {
                bail!("duplicate TAS branch ID {:?}", branch.id);
            }
            if branch.frame_count > MAX_PROJECT_FRAMES {
                bail!(
                    "TAS branch {:?} exceeds {MAX_PROJECT_FRAMES} frames",
                    branch.id
                );
            }
            input_span_count = input_span_count
                .checked_add(branch.input_spans.len())
                .ok_or_else(|| anyhow::anyhow!("TAS input span count overflow"))?;
            validate_input_spans(branch, &self.assets)?;
            let event_bytes = encode_replay_event_stream(&branch.events)?;
            if decode_replay_event_stream(&event_bytes)? != branch.events {
                bail!(
                    "TAS branch {:?} events are not in canonical order",
                    branch.id
                );
            }
            if branch.events.iter().any(|event| {
                event
                    .required_frame_count()
                    .is_none_or(|required| required > branch.frame_count)
            }) {
                bail!("TAS branch {:?} contains an event past its end", branch.id);
            }
            validate_replay_start_events(&self.replay_start, &branch.events)?;
            if let Some(verification) = &branch.verification {
                let mut previous_cursor = None;
                for checkpoint in &verification.checkpoints {
                    if checkpoint.cursor > branch.frame_count
                        || previous_cursor.is_some_and(|cursor| cursor >= checkpoint.cursor)
                    {
                        bail!("TAS verification checkpoints must be ordered and in range");
                    }
                    previous_cursor = Some(checkpoint.cursor);
                }
            }
        }
        if input_span_count > MAX_PROJECT_INPUT_SPANS {
            bail!("TAS project exceeds {MAX_PROJECT_INPUT_SPANS} input spans");
        }
        if !branch_ids.contains(self.active_branch_id.as_str()) {
            bail!("active TAS branch does not exist");
        }

        for branch in &self.branches {
            if let Some(parent) = &branch.parent {
                validate_id(&parent.branch_id, "parent branch ID")?;
                if parent.branch_id == branch.id || !branch_ids.contains(parent.branch_id.as_str())
                {
                    bail!("invalid parent for TAS branch {:?}", branch.id);
                }
                if parent.fork_cursor > branch.frame_count {
                    bail!("TAS branch {:?} fork cursor is past its end", branch.id);
                }
            }
        }
        for branch in &self.branches {
            let mut visited = HashSet::new();
            let mut current = branch;
            while let Some(parent) = &current.parent {
                if !visited.insert(current.id.as_str()) {
                    bail!("TAS branch ancestry contains a cycle");
                }
                current = self
                    .branch(&parent.branch_id)
                    .expect("parent existence was validated above");
            }
        }

        let mut marker_ids = HashSet::new();
        for marker in &self.markers {
            validate_id(&marker.id, "marker ID")?;
            validate_text(&marker.name, "marker name", 1024)?;
            if !marker_ids.insert(marker.id.as_str()) {
                bail!("duplicate TAS marker ID {:?}", marker.id);
            }
            let branch = self
                .branch(&marker.branch_id)
                .ok_or_else(|| anyhow::anyhow!("TAS marker references an unknown branch"))?;
            if marker.cursor > branch.frame_count {
                bail!("TAS marker cursor is past its branch end");
            }
        }

        let mut annotation_ids = HashSet::new();
        for annotation in &self.annotations {
            validate_id(&annotation.id, "annotation ID")?;
            validate_id(&annotation.kind, "annotation kind")?;
            validate_text(&annotation.text, "annotation text", 64 * 1024)?;
            if annotation.length == 0 {
                bail!("TAS annotations cannot be empty");
            }
            if !annotation_ids.insert(annotation.id.as_str()) {
                bail!("duplicate TAS annotation ID {:?}", annotation.id);
            }
            let branch = self
                .branch(&annotation.branch_id)
                .ok_or_else(|| anyhow::anyhow!("TAS annotation references an unknown branch"))?;
            let end = annotation
                .start
                .checked_add(annotation.length)
                .ok_or_else(|| anyhow::anyhow!("TAS annotation range overflows"))?;
            if end > branch.frame_count {
                bail!("TAS annotation extends past its branch end");
            }
        }

        let mut total_asset_bytes = 0usize;
        for (digest, bytes) in &self.assets {
            if bytes.len() > MAX_CAMERA_ASSET_BYTES {
                bail!("TAS camera asset exceeds {MAX_CAMERA_ASSET_BYTES} bytes");
            }
            if TasDigest::from_bytes(bytes) != *digest {
                bail!("TAS asset SHA-256 does not match its identity");
            }
            total_asset_bytes = total_asset_bytes
                .checked_add(bytes.len())
                .ok_or_else(|| anyhow::anyhow!("TAS asset size overflow"))?;
        }
        if total_asset_bytes > MAX_TOTAL_ASSET_BYTES {
            bail!("TAS project assets exceed {MAX_TOTAL_ASSET_BYTES} bytes");
        }
        Ok(())
    }

    pub fn branch(&self, id: &str) -> Option<&TasBranch> {
        self.branches.iter().find(|branch| branch.id == id)
    }

    pub(super) fn canonical_identity(&self) -> TasProjectIdentity {
        let mut identity = self.identity.clone();
        identity
            .firmware
            .sort_by(|left, right| firmware_id(left).cmp(firmware_id(right)));
        identity
            .devices
            .sort_by(|left, right| left.port.cmp(&right.port));
        identity
    }

    pub fn verification_is_current(&self, branch_id: &str) -> Result<bool> {
        let branch = self
            .branch(branch_id)
            .ok_or_else(|| anyhow::anyhow!("unknown TAS branch {branch_id:?}"))?;
        let Some(verification) = &branch.verification else {
            return Ok(false);
        };
        Ok(verification.branch_movie_sha256 == self.branch_movie_sha256(branch_id)?)
    }
}

impl TasBranch {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn comment(&self) -> &str {
        &self.comment
    }

    pub fn parent(&self) -> Option<&TasBranchOrigin> {
        self.parent.as_ref()
    }

    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    pub fn input_spans(&self) -> &[TasInputSpan] {
        &self.input_spans
    }

    pub fn events(&self) -> &[ReplayEvent] {
        &self.events
    }

    pub fn verification(&self) -> Option<&TasVerificationProvenance> {
        self.verification.as_ref()
    }

    pub fn input_at(&self, frame: u64) -> TasInputFrame {
        let index = self
            .input_spans
            .partition_point(|span| span.start.saturating_add(span.length) <= frame);
        self.input_spans
            .get(index)
            .filter(|span| span.start <= frame)
            .map_or_else(TasInputFrame::default, |span| span.input)
    }
}

fn validate_identity(identity: &TasProjectIdentity) -> Result<()> {
    validate_id(&identity.system, "system")?;
    validate_id(&identity.core_family, "core family")?;
    validate_id(&identity.determinism_abi, "determinism ABI")?;
    validate_id(
        &identity.state_format_compatibility_id,
        "state format compatibility ID",
    )?;
    if identity.devices.is_empty() {
        bail!("TAS project identity must declare at least one device");
    }
    for patch in &identity.patches {
        validate_id(&patch.format, "patch format")?;
    }
    let mut firmware_ids = HashSet::new();
    for firmware in &identity.firmware {
        let firmware_id = match firmware {
            TasFirmwareIdentity::External { firmware_id, .. }
            | TasFirmwareIdentity::Hle { firmware_id, .. }
            | TasFirmwareIdentity::BuiltinOpenSource { firmware_id, .. }
            | TasFirmwareIdentity::Skipped { firmware_id, .. } => firmware_id,
        };
        if !firmware_ids.insert(firmware_id.as_str()) {
            bail!("duplicate TAS firmware ID {firmware_id:?}");
        }
        match firmware {
            TasFirmwareIdentity::External {
                firmware_id,
                variant,
                ..
            } => {
                validate_id(firmware_id, "firmware ID")?;
                if let Some(variant) = variant {
                    validate_text(variant, "firmware variant", 256)?;
                }
            }
            TasFirmwareIdentity::Hle {
                firmware_id,
                implementation,
                ..
            }
            | TasFirmwareIdentity::BuiltinOpenSource {
                firmware_id,
                implementation,
                ..
            } => {
                validate_id(firmware_id, "firmware ID")?;
                validate_id(implementation, "firmware implementation")?;
            }
            TasFirmwareIdentity::Skipped { firmware_id, .. } => {
                validate_id(firmware_id, "firmware ID")?;
            }
        }
    }
    let mut ports = HashSet::new();
    for device in &identity.devices {
        validate_id(&device.port, "device port")?;
        validate_id(&device.device, "device")?;
        if !ports.insert(device.port.as_str()) {
            bail!("duplicate TAS device port {:?}", device.port);
        }
    }
    Ok(())
}

fn firmware_id(identity: &TasFirmwareIdentity) -> &str {
    match identity {
        TasFirmwareIdentity::External { firmware_id, .. }
        | TasFirmwareIdentity::Hle { firmware_id, .. }
        | TasFirmwareIdentity::BuiltinOpenSource { firmware_id, .. }
        | TasFirmwareIdentity::Skipped { firmware_id, .. } => firmware_id,
    }
}

fn validate_input_spans(branch: &TasBranch, assets: &BTreeMap<TasDigest, Vec<u8>>) -> Result<()> {
    let mut previous_end = 0;
    let mut previous_input = None;
    for span in &branch.input_spans {
        if span.length == 0 {
            bail!("TAS input spans cannot be empty");
        }
        let end = span
            .start
            .checked_add(span.length)
            .ok_or_else(|| anyhow::anyhow!("TAS input span overflows"))?;
        if end > branch.frame_count {
            bail!("TAS input span extends past its branch end");
        }
        if span.start < previous_end {
            bail!("TAS input spans must be sorted and non-overlapping");
        }
        if span.start == previous_end && previous_input == Some(span.input) {
            bail!("adjacent identical TAS input spans must be merged");
        }
        if span.input == TasInputFrame::default() {
            bail!("neutral TAS input spans must be omitted");
        }
        if let TasCameraInput::Blob(digest) = span.input.camera
            && !assets.contains_key(&digest)
        {
            bail!("TAS input references a missing camera asset");
        }
        previous_end = end;
        previous_input = Some(span.input);
    }
    Ok(())
}

fn validate_id(value: &str, name: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        bail!("invalid TAS {name} {value:?}");
    }
    Ok(())
}

fn validate_text(value: &str, name: &str, max_bytes: usize) -> Result<()> {
    if value.len() > max_bytes || value.chars().any(char::is_control) {
        bail!("invalid TAS {name}");
    }
    Ok(())
}
