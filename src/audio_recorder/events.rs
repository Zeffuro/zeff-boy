use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;

use crate::audio_tooling::{
    AudioChannelDescriptor, AudioChannelId, AudioRecordingContext, AudioSemanticCaps,
    AudioSemanticFrame, AudioVoiceClass, AudioVoiceState,
};

use super::AudioTimelineDiscontinuity;

const SCHEMA: &str = "zeff-audio-events/1";
static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

pub(super) struct ZeffAudioEventWriter {
    target_path: PathBuf,
    temp_path: PathBuf,
    writer: Option<BufWriter<File>>,
    context: AudioRecordingContext,
    epoch: u32,
    epoch_origin: Option<u64>,
    last_source_frame: Option<u64>,
    last_observed: Option<(u32, u64)>,
    previous: BTreeMap<AudioChannelId, NormalizedVoiceState>,
    frames_written: u64,
    state_events_written: u64,
    poisoned: bool,
    committed: bool,
}

impl ZeffAudioEventWriter {
    pub(super) fn start(target_path: &Path, context: AudioRecordingContext) -> io::Result<Self> {
        validate_context(context)?;
        let (temp_path, file) = create_sibling_temp(target_path)?;
        let mut this = Self {
            target_path: target_path.to_path_buf(),
            temp_path,
            writer: Some(BufWriter::new(file)),
            context,
            epoch: 0,
            epoch_origin: None,
            last_source_frame: None,
            last_observed: None,
            previous: BTreeMap::new(),
            frames_written: 0,
            state_events_written: 0,
            poisoned: false,
            committed: false,
        };
        this.write_topology()?;
        this.write_record(&WireRecord::Epoch {
            epoch: 0,
            reason: "recording_start",
        })?;
        Ok(this)
    }

    pub(super) fn write_frame(&mut self, frame: &AudioSemanticFrame) -> io::Result<()> {
        self.validate_frame(frame)?;
        let voices = frame
            .voices
            .iter()
            .map(|voice| (voice.channel, voice))
            .collect::<BTreeMap<_, _>>();
        let origin = self.epoch_origin.unwrap_or(frame.frame);
        let relative_frame = frame
            .frame
            .checked_sub(origin)
            .ok_or_else(|| invalid_data("audio semantic frame precedes its epoch origin"))?;
        let mut changed = Vec::new();
        let mut next_previous = BTreeMap::new();
        for channel in self.context.topology.channels {
            let voice = voices
                .get(&channel.id)
                .copied()
                .expect("validated frame contains every topology channel");
            let normalized = NormalizedVoiceState::from(voice);
            if self.previous.get(&channel.id) != Some(&normalized) {
                changed.push(WireVoiceState {
                    channel: channel.id.0,
                    active: normalized.active,
                    pitch_hz: normalized.pitch_hz,
                    level: normalized.level,
                });
            }
            next_previous.insert(channel.id, normalized);
        }

        let changed_count = u64::try_from(changed.len())
            .map_err(|_| invalid_data("audio state-event count overflow"))?;
        let next_frames_written = self
            .frames_written
            .checked_add(1)
            .ok_or_else(|| invalid_data("audio frame count overflow"))?;
        let next_state_events_written = self
            .state_events_written
            .checked_add(changed_count)
            .ok_or_else(|| invalid_data("audio state-event count overflow"))?;
        if !changed.is_empty() {
            self.write_record(&WireRecord::Frame {
                epoch: self.epoch,
                frame: relative_frame,
                voices: changed,
            })?;
        }

        self.epoch_origin = Some(origin);
        self.last_source_frame = Some(frame.frame);
        self.last_observed = Some((self.epoch, relative_frame));
        self.previous = next_previous;
        self.frames_written = next_frames_written;
        self.state_events_written = next_state_events_written;
        Ok(())
    }

    pub(super) fn begin_epoch(&mut self, reason: AudioTimelineDiscontinuity) -> io::Result<()> {
        let next_epoch = self
            .epoch
            .checked_add(1)
            .ok_or_else(|| invalid_data("audio event epoch overflow"))?;
        self.write_record(&WireRecord::Epoch {
            epoch: next_epoch,
            reason: discontinuity_name(reason),
        })?;
        self.epoch = next_epoch;
        self.epoch_origin = None;
        self.last_source_frame = None;
        self.previous.clear();
        Ok(())
    }

    pub(super) fn finish(mut self) -> io::Result<PathBuf> {
        if self.poisoned {
            return Err(invalid_data(
                "audio event writer cannot finish after a write failure",
            ));
        }
        self.write_record(&WireRecord::End {
            epochs: self
                .epoch
                .checked_add(1)
                .ok_or_else(|| invalid_data("audio epoch count overflow"))?,
            frames: self.frames_written,
            state_events: self.state_events_written,
            last_epoch: self.last_observed.map(|(epoch, _)| epoch),
            last_frame: self.last_observed.map(|(_, frame)| frame),
        })?;
        let mut writer = self
            .writer
            .take()
            .expect("unfinished event writer owns its temp file");
        writer.flush()?;
        writer.get_ref().sync_all()?;
        drop(writer);

        replace_file(&self.temp_path, &self.target_path)?;
        sync_parent_best_effort(&self.target_path);
        self.committed = true;
        Ok(self.target_path.clone())
    }

