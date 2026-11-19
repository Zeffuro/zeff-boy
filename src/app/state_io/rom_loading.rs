use super::App;
#[cfg(not(target_arch = "wasm32"))]
use crate::emu_backend::loader::is_direct_pce_cd_path;
use crate::emu_backend::{ActiveSystem, BackendLoadConfig, EmuBackend};
use crate::emu_thread::{EmuCommand, EmuResponse, TasControlCommandKind};
use crate::rom_archive::PendingArchiveSelection;
#[cfg(not(target_arch = "wasm32"))]
use anyhow::Context;
use std::path::{Path, PathBuf};

pub(super) use self::backend::direct_netplay_media;
#[cfg(target_arch = "wasm32")]
use self::detection::detect_and_extract_archive_entry_path;
#[cfg(not(target_arch = "wasm32"))]
use self::detection::detect_and_extract_archive_entry_path_with_zip_witness;
use self::detection::{detect_and_extract_archive_entry, is_zip_path};
mod backend;

#[cfg(target_arch = "wasm32")]
#[path = "rom_loading/browser_netplay.rs"]
mod browser_netplay;

mod detection;
mod lifecycle;
#[cfg(not(target_arch = "wasm32"))]
mod netplay;
#[cfg(not(target_arch = "wasm32"))]
mod preparation;
#[cfg(not(target_arch = "wasm32"))]
mod symbols;

pub(crate) use detection::detect_and_extract_rom;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use detection::detect_and_extract_rom_with_zip_witness;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use detection::is_native_archive_path;
#[cfg(not(target_arch = "wasm32"))]
use detection::{ZipMediaRoute, zip_media_route};

fn automatic_symbol_loading_available(backend: &EmuBackend) -> bool {
    backend.supports_symbol_loading()
}

fn should_inspect_recovery(normal_rom_open: bool, supports_save_states: bool) -> bool {
    normal_rom_open && supports_save_states
}

struct PreparedRomLoad {
    source_path: PathBuf,
    rom_path: PathBuf,
    system: ActiveSystem,
    auto_load_state: bool,
    backend: EmuBackend,
    original_crc: u32,
}

struct PreparedRomInput {
    rom_path: PathBuf,
    preloaded_data: Option<Vec<u8>>,
    authenticated_zip_member: Option<crate::rom_archive::AuthenticatedZipMember>,
    system: ActiveSystem,
}

fn dismiss_archive_selection_for_new_load(slot: &mut Option<PendingArchiveSelection>) {
    *slot = None;
}

#[cfg(not(target_arch = "wasm32"))]
fn preflight_direct_pce_cd_source(path: &Path) -> anyhow::Result<()> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("couldn't open PCE-CD source '{}'", path.display()))?;
    if !file
        .metadata()
        .with_context(|| format!("couldn't inspect PCE-CD source '{}'", path.display()))?
        .is_file()
    {
        anyhow::bail!("PCE-CD source '{}' is not a file", path.display());
    }
    Ok(())
}

impl App {
    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn reevaluate_tas_execution_attachment(&mut self) {
        let running_system = self
            .rom_info
            .source_path
            .as_ref()
            .map(|_| self.active_system);
        let project = self
            .debug_windows
            .tas_editor
            .active_session()
            .map(|session| session.project().clone());
        let attachment = crate::emu_backend::loader::select_private_tas_execution_attachment(
            self.rom_info.source_path.clone(),
            self.rom_info.rom_path.clone(),
            running_system,
            self.settings.emulation.firmware_search_dirs(),
            project.as_ref(),
        );
        self.debug_windows.tas_editor.attach_execution(attachment);
    }

