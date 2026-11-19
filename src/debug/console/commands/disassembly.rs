use super::*;

pub(super) fn open(
    state: &mut DebugConsoleState,
    symbols: &SymbolSession,
    args: &[&str],
    actions: &mut DebugUiActions,
) {
    if symbols.exec_mode() != ExecMode::M68000 {
        state.push("ROM code inspection is unavailable for this architecture".into());
        return;
    }
    let [value] = args else {
        state.push("Usage: disasm rom:<hex offset>".into());
        return;
    };
    let Some(offset) = value.strip_prefix("rom:").and_then(|value| {
        let value = value
            .strip_prefix("0x")
            .or_else(|| value.strip_prefix("0X"))
            .unwrap_or(value);
        u64::from_str_radix(value, 16).ok()
    }) else {
        state.push("Usage: disasm rom:<hex offset>".into());
        return;
    };
    actions.disasm_target = Some(DisassemblyTarget::RomStorage(offset));
    actions.focus_tab = Some(DebugTab::Disassembler);
    state.push(format!("Inspecting original ROM +{offset:X}"));
}

pub(super) fn label_storage(
    state: &mut DebugConsoleState,
    view: &DisassemblyView,
    name: &str,
    actions: &mut DebugUiActions,
) {
    let Some(offset) = view.selected_line().and_then(|line| line.storage_offset) else {
        state.push("Current ROM code location is unavailable".into());
        return;
    };
    actions.user_symbol = Some(UserSymbolDraft {
        name: name.into(),
        location: SymbolLocation {
            cpu: None,
            storage: Some(StorageLocation {
                image: ImageId(0),
                region: RegionId(0),
                offset,
            }),
            bank: None,
            exec_mode: ExecMode::M68000,
        },
        value: None,
        kind: SymbolKind::Label,
        size: None,
        comment: None,
    });
    state.push(format!("Adding label {name} at ROM +{offset:X}"));
}
