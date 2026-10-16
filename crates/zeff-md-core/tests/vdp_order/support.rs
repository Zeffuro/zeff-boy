use zeff_emu_common::time::MasterTicks;
use zeff_m68k::Registers;
use zeff_m68k::bus::{BusAccess, BusResponse, ByteLanes};
use zeff_m68k::timing::{ClockedBus, CpuClocks, CycleResult, InterruptVector, PrefetchCpu};
use zeff_md_core::{Region, VdpEvent, VdpInterrupts, VdpRegisterPipeline, VdpScheduleError};

pub fn tick(value: u64) -> MasterTicks {
    MasterTicks::new(value)
}

pub fn publish(registers: &mut VdpRegisterPipeline, index: u8, value: u8) {
    registers
        .advance_to(tick(registers.now().get() + 1))
        .unwrap();
    let write = registers.queue_register(index, value).unwrap();
    registers.advance_to(write.publish_at).unwrap();
}

pub fn known(
    epoch: u64,
    counter: u8,
    h_pending: bool,
    v_pending: bool,
) -> (VdpInterrupts, VdpRegisterPipeline) {
    let mut registers = VdpRegisterPipeline::at_h32_edge(tick(epoch - 80));
    for (index, value) in [(0, 0x10), (1, 0x64), (12, 0), (10, 2)] {
        publish(&mut registers, index, value);
    }
    (
        VdpInterrupts::at_counters_zero(tick(epoch), counter, h_pending, v_pending),
        registers,
    )
}

pub fn drain(state: &mut VdpInterrupts, registers: &mut VdpRegisterPipeline, at: u64) -> usize {
    let mut count = 0;
    while state
        .advance_registers_to(tick(at), Region::NtscUs, registers)
        .unwrap()
        .is_some()
    {
        count += 1;
    }
    assert_eq!(state.now(), registers.now());
    assert_eq!(state.now(), tick(at));
    count
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuBusError {
    Schedule(VdpScheduleError),
    Injected,
    Address,
    Clock,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Record {
    Read(BusAccess, u64),
    Write(BusAccess, u16, u64),
    Ack(BusAccess, u64, u64, u8),
    Idle(u64, u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpuBus {
    pub state: VdpInterrupts,
    pub registers: VdpRegisterPipeline,
    pub clock: CpuClocks,
    pub memory: [u8; 4096],
    pub records: [Option<Record>; 32],
    pub record_count: usize,
    pub events: [Option<VdpEvent>; 16],
    pub event_count: usize,
    pub wait: u32,
    pub fail_ack: bool,
    pub unsupported_ack: bool,
}

impl CpuBus {
    pub fn ready(at: u64, h_pending: bool, v_pending: bool) -> Self {
        let (mut state, mut registers) = known(1000, 255, h_pending, v_pending);
        drain(&mut state, &mut registers, at);
        let mut result = Self {
            state,
            registers,
            clock: CpuClocks(0),
            memory: [0; 4096],
            records: [None; 32],
            record_count: 0,
            events: [None; 16],
            event_count: 0,
            wait: 0,
            fail_ack: false,
            unsupported_ack: false,
        };
        for (address, value) in [
            (112, 0),
            (114, 0x200),
            (120, 0),
            (122, 0x300),
            (0x200, 0x4e71),
            (0x202, 0x7201),
            (0x300, 0x4e71),
            (0x302, 0x7201),
        ] {
            result.word(address, value);
        }
        result
    }
    pub fn word(&mut self, address: usize, value: u16) {
        self.memory[address..address + 2].copy_from_slice(&value.to_be_bytes());
    }
    pub fn read_word(&self, address: usize) -> u16 {
        u16::from_be_bytes(self.memory[address..address + 2].try_into().unwrap())
    }
    fn record(&mut self, record: Record) {
        self.records[self.record_count] = Some(record);
        self.record_count += 1;
    }
    fn advance(&mut self, start: CpuClocks, clocks: u32) -> Result<(), CpuBusError> {
        if start != self.clock {
            return Err(CpuBusError::Clock);
        }
        let at = self.state.now().get() + u64::from(clocks) * 7;
        while let Some(event) = self
            .state
            .advance_registers_to(tick(at), Region::NtscUs, &mut self.registers)
            .map_err(CpuBusError::Schedule)?
        {
            self.events[self.event_count] = Some(event);
            self.event_count += 1;
        }
        self.clock.0 += u64::from(clocks);
        Ok(())
    }
}

impl ClockedBus for CpuBus {
    type Error = CpuBusError;
    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, CpuBusError> {
        self.record(Record::Read(access, self.state.now().get()));
        let result = self.advance(start, 4).and_then(|()| {
            let address = access.bus_address as usize;
            if address + 1 >= self.memory.len() {
                Err(CpuBusError::Address)
            } else {
                Ok(self.read_word(address))
            }
        });
        CycleResult {
            result,
            wait_clocks: 0,
        }
    }
    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), CpuBusError> {
        self.record(Record::Write(access, value, self.state.now().get()));
        let result = self.advance(start, 4).and_then(|()| {
            let address = access.bus_address as usize;
            if address + 1 >= self.memory.len() {
                return Err(CpuBusError::Address);
            }
            let old = self.read_word(address);
            let value = match access.lanes {
                ByteLanes::Both => value,
                ByteLanes::Upper => (old & 0xff) | (value & 0xff00),
                ByteLanes::Lower => (old & 0xff00) | (value & 0xff),
            };
            self.word(address, value);
            Ok(())
        });
        CycleResult {
            result,
            wait_clocks: 0,
        }
    }
    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        self.record(Record::Idle(self.state.now().get(), clocks));
        self.advance(start, clocks).unwrap();
    }
    fn acknowledge_cycle(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> CycleResult<BusResponse<InterruptVector>, CpuBusError> {
        if self.fail_ack {
            return CycleResult {
                result: Err(CpuBusError::Injected),
                wait_clocks: 0,
            };
        }
        if self.unsupported_ack {
            return CycleResult::ready(BusResponse::Unsupported);
        }
        if start != self.clock {
            return CycleResult {
                result: Err(CpuBusError::Clock),
                wait_clocks: 0,
            };
        }
        let begin = self.state.now().get();
        let at = begin + (4 + u64::from(self.wait)) * 7;
        let result = self
            .state
            .complete_scheduled_acknowledge(tick(at), Region::NtscUs, &mut self.registers)
            .map_err(CpuBusError::Schedule)
            .map(|cleared| {
                self.clock.0 += 4 + u64::from(self.wait);
                self.record(Record::Ack(access, begin, at, cleared));
                BusResponse::Complete(InterruptVector::Autovector)
            });
        CycleResult {
            result,
            wait_clocks: self.wait,
        }
    }
}

pub fn cpu(bus: &CpuBus) -> PrefetchCpu {
    let mut registers = Registers::new();
    registers.pc = 0x100;
    registers.ssp = 0x1000;
    registers.set_status_register(0x2000);
    let mut cpu = PrefetchCpu::from_prefetched(registers, [0x4e71; 2], CpuClocks(0)).unwrap();
    cpu.interrupt_input()
        .set_level(bus.state.interrupt_level(&bus.registers).unwrap());
    cpu
}
