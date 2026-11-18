use super::*;

#[derive(Clone, Debug)]
pub(crate) struct Pulse {
    pub half: u64,
    pub pass: u8,
    pub main: Option<main_bus::Snapshot>,
    pub pins: main_bus::Pins,
    pub companion: Companion,
    pub released: bool,
    pub grant_tristate: bool,
    pub main_address_selected: bool,
    pub main_data_selected: bool,
    pub protocol: bool,
    pub protocol_drive: bool,
    pub ram_read_lanes: [bool; 2],
    pub cartridge_drive: bool,
    pub sound_data_drive: bool,
    pub sound_low_address: bool,
    pub address: u32,
    pub data: u16,
    pub ram_address14: bool,
    pub high: u16,
    pub low: u16,
    pub row_strobe: bool,
    pub write_strobes: [bool; 2],
    pub effects: Vec<(u16, u8)>,
}

impl Board {
    pub(crate) fn work_witness_tristate(&self) -> bool {
        self.grant.tristate()
    }
}
