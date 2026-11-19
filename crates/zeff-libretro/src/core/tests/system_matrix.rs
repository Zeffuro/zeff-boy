use super::*;

#[test]
fn system_specs_map_to_libretro_core_state() {
    let valid_extensions = crate::callbacks::VALID_EXTENSIONS
        .to_str()
        .expect("valid extensions should be UTF-8");

    for spec in System::specs() {
        for extension in spec.rom_extensions {
            if spec.system == System::Coleco {
                assert!(
                    !valid_extensions.split('|').any(|entry| entry == *extension),
                    "deferred ColecoVision extension {extension} must not be registered"
                );
                continue;
            }
            if spec.system == System::Pce && *extension != "pce" {
                continue;
            }
            assert!(
                valid_extensions.split('|').any(|entry| entry == *extension),
                "libretro valid extension list is missing {extension}"
            );

            if *extension == "fds" {
                continue;
            }

            let rom = rom_for_system(spec.system);
            let path = format!("matrix.{extension}");
            let state = CoreState::from_rom(&rom, &path).unwrap_or_else(|err| {
                panic!(
                    "libretro core should initialize {} ROM {path}: {err}",
                    spec.code
                )
            });

            assert_eq!(active_core_family(&state), spec.core_family);
            assert_eq!(
                state.video_geometry().base_width,
                expected_dimensions(spec.system).0
            );
            assert_eq!(
                state.video_geometry().base_height,
                expected_dimensions(spec.system).1
            );
            assert_eq!(state.system_label(), expected_system_label(spec.system));
            assert_eq!(
                state.system_ram_size(),
                expected_system_ram_size(spec.system),
                "unexpected system RAM size for {}",
                spec.code
            );
        }
    }
}

#[test]
fn video_geometry_matches_registered_system_matrix() {
    for spec in System::specs() {
        for extension in spec.rom_extensions {
            if *extension == "fds"
                || spec.system == System::Coleco
                || spec.system == System::Pce && *extension != "pce"
            {
                continue;
            }

            let rom = rom_for_system(spec.system);
            let state = CoreState::from_rom(&rom, &format!("geometry.{extension}"))
                .unwrap_or_else(|err| panic!("{} geometry fixture failed: {err}", spec.code));
            let geometry = state.video_geometry();

            assert_eq!(
                (geometry.base_width, geometry.base_height),
                expected_dimensions(spec.system),
                "unexpected base geometry for {}",
                spec.code
            );
            let expected_maximum = if spec.system == System::Pce {
                (640, 480)
            } else if spec.system == System::Md {
                (320, 480)
            } else {
                (256, 240)
            };
            assert_eq!(
                (geometry.max_width, geometry.max_height),
                expected_maximum,
                "unexpected maximum geometry for {}",
                spec.code
            );
            let expected_aspect = if spec.system == System::Pce {
                4.0 / 3.0
            } else {
                0.0
            };
            assert_eq!(
                geometry.aspect_ratio, expected_aspect,
                "unexpected aspect hint for {}",
                spec.code
            );
        }
    }
}

fn expected_dimensions(system: System) -> (u32, u32) {
    if system == System::Md {
        (256, 224)
    } else {
        system.spec().screen_size
    }
}
