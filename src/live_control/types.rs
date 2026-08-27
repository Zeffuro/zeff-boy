use std::path::PathBuf;
use std::sync::mpsc::Sender;

use crate::input::HostButton;
use serde_json::Value;

#[derive(Debug)]
pub(crate) enum LiveCommand {
    Status,
    DebugInfo,
    Pause,
    Resume,
    TogglePause,
    FrameAdvance,
    SetSlowMotion(bool),
    SetFastForward(bool),
    SetUncapped(bool),
    Button {
        player: u8,
        key: HostButton,
        pressed: bool,
    },
    Tap {
        player: u8,
        key: HostButton,
        frames: usize,
    },
    ColecoKeypad {
        player: u8,
        key: u8,
        pressed: bool,
    },
    TapColecoKeypad {
        player: u8,
        key: u8,
        frames: usize,
    },
    Zapper {
        enabled: bool,
        trigger: bool,
        hit: bool,
        screen_pos: Option<(u16, u16)>,
    },
    Screenshot {
        path: Option<PathBuf>,
    },
    SaveState {
        path: Option<PathBuf>,
    },
    LoadState {
        path: PathBuf,
    },
    SaveStateSlot {
        slot: u8,
    },
    LoadStateSlot {
        slot: u8,
    },
    StartReplayRecording {
        path: PathBuf,
    },
    StopReplayRecording,
    HostLink {
        addr: Option<String>,
    },
    JoinLink {
        addr: Option<String>,
    },
    DisconnectLink,
    MemoryRead {
        space: LiveMemorySpace,
        start: u32,
        length: usize,
    },
    GraphicsInfo,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum LiveMemorySpace {
    #[default]
    Cpu,
    Region(String),
}

impl LiveMemorySpace {
    pub(crate) fn from_wire(value: impl Into<String>) -> Self {
        let value = value.into();
        if is_cpu_space(&value) {
            Self::Cpu
        } else {
            Self::Region(value)
        }
    }

    pub(crate) fn request_name(&self) -> &str {
        match self {
            Self::Cpu => "cpu",
            Self::Region(space) => space,
        }
    }
}

fn is_cpu_space(value: &str) -> bool {
    let normalized = value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|ch| !matches!(ch, '-' | '_' | ' ' | '.'))
        .collect::<String>();
    matches!(normalized.as_str(), "cpu" | "memory" | "ram")
}

pub(crate) enum LiveReply {
    Ok(Value),
    Error(String),
}

pub(crate) struct LiveRequest {
    pub(crate) command: LiveCommand,
    pub(super) response_tx: Sender<LiveReply>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LiveInput {
    Button(HostButton),
    ColecoKeypad(u8),
}

#[derive(Clone, Copy)]
pub(crate) struct PendingButtonRelease {
    pub(crate) player: u8,
    pub(crate) input: LiveInput,
    pub(crate) frames_remaining: usize,
}

impl LiveRequest {
    pub(crate) fn respond_with(self, f: impl FnOnce(LiveCommand) -> LiveReply) {
        let reply = f(self.command);
        let _ = self.response_tx.send(reply);
    }
}

impl LiveReply {
    pub(crate) fn ok(value: Value) -> Self {
        Self::Ok(value)
    }

    pub(crate) fn error(message: impl Into<String>) -> Self {
        Self::Error(message.into())
    }
}
