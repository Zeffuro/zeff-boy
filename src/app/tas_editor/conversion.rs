use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use crate::emu_backend::loader::PrivateTasExecutionLoader;
use crate::tas_project::TasEditorSession;
use crate::tas_project::verification::TasVerifiedReplayExportPhase;
use anyhow::{Result, bail};

use super::App;

impl App {
    pub(super) fn import_replay_as_tas_dialog(&mut self) {
        let Some(source_path) = self.rom_info.source_path.clone() else {
            self.toast_manager.error("Load the replay's game first");
            return;
        };
        let loader =
            match crate::emu_backend::loader::select_private_tas_execution_loader_with_rom_path(
                source_path.clone(),
                self.rom_info.rom_path.clone(),
                self.active_system,
                self.settings.emulation.firmware_search_dirs(),
            ) {
                Ok(loader) => loader,
                Err(error) => {
                    self.toast_manager
                        .error(format!("Could not import replay: {error:#}"));
                    return;
                }
            };

        self.pause_for_dialog();
        let replay_path = crate::platform::FileDialog::new()
            .set_title("Import replay as TAS")
            .add_filter("Zeff replay", &["zrpl"])
            .pick_file();
        self.resume_after_dialog();
        let Some(replay_path) = replay_path else {
            return;
        };

        let mut dialog = crate::platform::FileDialog::new()
            .set_title("Save imported TAS project")
            .add_filter("Zeff TAS project", &["ztas"])
            .set_file_name(&default_import_name(&replay_path));
        if let Some(parent) = replay_path.parent() {
            dialog = dialog.set_directory(parent);
        }
        self.pause_for_dialog();
        let project_path = dialog.save_file().map(ensure_project_extension);
        self.resume_after_dialog();
        let Some(project_path) = project_path else {
            return;
        };

        let replacing = project_path.exists();
        match loader.import_replay_file(&replay_path, &project_path, replacing) {
            Ok(_) => {
                self.cancel_tas_control();
                match self
                    .debug_windows
                    .tas_editor
                    .open_project(project_path.clone())
                {
                    Ok(()) => {
                        self.reevaluate_tas_execution_attachment();
                        let verb = if replacing { "Replaced" } else { "Imported" };
                        self.toast_manager
                            .info(format!("{verb} {}", project_path.display()));
                    }
                    Err(error) => self.toast_manager.error(format!(
                        "Imported {}, but could not open it: {error:#}",
                        project_path.display()
                    )),
                }
            }
            Err(error) => self
                .toast_manager
                .error(format!("Failed to import replay: {error:#}")),
        }
    }

    pub(super) fn export_tas_as_verified_replay_dialog(&mut self) {
        let Some(session) = self.debug_windows.tas_editor.active_session() else {
            self.toast_manager.error("Open a TAS project first");
            return;
        };
        let Some(source_path) = self.rom_info.source_path.clone() else {
            self.toast_manager
                .error("Load the TAS project's game first");
            return;
        };
        let manual_path = session.manual_path().to_owned();
        let loader =
            match crate::emu_backend::loader::select_private_tas_execution_loader_with_rom_path(
                source_path.clone(),
                self.rom_info.rom_path.clone(),
                self.active_system,
                self.settings.emulation.firmware_search_dirs(),
            ) {
                Ok(loader) => loader,
                Err(error) => {
                    self.toast_manager
                        .error(format!("Could not export replay: {error:#}"));
                    return;
                }
            };

        let mut dialog = crate::platform::FileDialog::new()
            .set_title("Export active TAS branch as verified replay")
            .add_filter("Zeff replay", &["zrpl"])
            .set_file_name(&default_export_name(&manual_path));
        if let Some(parent) = manual_path.parent() {
            dialog = dialog.set_directory(parent);
        }
        self.pause_for_dialog();
        let replay_path = dialog.save_file().map(ensure_replay_extension);
        self.resume_after_dialog();
        let Some(replay_path) = replay_path else {
            return;
        };
        if replay_path.exists() {
            self.toast_manager.error(
                "Verified replay export does not overwrite an existing file; choose a new name",
            );
            return;
        }

        let Some(session) = self.debug_windows.tas_editor.active_session() else {
            self.toast_manager.error("The TAS project was closed");
            return;
        };
        if self.tas_verified_replay_export.is_some() {
            self.toast_manager
                .error("A verified replay export is already running");
            return;
        }
        if self
            .debug_windows
            .tas_editor
            .live_status()
            .holds_authority()
        {
            self.toast_manager
                .error("Finish the live TAS decision before exporting a replay");
            return;
        }

        let source_project = match session.project().encode() {
            Ok(bytes) => bytes,
            Err(error) => {
                self.toast_manager
                    .error(format!("Could not snapshot the TAS project: {error:#}"));
                return;
            }
        };
        let request = VerifiedReplayExportCoordinator::start(
            loader,
            session.clone(),
            replay_path.clone(),
            source_project,
        );
        self.debug_windows.tas_editor.set_verified_export_busy(true);
        self.tas_verified_replay_export = Some(request);
        self.toast_manager.info(format!(
            "Verifying and exporting {} in the background",
            replay_path.display()
        ));
    }

