use super::*;

mod static_disassembly;
use crate::symbols::import::{ImportContext, TargetInfo, import_symbols};
use crate::symbols::{ExecMode, SymbolLocation, UserSymbolDraft};

fn session() -> SymbolSession {
    let mut session = SymbolSession {
        identity: Some(RomIdentity {
            system: System::Md,
            sha256: [0; 32],
        }),
        ..Default::default()
    };
    let module = import_symbols(
        "game.sym",
        b"00123400 8 T Entry\n00FF8040 2 B Counter",
        &ImportContext {
            target: TargetInfo { system: System::Md },
            image: ImageId(0),
            rom_region: RegionId(0),
            cpu_space: AddressSpaceId(0),
            source_name: None,
        },
    )
    .unwrap();
    session.store.extend(module.symbols);
    session
        .store
        .extend(crate::symbols::platform::symbols(System::Md));
    session
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_symbol_session_resolves_cpu_names_and_platform_labels_without_rom_offsets() {
    let session = session();
    assert_eq!(session.exec_mode(), ExecMode::M68000);
    assert_eq!(session.resolve_cpu_name("entry"), Some(0x123400));
    assert_eq!(session.resolve_cpu_name("Counter"), Some(0xFF8040));
    assert_eq!(session.resolve_cpu_name("VDP_CONTROL"), Some(0xC00004));
    assert_eq!(session.resolve_rom_name("Entry"), None);
    assert_eq!(session.symbol_name_at_rom_offset(0x123400), None);
    assert_eq!(session.symbol_name_at_cpu_address(0x123400), Some("Entry"));
    assert_eq!(
        session.symbol_name_at_cpu_address(0xFF8040),
        Some("Counter")
    );
    assert!(session.has_code_symbol(SymbolLocation {
        cpu: Some(CpuLocation {
            space: AddressSpaceId(0),
            address: 0x123400,
        }),
        storage: None,
        bank: None,
        exec_mode: ExecMode::M68000
    }));
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_user_label_validation_is_transactional_and_preserves_full_addresses() {
    let mut session = session();
    let draft = UserSymbolDraft {
        name: "UserEntry".into(),
        location: SymbolLocation {
            cpu: Some(CpuLocation {
                space: AddressSpaceId(0),
                address: 0x234500,
            }),
            storage: None,
            bank: None,
            exec_mode: ExecMode::Unknown,
        },
        value: None,
        kind: SymbolKind::Function,
        size: Some(8),
        comment: None,
    };
    session.upsert_user_symbol(draft.clone()).unwrap();
    assert_eq!(session.resolve_cpu_name("UserEntry"), Some(0x234500));
    assert_eq!(
        session
            .store
            .lookup_name("UserEntry")
            .next()
            .unwrap()
            .location
            .exec_mode,
        ExecMode::M68000
    );
    for (address, mode) in [
        (0x1_000000, ExecMode::M68000),
        (0xFFFFFF, ExecMode::Unknown),
        (0x123400, ExecMode::Arm),
    ] {
        let mut invalid = draft.clone();
        invalid.location.cpu.as_mut().unwrap().address = address;
        invalid.location.exec_mode = mode;
        assert!(session.upsert_user_symbol(invalid).is_err());
        assert_eq!(session.resolve_cpu_name("UserEntry"), Some(0x234500));
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn md_symbol_sidecars_load_automatically_and_selected_import_errors_stay_visible() {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "zeff-md-session-{}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let rom = dir.join("game.md");
    std::fs::write(&rom, []).unwrap();
    std::fs::write(
        dir.join("game.sym"),
        b"00123400 T Entry\n00FF8040 B Counter",
    )
    .unwrap();
    let session = SymbolSession::load_for_paths(System::Md, &rom, &rom, [0; 32]);
    assert_eq!(session.exec_mode(), ExecMode::M68000);
    assert_eq!(session.resolve_cpu_name("Counter"), Some(0xFF8040));
    assert!(session.diagnostics.is_empty());
    assert_eq!(session.modules[0].format, "GNU nm M68000 .sym");
    let selected = dir.join("selected.sym");
    std::fs::write(&selected, b"00234500 T Selected").unwrap();
    let manual =
        SymbolSession::load_for_paths_with_sidecar(System::Md, &rom, &rom, [0; 32], &selected);
    assert_eq!(manual.resolve_cpu_name("Selected"), Some(0x234500));
    assert_eq!(manual.resolve_cpu_name("Entry"), None);
    std::fs::write(&selected, b"01000000 T Invalid").unwrap();
    let rejected =
        SymbolSession::load_for_paths_with_sidecar(System::Md, &rom, &rom, [0; 32], &selected);
    assert!(!rejected.diagnostics.is_empty());
    assert_eq!(rejected.resolve_cpu_name("Invalid"), None);
    assert!(rejected.modules.iter().all(LoadedSymbolModule::is_builtin));
    std::fs::remove_dir_all(dir).unwrap();
}
