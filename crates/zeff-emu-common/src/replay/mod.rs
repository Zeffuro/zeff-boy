mod events;
mod format;
mod input;
mod metadata;
mod reader;
mod validation;
mod writer;
pub use events::{
    ReplayEvent, ReplayGameBoyLinkAction, ReplayGameBoyLinkCoordinatorOwner,
    ReplayGameBoyLinkCoordinatorState, ReplayGameBoyLinkEvent, ReplayGameBoyLinkReply,
    ReplayGameBoyLinkState, ReplayGameBoyPassiveCompletion, ReplayWonderSwanLinkEvent,
};
pub(crate) use format::{
    CAMERA_REPEAT_SENTINEL, FRAME_FIXED_BYTES, LEGACY_GB_SAVE_STATE_MAGIC,
    LEGACY_NES_SAVE_STATE_MAGIC, MAGIC, MAX_REPLAY_CAMERA_FRAME_BYTES, MetadataCursor, VERSION,
    read_bool, read_optional_u8, write_optional_hash, write_optional_string, write_optional_u8,
    write_optional_u64, write_string, write_u32, write_u64,
};
pub use input::{POCKET_CAMERA_FRAME_BYTES, ReplayJoypadFrame, ReplayZapperFrame};
pub use metadata::{ReplayCheckpoint, ReplayFirmwareManifest, ReplayMetadata};
pub use reader::ReplayPlayer;
pub use writer::ReplayRecorder;

#[cfg(test)]
mod tests;
