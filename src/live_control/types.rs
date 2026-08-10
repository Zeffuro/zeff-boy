use std::path::PathBuf;
use std::sync::mpsc::Sender;

use serde_json::Value;
use zeff_gb_core::hardware::joypad::JoypadKey;

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
        key: JoypadKey,
        pressed: bool,
    },
    Tap {
        key: JoypadKey,
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
    MemoryRead {
        space: String,
        start: u32,
        length: usize,
    },
    GraphicsInfo,
}

pub(crate) enum LiveReply {
    Ok(Value),
    Error(String),
}

pub(crate) struct LiveRequest {
    pub(crate) command: LiveCommand,
    pub(super) response_tx: Sender<LiveReply>,
}

#[derive(Clone, Copy)]
pub(crate) struct PendingButtonRelease {
    pub(crate) key: JoypadKey,
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
