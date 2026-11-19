use super::*;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn game_genie_decodes_each_payload_bit_and_rejects_extra_symbols() {
    for (code, address, value) in [
        ("BAAA-AAAA", 0x000000, 0x0008),
        ("CAAA-AAAA", 0x000000, 0x0010),
        ("EAAA-AAAA", 0x000000, 0x0020),
        ("JAAA-AAAA", 0x000000, 0x0040),
        ("TAAA-AAAA", 0x000000, 0x0080),
        ("ABAA-AAAA", 0x004000, 0x0000),
        ("ACAA-AAAA", 0x008000, 0x0000),
        ("AEAA-AAAA", 0x000000, 0x0001),
        ("AJAA-AAAA", 0x000000, 0x0002),
        ("ATAA-AAAA", 0x000000, 0x0004),
        ("AABA-AAAA", 0x000200, 0x0000),
        ("AACA-AAAA", 0x000400, 0x0000),
        ("AAEA-AAAA", 0x000800, 0x0000),
        ("AAJA-AAAA", 0x001000, 0x0000),
        ("AATA-AAAA", 0x002000, 0x0000),
        ("AAAB-AAAA", 0x100000, 0x0000),
        ("AAAC-AAAA", 0x200000, 0x0000),
        ("AAAE-AAAA", 0x400000, 0x0000),
        ("AAAJ-AAAA", 0x800000, 0x0000),
        ("AAAT-AAAA", 0x000100, 0x0000),
        ("AAAA-BAAA", 0x000000, 0x1000),
        ("AAAA-CAAA", 0x010000, 0x0000),
        ("AAAA-EAAA", 0x020000, 0x0000),
        ("AAAA-JAAA", 0x040000, 0x0000),
        ("AAAA-TAAA", 0x080000, 0x0000),
        ("AAAA-ABAA", 0x000000, 0x8000),
        ("AAAA-ACAA", 0x000000, 0x0100),
        ("AAAA-AEAA", 0x000000, 0x0200),
        ("AAAA-AJAA", 0x000000, 0x0400),
        ("AAAA-ATAA", 0x000000, 0x0800),
        ("AAAA-AABA", 0x000020, 0x0000),
        ("AAAA-AACA", 0x000040, 0x0000),
        ("AAAA-AAEA", 0x000080, 0x0000),
        ("AAAA-AAJA", 0x000000, 0x2000),
        ("AAAA-AATA", 0x000000, 0x4000),
        ("AAAA-AAAB", 0x000001, 0x0000),
        ("AAAA-AAAC", 0x000002, 0x0000),
        ("AAAA-AAAE", 0x000004, 0x0000),
        ("AAAA-AAAJ", 0x000008, 0x0000),
        ("AAAA-AAAT", 0x000010, 0x0000),
        ("AJBT-AA4Y", 0x000354, 0x6002),
        ("SCRA-BJX0", 0x009c76, 0x5478),
        ("REBT-A6XL", 0x00036a, 0x4e71),
        ("FFFF-FFFF", 0x524aa5, 0x9229),
        ("9999-9999", 0xffffff, 0xffff),
    ] {
        assert_eq!(decode_game_genie(code), Some((address, value)), "{code}");
        assert_eq!(
            decode_game_genie(&code.to_ascii_lowercase()),
            Some((address, value))
        );
    }
    for code in [
        "",
        "AAAAAAAA",
        "AAAA-AAAAx",
        "AAAA-AAA",
        "AAA--AAAA",
        "AAAA--AAA",
        "AAAA-AAA-",
        "AAAA-AAA?",
        "IAAA-AAAA",
        "OAAA-AAAA",
        "QAAA-AAAA",
        "UAAA-AAAA",
        "あAAA-AAAA",
    ] {
        assert_eq!(decode_game_genie(code), None, "{code}");
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn rom_formats_preserve_width_address_range_and_atomic_mixed_lists() {
    let (patches, format) = parse_cheats(" $012345 : $A55A + FF0040:0008 + AAAA-AAAB ").unwrap();
    assert_eq!(format, CheatType::GameGenie);
    assert_eq!(
        patches,
        vec![
            CheatPatch::WideRomWrite {
                address: 0x12345,
                value: CheatValue::Constant(0xa5)
            },
            CheatPatch::WideRomWrite {
                address: 0x12346,
                value: CheatValue::Constant(0x5a)
            },
            CheatPatch::WideRamWrite {
                address: 0xff0040,
                value: CheatValue::Constant(0)
            },
            CheatPatch::WideRamWrite {
                address: 0xff0041,
                value: CheatValue::Constant(8)
            },
            CheatPatch::WideRomWrite {
                address: 0,
                value: CheatValue::Constant(0)
            },
            CheatPatch::WideRomWrite {
                address: 1,
                value: CheatValue::Constant(0)
            },
        ]
    );
    for text in [
        "000000:00",
        "3FFFFE:0008",
        "3FFFFF:FF",
        "3FFFFF:0008",
        "400000:11",
        "7FFFFE:0008",
        "7FFFFF:FF",
        "FFFFFE:0008",
        "FFFFFF:FF",
        "003FFFFE=FFFF",
        "+0X000200=0X7002++",
        "F F 0 0 4 0 : 0 0 0 8",
    ] {
        assert!(parse_cheats(text).is_some(), "{text}");
    }
    for text in [
        "",
        "++",
        "000200:4",
        "000200:123",
        "000200:12345",
        "800000:11",
        "FEFFFF:11",
        "7FFFFF:0008",
        "AAAE-AAAA",
        "FFFFFF:0008",
        "01000200:11",
        "000200:70+A10003:00",
        "FF0040:12+AAAA-AAA?",
        "9999-9999",
        "000200:7002+nonsense",
    ] {
        assert_eq!(parse_cheats(text), None, "{text}");
    }
    assert_eq!(super::super::parse_raw_cheats("000200:7002"), None);
    assert_eq!(super::super::parse_raw_cheats("AAAA-AAAA"), None);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn explicit_sources_preserve_legacy_ram_and_reject_boundary_overflow() {
    let (set, format) =
        parse_cheats("ROM:00800000:A55A + rom:00FF0000:C7 + ROM:07FFFFFF:D8 + FF0000:11").unwrap();
    assert_eq!(format, CheatType::Raw);
    assert_eq!(
        set,
        vec![
            CheatPatch::WideRomWrite {
                address: 0x800000,
                value: CheatValue::Constant(0xa5)
            },
            CheatPatch::WideRomWrite {
                address: 0x800001,
                value: CheatValue::Constant(0x5a)
            },
            CheatPatch::WideRomWrite {
                address: 0xff0000,
                value: CheatValue::Constant(0xc7)
            },
            CheatPatch::WideRomWrite {
                address: 0x7ffffff,
                value: CheatValue::Constant(0xd8)
            },
            CheatPatch::WideRamWrite {
                address: 0xff0000,
                value: CheatValue::Constant(0x11)
            },
        ]
    );
    assert_eq!(
        parse_cheats("ROM:07FFFFFE:A55A").unwrap().0,
        vec![
            CheatPatch::WideRomWrite {
                address: 0x7fffffe,
                value: CheatValue::Constant(0xa5)
            },
            CheatPatch::WideRomWrite {
                address: 0x7ffffff,
                value: CheatValue::Constant(0x5a)
            },
        ]
    );
    assert_eq!(
        parse_cheats("ROM:00800001:A5").unwrap().0,
        vec![CheatPatch::WideRomWrite {
            address: 0x800001,
            value: CheatValue::Constant(0xa5)
        },]
    );
    for text in [
        "ROM:08000000:11",
        "ROM:FFFFFFFF:11",
        "ROM:FFFFFFFE:1234",
        "ROM:07FFFFFF:1234",
        "ROM:00800001:1234",
        "ROM:800000:11",
        "ROM:00800000:1",
        "ROM:00800000:123",
        "ROM:00800000:12345",
        "ROM:00800000:GG",
        "ROM:00800000:11+ROM:08000000:22",
        "ROM:00800000:11+unknown",
        "ROM::00800000:11",
    ] {
        assert_eq!(parse_cheats(text), None, "{text}");
    }
}
