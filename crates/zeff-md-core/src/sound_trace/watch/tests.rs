use super::*;
use crate::sound_trace::{AttemptCapture, CapturedBus};
use zeff_z80::{BusState, Cpu, Z80Bus};
struct Reads;
impl Z80Bus for Reads {
    fn cpu_read(&self, _: u16) -> u8 {
        0x5a
    }
    fn cpu_write(&mut self, _: u16, _: u8) {}
    fn io_read(&mut self, _: u8) -> u8 {
        0xff
    }
    fn io_write(&mut self, _: u8, _: u8) {}
    fn maskable_interrupt_pending(&self) -> bool {
        false
    }
    fn non_maskable_interrupt_pending(&self) -> bool {
        false
    }
    fn acknowledge_non_maskable_interrupt(&mut self) -> bool {
        false
    }
}
#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn standard_sound_watch_overflow_component_refuses_whole_attempt_instead_of_truncating_hits() {
    let mut attempt = Attempt::begin(&Cpu::new(), BusState::default());
    let bus = CapturedBus {
        inner: &mut Reads,
        attempt: &mut attempt,
    };
    for address in 0..7 {
        assert_eq!(bus.cpu_read(address), 0x5a);
    }
    let mut state = WatchState::default();
    state
        .edit(
            SoundWatchRule {
                space: SoundWatchSpace::Memory,
                start: 0,
                end: 7,
                kind: WatchType::Read,
            },
            true,
        )
        .unwrap();
    state.accept(&attempt, 0, false);
    assert!(state.blocked);
    assert_eq!(state.gaps, 1);
    assert_eq!(state.rules[0].hits, 0);
    assert!(state.first.is_none());
}