    fn load_prepared_rom_with_options(
        &mut self,
        source_path: &Path,
        input: PreparedRomInput,
        auto_load_state: bool,
    ) {
        let PreparedRomInput {
            rom_path,
            preloaded_data,
            authenticated_zip_member,
            system,
        } = input;
        #[cfg(not(target_arch = "wasm32"))]
        self.stop_emu_thread();
        let mut config = self.backend_load_config(system);
        config.authenticated_zip_member = authenticated_zip_member;
        let (backend, original_crc) =
            match self.init_backend(system, source_path, &rom_path, preloaded_data, config) {
                Ok(result) => result,
                Err(e) => {
                    #[cfg(not(target_arch = "wasm32"))]
                    if self.rom_info.rom_path.is_some() {
                        self.stop_game();
                    }
                    log::error!("Failed to load ROM '{}': {}", source_path.display(), e);
                    self.toast_manager.error(format!("Failed to load ROM: {e}"));
                    return;
                }
            };
        self.commit_prepared_rom(PreparedRomLoad {
            source_path: source_path.to_path_buf(),
            rom_path,
            system,
            auto_load_state,
            backend,
            original_crc,
        });
    }

    pub(in crate::app) fn inspect_recovery_after_normal_open(&mut self) {
        self.inspect_recovery(self.settings.emulation.resume_recovery_state);
    }

    pub(in crate::app) fn load_available_recovery(&mut self) {
        self.inspect_recovery(true);
    }

    fn inspect_recovery(&mut self, resume: bool) {
        if !self.core_supports_save_states() {
            return;
        }
        if let Err(error) = self.preflight_emu_command_kind(TasControlCommandKind::StateOrRecovery)
        {
            self.toast_manager.error(error.to_string());
            return;
        }
        let (buttons_pressed, dpad_pressed) = self.current_host_joypad_input();
        if let Err(error) = self.send_emu_command_checked(EmuCommand::InspectRecovery {
            resume,
            buttons_pressed,
            dpad_pressed,
        }) {
            self.toast_manager.error(error.to_string());
            return;
        }
        match self.recv_cold_response() {
            Some(EmuResponse::LoadStateOk {
                path,
                warning,
                media_slot_snapshot,
                game_boy_serial_device,
            }) => {
                self.media_slot_snapshot = media_slot_snapshot;
                if let Some(device) = game_boy_serial_device {
                    self.game_boy_serial_device = device;
                }
                if let Some(thread) = &self.emu_thread {
                    self.latest_frame = thread.shared_framebuffer().load_full();
                }
                log::info!("Resumed recovery state from {path}");
                self.recovery_state_available = false;
                match warning {
                    Some(warning) => self.toast_manager.warning(warning.message()),
                    None => self.toast_manager.success("Resumed recovery state"),
                }
            }
            Some(EmuResponse::RecoveryAvailable(freshness)) => {
                use crate::save_paths::recovery_state::RecoveryFreshness;
                let message = match freshness {
                    RecoveryFreshness::Fresh => {
                        self.recovery_state_available = true;
                        "Recovery state available in File > Load State"
                    }
                    RecoveryFreshness::Stale => "A stale recovery state is available",
                    RecoveryFreshness::Unknown | RecoveryFreshness::Inconsistent => {
                        "Recovery state is available but could not be verified as fresh"
                    }
                };
                if freshness != RecoveryFreshness::Fresh {
                    self.recovery_state_available = false;
                }
                self.toast_manager.info(message);
            }
            Some(EmuResponse::RecoveryRejected(error)) => {
                self.recovery_state_available = false;
                log::warn!("Recovery state rejected: {error}");
                self.toast_manager
                    .error(format!("Recovery state was not loaded: {error}"));
            }
            Some(EmuResponse::RecoveryMissing) => {
                self.recovery_state_available = false;
            }
            Some(EmuResponse::LoadStateFailed(error)) => {
                log::warn!("Recovery state load failed: {error}");
                self.toast_manager
                    .error(format!("Recovery state was not loaded: {error}"));
            }
            _ => {}
        }
    }

