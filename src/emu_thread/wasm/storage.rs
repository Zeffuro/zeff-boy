use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use super::{EmuThread, Inner};
use crate::emu_backend::EmuBackend;
use crate::emu_thread::recovery::{RecoveryCoordinator, browser_battery_flush_due};
use crate::emu_thread::speculation::SpeculationBoundary;
use crate::emu_thread::types::{EmuCommand, EmuResponse, SharedFramebuffer};

pub(super) const BATTERY_FLUSH_INTERVAL: Duration = Duration::from_secs(30);
pub(super) const MAX_DEFERRED_STORAGE_COMMANDS: usize = 16;

enum StorageCompletion {
    SaveState {
        path: PathBuf,
        backup_created: bool,
    },
    RestoreStateBackup {
        path: PathBuf,
    },
    Battery {
        epoch: u64,
        snapshot: Vec<crate::platform::SaveWrite>,
        path: Option<String>,
        generation: crate::save_paths::recovery_state::BatteryGenerationRecord,
        recovery_path: Option<PathBuf>,
        shutdown: bool,
    },
}

pub(super) struct PendingStorage {
    completion: crate::platform::SaveBatchCompletion,
    response: StorageCompletion,
}

pub(super) struct CapturedBatteryWrites {
    pub(super) path: Option<String>,
    pub(super) generation: crate::save_paths::recovery_state::BatteryGenerationRecord,
    pub(super) recovery_path: Option<PathBuf>,
}

impl EmuThread {
    pub(super) fn is_storage_ordered_command(command: &EmuCommand) -> bool {
        matches!(
            command,
            EmuCommand::SaveStateSlot(_)
                | EmuCommand::LoadStateSlot { .. }
                | EmuCommand::SaveStateToPath(_)
                | EmuCommand::LoadStateFromPath { .. }
                | EmuCommand::InspectRecovery { .. }
                | EmuCommand::FlushBatterySram
                | EmuCommand::RestoreStateBackup(_)
                | EmuCommand::Shutdown
        )
    }

    pub(super) fn begin_save_state(
        backend: &EmuBackend,
        pending_storage: &mut Option<PendingStorage>,
        pending_responses: &mut VecDeque<EmuResponse>,
        path: PathBuf,
    ) {
        let captured = crate::platform::capture_save_writes(|| {
            let bytes = backend.encode_external_state_bytes()?;
            crate::save_paths::write_state_bytes_to_file_with_backup(&path, &bytes)
        });
        let (backup_created, writes) = match captured {
            Ok(captured) => captured,
            Err(error) => {
                pending_responses.push_back(EmuResponse::SaveStateFailed(error.to_string()));
                return;
            }
        };
        let completion = Rc::new(RefCell::new(None));
        crate::platform::commit_save_writes(writes, completion.clone());
        *pending_storage = Some(PendingStorage {
            completion,
            response: StorageCompletion::SaveState {
                path,
                backup_created,
            },
        });
    }

