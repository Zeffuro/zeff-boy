mod midi;

#[cfg(not(target_arch = "wasm32"))]
mod events;

#[cfg(test)]
mod tests;

pub(crate) fn ogg_vorbis_supported() -> bool {
    cfg!(feature = "audio-recording")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AudioTimelineDiscontinuity {
    StateLoad,
    Rewind,
    Reset,
    DebuggerMutation,
    GuestCallUndo,
}

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use native::AudioRecorder;

#[cfg(target_arch = "wasm32")]
mod wasm;
#[cfg(target_arch = "wasm32")]
pub(crate) use wasm::AudioRecorder;