    fn write_topology(&mut self) -> io::Result<()> {
        let channels = self
            .context
            .topology
            .channels
            .iter()
            .map(wire_channel)
            .collect();
        self.write_record(&WireRecord::Header {
            schema: SCHEMA,
            system: self.context.system.code(),
            timebase: WireTimebase {
                unit: "emulated_frame",
                origin: "per_epoch",
                sample: "end_of_frame",
            },
            topology: WireTopology {
                generation: self.context.topology.generation,
                channels,
            },
        })
    }

    fn write_record(&mut self, record: &WireRecord<'_>) -> io::Result<()> {
        if self.poisoned {
            return Err(invalid_data(
                "audio event writer is poisoned by an earlier write failure",
            ));
        }
        let mut line = serde_json::to_vec(record).map_err(io::Error::other)?;
        line.push(b'\n');
        let writer = self
            .writer
            .as_mut()
            .ok_or_else(|| invalid_data("audio event writer is already finalized"))?;
        if let Err(error) = writer.write_all(&line) {
            self.poisoned = true;
            return Err(error);
        }
        Ok(())
    }

    fn validate_frame(&self, frame: &AudioSemanticFrame) -> io::Result<()> {
        if self
            .last_source_frame
            .is_some_and(|previous| frame.frame <= previous)
        {
            return Err(invalid_data(format!(
                "audio semantic frame {} is not after frame {} in epoch {}",
                frame.frame,
                self.last_source_frame.unwrap_or_default(),
                self.epoch
            )));
        }
        if frame.voices.len() != self.context.topology.channels.len() {
            return Err(invalid_data(format!(
                "audio semantic frame has {} voices for {} topology channels",
                frame.voices.len(),
                self.context.topology.channels.len()
            )));
        }

        let mut seen = BTreeSet::new();
        for voice in &frame.voices {
            if !seen.insert(voice.channel) {
                return Err(invalid_data(format!(
                    "audio semantic frame repeats channel {}",
                    voice.channel.0
                )));
            }
            let channel = self
                .context
                .topology
                .channels
                .iter()
                .find(|channel| channel.id == voice.channel)
                .ok_or_else(|| {
                    invalid_data(format!(
                        "audio semantic frame contains unknown channel {}",
                        voice.channel.0
                    ))
                })?;
            validate_voice(channel, voice)?;
        }
        Ok(())
    }
}

impl Drop for ZeffAudioEventWriter {
    fn drop(&mut self) {
        if !self.committed {
            self.writer.take();
            let _ = std::fs::remove_file(&self.temp_path);
        }
    }
}

fn validate_context(context: AudioRecordingContext) -> io::Result<()> {
    if context.topology.generation == 0 {
        return Err(invalid_input("audio topology generation must be non-zero"));
    }
    if context.topology.channels.is_empty() {
        return Err(invalid_input(
            "audio topology must contain at least one channel",
        ));
    }
    let mut ids = BTreeSet::new();
    for channel in context.topology.channels {
        if !ids.insert(channel.id) {
            return Err(invalid_input(format!(
                "audio topology repeats channel {}",
                channel.id.0
            )));
        }
        if channel.name.is_empty() || channel.group.is_empty() {
            return Err(invalid_input(format!(
                "audio topology channel {} has an empty name or group",
                channel.id.0
            )));
        }
        if !channel.caps.contains(AudioSemanticCaps::GATE) {
            return Err(invalid_input(format!(
                "audio topology channel {} does not expose gate state",
                channel.id.0
            )));
        }
    }
    Ok(())
}

fn validate_voice(channel: &AudioChannelDescriptor, voice: &AudioVoiceState) -> io::Result<()> {
    if !channel.caps.contains(AudioSemanticCaps::PITCH) && voice.pitch_hz.is_some() {
        return Err(invalid_data(format!(
            "audio semantic channel {} reports unsupported pitch",
            voice.channel.0
        )));
    }
    if !channel.caps.contains(AudioSemanticCaps::LEVEL) && voice.level.is_some() {
        return Err(invalid_data(format!(
            "audio semantic channel {} reports unsupported level",
            voice.channel.0
        )));
    }
    if voice
        .pitch_hz
        .is_some_and(|pitch| !pitch.is_finite() || pitch < 0.0)
    {
        return Err(invalid_data(format!(
            "audio semantic channel {} has invalid pitch",
            voice.channel.0
        )));
    }
    if voice
        .level
        .is_some_and(|level| !level.is_finite() || !(0.0..=1.0).contains(&level))
    {
        return Err(invalid_data(format!(
            "audio semantic channel {} has invalid level",
            voice.channel.0
        )));
    }
    Ok(())
}

