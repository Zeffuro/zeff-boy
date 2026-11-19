use super::*;
use crate::debug::{
    DebugTarget,
    sound_watchpoints::{SoundWatchRequest, SoundWatchView},
};
use crate::ui::TraceStreamId;
use zeff_md_core::sound_trace::{SoundWatchRule, SoundWatchSpace};

fn owner(header: u8) -> Owner {
    let mut setup = vec![0x33fc, 0x100, 0xa1, 0x1100, 0x33fc, 0x100, 0xa1, 0x1200];
    for (address, byte) in [0xdb, 0x12, 0xd3, 0x12, 0x18, 0xfa].into_iter().enumerate() {
        setup.extend([0x13fc, byte, 0xa0, address as u16]);
    }
    setup.extend([
        0x33fc, 0x100, 0xa1, 0x1200, 0x33fc, 0, 0xa1, 0x1100, 0x4ef9, 0, 0x200,
    ]);
    Owner::new(fixture(
        header,
        TimingModel::Standard,
        ControllerKind::ThreeButton,
        &[(4, &[0, 0x1900]), (0x1900, &setup)],
    ))
}
fn snapshot(owner: &mut Owner) -> SoundWatchView {
    let mut input = guest_call_input(0);
    input.snapshot.debug_target = DebugTarget::Sound;
    input.snapshot.debug_revision = 91;
    EmuThread::collect_ui_snapshot(&mut owner.backend, &input.snapshot, input.buffers)
        .md_sound_watch
        .unwrap()
}
fn edit(owner: &mut Owner, rule: SoundWatchRule, add: bool) {
    let binding = snapshot(owner).binding.unwrap();
    let mut actions = DebugUiActions::none();
    actions.target = DebugTarget::Sound;
    actions.target_revision = Some(91);
    actions.sound_trace.watch = Some(SoundWatchRequest {
        binding,
        rule,
        add,
        request: TraceStreamId::allocate(),
    });
    let mut config = BackendRuntimeConfig::new(&actions);
    config.opcode_log_enabled = true;
    config.debug_view = (DebugTarget::Sound, 91);
    owner.backend.apply_runtime_config(config);
}
#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_sound_watch_guest_call_abort_restores_counts_and_host_undo_keeps_current_policy_mask_gap() {
    for header in *b"UJE" {
        for mask in [None, Some("001800:1234"), Some("001800:ABCD")] {
            let mut subject = owner(header);
            let mut control = owner(header);
            let io = SoundWatchRule {
                space: SoundWatchSpace::Io,
                start: 0x12,
                end: 0x12,
                kind: WatchType::ReadWrite,
            };
            let memory = SoundWatchRule {
                space: SoundWatchSpace::Memory,
                start: 0x1000,
                end: 0x1000,
                kind: WatchType::Write,
            };
            edit(&mut subject, io, true);
            let before = snapshot(&mut subject);
            let raw = frontend(&subject.backend);
            let effects = subject.dispatch(EmuCommand::ExecuteGuestCall(request(0x1000, 8)));
            assert!(!effects.potentially_dirty);
            let Some(EmuResponse::GuestCallFailed { error, .. }) = effects.response else {
                panic!("watch did not abort guest call")
            };
            assert!(error.contains("debugger stop") || error.contains("SOUND watch"));
            let after = snapshot(&mut subject);
            assert_eq!(
                (after.binding, after.watches, after.hit, after.gaps),
                (before.binding, before.watches, before.hit, before.gaps)
            );
            assert_eq!(frontend(&subject.backend), raw);
            trace_stream::compare_machine(&subject, &control);
            edit(&mut subject, io, false);
            edit(&mut subject, memory, true);
            let token = subject.call();
            let control_token = control.call();
            trace_stream::compare_machine(&subject, &control);
            edit(&mut subject, memory, false);
            edit(&mut subject, io, true);
            let current = snapshot(&mut subject);
            let patches = mask
                .map(|code| {
                    crate::cheats::parse_cheat_for_system(code, ActiveSystem::Md)
                        .unwrap()
                        .0
                })
                .unwrap_or_default();
            subject.backend.try_install_rom_patches(&patches).unwrap();
            control.backend.try_install_rom_patches(&patches).unwrap();
            subject.undo(token);
            control.undo(control_token);
            trace_stream::compare_machine(&subject, &control);
            let restored = snapshot(&mut subject);
            assert_eq!(restored.watches, current.watches);
            assert!(restored.hit.is_none());
            assert!(restored.gaps > current.gaps);
            assert_ne!(restored.binding, current.binding);
            assert_eq!(
                machine(&subject.backend).peek(0x1800),
                Some(mask.map_or(0, |code| if code.ends_with("ABCD") { 0xab } else { 0x12 }))
            );
        }
    }
}
