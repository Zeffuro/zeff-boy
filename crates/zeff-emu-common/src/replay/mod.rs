mod event_stream;
mod events;
mod format;
mod input;
mod metadata;
mod reader;
mod start_metadata;
mod validation;
mod writer;
pub use event_stream::{decode_replay_event_stream, encode_replay_event_stream};
pub use events::{
    ReplayEvent, ReplayGameBoyLinkAction, ReplayGameBoyLinkCoordinatorOwner,
    ReplayGameBoyLinkCoordinatorState, ReplayGameBoyLinkEvent, ReplayGameBoyLinkReply,
    ReplayGameBoyLinkState, ReplayGameBoyPassiveCompletion, ReplayWonderSwanLinkEvent,
};
pub(crate) use format::{
    CAMERA_REPEAT_SENTINEL, FRAME_FIXED_BYTES, LEGACY_GB_SAVE_STATE_MAGIC,
    LEGACY_NES_SAVE_STATE_MAGIC, MAGIC, MAX_REPLAY_CAMERA_FRAME_BYTES, MetadataCursor,
    V1_FRAME_FIXED_BYTES, VERSION, read_bool, read_optional_u8, write_optional_hash,
    write_optional_string, write_optional_u8, write_optional_u64, write_string, write_u32,
    write_u64,
};
pub use input::{POCKET_CAMERA_FRAME_BYTES, ReplayJoypadFrame, ReplayZapperFrame};
pub use metadata::{
    ReplayCheckpoint, ReplayFirmwareManifest, ReplayMetadata, firmware_manifests_match,
};
pub use reader::{ReplayLoadLimits, ReplayPlayer};
pub use start_metadata::{
    ReplayStartMetadata, decode_replay_start_metadata, encode_replay_start_metadata,
    validate_replay_start_events,
};
pub use writer::ReplayRecorder;

#[cfg(test)]
mod tests;
