use sha2::{Digest, Sha256};
use std::ffi::c_void;
use std::os::raw::c_uint;

use super::{
    CALLBACK_STATE, CallbackCounts, FrameAudioStats, SaveRamSnapshot, VideoFrameInfo,
    set_frame_index, snapshot_save_ram, validate_callback_buffers,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContinuationObservation {
    pub callbacks: CallbackCounts,
    pub last_video: VideoFrameInfo,
    pub video_hash: [u8; 32],
    pub audio_hash: [u8; 32],
    pub state_serialized: bool,
    pub state_hash: Option<[u8; 32]>,
    pub save_ram_size: usize,
    pub save_ram_nonnull: bool,
    pub save_ram_hash: Option<[u8; 32]>,
}

#[derive(Clone, Debug)]
pub struct ContinuationProof {
    pub frames: usize,
    pub uninterrupted: ContinuationObservation,
    pub restored: Option<ContinuationObservation>,
    pub callback_counts_match: Option<bool>,
    pub video_match: Option<bool>,
    pub audio_match: Option<bool>,
    pub state_match: Option<bool>,
    pub save_ram_match: Option<bool>,
    pub matches: bool,
}

struct SuspendedCallbackWitness {
    frame_index: usize,
    active_pixel_format: c_uint,
    counts: CallbackCounts,
    last_video: VideoFrameInfo,
    video_hasher: Sha256,
    audio_hasher: Sha256,
    invalid_video_pitch: bool,
    invalid_video_buffer_len: bool,
    invalid_audio_buffer_len: bool,
    capture_frame: Option<usize>,
    frame_audio: Option<Vec<FrameAudioStats>>,
    capture_audio_s16le: bool,
    blackhole_output: bool,
    invalid_audio_frame_index: bool,
    unsupported_environment_commands: Vec<c_uint>,
}

#[derive(Debug)]
struct ObservedContinuation {
    public: ContinuationObservation,
    state: Option<Vec<u8>>,
    save_ram: SaveRamSnapshot,
}

#[derive(Debug)]
pub(super) struct StateCheckResult {
    pub(super) restore_accepted: bool,
    pub(super) reserialize_succeeded: bool,
    pub(super) roundtrip: bool,
    pub(super) save_ram_post_roundtrip: SaveRamSnapshot,
    pub(super) continuation: Option<ContinuationProof>,
}

pub(super) trait ContinuationCore {
    fn run_frame(&mut self);
    fn serialize(&mut self, state: &mut [u8]) -> bool;
    fn unserialize(&mut self, state: &[u8]) -> bool;
    fn snapshot_save_ram(&mut self) -> anyhow::Result<SaveRamSnapshot>;
}

pub(super) struct AbiContinuationCore {
    run: unsafe extern "C" fn(),
    serialize: unsafe extern "C" fn(*mut c_void, usize) -> bool,
    unserialize: unsafe extern "C" fn(*const c_void, usize) -> bool,
    get_memory_data: unsafe extern "C" fn(c_uint) -> *mut c_void,
    get_memory_size: unsafe extern "C" fn(c_uint) -> usize,
}

impl AbiContinuationCore {
    pub(super) const fn new(
        run: unsafe extern "C" fn(),
        serialize: unsafe extern "C" fn(*mut c_void, usize) -> bool,
        unserialize: unsafe extern "C" fn(*const c_void, usize) -> bool,
        get_memory_data: unsafe extern "C" fn(c_uint) -> *mut c_void,
        get_memory_size: unsafe extern "C" fn(c_uint) -> usize,
    ) -> Self {
        Self {
            run,
            serialize,
            unserialize,
            get_memory_data,
            get_memory_size,
        }
    }
}

impl ContinuationCore for AbiContinuationCore {
    fn run_frame(&mut self) {
        unsafe { (self.run)() };
    }

    fn serialize(&mut self, state: &mut [u8]) -> bool {
        unsafe { (self.serialize)(state.as_mut_ptr().cast(), state.len()) }
    }

    fn unserialize(&mut self, state: &[u8]) -> bool {
        unsafe { (self.unserialize)(state.as_ptr().cast(), state.len()) }
    }

    fn snapshot_save_ram(&mut self) -> anyhow::Result<SaveRamSnapshot> {
        unsafe { snapshot_save_ram(self.get_memory_data, self.get_memory_size) }
    }
}

pub(super) fn check_state_and_continuation(
    core: &mut impl ContinuationCore,
    checkpoint: &[u8],
    continuation_start_frame: usize,
    continuation_frames: usize,
) -> anyhow::Result<StateCheckResult> {
    if continuation_frames == 0 {
        let restore_accepted = core.unserialize(checkpoint);
        let mut roundtrip_state = vec![0; checkpoint.len()];
        let reserialize_succeeded = restore_accepted && core.serialize(&mut roundtrip_state);
        let roundtrip = reserialize_succeeded && roundtrip_state == checkpoint;
        return Ok(StateCheckResult {
            restore_accepted,
            reserialize_succeeded,
            roundtrip,
            save_ram_post_roundtrip: core.snapshot_save_ram()?,
            continuation: None,
        });
    }

    let continuation_end_frame = continuation_start_frame
        .checked_add(continuation_frames)
        .ok_or_else(|| anyhow::anyhow!("continuation frame range overflowed"))?;
    with_isolated_continuation_callbacks(|initial_pixel_format| {
        let uninterrupted = observe_continuation(
            core,
            checkpoint.len(),
            continuation_start_frame..continuation_end_frame,
            initial_pixel_format,
        )?;
        let restore_accepted = core.unserialize(checkpoint);
        let mut roundtrip_state = vec![0; checkpoint.len()];
        let reserialize_succeeded = restore_accepted && core.serialize(&mut roundtrip_state);
        let roundtrip = reserialize_succeeded && roundtrip_state == checkpoint;
        let save_ram_post_roundtrip = core.snapshot_save_ram()?;
        let restored = restore_accepted
            .then(|| {
                observe_continuation(
                    core,
                    checkpoint.len(),
                    continuation_start_frame..continuation_end_frame,
                    initial_pixel_format,
                )
            })
            .transpose()?;
        let continuation = compare_continuations(continuation_frames, uninterrupted, restored);
        Ok(StateCheckResult {
            restore_accepted,
            reserialize_succeeded,
            roundtrip,
            save_ram_post_roundtrip,
            continuation: Some(continuation),
        })
    })
}

fn observe_continuation(
    core: &mut impl ContinuationCore,
    state_size: usize,
    frames: std::ops::Range<usize>,
    initial_pixel_format: c_uint,
) -> anyhow::Result<ObservedContinuation> {
    reset_continuation_callback_witness(initial_pixel_format);
    for frame in frames {
        set_frame_index(frame);
        core.run_frame();
    }
    let mut state = vec![0; state_size];
    let state_serialized = core.serialize(&mut state);
    if !state_serialized {
        state.clear();
    }
    let save_ram = core.snapshot_save_ram()?;
    let callback = continuation_callback_observation()?;
    Ok(ObservedContinuation {
        public: ContinuationObservation {
            callbacks: callback.callbacks,
            last_video: callback.last_video,
            video_hash: callback.video_hash,
            audio_hash: callback.audio_hash,
            state_serialized,
            state_hash: state_serialized.then(|| Sha256::digest(&state).into()),
            save_ram_size: save_ram.bytes.len(),
            save_ram_nonnull: save_ram.nonnull,
            save_ram_hash: save_ram.hash,
        },
        state: state_serialized.then_some(state),
        save_ram,
    })
}

#[derive(Clone, Copy)]
struct ContinuationCallbackObservation {
    callbacks: CallbackCounts,
    last_video: VideoFrameInfo,
    video_hash: [u8; 32],
    audio_hash: [u8; 32],
}

fn compare_continuations(
    frames: usize,
    uninterrupted: ObservedContinuation,
    restored: Option<ObservedContinuation>,
) -> ContinuationProof {
    let callback_counts_match = restored
        .as_ref()
        .map(|restored| uninterrupted.public.callbacks == restored.public.callbacks);
    let video_match = restored.as_ref().map(|restored| {
        uninterrupted.public.video_hash == restored.public.video_hash
            && uninterrupted.public.last_video == restored.public.last_video
    });
    let audio_match = restored
        .as_ref()
        .map(|restored| uninterrupted.public.audio_hash == restored.public.audio_hash);
    let state_match =
        restored
            .as_ref()
            .map(|restored| match (&uninterrupted.state, &restored.state) {
                (Some(uninterrupted), Some(restored)) => uninterrupted == restored,
                _ => false,
            });
    let save_ram_match = restored.as_ref().map(|restored| {
        uninterrupted.save_ram.nonnull == restored.save_ram.nonnull
            && uninterrupted.save_ram.bytes == restored.save_ram.bytes
    });
    let matches = [
        callback_counts_match,
        video_match,
        audio_match,
        state_match,
        save_ram_match,
    ]
    .into_iter()
    .all(|matched| matched == Some(true));
    ContinuationProof {
        frames,
        uninterrupted: uninterrupted.public,
        restored: restored.map(|restored| restored.public),
        callback_counts_match,
        video_match,
        audio_match,
        state_match,
        save_ram_match,
        matches,
    }
}

fn with_isolated_continuation_callbacks<T>(
    run: impl FnOnce(c_uint) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    let suspended = suspend_callback_witness();
    let result = run(suspended.active_pixel_format);
    restore_callback_witness(suspended);
    result
}

fn suspend_callback_witness() -> SuspendedCallbackWitness {
    let mut guard = CALLBACK_STATE
        .lock()
        .expect("libretro callback mutex poisoned");
    let state = guard.as_mut().expect("libretro harness is not active");
    let suspended = SuspendedCallbackWitness {
        frame_index: state.frame_index,
        active_pixel_format: state.active_pixel_format,
        counts: std::mem::take(&mut state.counts),
        last_video: std::mem::take(&mut state.last_video),
        video_hasher: std::mem::take(&mut state.video_hasher),
        audio_hasher: std::mem::take(&mut state.audio_hasher),
        invalid_video_pitch: std::mem::take(&mut state.invalid_video_pitch),
        invalid_video_buffer_len: std::mem::take(&mut state.invalid_video_buffer_len),
        invalid_audio_buffer_len: std::mem::take(&mut state.invalid_audio_buffer_len),
        capture_frame: state.capture_frame.take(),
        frame_audio: state.frame_audio.take(),
        capture_audio_s16le: std::mem::take(&mut state.capture_audio_s16le),
        blackhole_output: std::mem::take(&mut state.blackhole_output),
        invalid_audio_frame_index: std::mem::take(&mut state.invalid_audio_frame_index),
        unsupported_environment_commands: std::mem::take(
            &mut state.unsupported_environment_commands,
        ),
    };
    state.blackhole_output = false;
    suspended
}

fn restore_callback_witness(suspended: SuspendedCallbackWitness) {
    let mut guard = CALLBACK_STATE
        .lock()
        .expect("libretro callback mutex poisoned");
    let state = guard.as_mut().expect("libretro harness is not active");
    state.frame_index = suspended.frame_index;
    state.active_pixel_format = suspended.active_pixel_format;
    state.counts = suspended.counts;
    state.last_video = suspended.last_video;
    state.video_hasher = suspended.video_hasher;
    state.audio_hasher = suspended.audio_hasher;
    state.invalid_video_pitch = suspended.invalid_video_pitch;
    state.invalid_video_buffer_len = suspended.invalid_video_buffer_len;
    state.invalid_audio_buffer_len = suspended.invalid_audio_buffer_len;
    state.capture_frame = suspended.capture_frame;
    state.frame_audio = suspended.frame_audio;
    state.capture_audio_s16le = suspended.capture_audio_s16le;
    state.blackhole_output = suspended.blackhole_output;
    state.invalid_audio_frame_index = suspended.invalid_audio_frame_index;
    state.unsupported_environment_commands = suspended.unsupported_environment_commands;
}

fn reset_continuation_callback_witness(initial_pixel_format: c_uint) {
    let mut guard = CALLBACK_STATE
        .lock()
        .expect("libretro callback mutex poisoned");
    let state = guard.as_mut().expect("libretro harness is not active");
    state.active_pixel_format = initial_pixel_format;
    state.counts = CallbackCounts::default();
    state.last_video = VideoFrameInfo::default();
    state.video_hasher = Sha256::default();
    state.audio_hasher = Sha256::default();
    state.invalid_video_pitch = false;
    state.invalid_video_buffer_len = false;
    state.invalid_audio_buffer_len = false;
    state.invalid_audio_frame_index = false;
    state.unsupported_environment_commands.clear();
}

fn continuation_callback_observation() -> anyhow::Result<ContinuationCallbackObservation> {
    let guard = CALLBACK_STATE
        .lock()
        .expect("libretro callback mutex poisoned");
    let state = guard.as_ref().expect("libretro harness is not active");
    validate_callback_buffers(state)?;
    anyhow::ensure!(
        !state.invalid_audio_frame_index,
        "libretro core emitted audio outside the continuation witness"
    );
    Ok(ContinuationCallbackObservation {
        callbacks: state.counts,
        last_video: state.last_video,
        video_hash: state.video_hasher.clone().finalize().into(),
        audio_hash: state.audio_hasher.clone().finalize().into(),
    })
}