    pub(super) fn poll_verified_replay_export(&mut self) {
        if self
            .debug_windows
            .tas_editor
            .take_verified_export_cancellation_request()
            && let Some(coordinator) = self.tas_verified_replay_export.as_ref()
        {
            coordinator.request_cancel();
        }
        let Some(coordinator) = self.tas_verified_replay_export.as_mut() else {
            return;
        };
        self.debug_windows
            .tas_editor
            .set_verified_export_status(Some(coordinator.status_message()));
        let Some(result) = coordinator.poll() else {
            return;
        };
        self.debug_windows
            .tas_editor
            .set_verified_export_busy(false);
        self.debug_windows
            .tas_editor
            .set_verified_export_status(None);
        self.tas_verified_replay_export = None;
        match result {
            VerifiedReplayExportResult::Completed {
                session,
                replay_path,
                source_project,
            } => {
                let still_current = self
                    .debug_windows
                    .tas_editor
                    .active_session()
                    .and_then(|current| current.project().encode().ok())
                    .is_some_and(|current| current == source_project);
                if !still_current {
                    self.toast_manager.error(format!(
                        "Verified replay export completed for {}, but its project changed before the result could be installed",
                        replay_path.display()
                    ));
                    return;
                }
                self.debug_windows
                    .tas_editor
                    .install_verified_export_session(*session);
                self.toast_manager.info(format!(
                    "Verified, saved, and exported {}",
                    replay_path.display()
                ));
            }
            VerifiedReplayExportResult::Cancelled => self
                .toast_manager
                .info("Verified replay export canceled; no result was installed"),
            VerifiedReplayExportResult::Failed(error) => self
                .toast_manager
                .error(format!("Failed to export verified replay: {error:#}")),
        }
    }
}

pub(crate) struct VerifiedReplayExportCoordinator {
    state: VerifiedReplayExportState,
    task: Option<JoinHandle<Result<TasEditorSession>>>,
    replay_path: PathBuf,
    source_project: Vec<u8>,
    cancellation: Arc<AtomicBool>,
    progress: Arc<Mutex<VerifiedReplayExportProgress>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VerifiedReplayExportState {
    Pending,
    Completed,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VerifiedReplayExportProgress {
    Preparing,
    Capturing,
    Reproducing,
    Saving,
    Publishing,
}

impl VerifiedReplayExportProgress {
    pub(crate) fn message(self, cancel_requested: bool) -> &'static str {
        if cancel_requested {
            "Cancel requested; finishing the current verification step"
        } else {
            match self {
                Self::Preparing => "Preparing verified replay export",
                Self::Capturing => "Verifying replay capture",
                Self::Reproducing => "Reproducing verified replay",
                Self::Saving => "Saving verification to the TAS project",
                Self::Publishing => "Validating and publishing replay",
            }
        }
    }

    fn from_phase(phase: TasVerifiedReplayExportPhase) -> Self {
        match phase {
            TasVerifiedReplayExportPhase::Preparing => Self::Preparing,
            TasVerifiedReplayExportPhase::CaptureVerification => Self::Capturing,
            TasVerifiedReplayExportPhase::ReproductionVerification => Self::Reproducing,
            TasVerifiedReplayExportPhase::SavingProject => Self::Saving,
            TasVerifiedReplayExportPhase::PublishingReplay => Self::Publishing,
        }
    }
}

impl VerifiedReplayExportState {
    fn complete(&mut self) -> Result<()> {
        if *self != Self::Pending {
            bail!("verified replay export is already terminal");
        }
        *self = Self::Completed;
        Ok(())
    }

