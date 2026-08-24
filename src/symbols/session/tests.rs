use super::discovery::{
    discover_elf_sidecar, discover_map_sidecar, discover_namelist_sidecars, discover_symbol_sidecar,
};
use super::*;
use crate::symbols::identity::RomIdentity;
use crate::symbols::import::{ImportContext, TargetInfo, import_symbols};
use crate::symbols::{ExecMode, SegmentId, SymbolKind, SymbolLocation, UserSymbolDraft};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

fn temp_dir() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "zeff-symbol-session-{}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn discovers_regular_and_archived_rom_sidecars() {
    let dir = temp_dir();
    let rom = dir.join("game.gbc");
    let sym = dir.join("game.SYM");
    std::fs::write(&rom, []).unwrap();
    std::fs::write(&sym, b"00:0150 Entry").unwrap();
    assert_eq!(
        std::fs::canonicalize(discover_symbol_sidecar(&rom, &rom).unwrap()).unwrap(),
        std::fs::canonicalize(&sym).unwrap()
    );

    let archive = dir.join("collection.zip");
    assert_eq!(
        std::fs::canonicalize(discover_symbol_sidecar(&archive, Path::new("game.gbc")).unwrap())
            .unwrap(),
        std::fs::canonicalize(sym).unwrap()
    );

    let map = dir.join("game.map");
    std::fs::write(&map, b"ROM0 bank #0:").unwrap();
    assert_eq!(
        std::fs::canonicalize(discover_map_sidecar(&archive, Path::new("game.gbc")).unwrap())
            .unwrap(),
        std::fs::canonicalize(map).unwrap()
    );
    let elf = dir.join("game.elf");
    std::fs::write(&elf, b"\x7FELF").unwrap();
    assert_eq!(
        std::fs::canonicalize(discover_elf_sidecar(&archive, Path::new("game.gba")).unwrap())
            .unwrap(),
        std::fs::canonicalize(elf).unwrap()
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn loads_nocash_gba_sidecar() {
    let dir = temp_dir();
    let rom = dir.join("game.gba");
    let sym = dir.join("game.sym");
    std::fs::write(&rom, []).unwrap();
    std::fs::write(&sym, b"080001EC .thumb\n080001EC InitVideo").unwrap();

    let session =
        SymbolSession::load_sidecar(zeff_emu_common::system::System::Gba, &rom, &rom, [0; 32]);
    assert_eq!(session.modules[0].symbol_count, 1);
    assert_eq!(session.modules[0].format, "no$gba .sym");
    assert_eq!(session.resolve_rom_name("InitVideo"), Some(0x1EC));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn loads_gnu_nm_gba_sidecar() {
    let dir = temp_dir();
    let rom = dir.join("game.gba");
    let sym = dir.join("game.sym");
    std::fs::write(&rom, []).unwrap();
    std::fs::write(&sym, b"08000204 g 00000020 Init\n02000000 l 0001c000 gHeap").unwrap();

    let session = SymbolSession::load_sidecar(System::Gba, &rom, &rom, [0; 32]);
    assert_eq!(session.modules[0].format, "GNU nm .sym");
    assert_eq!(session.modules[0].symbol_count, 2);
    assert_eq!(session.resolve_rom_name("Init"), Some(0x204));
    assert_eq!(session.resolve_cpu_name("gHeap"), Some(0x0200_0000));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn explicit_sidecar_overrides_sibling_discovery() {
    let dir = temp_dir();
    let rom = dir.join("game.gbc");
    let selected = dir.join("selected.sym");
    std::fs::write(&rom, []).unwrap();
    std::fs::write(dir.join("game.sym"), b"01:4000 Sibling").unwrap();
    std::fs::write(&selected, b"02:4560 Selected").unwrap();

    let session =
        SymbolSession::load_for_paths_with_sidecar(System::Gb, &rom, &rom, [0; 32], &selected);
    assert_eq!(session.resolve_cpu_name("Selected"), Some(0x4560));
    assert_eq!(session.resolve_cpu_name("Sibling"), None);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
#[ignore = "requires ZEFF_TEST_GNU_NM_SYM with a large GNU nm symbol file"]
fn loads_external_gnu_nm_session_when_configured() {
    let path = std::env::var("ZEFF_TEST_GNU_NM_SYM")
        .expect("ZEFF_TEST_GNU_NM_SYM must name a GNU nm symbol file");
    let sym = PathBuf::from(path);
    let rom = sym.with_extension("gba");
    let started = std::time::Instant::now();
    let session = SymbolSession::load_sidecar(System::Gba, &rom, &rom, [0; 32]);
    eprintln!(
        "external GNU nm session: {} ms",
        started.elapsed().as_millis()
    );
    assert_eq!(session.modules[0].format, "GNU nm .sym");
    assert!(session.modules[0].symbol_count > 70_000);
}

#[test]
fn discovers_fceux_namelists_for_nes() {
    let dir = temp_dir();
    let rom = dir.join("contra.nes");
    std::fs::write(&rom, []).unwrap();
    std::fs::write(dir.join("contra.nes.0.nl"), b"$8000#Start#").unwrap();
    std::fs::write(dir.join("contra.nes.ram.nl"), b"$0010#Frame#").unwrap();
    let paths = discover_namelist_sidecars(System::Nes, &rom, &rom);
    assert_eq!(paths.len(), 2);
    let session = SymbolSession::load_sidecar(System::Nes, &rom, &rom, [0; 32]);
    assert_eq!(
        session
            .modules
            .iter()
            .filter(|module| !module.is_builtin())
            .map(|module| module.symbol_count)
            .sum::<usize>(),
        2
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn map_supplements_sym_with_sections_only() {
    let dir = temp_dir();
    let rom = dir.join("game.gbc");
    std::fs::write(&rom, []).unwrap();
    std::fs::write(dir.join("game.sym"), b"02:4560 UpdatePlayer").unwrap();
    std::fs::write(
            dir.join("game.map"),
            b"ROMX bank #2:\n\tSECTION: $4560-$457f ($0020 bytes) [\"Player code\"]\n\t         $4560 = MapCopy",
        )
        .unwrap();

    let session = SymbolSession::load_sidecar(System::Gb, &rom, &rom, [0; 32]);
    assert_eq!(session.modules.len(), 3);
    assert_eq!(
        session
            .modules
            .iter()
            .filter(|module| !module.is_builtin())
            .map(|module| module.symbol_count)
            .sum::<usize>(),
        2
    );
    assert_eq!(session.resolve_cpu_name("UpdatePlayer"), Some(0x4560));
    assert_eq!(session.resolve_cpu_name("MapCopy"), None);
    assert_eq!(
        session
            .store
            .lookup_name("Player code")
            .next()
            .unwrap()
            .size,
        Some(0x20)
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn map_labels_are_a_fallback_without_sym() {
    let dir = temp_dir();
    let rom = dir.join("game.gbc");
    std::fs::write(&rom, []).unwrap();
    std::fs::write(
            dir.join("game.map"),
            b"ROMX bank #2:\n\tSECTION: $4560-$457f ($0020 bytes) [\"Player code\"]\n\t         $4560 = UpdatePlayer",
        )
        .unwrap();

    let session = SymbolSession::load_sidecar(System::Gb, &rom, &rom, [0; 32]);
    assert_eq!(session.resolve_cpu_name("UpdatePlayer"), Some(0x4560));
    assert_eq!(
        session
            .modules
            .iter()
            .filter(|module| !module.is_builtin())
            .map(|module| module.symbol_count)
            .sum::<usize>(),
        2
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn saves_and_reloads_user_labels_without_a_build_sidecar() {
    let dir = temp_dir();
    let rom = dir.join("game.gbc");
    std::fs::write(&rom, []).unwrap();
    let hash = [7; 32];
    let mut session = SymbolSession::load_sidecar(System::Gb, &rom, &rom, hash);
    let path = session
        .upsert_user_symbol(UserSymbolDraft {
            name: "UpdatePlayer".to_owned(),
            location: SymbolLocation {
                cpu: Some(CpuLocation {
                    space: AddressSpaceId(0),
                    address: 0x4560,
                }),
                storage: Some(StorageLocation {
                    image: ImageId(0),
                    region: RegionId(0),
                    offset: 0x8560,
                }),
                bank: Some(2),
                exec_mode: ExecMode::Sm83,
            },
            value: None,
            kind: SymbolKind::Label,
            size: None,
            comment: Some("Movement update".to_owned()),
        })
        .unwrap()
        .unwrap();
    assert_eq!(path, rom.with_extension("user.zdbg.json"));

    let loaded = SymbolSession::load_sidecar(System::Gb, &rom, &rom, hash);
    assert_eq!(
        loaded
            .modules
            .iter()
            .filter(|module| !module.is_builtin())
            .map(|module| module.symbol_count)
            .sum::<usize>(),
        0
    );
    assert_eq!(
        loaded.symbol_count(),
        1 + super::super::platform::symbols(System::Gb).len()
    );
    assert_eq!(
        loaded.symbol_name_at_rom_offset(0x8560),
        Some("UpdatePlayer")
    );
    assert_eq!(
        loaded
            .store
            .lookup_name("UpdatePlayer")
            .next()
            .unwrap()
            .comment
            .as_deref(),
        Some("Movement update")
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn loads_exact_hash_zdbg_before_other_symbol_files() {
    let dir = temp_dir();
    let rom = dir.join("game.gbc");
    let zdbg = dir.join("game.zdbg.json");
    std::fs::write(&rom, []).unwrap();
    let identity = RomIdentity {
        system: System::Gb,
        sha256: [9; 32],
    };
    let mut source = SymbolSession {
        identity: Some(identity),
        user_path: Some(zdbg.clone()),
        ..SymbolSession::default()
    };
    source
        .upsert_user_symbol(UserSymbolDraft {
            name: "ExactLabel".to_owned(),
            location: SymbolLocation {
                cpu: Some(CpuLocation {
                    space: AddressSpaceId(0),
                    address: 0x4560,
                }),
                storage: Some(StorageLocation {
                    image: ImageId(0),
                    region: RegionId(0),
                    offset: 0x8560,
                }),
                bank: Some(2),
                exec_mode: ExecMode::Sm83,
            },
            value: None,
            kind: SymbolKind::Function,
            size: Some(8),
            comment: None,
        })
        .unwrap();
    std::fs::write(dir.join("game.sym"), b"02:4560 SymLabel").unwrap();

    let loaded = SymbolSession::load_sidecar(System::Gb, &rom, &rom, [9; 32]);
    assert_eq!(loaded.modules[0].format, "Zeff Debug Symbols");
    assert_eq!(loaded.resolve_cpu_name("ExactLabel"), Some(0x4560));
    assert_eq!(loaded.resolve_cpu_name("SymLabel"), None);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn annotates_disassembly_by_physical_rom_offset() {
    let mut session = SymbolSession::default();
    let mut module = import_symbols(
        "game.sym",
        b"02:4560 UpdatePlayer\n02:4660 UpdateNpc\n00:C100 PlayerData",
        &ImportContext {
            target: TargetInfo {
                system: zeff_emu_common::system::System::Gb,
            },
            image: ImageId(0),
            rom_region: RegionId(0),
            cpu_space: AddressSpaceId(0),
            source_name: None,
        },
    )
    .unwrap();
    module.symbols[0].size = Some(8);
    session.store.extend(module.symbols);
    let mut view = crate::debug::DisassemblyView {
        pc: 0x4560,
        mapping: Some(2),
        is_navigation_target: false,
        is_static_target: false,
        location_symbol: None,
        lines: vec![crate::debug::DisassembledLine {
            address: 0x4560,
            storage_offset: Some(0x8560),
            symbol: None,
            control_target: Some(0x4660),
            control_target_storage: Some(0x8660),
            control_target_symbol: None,
            source: None,
            bytes: Default::default(),
            mnemonic: Default::default(),
        }],
        breakpoints: Vec::new(),
        one_shot_breakpoints: Vec::new(),
        rom_breakpoints: Vec::new(),
        hit_rom_breakpoint: None,
    };

    session.annotate_disassembly(&mut view);
    assert_eq!(view.lines[0].symbol.as_deref(), Some("UpdatePlayer"));
    assert_eq!(view.location_symbol.as_deref(), Some("UpdatePlayer"));
    assert_eq!(
        view.lines[0].control_target_symbol.as_deref(),
        Some("UpdateNpc")
    );
    assert_eq!(session.resolve_rom_name("updateplayer"), Some(0x8560));
    assert_eq!(
        session.symbol_context_at_rom_offset(0x8562).as_deref(),
        Some("UpdatePlayer+$2")
    );
    assert_eq!(session.resolve_cpu_name("PlayerData"), Some(0xC100));
    assert_eq!(
        session.symbol_name_at_cpu_address(0xC100),
        Some("PlayerData")
    );
}

#[test]
fn annotates_disassembly_with_wla_source_location() {
    let mut session = SymbolSession::default();
    let mut module = import_symbols(
            "game.sym",
            b"[information]\nversion 3\n[source files v2]\n0001:0002 12345678 src/player.asm\n[addr-to-line mapping v2]\n00008560 02:4560 4560 0001:0002:0000002A\n00008564 02:4564 4564 0001:0002:0000002B",
            &ImportContext {
                target: TargetInfo { system: System::Gb },
                image: ImageId(0),
                rom_region: RegionId(0),
                cpu_space: AddressSpaceId(0),
                source_name: None,
            },
        )
        .unwrap();
    session.extend_source_metadata(module.source_files, module.source_lines, None);
    session.store.extend(module.symbols.drain(..));
    let mut view = crate::debug::DisassemblyView {
        pc: 0x4560,
        mapping: Some(2),
        is_navigation_target: false,
        is_static_target: false,
        location_symbol: None,
        lines: vec![crate::debug::DisassembledLine {
            address: 0x4560,
            storage_offset: Some(0x8560),
            symbol: None,
            control_target: None,
            control_target_storage: None,
            control_target_symbol: None,
            source: None,
            bytes: Default::default(),
            mnemonic: Default::default(),
        }],
        breakpoints: Vec::new(),
        one_shot_breakpoints: Vec::new(),
        rom_breakpoints: Vec::new(),
        hit_rom_breakpoint: None,
    };

    session.annotate_disassembly(&mut view);
    assert_eq!(view.lines[0].source.as_deref(), Some("src/player.asm:42"));
    let source = session.source_reference_for_disassembly(&view).unwrap();
    assert_eq!(source.path, PathBuf::from("src/player.asm"));
    assert_eq!(source.line, 42);
    assert_eq!(source.crc32, Some(0x1234_5678));
    assert_eq!(
        session.source_reference_at_rom_offset(0x8563).unwrap().line,
        42
    );
    assert_eq!(
        session.source_reference_at_rom_offset(0x8564).unwrap().line,
        43
    );
    assert_eq!(
        session.source_breakpoint_offsets(source.source_file, 42),
        [0x8560]
    );
    assert_eq!(
        session.source_breakpoint_addresses(source.source_file, 42),
        [0x4560]
    );
}

#[test]
fn resolves_explicit_overlay_instances() {
    let dir = temp_dir();
    let rom = dir.join("game.gbc");
    let sidecar = dir.join("game.zdbg.json");
    std::fs::write(&rom, []).unwrap();
    let symbols = serde_json::json!({
        "format": "zeff-debug-symbols",
        "version": 2,
        "system": "gb",
        "rom_sha256": "0b".repeat(32),
        "symbols": [
            {
                "name": "OldOverlay",
                "cpu_address": null,
                "rom_offset": 32768,
                "bank": null,
                "exec_mode": "sm83",
                "value": null,
                "size": 16,
                "kind": "function",
                "scope": "global",
                "comment": null
            },
            {
                "name": "CurrentOverlay",
                "cpu_address": null,
                "rom_offset": 36864,
                "bank": null,
                "exec_mode": "sm83",
                "value": null,
                "size": 16,
                "kind": "function",
                "scope": "global",
                "comment": null
            }
        ],
        "segments": [
            {"id": 1, "name": "old", "rom_offset": 32768, "size": 16,
             "linked_cpu_address": null, "exec_mode": "sm83"},
            {"id": 2, "name": "current", "rom_offset": 36864, "size": 16,
             "linked_cpu_address": null, "exec_mode": "sm83"}
        ],
        "load_instances": [
            {"id": 10, "segment_id": 1, "cpu_address": 49152,
             "generation": 1, "created_cycle": 100, "active": true},
            {"id": 11, "segment_id": 2, "cpu_address": 49152,
             "generation": 2, "created_cycle": 200, "active": true}
        ]
    });
    std::fs::write(&sidecar, serde_json::to_vec(&symbols).unwrap()).unwrap();

    let session =
        SymbolSession::load_for_paths_with_sidecar(System::Gb, &rom, &rom, [11; 32], &sidecar);
    assert_eq!(session.segments().len(), 2);
    assert_eq!(session.load_instances().len(), 2);
    let resolved = session
        .resolve_load_instance(CpuLocation {
            space: AddressSpaceId(0),
            address: 0xC004,
        })
        .unwrap();
    assert_eq!(resolved.segment, SegmentId(2));
    assert_eq!(resolved.storage.offset, 0x9004);
    assert_eq!(session.resolve_cpu_name("CurrentOverlay"), Some(0xC000));
    assert_eq!(
        session.symbol_name_at_cpu_address(0xC000),
        Some("CurrentOverlay")
    );

    let mut view = crate::debug::DisassemblyView {
        pc: 0xC000,
        mapping: None,
        is_navigation_target: false,
        is_static_target: false,
        location_symbol: None,
        lines: vec![crate::debug::DisassembledLine {
            address: 0xC000,
            storage_offset: None,
            symbol: None,
            control_target: None,
            control_target_storage: None,
            control_target_symbol: None,
            source: None,
            bytes: Default::default(),
            mnemonic: Default::default(),
        }],
        breakpoints: Vec::new(),
        one_shot_breakpoints: Vec::new(),
        rom_breakpoints: Vec::new(),
        hit_rom_breakpoint: None,
    };
    session.annotate_disassembly(&mut view);
    assert_eq!(view.lines[0].storage_offset, Some(0x9000));
    assert_eq!(view.lines[0].symbol.as_deref(), Some("CurrentOverlay"));
    std::fs::remove_dir_all(dir).unwrap();
}
