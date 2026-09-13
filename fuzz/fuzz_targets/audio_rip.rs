#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    zeff_audio_discovery::fuzzing::rip(data);
});
