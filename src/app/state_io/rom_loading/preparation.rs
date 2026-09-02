use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use crate::emu_backend::ActiveSystem;
use crate::rom_archive::PendingArchiveSelection;

use super::{App, PreparedRomLoad};

pub(super) enum RomPreparationPoll {
    Pending,
    Complete(super::super::super::RomPreparationOutcome),
    Disconnected,
}

pub(super) fn cancel_rom_preparation_slot(
    slot: &mut Option<super::super::super::PendingRomPreparation>,
) -> bool {
    let Some(pending) = slot.take() else {
        return false;
    };
    pending.cancel.store(true, Ordering::Release);
    true
}

pub(super) fn poll_rom_preparation_slot(
    slot: &mut Option<super::super::super::PendingRomPreparation>,
) -> RomPreparationPoll {
    loop {
        let Some(pending) = slot.as_ref() else {
            return RomPreparationPoll::Pending;
        };
        match pending.receiver.try_recv() {
            Ok(result) if result.request_id == pending.request_id => {
                slot.take();
                return RomPreparationPoll::Complete(result.outcome);
            }
            Ok(_) => {}
            Err(std::sync::mpsc::TryRecvError::Empty) => return RomPreparationPoll::Pending,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                slot.take();
                return RomPreparationPoll::Disconnected;
            }
        }
    }
}

impl App {
    pub(crate) fn cancel_pending_rom_preparation(&mut self, notify: bool) {
        if cancel_rom_preparation_slot(&mut self.pending_rom_preparation) && notify {
            self.toast_manager.info("Archive load canceled");
        }
    }

    pub(super) fn begin_native_archive_preparation(
        &mut self,
        source_path: &Path,
        selected_entry_index: Option<usize>,
        expected_rom_path: Option<PathBuf>,
        auto_load_state: bool,
    ) {
        self.cancel_pending_rom_preparation(false);
        self.stop_emu_thread();
        let request_id = self.next_rom_preparation_id;
        self.next_rom_preparation_id = self.next_rom_preparation_id.wrapping_add(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let progress =
            Arc::new(crate::emu_backend::pce_cd_archive::PceCdPackageProgress::default());
        let mut config = self.backend_load_config(ActiveSystem::Pce);
        config.initial_input = None;
        let source_path = source_path.to_path_buf();
        let worker_source_path = source_path.clone();
        let worker_cancel = Arc::clone(&cancel);
        let worker_progress = Arc::clone(&progress);
        let (sender, receiver) = std::sync::mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("zeff-archive-load".to_owned())
            .spawn(move || {
                let result = crate::emu_backend::loader::prepare_native_archive_backend(
                    &worker_source_path,
                    selected_entry_index,
                    expected_rom_path.as_deref(),
                    &config,
                    &worker_cancel,
                    &worker_progress,
                );
                let outcome = if worker_cancel.load(Ordering::Acquire) {
                    super::super::super::RomPreparationOutcome::Cancelled
                } else {
                    match result {
                        Ok(crate::emu_backend::loader::PreparedNativeArchiveBackend::Ready {
                            rom_path,
                            system,
                            loaded,
                        }) => super::super::super::RomPreparationOutcome::Ready {
                            source_path: worker_source_path,
                            rom_path,
                            system,
                            auto_load_state,
                            loaded,
                        },
                        Ok(
                            crate::emu_backend::loader::PreparedNativeArchiveBackend::Selection(
                                entries,
                            ),
                        ) => super::super::super::RomPreparationOutcome::ArchiveSelection {
                            source_path: worker_source_path,
                            entries,
                        },
                        Err(error) => {
                            super::super::super::RomPreparationOutcome::Failed(format!("{error:#}"))
                        }
                    }
                };
                let _ = sender.send(super::super::super::RomPreparationResult {
                    request_id,
                    outcome,
                });
            });
        match worker {
            Ok(_) => {
                self.pending_rom_preparation = Some(super::super::super::PendingRomPreparation {
                    request_id,
                    source_path,
                    started_at: std::time::Instant::now(),
                    cancel,
                    progress,
                    receiver,
                });
            }
            Err(error) => {
                self.toast_manager
                    .error(format!("Couldn't start archive loader: {error}"));
            }
        }
    }

    pub(in crate::app) fn poll_rom_preparation(&mut self) {
        let outcome = match poll_rom_preparation_slot(&mut self.pending_rom_preparation) {
            RomPreparationPoll::Pending => return,
            RomPreparationPoll::Complete(outcome) => outcome,
            RomPreparationPoll::Disconnected => {
                self.toast_manager
                    .error("Archive loader stopped unexpectedly");
                return;
            }
        };
        match outcome {
            super::super::super::RomPreparationOutcome::Ready {
                source_path,
                rom_path,
                system,
                auto_load_state,
                mut loaded,
            } => {
                let sample_rate = self
                    .audio
                    .as_ref()
                    .map_or(crate::audio::DEFAULT_AUDIO_SAMPLE_RATE, |audio| {
                        audio.emulator_sample_rate()
                    });
                loaded.backend.set_sample_rate(sample_rate);
                let (buttons, dpad) = self.host_joypad_input_for_system(system);
                loaded.backend.set_input(buttons, dpad);
                self.commit_prepared_rom(PreparedRomLoad {
                    source_path,
                    rom_path,
                    system,
                    auto_load_state,
                    backend: loaded.backend,
                    original_crc: loaded.original_crc32,
                });
            }
            super::super::super::RomPreparationOutcome::ArchiveSelection {
                source_path,
                entries,
            } => {
                self.pending_archive_selection = Some(PendingArchiveSelection {
                    archive_path: source_path,
                    entries,
                });
                self.toast_manager
                    .info("Archive contains multiple ROMs; choose one to load");
            }
            super::super::super::RomPreparationOutcome::Failed(error) => {
                log::error!("Failed to load archive: {error}");
                self.toast_manager
                    .error(format!("Failed to load archive: {error}"));
            }
            super::super::super::RomPreparationOutcome::Cancelled => {}
        }
    }
}
