use std::path::PathBuf;

use crate::app::SymbolLoadBinding;
use crate::debug::DebugTarget;
use crate::emu_backend::{ActiveSystem, EmuBackend};

#[cfg(test)]
mod sound_tests;

use super::{App, automatic_symbol_loading_available};

impl App {
    pub(in crate::app) fn start_symbol_load(&mut self, backend: &EmuBackend) {
        self.start_symbol_load_for_paths(
            backend.system(),
            backend.rom_path().to_path_buf(),
            backend.source_path().to_path_buf(),
            backend.rom_hash(),
            automatic_symbol_loading_available(backend),
        );
    }

    pub(in crate::app) fn start_symbol_load_for_paths(
        &mut self,
        system: ActiveSystem,
        rom_path: PathBuf,
        source_path: PathBuf,
        rom_hash: [u8; 32],
        supports_symbol_loading: bool,
    ) {
        self.pending_symbol_load = None;
        self.pending_sound_symbol_load = None;
        self.sound_symbols = if system == ActiveSystem::Md {
            crate::symbols::SymbolSession::for_md_sound(rom_hash)
        } else {
            crate::symbols::SymbolSession::default()
        };
        self.debug_windows.reset_target_sessions();
        self.symbol_load_owner = self.symbol_load_owner.saturating_add(1);
        if !supports_symbol_loading {
            self.pending_symbol_load = None;
            self.symbols = crate::symbols::SymbolSession::default();
            return;
        }
        self.start_symbol_load_request(
            DebugTarget::Primary,
            system,
            (rom_path, source_path),
            rom_hash,
            None,
        );
    }

    fn start_symbol_load_request(
        &mut self,
        target: DebugTarget,
        system: ActiveSystem,
        paths: (PathBuf, PathBuf),
        rom_hash: [u8; 32],
        sidecar_path: Option<PathBuf>,
    ) {
        let (rom_path, source_path) = paths;
        if self.symbol_load_owner == u64::MAX {
            self.toast_manager
                .error("Symbol loader ownership exhausted");
            return;
        }
        if target == DebugTarget::Sound && (system != ActiveSystem::Md || sidecar_path.is_none()) {
            return;
        }
        let Some(next) = self.next_symbol_load_id.checked_add(1) else {
            self.toast_manager.error("Symbol request IDs exhausted");
            return;
        };
        let request_id = self.next_symbol_load_id;
        self.next_symbol_load_id = next;
        let binding = SymbolLoadBinding {
            owner: self.symbol_load_owner,
            target,
            rom_hash,
            request_id,
        };
        *self.symbol_slot_mut(target) = if target == DebugTarget::Sound {
            crate::symbols::SymbolSession::for_md_sound(rom_hash)
        } else {
            crate::symbols::SymbolSession::loading()
        };
        *self.pending_symbol_slot_mut(target) = None;
        let (sender, receiver) = std::sync::mpsc::channel();
        let started = std::time::Instant::now();
        let worker = std::thread::Builder::new()
            .name("zeff-symbol-load".to_owned())
            .spawn(move || {
                let session = if target == DebugTarget::Sound {
                    crate::symbols::SymbolSession::load_md_sound(
                        rom_hash,
                        sidecar_path.as_ref().unwrap(),
                    )
                } else {
                    sidecar_path.map_or_else(
                        || {
                            crate::symbols::SymbolSession::load_for_paths(
                                system,
                                &rom_path,
                                &source_path,
                                rom_hash,
                            )
                        },
                        |path| {
                            crate::symbols::SymbolSession::load_for_paths_with_sidecar(
                                system,
                                &rom_path,
                                &source_path,
                                rom_hash,
                                &path,
                            )
                        },
                    )
                };
                let _ = sender.send(super::super::super::SymbolLoadResult {
                    binding,
                    request_id,
                    elapsed: started.elapsed(),
                    session,
                });
            });
        match worker {
            Ok(_) => {
                *self.pending_symbol_slot_mut(target) =
                    Some(super::super::super::PendingSymbolLoad {
                        binding,
                        request_id,
                        receiver,
                    });
                self.toast_manager.info("Loading symbols...");
            }
            Err(error) => {
                *self.symbol_slot_mut(target) = crate::symbols::SymbolSession::default();
                self.toast_manager
                    .error(format!("Couldn't start symbol loader: {error}"));
            }
        }
    }