fn wire_channel(channel: &AudioChannelDescriptor) -> WireChannel<'_> {
    let mut capabilities = Vec::with_capacity(3);
    for (capability, name) in [
        (AudioSemanticCaps::GATE, "gate"),
        (AudioSemanticCaps::PITCH, "pitch"),
        (AudioSemanticCaps::LEVEL, "level"),
    ] {
        if channel.caps.contains(capability) {
            capabilities.push(name);
        }
    }
    WireChannel {
        id: channel.id.0,
        name: channel.name,
        group: channel.group,
        class: class_name(channel.class),
        capabilities,
        muteable: channel.muteable,
    }
}

fn class_name(class: AudioVoiceClass) -> &'static str {
    match class {
        AudioVoiceClass::Pulse => "pulse",
        AudioVoiceClass::Tone => "tone",
        AudioVoiceClass::Triangle => "triangle",
        AudioVoiceClass::Noise => "noise",
        AudioVoiceClass::Pcm => "pcm",
        AudioVoiceClass::Wavetable => "wavetable",
        AudioVoiceClass::WavetableNoise => "wavetable_noise",
        AudioVoiceClass::Other => "other",
    }
}

fn discontinuity_name(reason: AudioTimelineDiscontinuity) -> &'static str {
    match reason {
        AudioTimelineDiscontinuity::StateLoad => "state_load",
        AudioTimelineDiscontinuity::Rewind => "rewind",
        AudioTimelineDiscontinuity::Reset => "reset",
        AudioTimelineDiscontinuity::DebuggerMutation => "debugger_mutation",
        AudioTimelineDiscontinuity::GuestCallUndo => "guest_call_undo",
    }
}

fn create_sibling_temp(target: &Path) -> io::Result<(PathBuf, File)> {
    let file_name = target
        .file_name()
        .ok_or_else(|| invalid_input("audio event target has no file name"))?;
    let parent = target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    for _ in 0..128 {
        let sequence = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let mut temp_name = OsString::from(file_name);
        temp_name.push(format!(".tmp.{}.{}", std::process::id(), sequence));
        let temp_path = parent.join(temp_name);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
        {
            Ok(file) => return Ok((temp_path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not reserve a unique audio event temp file",
    ))
}

#[cfg(unix)]
fn replace_file(source: &Path, target: &Path) -> io::Result<()> {
    std::fs::rename(source, target)
}

#[cfg(windows)]
fn replace_file(source: &Path, target: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let target = target
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(any(unix, windows)))]
fn replace_file(source: &Path, target: &Path) -> io::Result<()> {
    if target.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "atomic audio event replacement is unsupported on this platform",
        ));
    }
    std::fs::rename(source, target)
}

fn sync_parent_best_effort(target: &Path) {
    #[cfg(unix)]
    if let Some(parent) = target.parent() {
        let _ = File::open(parent).and_then(|directory| directory.sync_all());
    }
    #[cfg(not(unix))]
    let _ = target;
}

fn invalid_input(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[derive(Serialize)]
#[serde(tag = "record", rename_all = "snake_case")]
enum WireRecord<'a> {
    Header {
        schema: &'static str,
        system: &'static str,
        timebase: WireTimebase,
        topology: WireTopology<'a>,
    },
    Epoch {
        epoch: u32,
        reason: &'static str,
    },
    Frame {
        epoch: u32,
        frame: u64,
        voices: Vec<WireVoiceState>,
    },
    End {
        epochs: u32,
        frames: u64,
        state_events: u64,
        last_epoch: Option<u32>,
        last_frame: Option<u64>,
    },
}

#[derive(Serialize)]
struct WireTimebase {
    unit: &'static str,
    origin: &'static str,
    sample: &'static str,
}

#[derive(Serialize)]
struct WireTopology<'a> {
    generation: u32,
    channels: Vec<WireChannel<'a>>,
}

#[derive(Serialize)]
struct WireChannel<'a> {
    id: u16,
    name: &'a str,
    group: &'a str,
    class: &'static str,
    capabilities: Vec<&'static str>,
    muteable: bool,
}

#[derive(Serialize)]
struct WireVoiceState {
    channel: u16,
    active: bool,
    pitch_hz: Option<f64>,
    level: Option<f32>,
}

#[derive(Clone, Copy, Debug)]
struct NormalizedVoiceState {
    active: bool,
    pitch_hz: Option<f64>,
    level: Option<f32>,
}

impl From<&AudioVoiceState> for NormalizedVoiceState {
    fn from(voice: &AudioVoiceState) -> Self {
        Self {
            active: voice.active,
            pitch_hz: voice.pitch_hz.map(normalize_f64_zero),
            level: voice.level.map(normalize_f32_zero),
        }
    }
}

impl PartialEq for NormalizedVoiceState {
    fn eq(&self, other: &Self) -> bool {
        self.active == other.active
            && self.pitch_hz.map(f64::to_bits) == other.pitch_hz.map(f64::to_bits)
            && self.level.map(f32::to_bits) == other.level.map(f32::to_bits)
    }
}

fn normalize_f64_zero(value: f64) -> f64 {
    if value == 0.0 { 0.0 } else { value }
}

fn normalize_f32_zero(value: f32) -> f32 {
    if value == 0.0 { 0.0 } else { value }
}

#[cfg(test)]
mod tests;