    fn load_rom_with_options(&mut self, path: &Path, auto_load_state: bool) {
        #[cfg(not(target_arch = "wasm32"))]
        if is_native_archive_path(path) {
            self.begin_native_archive_preparation(path, None, None, auto_load_state);
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if is_direct_pce_cd_path(path) {
            if let Err(error) = preflight_direct_pce_cd_source(path) {
                let message = format!("{error:#}");
                log::warn!("{message}");
                self.toast_manager.error(message);
                return;
            }
            self.begin_native_archive_preparation(path, None, None, auto_load_state);
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if is_zip_path(path) {
            match zip_media_route(path, None) {
                Ok(ZipMediaRoute::PceCd) => {
                    self.begin_native_archive_preparation(path, None, None, auto_load_state);
                    return;
                }
                Ok(ZipMediaRoute::SelectionRequired) => {
                    if self.begin_archive_selection_if_needed(path) {
                        return;
                    }
                    self.toast_manager
                        .error("Choose a ZIP member before loading mixed PC Engine media");
                    return;
                }
                Ok(ZipMediaRoute::Generic) => {}
                Err(error) => {
                    self.toast_manager
                        .error(format!("Failed to inspect ZIP: {error}"));
                    return;
                }
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.cancel_pending_rom_preparation(false);
        #[cfg(not(target_arch = "wasm32"))]
        let detected = match detect_and_extract_rom_with_zip_witness(path) {
            Ok(result) => result,
            Err(e) => {
                let msg = format!("{e:#}");
                log::warn!("{msg}");
                self.toast_manager.error(msg);
                return;
            }
        };
        #[cfg(target_arch = "wasm32")]
        let (rom_path, preloaded_data, system) = match detect_and_extract_rom(path) {
            Ok(result) => result,
            Err(e) => {
                let msg = format!("{e:#}");
                log::warn!("{msg}");
                self.toast_manager.error(msg);
                return;
            }
        };

        #[cfg(not(target_arch = "wasm32"))]
        self.load_prepared_rom_with_options(
            path,
            PreparedRomInput {
                rom_path: detected.rom_path,
                preloaded_data: detected.preloaded_data,
                authenticated_zip_member: detected.authenticated_zip_member,
                system: detected.system,
            },
            auto_load_state,
        );
        #[cfg(target_arch = "wasm32")]
        self.load_prepared_rom_with_options(
            path,
            PreparedRomInput {
                rom_path,
                preloaded_data,
                authenticated_zip_member: None,
                system,
            },
            auto_load_state,
        );
    }

    pub(in crate::app) fn load_archive_entry_with_options(
        &mut self,
        archive_path: &Path,
        entry_index: usize,
        auto_load_state: bool,
    ) {
        #[cfg(not(target_arch = "wasm32"))]
        if is_zip_path(archive_path) {
            let selected_path =
                match super::list_rom_entries_in_zip(archive_path)
                    .ok()
                    .and_then(|entries| {
                        entries
                            .into_iter()
                            .find(|entry| entry.index == entry_index)
                            .map(|entry| archive_path.join(entry.name))
                    }) {
                    Some(path) => path,
                    None => {
                        self.toast_manager.error(format!(
                            "Selected ZIP member #{entry_index} is no longer available"
                        ));
                        return;
                    }
                };
            self.load_archive_entry_path_with_options(
                archive_path,
                &selected_path,
                auto_load_state,
            );
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if is_native_archive_path(archive_path) {
            self.begin_native_archive_preparation(
                archive_path,
                Some(entry_index),
                None,
                auto_load_state,
            );
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.cancel_pending_rom_preparation(false);
        let (rom_path, preloaded_data, system) =
            match detect_and_extract_archive_entry(archive_path, entry_index) {
                Ok(result) => result,
                Err(e) => {
                    let msg = format!("{e:#}");
                    log::warn!("{msg}");
                    self.toast_manager.error(msg);
                    return;
                }
            };

        self.load_prepared_rom_with_options(
            archive_path,
            PreparedRomInput {
                rom_path,
                preloaded_data,
                authenticated_zip_member: None,
                system,
            },
            auto_load_state,
        );
    }

    fn load_archive_entry_path_with_options(
        &mut self,
        archive_path: &Path,
        virtual_rom_path: &Path,
        auto_load_state: bool,
    ) {
        #[cfg(not(target_arch = "wasm32"))]
        if is_native_archive_path(archive_path) {
            self.begin_native_archive_preparation(
                archive_path,
                None,
                Some(virtual_rom_path.to_path_buf()),
                auto_load_state,
            );
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if is_zip_path(archive_path) {
            match zip_media_route(archive_path, Some(virtual_rom_path)) {
                Ok(ZipMediaRoute::PceCd) => {
                    self.begin_native_archive_preparation(
                        archive_path,
                        None,
                        Some(virtual_rom_path.to_path_buf()),
                        auto_load_state,
                    );
                    return;
                }
                Ok(ZipMediaRoute::Generic) => {}
                Ok(ZipMediaRoute::SelectionRequired) => {
                    self.toast_manager
                        .error("Choose a ZIP member before loading mixed PC Engine media");
                    return;
                }
                Err(error) => {
                    self.toast_manager
                        .error(format!("Failed to inspect ZIP: {error}"));
                    return;
                }
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.cancel_pending_rom_preparation(false);
        #[cfg(not(target_arch = "wasm32"))]
        let detected = match detect_and_extract_archive_entry_path_with_zip_witness(
            archive_path,
            virtual_rom_path,
        ) {
            Ok(result) => result,
            Err(e) => {
                let msg = format!("{e:#}");
                log::warn!("{msg}");
                self.toast_manager.error(msg);
                return;
            }
        };
        #[cfg(target_arch = "wasm32")]
        let (rom_path, preloaded_data, system) =
            match detect_and_extract_archive_entry_path(archive_path, virtual_rom_path) {
                Ok(result) => result,
                Err(e) => {
                    let msg = format!("{e:#}");
                    log::warn!("{msg}");
                    self.toast_manager.error(msg);
                    return;
                }
            };

        #[cfg(not(target_arch = "wasm32"))]
        self.load_prepared_rom_with_options(
            archive_path,
            PreparedRomInput {
                rom_path: detected.rom_path,
                preloaded_data: detected.preloaded_data,
                authenticated_zip_member: detected.authenticated_zip_member,
                system: detected.system,
            },
            auto_load_state,
        );
        #[cfg(target_arch = "wasm32")]
        self.load_prepared_rom_with_options(
            archive_path,
            PreparedRomInput {
                rom_path,
                preloaded_data,
                authenticated_zip_member: None,
                system,
            },
            auto_load_state,
        );
    }

    pub(in crate::app) fn load_rom(&mut self, path: &Path) {
        if let Err(error) = self.preflight_emu_command(&EmuCommand::Reset) {
            self.toast_manager.error(error.to_string());
            return;
        }
        dismiss_archive_selection_for_new_load(&mut self.pending_archive_selection);
        #[cfg(not(target_arch = "wasm32"))]
        self.cancel_pending_rom_preparation(false);
        #[cfg(not(target_arch = "wasm32"))]
        if is_zip_path(path)
            && let Err(error) = zip_media_route(path, None)
        {
            self.toast_manager
                .error(format!("Failed to inspect ZIP: {error}"));
            return;
        }
        if self.begin_archive_selection_if_needed(path) {
            return;
        }
        self.load_rom_with_options(path, true);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::preparation::{
        RomPreparationPoll, cancel_rom_preparation_slot, poll_rom_preparation_slot,
    };
    use super::{
        automatic_symbol_loading_available, dismiss_archive_selection_for_new_load,
        is_direct_pce_cd_path, is_native_archive_path, should_inspect_recovery,
    };
    use crate::emu_backend::{EmuBackend, PceBackend};
    use std::path::PathBuf;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    };

    fn pending_preparation(
        request_id: u64,
        cancel: Arc<AtomicBool>,
        receiver: mpsc::Receiver<super::super::super::RomPreparationResult>,
    ) -> super::super::super::PendingRomPreparation {
        super::super::super::PendingRomPreparation {
            request_id,
            source_path: PathBuf::from("disc.7z"),
            started_at: std::time::Instant::now(),
            cancel,
            progress: Arc::new(crate::emu_backend::pce_cd_archive::PceCdPackageProgress::default()),
            receiver,
        }
    }

    #[test]
    fn pce_disassembly_capability_enables_automatic_symbol_loading() {
        let mut rom = vec![0xEA; 0x2000];
        rom[..4].copy_from_slice(&[0xD4, 0xEA, 0x80, 0xFD]);
        rom[0x1FFE..].copy_from_slice(&0xE000_u16.to_le_bytes());
        let backend =
            EmuBackend::from_pce(PceBackend::new(rom, PathBuf::from("symbols.pce")).unwrap());

        assert!(automatic_symbol_loading_available(&backend));
    }

    #[test]
    fn recovery_inspection_is_limited_to_normal_supported_rom_opens() {
        assert!(should_inspect_recovery(true, true));
        assert!(!should_inspect_recovery(false, true));
        assert!(!should_inspect_recovery(true, false));
    }

    #[test]
    fn native_archive_route_is_separate_from_system_detection() {
        assert!(is_native_archive_path(&PathBuf::from("games.7Z")));
        assert!(is_native_archive_path(&PathBuf::from("disc.RAR")));
        assert_eq!(
            crate::emu_backend::ActiveSystem::from_path(&PathBuf::from("disc.7z")),
            None
        );
    }

    #[test]
    fn direct_pce_cd_media_routes_to_background_preparation() {
        assert!(is_direct_pce_cd_path(&PathBuf::from("disc.CUE")));
        assert!(is_direct_pce_cd_path(&PathBuf::from("disc.chd")));
        assert!(is_direct_pce_cd_path(&PathBuf::from("disc.Iso")));
        assert!(!is_direct_pce_cd_path(&PathBuf::from("game.pce")));
    }

    #[test]
    fn newer_top_level_load_dismisses_an_open_zip_chooser() {
        let mut selection = Some(crate::rom_archive::PendingArchiveSelection {
            archive_path: PathBuf::from("older.zip"),
            entries: Vec::new(),
        });

        dismiss_archive_selection_for_new_load(&mut selection);

        assert!(selection.is_none());
    }

    #[test]
    fn replacing_or_stopping_preparation_cancels_and_drops_its_receiver() {
        let cancel = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::channel();
        let mut slot = Some(pending_preparation(3, Arc::clone(&cancel), receiver));

        assert!(cancel_rom_preparation_slot(&mut slot));
        assert!(cancel.load(Ordering::Acquire));
        assert!(slot.is_none());
        assert!(
            sender
                .send(super::super::super::RomPreparationResult {
                    request_id: 3,
                    outcome: super::super::super::RomPreparationOutcome::Cancelled,
                })
                .is_err()
        );
    }

    #[test]
    fn coordinator_drops_stale_results_and_delivers_current_once() {
        let cancel = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::channel();
        let mut slot = Some(pending_preparation(8, cancel, receiver));
        sender
            .send(super::super::super::RomPreparationResult {
                request_id: 7,
                outcome: super::super::super::RomPreparationOutcome::Failed("stale".to_owned()),
            })
            .unwrap();
        sender
            .send(super::super::super::RomPreparationResult {
                request_id: 8,
                outcome: super::super::super::RomPreparationOutcome::Cancelled,
            })
            .unwrap();

        assert!(matches!(
            poll_rom_preparation_slot(&mut slot),
            RomPreparationPoll::Complete(super::super::super::RomPreparationOutcome::Cancelled)
        ));
        assert!(slot.is_none());
        assert!(matches!(
            poll_rom_preparation_slot(&mut slot),
            RomPreparationPoll::Pending
        ));
    }

    #[test]
    fn failed_preparation_is_reported_without_a_commit_payload() {
        let cancel = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::channel();
        let mut slot = Some(pending_preparation(11, cancel, receiver));
        sender
            .send(super::super::super::RomPreparationResult {
                request_id: 11,
                outcome: super::super::super::RomPreparationOutcome::Failed("broken".to_owned()),
            })
            .unwrap();

        assert!(matches!(
            poll_rom_preparation_slot(&mut slot),
            RomPreparationPoll::Complete(super::super::super::RomPreparationOutcome::Failed(
                error
            )) if error == "broken"
        ));
        assert!(slot.is_none());
    }
}
