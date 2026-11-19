use super::*;
use zeff_emu_common::audio_trace::AudioTraceInvalidation;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn download_source_equal_mask_retains_future_write_authority_and_invalidates_audio_trace() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent_sound in [false, true] {
            let mut machine = Emulator::new(
                tone_image(&[0x13fc, 0, 0xa1, 0x30f1, 0x13fc, 0x5a, 1, 0x8000, 0x60fe]),
                LoadConfig {
                    region,
                    concurrent_sound,
                },
            )
            .unwrap();
            assert_eq!(machine.peek(DATA), Some(0));
            let original = machine.bus.cartridge.bytes.clone();
            let digest = machine.bus.cartridge.rom_digest();
            machine.reset_and_begin_audio_trace(8).unwrap();
            let raw = machine.encode_state().unwrap();
            machine.install_rom_patches(&[patch(DATA, 0)]).unwrap();
            assert!(machine.encode_state().unwrap() == raw);
            assert_eq!(machine.peek(DATA), Some(0));
            let trace = machine.finish_audio_trace().unwrap().trace;
            assert_eq!(
                trace.invalidated,
                Some(AudioTraceInvalidation::ExternalMutation)
            );
            assert!(trace.validate_complete().is_err());
            let before = machine.clone();
            machine.install_rom_patches(&[patch(DATA, 0)]).unwrap();
            assert_eq!(machine, before);
            assert!(machine.reset_and_begin_audio_trace(8).is_err());
            assert_eq!(machine, before);
            for _ in 0..5 {
                machine.step_instruction().unwrap();
            }
            assert_eq!(
                machine.bus.cartridge.download.as_ref().unwrap().read(DATA),
                Some(0x5a)
            );
            assert_eq!(machine.peek(DATA), Some(0));
            let raw = machine.encode_state().unwrap();
            machine.clear_rom_patches();
            assert_eq!(machine.peek(DATA), Some(0x5a));
            assert!(machine.encode_state().unwrap() == raw);
            assert_eq!(machine.bus.cartridge.rom_digest(), digest);
            assert!(Arc::ptr_eq(&original, &machine.bus.cartridge.bytes));
            assert_eq!(original[DATA as usize], 0);
            machine.reset_and_begin_audio_trace(8).unwrap();
            machine.clear_rom_patches();
            let trace = machine.finish_audio_trace().unwrap().trace;
            assert_eq!(trace.invalidated, None);
            assert!(trace.validate_complete().is_ok());
        }
    }
}
