use std::path::PathBuf;

use crate::emu_backend::{ActiveSystem, EmuBackend};

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
        if !supports_symbol_loading {
            self.pending_symbol_load = None;
            self.symbols = crate::symbols::SymbolSession::default();
            return;
        }
        self.start_symbol_load_request(system, rom_path, source_path, rom_hash, None);
    }

    fn start_symbol_load_request(
        &mut self,
        system: ActiveSystem,
        rom_path: PathBuf,
        source_path: PathBuf,
        rom_hash: [u8; 32],
        sidecar_path: Option<PathBuf>,
    ) {
        self.pending_symbol_load = None;
        self.symbols = crate::symbols::SymbolSession::loading();
        let request_id = self.next_symbol_load_id;
        self.next_symbol_load_id = self.next_symbol_load_id.wrapping_add(1);
        let (sender, receiver) = std::sync::mpsc::channel();
        let started = std::time::Instant::now();
        let worker = std::thread::Builder::new()
            .name("zeff-symbol-load".to_owned())
            .spawn(move || {
                let session = sidecar_path.map_or_else(
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
                );
                let _ = sender.send(super::super::super::SymbolLoadResult {
                    request_id,
                    elapsed: started.elapsed(),
                    session,
                });
            });
        match worker {
            Ok(_) => {
                self.pending_symbol_load = Some(super::super::super::PendingSymbolLoad {
                    request_id,
                    receiver,
                });
                self.toast_manager.info("Loading symbols...");
            }
            Err(error) => {
                self.symbols = crate::symbols::SymbolSession::default();
                self.toast_manager
                    .error(format!("Couldn't start symbol loader: {error}"));
            }
        }
    }

    pub(in crate::app) fn open_symbol_file_dialog(&mut self) {
        if !self.core_supports_debugger() {
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
        let was_paused = self.pause_for_dialog();
        let path = crate::platform::FileDialog::new()
            .add_filter(
                "Symbol files",
                &["elf", "axf", "sym", "map", "dbg", "nl", "json"],
            )
            .add_filter("All files", &["*"])
            .set_title("Load Symbol File")
            .pick_file();
        self.resume_after_dialog(was_paused);
        if let Some(path) = path {
            self.start_symbol_load_request(
                self.active_system,
                rom_path,
                source_path,
                rom_hash,
                Some(path),
            );
        }
    }

    pub(in crate::app) fn poll_symbol_load(&mut self) {
        let Some(pending) = self.pending_symbol_load.as_ref() else {
            return;
        };
        let result = match pending.receiver.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.pending_symbol_load = None;
                self.symbols = crate::symbols::SymbolSession::default();
                self.toast_manager
                    .error("Symbol loader stopped unexpectedly");
                return;
            }
        };
        if result.request_id != pending.request_id {
            return;
        }
        self.pending_symbol_load = None;
        let imported_symbol_count = result
            .session
            .modules
            .iter()
            .filter(|module| !module.is_builtin())
            .map(|module| module.symbol_count)
            .sum::<usize>();
        self.symbols = result.session;
        if imported_symbol_count > 0 {
            self.toast_manager.info(format!(
                "Loaded {imported_symbol_count} symbols in {} ms",
                result.elapsed.as_millis()
            ));
        } else if let Some(diagnostic) = self.symbols.diagnostics.first() {
            self.toast_manager
                .info(format!("Symbol load skipped: {diagnostic}"));
        }
    }
}
