use std::path::PathBuf;

#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

use anyhow::Context;
use zeff_emu_common::system::System;

use crate::emu_backend::EmuBackend;

#[cfg(not(target_arch = "wasm32"))]
use super::discovery::{
    discover_dbg_sidecar, discover_elf_sidecar, discover_map_sidecar, discover_namelist_sidecars,
    discover_symbol_sidecar, discover_zdbg_sidecar, user_symbol_sidecar_path,
};
use super::{LoadedSymbolModule, SymbolSession};
use crate::symbols::identity::{IdentityStatus, RomIdentity};
#[cfg(not(target_arch = "wasm32"))]
use crate::symbols::import::{ImportContext, TargetInfo, import_symbols};
#[cfg(not(target_arch = "wasm32"))]
use crate::symbols::{AddressSpaceId, ImageId, RegionId, SymbolKind};
use crate::symbols::{
    Confidence, Provenance, ProvenanceKind, SymbolId, SymbolRecord, SymbolScope, UserSymbolDraft,
};

#[cfg(not(target_arch = "wasm32"))]
const MAX_SYMBOL_FILE_BYTES: u64 = 64 * 1024 * 1024;

impl SymbolSession {
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn load_for_backend(backend: &EmuBackend) -> Self {
        Self::load_for_paths(
            backend.system(),
            backend.rom_path(),
            backend.source_path(),
            backend.rom_hash(),
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn load_for_paths(
        system: System,
        rom_path: &Path,
        source_path: &Path,
        rom_sha256: [u8; 32],
    ) -> Self {
        Self::load_sidecar(system, rom_path, source_path, rom_sha256)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn load_for_paths_with_sidecar(
        system: System,
        rom_path: &Path,
        source_path: &Path,
        rom_sha256: [u8; 32],
        sidecar_path: &Path,
    ) -> Self {
        let identity = RomIdentity {
            system,
            sha256: rom_sha256,
        };
        let mut session = Self {
            identity: Some(identity),
            user_path: Some(user_symbol_sidecar_path(source_path, rom_path)),
            ..Self::default()
        };
        let selected_user_path = session.user_path.as_deref() == Some(sidecar_path);
        if sidecar_path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.to_ascii_lowercase().ends_with(".zdbg.json"))
        {
            session.load_zdbg_sidecar(sidecar_path.to_path_buf(), identity);
        } else {
            session.load_imported_sidecar(sidecar_path.to_path_buf(), identity, false);
        }
        if !selected_user_path {
            session.load_user_sidecar();
        }
        session.load_platform_symbols(system);
        session
    }

    pub(crate) fn loading() -> Self {
        Self {
            loading: true,
            ..Self::default()
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn load_for_backend(backend: &EmuBackend) -> Self {
        let mut session = Self {
            identity: Some(RomIdentity {
                system: backend.system(),
                sha256: backend.rom_hash(),
            }),
            ..SymbolSession::default()
        };
        session.load_platform_symbols(backend.system());
        session
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn load_sidecar(
        system: System,
        rom_path: &Path,
        source_path: &Path,
        rom_sha256: [u8; 32],
    ) -> Self {
        let identity = RomIdentity {
            system,
            sha256: rom_sha256,
        };
        let mut session = Self {
            identity: Some(identity),
            user_path: Some(user_symbol_sidecar_path(source_path, rom_path)),
            ..Self::default()
        };
        let mut labels_loaded = false;
        if let Some(path) = discover_zdbg_sidecar(source_path, rom_path) {
            labels_loaded = session.load_zdbg_sidecar(path, identity);
        }
        if system == System::Gba
            && let Some(path) = discover_elf_sidecar(source_path, rom_path)
        {
            labels_loaded |= session.load_imported_sidecar(path, identity, labels_loaded);
        }
        if let Some(path) = discover_symbol_sidecar(source_path, rom_path) {
            labels_loaded |= session.load_imported_sidecar(path, identity, labels_loaded);
        }
        if let Some(path) = discover_map_sidecar(source_path, rom_path) {
            session.load_imported_sidecar(path, identity, labels_loaded);
        }
        if let Some(path) = discover_dbg_sidecar(source_path, rom_path) {
            session.load_imported_sidecar(path, identity, false);
        }
        for path in discover_namelist_sidecars(system, source_path, rom_path) {
            session.load_imported_sidecar(path, identity, false);
        }
        session.load_user_sidecar();
        session.load_platform_symbols(system);
        session
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn load_zdbg_sidecar(&mut self, path: PathBuf, identity: RomIdentity) -> bool {
        match crate::symbols::user::load_zdbg_symbols(&path, identity) {
            Ok(loaded) => {
                let supplied_labels = loaded
                    .symbols
                    .iter()
                    .any(|symbol| symbol.kind != SymbolKind::Section);
                let count = loaded.symbols.len();
                self.store.extend(loaded.symbols);
                self.segments.extend(loaded.segments);
                self.load_instances.extend(loaded.load_instances);
                self.modules.push(LoadedSymbolModule {
                    path,
                    format: "Zeff Debug Symbols".to_owned(),
                    identity: IdentityStatus::Exact,
                    symbol_count: count,
                    source_line_count: 0,
                    diagnostics: Vec::new(),
                });
                supplied_labels
            }
            Err(error) => {
                self.diagnostics
                    .push(format!("{}: {error}", path.display()));
                false
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn load_imported_sidecar(
        &mut self,
        path: PathBuf,
        identity: RomIdentity,
        sections_only: bool,
    ) -> bool {
        let result = (|| -> anyhow::Result<_> {
            let metadata = std::fs::metadata(&path)?;
            anyhow::ensure!(
                metadata.len() <= MAX_SYMBOL_FILE_BYTES,
                "symbol file is larger than 64 MiB"
            );
            let data = std::fs::read(&path)?;
            let mut module = import_symbols(
                path.file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("symbols.sym"),
                &data,
                &ImportContext {
                    target: TargetInfo {
                        system: identity.system,
                    },
                    image: ImageId(0),
                    rom_region: RegionId(0),
                    cpu_space: AddressSpaceId(0),
                    source_name: Some(path.display().to_string()),
                },
            )?;
            let supplied_labels = module
                .symbols
                .iter()
                .any(|symbol| symbol.kind != SymbolKind::Section);
            if sections_only {
                module
                    .symbols
                    .retain(|symbol| symbol.kind == SymbolKind::Section);
            }
            let count = module.symbols.len();
            let source_line_count = module.source_lines.len();
            self.extend_source_metadata(
                module.source_files,
                module.source_lines,
                path.parent().map(Path::to_path_buf),
            );
            self.store.extend(module.symbols.drain(..));
            Ok((
                LoadedSymbolModule {
                    path: path.clone(),
                    format: module.format,
                    identity: IdentityStatus::compare(None, identity),
                    symbol_count: count,
                    source_line_count,
                    diagnostics: module.diagnostics,
                },
                supplied_labels,
            ))
        })();
        match result {
            Ok((module, supplied_labels)) => {
                log::info!(
                    "Loaded {} symbols from {} ({})",
                    module.symbol_count,
                    module.path.display(),
                    module.identity.label()
                );
                self.modules.push(module);
                supplied_labels
            }
            Err(error) => {
                self.diagnostics
                    .push(format!("{}: {error}", path.display()));
                false
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn load_user_sidecar(&mut self) {
        let (Some(path), Some(identity)) = (&self.user_path, self.identity) else {
            return;
        };
        if !path.is_file() {
            return;
        }
        match crate::symbols::user::load_user_symbols(path, identity) {
            Ok(symbols) => {
                log::info!(
                    "Loaded {} user symbols from {}",
                    symbols.len(),
                    path.display()
                );
                self.user_symbols = symbols;
                self.store.replace_user_symbols(self.user_symbols.clone());
            }
            Err(error) => self
                .diagnostics
                .push(format!("{}: {error}", path.display())),
        }
    }

    fn load_platform_symbols(&mut self, system: System) {
        let symbols = crate::symbols::platform::symbols(system);
        if symbols.is_empty() {
            return;
        }
        let count = symbols.len();
        self.store.extend(symbols);
        self.modules.push(LoadedSymbolModule {
            path: PathBuf::from("Built-in"),
            format: "Platform labels".to_owned(),
            identity: IdentityStatus::BuiltIn,
            symbol_count: count,
            source_line_count: 0,
            diagnostics: Vec::new(),
        });
    }

    pub(crate) fn upsert_user_symbol(
        &mut self,
        draft: UserSymbolDraft,
    ) -> anyhow::Result<Option<PathBuf>> {
        anyhow::ensure!(!draft.name.trim().is_empty(), "symbol name is empty");
        anyhow::ensure!(draft.name.len() <= 256, "symbol name is too long");
        anyhow::ensure!(
            !draft.name.chars().any(char::is_control),
            "symbol name contains a control character"
        );
        anyhow::ensure!(
            draft.location.cpu.is_some()
                || draft.location.storage.is_some()
                || draft.value.is_some(),
            "symbol has no location or value"
        );
        let identity = self.identity.context("no ROM is loaded")?;
        let source = {
            #[cfg(not(target_arch = "wasm32"))]
            {
                self.user_path
                    .as_ref()
                    .map(|path| path.display().to_string())
            }
            #[cfg(target_arch = "wasm32")]
            {
                None
            }
        };
        let record = SymbolRecord {
            id: SymbolId(0),
            name: draft.name,
            location: draft.location,
            value: draft.value,
            size: draft.size,
            kind: draft.kind,
            scope: SymbolScope::Global,
            provenance: Provenance {
                kind: ProvenanceKind::User,
                source,
            },
            confidence: Confidence::Exact,
            comment: draft.comment,
        };
        let mut symbols = self.user_symbols.clone();
        symbols.retain(|symbol| !symbol.name.eq_ignore_ascii_case(&record.name));
        symbols.push(record);

        #[cfg(not(target_arch = "wasm32"))]
        if let Some(path) = &self.user_path {
            crate::symbols::user::save_user_symbols(path, identity, &symbols)?;
        }

        self.user_symbols = symbols;
        self.store.replace_user_symbols(self.user_symbols.clone());
        #[cfg(not(target_arch = "wasm32"))]
        return Ok(self.user_path.clone());
        #[cfg(target_arch = "wasm32")]
        Ok(None)
    }

    pub(crate) fn remove_user_symbol(&mut self, name: &str) -> anyhow::Result<Option<PathBuf>> {
        let identity = self.identity.context("no ROM is loaded")?;
        let mut symbols = self.user_symbols.clone();
        let original_len = symbols.len();
        symbols.retain(|symbol| !symbol.name.eq_ignore_ascii_case(name));
        anyhow::ensure!(symbols.len() != original_len, "no user label named {name}");

        #[cfg(not(target_arch = "wasm32"))]
        if let Some(path) = &self.user_path {
            crate::symbols::user::save_user_symbols(path, identity, &symbols)?;
        }

        self.user_symbols = symbols;
        self.store.replace_user_symbols(self.user_symbols.clone());
        #[cfg(not(target_arch = "wasm32"))]
        return Ok(self.user_path.clone());
        #[cfg(target_arch = "wasm32")]
        Ok(None)
    }
}
