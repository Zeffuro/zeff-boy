use serde::Deserialize;
use zeff_m68k::Registers;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub cohort: String,
    pub name: String,
    pub initial: State,
    #[serde(rename = "final")]
    pub expected: State,
    pub reference_total_cycles: u32,
    pub transactions: Vec<Phase>,
    pub normalization_exclusion: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub d0: u32,
    pub d1: u32,
    pub d2: u32,
    pub d3: u32,
    pub d4: u32,
    pub d5: u32,
    pub d6: u32,
    pub d7: u32,
    pub a0: u32,
    pub a1: u32,
    pub a2: u32,
    pub a3: u32,
    pub a4: u32,
    pub a5: u32,
    pub a6: u32,
    pub usp: u32,
    pub ssp: u32,
    pub sr: u16,
    pub pc: u32,
    pub prefetch: [u16; 2],
    pub ram: Vec<(u32, u8)>,
}

impl State {
    pub fn registers(&self) -> Result<Registers, String> {
        let mut registers = Registers::new();
        registers.d = [
            self.d0, self.d1, self.d2, self.d3, self.d4, self.d5, self.d6, self.d7,
        ];
        registers.a = [
            self.a0, self.a1, self.a2, self.a3, self.a4, self.a5, self.a6,
        ];
        registers.usp = self.usp;
        registers.ssp = self.ssp;
        registers.pc = self.pc.wrapping_sub(4);
        registers.set_status_register(self.sr);
        if registers.status_register() != self.sr {
            return Err("unsupported SR bits".into());
        }
        Ok(registers)
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Phase {
    Idle { duration_cycles: u32 },
    Read(Transfer),
    Write(Transfer),
    Tas(Transfer),
    ReadAddressError(Transfer),
    WriteAddressError(Transfer),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transfer {
    pub duration_cycles: u32,
    pub function_code: u32,
    pub address_bus: u32,
    pub data_bus: u32,
    pub uds: u32,
    pub lds: u32,
}

impl Phase {
    pub fn duration(&self) -> u32 {
        match self {
            Self::Idle { duration_cycles } => *duration_cycles,
            Self::Read(value)
            | Self::Write(value)
            | Self::Tas(value)
            | Self::ReadAddressError(value)
            | Self::WriteAddressError(value) => value.duration_cycles,
        }
    }
}
