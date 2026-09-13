use super::{Cue, GbNativeTiming};

pub(super) struct DriverRecipe {
    pub id: &'static str,
    pub rom_len: usize,
    pub timing: GbNativeTiming,
    pub selector_base: u8,
    pub tick_len: usize,
    pub bootstrap_address: u16,
    pub bootstrap_len: usize,
}

pub(super) const RECIPE: DriverRecipe = DriverRecipe {
    id: "gb-cgb-mbc5-banked",
    rom_len: 0x20_0000,
    timing: GbNativeTiming::CgbDouble,
    selector_base: 0x30,
    tick_len: 0x2a,
    bootstrap_address: 0x3f00,
    bootstrap_len: 0x40,
};

pub(super) struct DriverVariant {
    pub id: &'static str,
    pub parameters: VariantParameters,
    pub qualification: KnownRomQualification,
    pub driver_sha256: Option<&'static str>,
}

#[derive(Clone, Copy)]
pub(super) struct VariantParameters {
    pub cartridge_type: u8,
    pub init: u16,
    pub init_len: u16,
    pub selector: u16,
    pub tick: u16,
    pub driver: u16,
    pub driver_end: u16,
    pub table: u16,
    pub table_rows: u16,
    pub hook: u16,
}

pub(super) struct KnownRomQualification {
    pub sources: &'static [&'static str],
    pub cues: &'static [Cue],
}
