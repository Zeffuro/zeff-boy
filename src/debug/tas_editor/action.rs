use std::path::PathBuf;

use crate::tas_project::TasColecoKeypadKey;

use super::DigitalField;
use super::coleco_input::ColecoControl;
use super::{
    branch_diff_editor, event_editor, input_clipboard, metadata_editor, special_input_editor,
};

#[derive(Debug, Eq, PartialEq)]
pub(super) enum TasEditorAction {
    OpenProject(PathBuf),
    SaveManual,
    Autosave,
    RecoverAutosave,
    Undo,
    Redo,
    ContinueFileRequest {
        save: bool,
    },
    CancelFileRequest,
    CancelAutosaveRecovery,
    SelectBranch(String),
    SelectCursor(u64),
    SelectTimelineFrame {
        frame: u64,
        extend_selection: bool,
    },
    SelectTimelineRange {
        anchor: u64,
        active: u64,
    },
    NavigateTimelineSelection {
        navigation: TasTimelineNavigation,
        extend_selection: bool,
    },
    SelectAllTimelineFrames,
    ClearTimelineSelection,
    SelectLiveExecutionBoundary,
    RequestLiveGoToSelection,
    ExecuteSeek(u64),
    JumpToBranchDiffHunk(branch_diff_editor::TasBranchDiffJumpAction),
    InputClipboard(input_clipboard::TasInputClipboardAction),
    Event(event_editor::TasEventAction),
    Metadata(metadata_editor::TasMetadataAction),
    SpecialInput(special_input_editor::TasSpecialInputAction),
    ToggleDigital {
        cursor: u64,
        player: usize,
        field: DigitalField,
        mask: u8,
    },
    SetDigital {
        cursor: u64,
        player: usize,
        field: DigitalField,
        mask: u8,
        pressed: bool,
    },
    ToggleColecoControl {
        cursor: u64,
        player: usize,
        control: ColecoControl,
    },
    SetColecoKeypad {
        cursor: u64,
        player: usize,
        key: TasColecoKeypadKey,
    },
    InsertNeutralFrames {
        cursor: u64,
        count: u64,
    },
    DeleteFrames {
        start: u64,
        count: u64,
    },
    StartRecordingAtEnd,
    CaptureRecordingFrame,
    StopRecording,
    ForkBranch {
        id: String,
        name: String,
    },
    DeleteBranchSubtree {
        id: String,
    },
    RenameActiveBranch {
        name: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TasTimelineNavigation {
    Previous,
    Next,
    Start,
    End,
}

impl TasEditorAction {
    pub(super) fn blocked_while_live_authority_is_active(&self) -> bool {
        matches!(
            self,
            Self::RecoverAutosave
                | Self::Undo
                | Self::Redo
                | Self::ContinueFileRequest { .. }
                | Self::SelectBranch(_)
                | Self::SelectCursor(_)
                | Self::SelectTimelineFrame { .. }
                | Self::SelectTimelineRange { .. }
                | Self::NavigateTimelineSelection { .. }
                | Self::SelectAllTimelineFrames
                | Self::ClearTimelineSelection
                | Self::SelectLiveExecutionBoundary
                | Self::ExecuteSeek(_)
                | Self::JumpToBranchDiffHunk(_)
                | Self::InputClipboard(_)
                | Self::Event(_)
                | Self::Metadata(_)
                | Self::SpecialInput(_)
                | Self::ToggleDigital { .. }
                | Self::SetDigital { .. }
                | Self::ToggleColecoControl { .. }
                | Self::SetColecoKeypad { .. }
                | Self::InsertNeutralFrames { .. }
                | Self::DeleteFrames { .. }
                | Self::StartRecordingAtEnd
                | Self::CaptureRecordingFrame
                | Self::StopRecording
                | Self::ForkBranch { .. }
                | Self::DeleteBranchSubtree { .. }
                | Self::RenameActiveBranch { .. }
        )
    }
}