    pub(in crate::app) fn open_symbol_file_dialog(&mut self) {
        if self.netplay.fenced() {
            self.toast_manager
                .info("Disconnect netplay before opening symbol files");
            return;
        }
        if !self.core_supports_symbol_loading() {
            self.pending_symbol_load = None;
            self.symbols = crate::symbols::SymbolSession::default();
            return;
        }
        let (Some(rom_path), Some(source_path), Some(rom_hash)) = (
            self.rom_info.rom_path.clone(),
            self.rom_info.source_path.clone(),
            self.rom_info.rom_hash,
        ) else {
            self.toast_manager.error("Load a ROM first");
            return;
        };
        self.pause_for_dialog();
        let path = crate::platform::FileDialog::new()
            .add_filter(
                "Symbol files",
                &["elf", "axf", "sym", "map", "dbg", "nl", "json"],
            )
            .add_filter("All files", &["*"])
            .set_title("Load Symbol File")
            .pick_file();
        self.resume_after_dialog();
        if let Some(path) = path {
            self.start_symbol_load_request(
                self.debug_windows.debug_target,
                self.active_system,
                (rom_path, source_path),
                rom_hash,
                Some(path),
            );
        }
    }

    fn symbol_slot_mut(&mut self, target: DebugTarget) -> &mut crate::symbols::SymbolSession {
        match target {
            DebugTarget::Primary => &mut self.symbols,
            DebugTarget::Sound => &mut self.sound_symbols,
        }
    }

    fn pending_symbol_slot_mut(
        &mut self,
        target: DebugTarget,
    ) -> &mut Option<crate::app::PendingSymbolLoad> {
        match target {
            DebugTarget::Primary => &mut self.pending_symbol_load,
            DebugTarget::Sound => &mut self.pending_sound_symbol_load,
        }
    }

    pub(in crate::app) fn poll_symbol_load(&mut self) {
        self.poll_symbol_target(DebugTarget::Primary);
        self.poll_symbol_target(DebugTarget::Sound);
    }

    fn poll_symbol_target(&mut self, target: DebugTarget) {
        let slot = self.pending_symbol_slot_mut(target);
        let Some(pending) = slot.as_ref() else {
            return;
        };
        let result = match pending.receiver.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                *self.pending_symbol_slot_mut(target) = None;
                *self.symbol_slot_mut(target) = crate::symbols::SymbolSession::default();
                self.toast_manager
                    .error("Symbol loader stopped unexpectedly");
                return;
            }
        };
        let expected = pending.binding;
        if result.request_id != pending.request_id
            || result.binding != expected
            || result.binding.target != target
            || result.session.target() != target
            || !result
                .session
                .matches_rom(self.active_system, result.binding.rom_hash)
            || result.binding.owner != self.symbol_load_owner
            || self.rom_info.rom_hash != Some(result.binding.rom_hash)
        {
            *self.pending_symbol_slot_mut(target) = None;
            return;
        }
        *self.pending_symbol_slot_mut(target) = None;
        let imported_symbol_count = result
            .session
            .modules
            .iter()
            .filter(|module| !module.is_builtin())
            .map(|module| module.symbol_count)
            .sum::<usize>();
        *self.symbol_slot_mut(target) = result.session;
        self.debug_windows.trace.cached_sequence = None;
        self.debug_windows.trace.cached_filter.clear();
        if imported_symbol_count > 0 {
            self.toast_manager.info(format!(
                "Loaded {imported_symbol_count} symbols in {} ms",
                result.elapsed.as_millis()
            ));
        } else if let Some(diagnostic) = self.symbol_slot_mut(target).diagnostics.first().cloned() {
            self.toast_manager
                .info(format!("Symbol load skipped: {diagnostic}"));
        }
    }
}
