use super::*;
use crate::api::{RETRO_REGION_NTSC, RETRO_REGION_PAL, retro_system_av_info, retro_system_timing};
use crate::callbacks::{ABI_TEST_LOCK, CORE, lock};

#[test]
fn nes_libretro_advertises_resolved_machine_timing() {
    let _abi = lock(&ABI_TEST_LOCK);
    let previous = lock(&CORE).take();
    for (timing_tag, expected_fps, expected_region) in [
        (0, 60.098_477_556_112_265, RETRO_REGION_NTSC),
        (1, 50.006_978_908_188_586, RETRO_REGION_PAL),
        (2, 60.098_477_556_112_265, RETRO_REGION_NTSC),
        (3, 50.006_978_908_188_586, RETRO_REGION_PAL),
    ] {
        let mut rom = nes_rom();
        rom[7] = 0x08;
        rom[12] = timing_tag;
        let state = CoreState::from_rom(&rom, "timing.nes").unwrap();
        *lock(&CORE) = Some(state);
        let mut av = retro_system_av_info {
            geometry: CoreState::default_video_geometry(),
            timing: retro_system_timing {
                fps: 0.0,
                sample_rate: 0.0,
            },
        };
        crate::retro_get_system_av_info(&mut av);
        assert!(
            (av.timing.fps - expected_fps).abs() < 0.000_01,
            "timing tag {timing_tag}: advertised {} Hz",
            av.timing.fps
        );
        assert_eq!(crate::retro_get_region(), expected_region);
        assert_eq!(av.timing.sample_rate, 48_000.0);
    }
    *lock(&CORE) = previous;
}
