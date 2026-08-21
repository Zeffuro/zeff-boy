pub mod apu;
pub mod barcode_boy;
pub mod bardigun;
pub mod bus;
pub mod cartridge;
pub mod cpu;
mod io;
pub mod joypad;
mod opcodes;
pub mod ppu;
pub mod printer;
pub mod rom_header;
pub(crate) mod serial;
mod sgb;
mod timer;
pub mod types;

pub use bardigun::MAX_BARDIGUN_SCAN_BYTES;
pub use printer::{
    GAME_BOY_PRINTER_FEED_HEIGHT, GAME_BOY_PRINTER_MAX_HEIGHT, GAME_BOY_PRINTER_WIDTH,
    GameBoyPrinterJob,
};
pub use serial::GameBoySerialDevice;
