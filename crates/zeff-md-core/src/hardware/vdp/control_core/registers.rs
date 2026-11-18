use super::*;

const MASKS: [u8; 19] = [
    0xff, 0xff, 0x7e, 0x7e, 0x0f, 0xff, 0x24, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f, 0x11, 0xff,
    0x33, 0x9f, 0x9f,
];
const ALL_EXCEPT_HINT: u32 = ((1 << 24) - 1) & !(1 << 10);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::hardware::vdp) enum RegisterStage {
    #[allow(
        dead_code,
        reason = "Retains the pre-evaluation stage in physical register ordering"
    )]
    Before,
    HintReset,
    Control,
    DmaControl,
    DmaCounters,
    ScrollY,
    PlaneGeometry,
    Window,
    ScrollX,
    Sprite,
    Backdrop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct RegisterViews {
    before: RegisterBank,
    after: RegisterBank,
    reset: bool,
}

impl RegisterViews {
    pub(super) fn new(before: RegisterBank, after: RegisterBank, reset: bool) -> Self {
        Self {
            before,
            after,
            reset,
        }
    }

    pub fn before(self) -> RegisterBank {
        self.before
    }
    pub fn after(self) -> RegisterBank {
        self.after
    }

    pub fn at(self, stage: RegisterStage) -> [u8; 24] {
        let mut bank = self.before;
        if stage >= RegisterStage::HintReset && self.reset {
            bank.bytes[10] = 0xff;
        }
        if stage >= RegisterStage::Control {
            for index in [0, 1, 10, 11, 12, 15] {
                bank.bytes[index] = self.after.bytes[index];
            }
        }
        if stage >= RegisterStage::DmaControl {
            bank.dma.control = self.after.dma.control;
        }
        if stage >= RegisterStage::DmaCounters {
            bank.dma.length = self.after.dma.length;
            bank.dma.source = self.after.dma.source;
        }
        if stage >= RegisterStage::ScrollY {
            bank.bytes[9] = self.after.bytes[9];
        }
        if stage >= RegisterStage::PlaneGeometry {
            for index in [2, 4, 16] {
                bank.bytes[index] = self.after.bytes[index];
            }
        }
        if stage >= RegisterStage::Window {
            for index in [3, 13, 14, 17, 18] {
                bank.bytes[index] = self.after.bytes[index];
            }
        }
        if stage >= RegisterStage::ScrollX {
            bank.bytes[8] = self.after.bytes[8];
        }
        if stage >= RegisterStage::Sprite {
            for index in [5, 6] {
                bank.bytes[index] = self.after.bytes[index];
            }
        }
        if stage >= RegisterStage::Backdrop {
            bank.bytes[7] = self.after.bytes[7];
        }
        bank.logical()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct RegisterLoad {
    pub level: bool,
    pub selected: u8,
    pub value: u8,
    pub source_address: u32,
    pub enabled: u32,
}

impl RegisterLoad {
    pub(super) fn decode(level: bool, address: u32, mode5: bool, reset: bool) -> Self {
        let selected = ((address >> 8) & 31) as u8;
        let enabled = if reset {
            ALL_EXCEPT_HINT
        } else if level && selected < 24 && (mode5 || selected <= 10) {
            1 << selected
        } else {
            0
        };
        Self {
            level,
            selected,
            value: address as u8,
            source_address: address,
            enabled,
        }
    }

    pub fn includes(self, index: u8) -> bool {
        index < 24 && self.enabled & (1 << index) != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct DmaBanks {
    pub(super) length: [u16; 2],
    pub(super) source: [u16; 2],
    pub(super) control: u8,
}

impl DmaBanks {
    pub(super) fn software_origin() -> Self {
        Self {
            length: [0xffff; 2],
            source: [0; 2],
            control: 0,
        }
    }

    pub fn length_banks(self) -> [u16; 2] {
        self.length
    }
    pub fn remaining(self) -> u16 {
        !self.length[1]
    }
    pub fn source_low(self) -> u16 {
        self.source[1]
    }
    pub fn source_high(self) -> u8 {
        self.control & 63
    }
    pub fn mode(self) -> u8 {
        self.control >> 6
    }
    fn advance_pair(
        old: u16,
        low_load: bool,
        high_load: bool,
        value: u8,
        advance: bool,
        double: bool,
    ) -> u16 {
        let mut low = u16::from(if low_load { value } else { old as u8 });
        let mut high = u16::from(if high_load { value } else { (old >> 8) as u8 });
        low += u16::from(advance && !double);
        high += u16::from(low & 0x100 != 0 || advance && double);
        (low & 255) | ((high & 255) << 8)
    }

    pub(super) fn evaluate(
        &mut self,
        phases: PhaseLevels,
        load: RegisterLoad,
        advance: bool,
        double: bool,
    ) {
        if load.includes(23) {
            self.control = load.value;
        }
        if phases.horizontal_master {
            // A load and its carry share one evaluation of the old slave bank.
            self.length[0] = Self::advance_pair(
                self.length[1],
                load.includes(19),
                load.includes(20),
                !load.value,
                advance,
                double,
            );
            self.source[0] = Self::advance_pair(
                self.source[1],
                load.includes(21),
                load.includes(22),
                load.value,
                advance,
                double,
            );
        }
        if phases.horizontal_slave {
            self.length[1] = self.length[0];
            self.source[1] = self.source[0];
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct RegisterBank {
    pub(super) bytes: [u8; 19],
    pub(super) dma: DmaBanks,
}

impl RegisterBank {
    pub(super) fn software_origin() -> Self {
        let mut bytes = [0; 19];
        bytes[1] = 4;
        Self {
            bytes,
            dma: DmaBanks::software_origin(),
        }
    }

    pub fn logical(self) -> [u8; 24] {
        let mut values = [0; 24];
        values[..19].copy_from_slice(&self.bytes);
        let length = self.dma.remaining().to_le_bytes();
        let source = self.dma.source_low().to_le_bytes();
        values[19..21].copy_from_slice(&length);
        values[21..23].copy_from_slice(&source);
        values[23] = self.dma.control;
        values
    }

    pub fn mode5(self) -> bool {
        self.bytes[1] & 4 != 0
    }
    pub fn increment(self) -> u8 {
        self.bytes[15]
    }
    pub fn hint(self) -> u8 {
        !self.bytes[10]
    }
    pub fn dma(self) -> DmaBanks {
        self.dma
    }

    pub(super) fn evaluate(
        &mut self,
        phases: PhaseLevels,
        load: RegisterLoad,
        advance: bool,
        double: bool,
        reset: bool,
    ) {
        if reset {
            self.bytes[10] = 0xff;
        }
        for (index, mask) in MASKS.into_iter().enumerate() {
            if load.includes(index as u8) {
                self.bytes[index] = load.value & mask;
            }
        }
        self.dma.evaluate(phases, load, advance, double);
    }

    pub(super) fn validate(self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.bytes
                .into_iter()
                .zip(MASKS)
                .all(|(value, mask)| value & !mask == 0),
            "invalid physical VDP register bits"
        );
        Ok(())
    }
}
