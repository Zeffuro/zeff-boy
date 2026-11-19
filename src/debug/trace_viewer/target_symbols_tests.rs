use super::*;
use crate::symbols::import::{ImportContext, TargetInfo, import_md_sound_symbols, import_symbols};
use crate::symbols::{AddressSpaceId, ImageId, RegionId, SymbolSession};

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn trace_target_symbols_and_filter_do_not_follow_general_debugger_selection() {
    let mut main = SymbolSession::default();
    let mut sound = SymbolSession::for_md_sound([0x5a; 32]);
    let ctx = ImportContext {
        target: TargetInfo {
            system: zeff_emu_common::system::System::Md,
        },
        image: ImageId(0),
        rom_region: RegionId(0),
        cpu_space: AddressSpaceId(0),
        source_name: None,
    };
    main.store.extend(
        import_symbols("main.sym", b"000200 T MainEntry", &ctx)
            .unwrap()
            .symbols,
    );
    let sound_ctx = ImportContext {
        cpu_space: AddressSpaceId(1),
        ..ctx
    };
    sound.store.extend(
        import_md_sound_symbols("sound.sym", b"[labels]\n00:0200 SoundEntry", &sound_ctx)
            .unwrap()
            .symbols,
    );
    let primary =
        InstructionTraceRecord::new(TraceExecMode::M68000, 0x200, None, 0, 0, &[0x4e, 0x71]);
    let secondary = InstructionTraceRecord::new(TraceExecMode::Z80, 0x200, None, 0, 0, &[0]);
    use crate::ui::TraceTarget;
    assert_eq!(
        trace_symbol_for_target(TraceTarget::Primary, &main, &primary).as_deref(),
        Some("MainEntry")
    );
    assert_eq!(
        trace_symbol_for_target(TraceTarget::Sound, &sound, &secondary).as_deref(),
        Some("SoundEntry")
    );
    assert!(trace_symbol_for_target(TraceTarget::Sound, &main, &secondary).is_none());
    assert!(trace_symbol_for_target(TraceTarget::Primary, &sound, &primary).is_none());
    assert!(entry_matches_target(
        TraceTarget::Primary,
        &primary,
        "mainentry",
        &main
    ));
    assert!(!entry_matches_target(
        TraceTarget::Primary,
        &primary,
        "soundentry",
        &main
    ));
    assert!(entry_matches_target(
        TraceTarget::Sound,
        &secondary,
        "soundentry",
        &sound
    ));
    assert!(!entry_matches_target(
        TraceTarget::Sound,
        &secondary,
        "mainentry",
        &sound
    ));
}
