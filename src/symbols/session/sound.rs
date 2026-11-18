use super::*;
use crate::debug::DebugTarget;

impl SymbolSession {
    pub(crate) fn for_md_sound(sha256: [u8; 32]) -> Self {
        Self {
            target: DebugTarget::Sound,
            identity: Some(RomIdentity {
                system: System::Md,
                sha256,
            }),
            ..Self::default()
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn load_md_sound(sha256: [u8; 32], path: &std::path::Path) -> Self {
        let mut session = Self::for_md_sound(sha256);
        session.load_imported_sidecar(path.to_path_buf(), session.identity.unwrap(), false);
        session
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbols::import::{
        ImportContext, TargetInfo, import_md_sound_symbols, import_symbols,
    };

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn independent_md_sessions_resolve_equal_addresses_and_source_only_in_target() {
        let mut sound = SymbolSession::for_md_sound([0x5a; 32]);
        let mut main = SymbolSession {
            identity: sound.identity,
            ..SymbolSession::default()
        };
        let ctx = ImportContext {
            target: TargetInfo { system: System::Md },
            image: ImageId(0),
            rom_region: RegionId(0),
            cpu_space: DebugTarget::Sound.cpu_space(),
            source_name: None,
        };
        let module = import_md_sound_symbols("sound.sym", b"[labels]\n00:0200 SoundEntry\n[source files v2]\n0001:0002 12345678 sound.asm\n[addr-to-line mapping v2]\n00000000 00:0200 0200 0001:0002:0000002A", &ctx).unwrap();
        sound.store.extend(module.symbols);
        sound.extend_source_metadata(module.source_files, module.source_lines, None);
        let primary = ImportContext {
            cpu_space: DebugTarget::Primary.cpu_space(),
            ..ctx
        };
        main.store.extend(
            import_symbols("main.sym", b"000200 T MainEntry", &primary)
                .unwrap()
                .symbols,
        );
        assert_eq!(sound.symbol_name_at_cpu_address(0x200), Some("SoundEntry"));
        assert_eq!(main.symbol_name_at_cpu_address(0x200), Some("MainEntry"));
        assert_eq!(sound.resolve_cpu_name("SoundEntry"), Some(0x200));
        assert_eq!(main.resolve_cpu_name("SoundEntry"), None);
        assert_eq!(sound.resolve_cpu_name("MainEntry"), None);
        assert_eq!(sound.symbol_name_at_cpu_address(0x1_0200), None);
        assert_eq!(sound.identity.unwrap().system, System::Md);
        assert_eq!(sound.identity.unwrap().sha256, [0x5a; 32]);
        assert_eq!(sound.exec_mode(), crate::symbols::ExecMode::Z80);
        assert!(sound.resolve_rom_name("SoundEntry").is_none());
        assert!(sound.load_instances().is_empty());
        let location = CpuLocation {
            space: DebugTarget::Sound.cpu_space(),
            address: 0x200,
        };
        assert_eq!(sound.source_at_cpu(location).unwrap().line, 42);
        assert!(main.source_at_cpu(location).is_none());
        assert!(sound.remove_user_symbol("SoundEntry").is_err());
    }
}