    pub(super) fn begin_restore_state_backup(
        pending_storage: &mut Option<PendingStorage>,
        pending_responses: &mut VecDeque<EmuResponse>,
        path: PathBuf,
    ) {
        let captured = crate::platform::capture_save_writes(|| {
            crate::save_paths::restore_state_file_backup(&path)
        });
        let (_, writes) = match captured {
            Ok(captured) => captured,
            Err(error) => {
                pending_responses
                    .push_back(EmuResponse::StateBackupRestoreFailed(error.to_string()));
                return;
            }
        };
        let completion = Rc::new(RefCell::new(None));
        crate::platform::commit_save_writes(writes, completion.clone());
        *pending_storage = Some(PendingStorage {
            completion,
            response: StorageCompletion::RestoreStateBackup { path },
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn request_battery_flush(
        backend: &mut EmuBackend,
        recovery: &mut RecoveryCoordinator,
        save_recovery_on_shutdown: bool,
        pending_storage: &mut Option<PendingStorage>,
        pending_responses: &mut VecDeque<EmuResponse>,
        next_battery_flush: &mut web_time::Instant,
        battery_dirty: &mut crate::platform::DirtyEpoch<Vec<crate::platform::SaveWrite>>,
        battery_flush_requested: &mut bool,
        battery_potentially_dirty: &mut bool,
        shutdown_requested: &mut bool,
        speculation: &mut SpeculationBoundary,
        shutdown: bool,
    ) {
        *shutdown_requested |= shutdown;
        let terminal = *shutdown_requested;
        let terminal_ready = terminal.then(|| speculation.prepare_terminal_persistence());
        if pending_storage.is_some() {
            *battery_flush_requested = true;
            return;
        }
        *battery_flush_requested = false;
        *next_battery_flush = web_time::Instant::now() + BATTERY_FLUSH_INTERVAL;
        let captured = match terminal_ready {
            Some(ready) => Self::capture_terminal_save_writes(
                ready,
                backend,
                recovery,
                save_recovery_on_shutdown,
            ),
            None => Self::capture_battery_save_writes(backend, recovery, false),
        };
        let (captured, snapshot) = match captured {
            Ok(captured) => captured,
            Err(error) => {
                pending_responses.push_back(EmuResponse::SramFlushFailed(error.to_string()));
                if terminal && save_recovery_on_shutdown {
                    pending_responses.push_back(EmuResponse::RecoverySaveFailed(
                        "browser recovery transaction could not be prepared".to_string(),
                    ));
                }
                if std::mem::take(shutdown_requested) {
                    pending_responses.push_back(EmuResponse::ShutdownComplete);
                }
                return;
            }
        };
        let CapturedBatteryWrites {
            path,
            generation,
            recovery_path,
        } = captured;
        let epoch = battery_dirty.observe(&snapshot);
        if crate::platform::save_writes_are_committed(&snapshot) {
            backend.acknowledge_battery_commit(true);
            recovery.acknowledge_generation(generation);
            *battery_potentially_dirty = false;
            pending_responses.push_back(EmuResponse::SramFlushed(None));
            if let Some(path) = recovery_path {
                pending_responses.push_back(EmuResponse::RecoverySaved(path));
            }
            if std::mem::take(shutdown_requested) {
                pending_responses.push_back(EmuResponse::ShutdownComplete);
            }
            return;
        }

        let completion = Rc::new(RefCell::new(None));
        crate::platform::commit_save_writes(snapshot.clone(), completion.clone());
        *pending_storage = Some(PendingStorage {
            completion,
            response: StorageCompletion::Battery {
                epoch,
                snapshot,
                path,
                generation,
                recovery_path,
                shutdown: terminal,
            },
        });
    }

    pub(super) fn capture_terminal_save_writes(
        _ready: crate::emu_thread::speculation::TerminalPersistenceReady,
        backend: &mut EmuBackend,
        recovery: &RecoveryCoordinator,
        save_recovery_on_shutdown: bool,
    ) -> anyhow::Result<(CapturedBatteryWrites, Vec<crate::platform::SaveWrite>)> {
        let include_recovery = save_recovery_on_shutdown && backend.supports_save_states();
        Self::capture_battery_save_writes(backend, recovery, include_recovery)
    }

    fn capture_battery_save_writes(
        backend: &mut EmuBackend,
        recovery: &RecoveryCoordinator,
        include_recovery: bool,
    ) -> anyhow::Result<(CapturedBatteryWrites, Vec<crate::platform::SaveWrite>)> {
        crate::platform::capture_save_writes(|| {
            let path = backend.flush_battery_sram()?;
            let generation = recovery.capture_generation_write(backend)?;
            let recovery_path = include_recovery
                .then(|| recovery.encode_and_capture_recovery_write(backend, generation))
                .transpose()?;
            Ok(CapturedBatteryWrites {
                path,
                generation,
                recovery_path,
            })
        })
    }

    pub(super) fn poll_storage(&self) {
        let mut next_command = None;
        {
            let mut inner = self.inner.borrow_mut();
            let completed = inner.pending_storage.as_ref().and_then(|pending| {
                pending
                    .completion
                    .borrow_mut()
                    .take()
                    .map(|result| (result, ()))
            });
            if let Some((result, ())) = completed {
                let pending = inner
                    .pending_storage
                    .take()
                    .expect("pending storage disappeared");
                Self::finish_storage(&mut inner, pending.response, result);
            }

            let browser_flush_due = browser_battery_flush_due(
                inner.battery_flush_requested,
                inner.battery_potentially_dirty,
                web_time::Instant::now() >= inner.next_battery_flush,
            );
            if inner.pending_storage.is_none()
                && inner.deferred_storage_commands.is_empty()
                && browser_flush_due
            {
                let Inner {
                    backend,
                    pending_responses,
                    pending_storage,
                    next_battery_flush,
                    battery_dirty,
                    battery_flush_requested,
                    battery_potentially_dirty,
                    shutdown_requested,
                    recovery,
                    save_recovery_on_shutdown,
                    speculation,
                    ..
                } = &mut *inner;
                Self::request_battery_flush(
                    backend,
                    recovery,
                    *save_recovery_on_shutdown,
                    pending_storage,
                    pending_responses,
                    next_battery_flush,
                    battery_dirty,
                    battery_flush_requested,
                    battery_potentially_dirty,
                    shutdown_requested,
                    speculation,
                    false,
                );
            }
            if inner.pending_storage.is_none() {
                next_command = inner.deferred_storage_commands.pop_front();
            }
        }
        if let Some(command) = next_command {
            self.send(command);
        }
    }

    fn finish_storage(
        inner: &mut Inner,
        completion: StorageCompletion,
        result: Result<(), String>,
    ) {
        match completion {
            StorageCompletion::SaveState {
                path,
                backup_created,
            } => match result {
                Ok(()) => inner.pending_responses.push_back(EmuResponse::SaveStateOk {
                    path,
                    backup_created,
                }),
                Err(error) => inner
                    .pending_responses
                    .push_back(EmuResponse::SaveStateFailed(error)),
            },
            StorageCompletion::RestoreStateBackup { path } => match result {
                Ok(()) => inner
                    .pending_responses
                    .push_back(EmuResponse::StateBackupRestored(path)),
                Err(error) => inner
                    .pending_responses
                    .push_back(EmuResponse::StateBackupRestoreFailed(error)),
            },
            StorageCompletion::Battery {
                epoch,
                snapshot,
                path,
                generation,
                recovery_path,
                shutdown,
            } => match result {
                Ok(()) => {
                    let still_current = inner.battery_dirty.acknowledges(epoch, &snapshot)
                        && inner.backend.battery_component_hash() == generation.component_sha256
                        && crate::platform::save_writes_are_committed(&snapshot);
                    if still_current {
                        inner.backend.acknowledge_battery_commit(true);
                        inner.recovery.acknowledge_generation(generation);
                        inner.battery_potentially_dirty = false;
                        inner
                            .pending_responses
                            .push_back(EmuResponse::SramFlushed(path));
                        if let Some(path) = recovery_path {
                            inner
                                .pending_responses
                                .push_back(EmuResponse::RecoverySaved(path));
                        }
                        if shutdown || inner.shutdown_requested {
                            inner.shutdown_requested = false;
                            inner
                                .pending_responses
                                .push_back(EmuResponse::ShutdownComplete);
                        }
                    } else {
                        inner.backend.acknowledge_battery_commit(false);
                        inner.battery_potentially_dirty = true;
                        inner.battery_flush_requested = true;
                    }
                }
                Err(error) => {
                    inner.battery_potentially_dirty = true;
                    inner
                        .pending_responses
                        .push_back(EmuResponse::SramFlushFailed(error));
                    if shutdown && inner.save_recovery_on_shutdown {
                        inner
                            .pending_responses
                            .push_back(EmuResponse::RecoverySaveFailed(
                                "browser recovery transaction did not commit".to_string(),
                            ));
                    }
                    if shutdown || inner.shutdown_requested {
                        inner.shutdown_requested = false;
                        inner
                            .pending_responses
                            .push_back(EmuResponse::ShutdownComplete);
                    }
                }
            },
        }
    }

    pub(super) fn load_state_sync(
        backend: &mut EmuBackend,
        slot: u8,
        buttons_pressed: u8,
        dpad_pressed: u8,
        shared_fb: &SharedFramebuffer,
    ) -> EmuResponse {
        let path = match backend.slot_path(slot) {
            Ok(p) => p,
            Err(e) => return EmuResponse::LoadStateFailed(e.to_string()),
        };
        let result = backend.load_state_from_path(&path);
        Self::respond_load_state(
            backend,
            result,
            path.display().to_string(),
            buttons_pressed,
            dpad_pressed,
            shared_fb,
        )
    }
}
