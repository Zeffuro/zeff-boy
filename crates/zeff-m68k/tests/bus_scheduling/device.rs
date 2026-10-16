use zeff_emu_common::time::{ClockRate, MachineTiming, MasterTicks, TimingSnapshot};
use zeff_m68k::bus::{BusAccess, ByteLanes, Direction, M68kBus};
use zeff_m68k::timing::{ClockedBus, CpuClocks, CycleResult};

const CPU_DIVISOR: u64 = 7;
const DEVICE_DIVISOR: u64 = 15;
const RAM_START: u32 = 0xe0_0000;
const RAM_MASK: usize = 0xffff;
const TRANSFER_CLOCKS: u32 = 4;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transfer {
    pub access: BusAccess,
    pub direction: Direction,
    pub value: u16,
    pub start: u64,
    pub end: u64,
    pub waited: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub rom: Vec<u8>,
    pub ram: Vec<u8>,
    pub cpu_clock: u64,
    pub ticks: MasterTicks,
    pub device_cycles: u64,
    pub device_phase: u64,
    pub hold: Option<(u32, Direction, u64)>,
    pub held_ticks: Vec<(MasterTicks, [u8; 4])>,
    pub transfers: Vec<Transfer>,
    pub reset_reads: Vec<BusAccess>,
}

impl Device {
    pub fn new(program: &[u16]) -> Self {
        let mut rom = vec![0; 0x200];
        rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
        for (index, word) in program.iter().enumerate() {
            rom[0x100 + index * 2..0x102 + index * 2].copy_from_slice(&word.to_be_bytes());
        }
        Self {
            rom,
            ram: vec![0; RAM_MASK + 1],
            cpu_clock: 0,
            ticks: MasterTicks::ZERO,
            device_cycles: 0,
            device_phase: 0,
            hold: None,
            held_ticks: Vec::new(),
            transfers: Vec::new(),
            reset_reads: Vec::new(),
        }
    }

    fn advance(&mut self, clocks: u32) {
        self.cpu_clock += u64::from(clocks);
        let elapsed = u64::from(clocks) * CPU_DIVISOR;
        self.ticks = MasterTicks::new(self.ticks.get() + elapsed);
        let device_ticks = self.device_phase + elapsed;
        self.device_cycles += device_ticks / DEVICE_DIVISOR;
        self.device_phase = device_ticks % DEVICE_DIVISOR;
    }

    fn begin(&mut self, access: BusAccess, direction: Direction, start: CpuClocks) -> u32 {
        assert_eq!(self.cpu_clock, start.0);
        let mut waited = 0;
        if let Some((address, owner, release)) = self.hold
            && (access.bus_address, direction) == (address, owner)
        {
            assert!(release.saturating_sub(self.cpu_clock) <= 16);
            while self.cpu_clock < release {
                self.advance(1);
                self.held_ticks
                    .push((self.ticks, self.ram[..4].try_into().unwrap()));
                waited += 1;
            }
            self.hold = None;
        }
        self.advance(TRANSFER_CLOCKS);
        waited
    }

    fn byte(&self, address: u32) -> Result<u8, ()> {
        if address >= RAM_START {
            Ok(self.ram[address as usize & RAM_MASK])
        } else {
            self.rom.get(address as usize).copied().ok_or(())
        }
    }
}

impl MachineTiming for Device {
    fn timing_snapshot(&self) -> TimingSnapshot {
        TimingSnapshot::new(self.ticks, ClockRate::from_hz(105))
    }
}

impl M68kBus for Device {
    type Error = ();
    fn read(&mut self, access: BusAccess) -> Result<u16, ()> {
        self.reset_reads.push(access);
        Ok(u16::from_be_bytes([
            self.byte(access.bus_address)?,
            self.byte(access.bus_address + 1)?,
        ]))
    }
    fn write(&mut self, _: BusAccess, _: u16) -> Result<(), ()> {
        Err(())
    }
}

impl ClockedBus for Device {
    type Error = ();
    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, ()> {
        let waited = self.begin(access, Direction::Read, start);
        let result = self.byte(access.bus_address).and_then(|high| {
            self.byte(access.bus_address + 1)
                .map(|low| u16::from_be_bytes([high, low]))
        });
        if let Ok(value) = result {
            self.transfers.push(Transfer {
                access,
                direction: Direction::Read,
                value,
                start: start.0,
                end: self.cpu_clock,
                waited,
            });
        }
        CycleResult {
            result,
            wait_clocks: waited,
        }
    }
    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), ()> {
        let waited = self.begin(access, Direction::Write, start);
        let result = if access.bus_address >= RAM_START {
            let index = access.bus_address as usize & RAM_MASK;
            let bytes = value.to_be_bytes();
            if access.lanes != ByteLanes::Lower {
                self.ram[index] = bytes[0];
            }
            if access.lanes != ByteLanes::Upper {
                self.ram[(index + 1) & RAM_MASK] = bytes[1];
            }
            Ok(())
        } else {
            Err(())
        };
        self.transfers.push(Transfer {
            access,
            direction: Direction::Write,
            value,
            start: start.0,
            end: self.cpu_clock,
            waited,
        });
        CycleResult {
            result,
            wait_clocks: waited,
        }
    }
    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        assert_eq!(self.cpu_clock, start.0);
        self.advance(clocks);
    }
}
