use super::guest::{Guest, PORT};

const RESULTS: u32 = 0xff2000;

impl Guest {
    pub fn address(&mut self, address: u8) -> (usize, usize) {
        self.start();
        let device = self.send(0xa0);
        let word = self.send(address);
        (device, word)
    }

    pub fn read(&mut self, address: u8) {
        self.address(address);
        self.start();
        self.send(0xa1);
    }

    pub fn sample_upper(&mut self) -> usize {
        let index = self.rom.results;
        self.rom.word(0x1039);
        self.rom.long(PORT);
        self.rom.word(0x13c0);
        self.rom.long(RESULTS + index as u32);
        self.rom.results += 1;
        index
    }
}

pub fn serial(raw: &[u8]) -> std::ops::Range<usize> {
    super::state_layout::serial_payload_range(raw, 256, 27)
}

pub fn cells() -> Vec<u8> {
    (0..256)
        .map(|n| (n as u8).wrapping_mul(73) ^ 0x59)
        .collect()
}