    fn fail(&mut self) -> Result<()> {
        if *self != Self::Pending {
            bail!("verified replay export is already terminal");
        }
        *self = Self::Failed;
        Ok(())
    }
}

pub(super) enum VerifiedReplayExportResult {
    Completed {
        session: Box<TasEditorSession>,
        replay_path: PathBuf,
        source_project: Vec<u8>,
    },
    Cancelled,
    Failed(anyhow::Error),
}

impl VerifiedReplayExportCoordinator {
    fn start(
        loader: PrivateTasExecutionLoader,
        mut session: TasEditorSession,
        replay_path: PathBuf,
        source_project: Vec<u8>,
    ) -> Self {
        let task_replay_path = replay_path.clone();
        let cancellation = Arc::new(AtomicBool::new(false));
        let task_cancellation = Arc::clone(&cancellation);
        let progress = Arc::new(Mutex::new(VerifiedReplayExportProgress::Preparing));
        let task_progress = Arc::clone(&progress);
        let task = std::thread::spawn(move || {
            loader.verify_and_export_editor_session_cancellable(
                &mut session,
                &task_replay_path,
                &task_cancellation,
                &mut |phase| {
                    *task_progress
                        .lock()
                        .unwrap_or_else(|error| error.into_inner()) =
                        VerifiedReplayExportProgress::from_phase(phase);
                },
            )?;
            Ok(session)
        });
        Self {
            state: VerifiedReplayExportState::Pending,
            task: Some(task),
            replay_path,
            source_project,
            cancellation,
            progress,
        }
    }

    pub(crate) fn request_cancel(&self) {
        self.cancellation.store(true, Ordering::Release);
    }

    pub(crate) fn status_message(&self) -> String {
        let (progress, cancel_requested) = (
            *self
                .progress
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
            self.cancellation.load(Ordering::Acquire),
        );
        progress.message(cancel_requested).to_owned()
    }

    fn poll(&mut self) -> Option<VerifiedReplayExportResult> {
        let task = self.task.as_ref()?;
        if !task.is_finished() {
            return None;
        }
        let task = self.task.take().expect("completed export task is retained");
        match task.join() {
            Ok(Ok(session)) => {
                self.state.complete().expect("completed export was pending");
                Some(VerifiedReplayExportResult::Completed {
                    session: Box::new(session),
                    replay_path: self.replay_path.clone(),
                    source_project: std::mem::take(&mut self.source_project),
                })
            }
            Ok(Err(_error)) if self.cancellation.load(Ordering::Acquire) => {
                self.state.fail().expect("canceled export was pending");
                Some(VerifiedReplayExportResult::Cancelled)
            }
            Ok(Err(error)) => {
                self.state.fail().expect("failed export was pending");
                Some(VerifiedReplayExportResult::Failed(error))
            }
            Err(_) => {
                self.state.fail().expect("panicked export was pending");
                Some(VerifiedReplayExportResult::Failed(anyhow::anyhow!(
                    "verified replay export worker panicked"
                )))
            }
        }
    }
}

fn default_import_name(replay_path: &Path) -> String {
    let stem = replay_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("movie");
    format!("{stem}.ztas")
}

fn ensure_project_extension(path: PathBuf) -> PathBuf {
    if path.extension().is_none() {
        path.with_extension("ztas")
    } else {
        path
    }
}

fn default_export_name(project_path: &Path) -> String {
    let stem = project_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("movie");
    format!("{stem}.zrpl")
}

fn ensure_replay_extension(path: PathBuf) -> PathBuf {
    if path.extension().is_none() {
        path.with_extension("zrpl")
    } else {
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_state_reaches_one_terminal_outcome() {
        let mut completed = VerifiedReplayExportState::Pending;
        completed.complete().unwrap();
        assert_eq!(completed, VerifiedReplayExportState::Completed);
        assert!(completed.fail().is_err());

        let mut failed = VerifiedReplayExportState::Pending;
        failed.fail().unwrap();
        assert_eq!(failed, VerifiedReplayExportState::Failed);
        assert!(failed.complete().is_err());
    }

    #[test]
    fn export_progress_distinguishes_cancellation_from_completion() {
        assert_eq!(
            VerifiedReplayExportProgress::Capturing.message(false),
            "Verifying replay capture"
        );
        assert_eq!(
            VerifiedReplayExportProgress::Publishing.message(true),
            "Cancel requested; finishing the current verification step"
        );
    }

    #[test]
    fn import_dialog_helpers_preserve_explicit_extensions() {
        assert_eq!(default_import_name(Path::new("run.zrpl")), "run.ztas");
        assert_eq!(default_export_name(Path::new("run.ztas")), "run.zrpl");
        assert_eq!(
            ensure_project_extension(PathBuf::from("run")),
            PathBuf::from("run.ztas")
        );
        assert_eq!(
            ensure_project_extension(PathBuf::from("run.bin")),
            PathBuf::from("run.bin")
        );
        assert_eq!(
            ensure_replay_extension(PathBuf::from("run")),
            PathBuf::from("run.zrpl")
        );
    }
}
