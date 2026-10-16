use zeff_emu_common::time::MasterTicks;
use zeff_md_core::{
    ControlCommand, ControlWrite, Region, Vdp, VdpDmaRegisters, VdpInterrupts, VdpLineError,
    VdpPortError, VdpRegisterPipeline, VdpRegisterWrite, VdpScheduleError,
};

pub const REGION: Region = Region::NtscUs;
pub const START: u64 = 1001;
const REFRESH: [u64; 5] = [530, 1170, 1810, 2450, 3310];

#[derive(Clone, Copy, Debug)]
pub enum Op {
    Seed(ControlCommand, Option<u16>),
    Advance(u64),
    Acknowledge(u64),
    Capture(u64),
    Status,
    Control(u16),
    Data(u16),
    Read,
    ReadWait,
    WriteWait,
    Register(u8, u8),
}

#[derive(Debug, PartialEq, Eq)]
pub enum Value {
    Unit,
    Word(u16),
    Level(u8),
    Wait(Option<MasterTicks>),
    Control(ControlWrite),
    Register(VdpRegisterWrite),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Port(VdpPortError),
    Schedule(VdpScheduleError),
    Line(VdpLineError),
}

pub type Run<'a> = dyn FnMut(&mut Vdp, Op) -> Result<Value, Error> + 'a;

pub fn execute(device: &mut Vdp, op: Op) -> Result<Value, Error> {
    match op {
        Op::Seed(command, data) => device
            .resume_dma_from_known_phase(command, data, REGION)
            .map(|()| Value::Unit)
            .map_err(Error::Port),
        Op::Advance(at) => device
            .advance_to(tick(at), REGION)
            .map(|()| Value::Unit)
            .map_err(Error::Schedule),
        Op::Acknowledge(at) => device
            .acknowledge_to(tick(at), REGION)
            .map(Value::Level)
            .map_err(Error::Schedule),
        Op::Capture(at) => device
            .capture_line_at(tick(at), REGION)
            .map(|_| Value::Unit)
            .map_err(Error::Line),
        Op::Status => device
            .read_status(REGION)
            .map(Value::Word)
            .map_err(Error::Port),
        Op::Control(word) => device
            .write_control(word, REGION)
            .map(Value::Control)
            .map_err(Error::Port),
        Op::Data(word) => device
            .write_data(word, REGION)
            .map(|()| Value::Unit)
            .map_err(Error::Port),
        Op::Read => device
            .read_data(REGION)
            .map(Value::Word)
            .map_err(Error::Port),
        Op::ReadWait => device
            .data_read_wait(REGION)
            .map(Value::Wait)
            .map_err(Error::Port),
        Op::WriteWait => device
            .data_write_wait(REGION)
            .map(Value::Wait)
            .map_err(Error::Port),
        Op::Register(index, value) => device
            .queue_register(index, value, REGION)
            .map(Value::Register)
            .map_err(Error::Schedule),
    }
}

pub fn tick(at: u64) -> MasterTicks {
    MasterTicks::new(at)
}

pub fn command(address: u32, code: u8) -> ControlCommand {
    ControlCommand { address, code }
}

pub fn slot_after(now: u64) -> u64 {
    let mut next = (now / 20) * 20 + 10;
    if next <= now {
        next += 20;
    }
    if REFRESH.contains(&(next % 3420)) {
        next += 20;
    }
    next
}

pub fn slot(count: usize) -> u64 {
    (0..count).fold(START, |at, _| slot_after(at))
}

pub fn register(device: &mut Vdp, index: u8, value: u8) {
    let write = device.queue_register(index, value, REGION).unwrap();
    device
        .advance_to(tick(write.publish_at.get() + 1), REGION)
        .unwrap();
}

pub fn port_command(device: &mut Vdp, address: u32, code: u8) {
    device
        .write_control(
            (address as u16 & 0x3fff) | (u16::from(code & 3) << 14),
            REGION,
        )
        .unwrap();
    device
        .write_control(
            ((address >> 14) as u16 & 7) | (u16::from(code & 0x3c) << 2),
            REGION,
        )
        .unwrap();
}

pub fn word(device: &mut Vdp, address: u32, data: u16) {
    port_command(device, address, 1);
    device.write_data(data, REGION).unwrap();
    let end = slot_after(slot_after(device.now().get()));
    device.advance_to(tick(end + 1), REGION).unwrap();
    assert!(device.fifo().is_empty());
}

pub fn fixture(length: u16, source: u16, mode: u8, increment: u8) -> Vdp {
    let mut device = Vdp::bootstrap_mode5();
    device.advance_to(tick(1), REGION).unwrap();
    register(&mut device, 15, 0);
    for (address, data) in [
        (0, 0x1234),
        (2, 0x5678),
        (0xfffe, 0xabcd),
        (0x100, 0x1020),
        (0x102, 0x3040),
        (0x104, 0x5060),
    ] {
        word(&mut device, address, data);
    }
    for (index, value) in [
        (15, increment),
        (19, length as u8),
        (20, (length >> 8) as u8),
        (21, source as u8),
        (22, (source >> 8) as u8),
        (23, mode),
    ] {
        register(&mut device, index, value);
    }
    device.advance_to(tick(START), REGION).unwrap();
    assert!(device.fifo().is_empty());
    assert!(device.control().pending_word().is_none());
    assert!(!device.read_cache().is_ready());
    device
}

pub fn counters(device: &Vdp, length: u16, source: u16, mode: u8) {
    let dma = VdpDmaRegisters::from_registers(device.registers()).unwrap();
    assert_eq!(
        (dma.length(), dma.source(), dma.source_high()),
        (length, source, mode)
    );
    assert!(device.registers().pending().is_none());
    assert_eq!(device.registers().now(), device.now());
}

pub fn advance(run: &mut Run<'_>, device: &mut Vdp, at: u64) {
    assert_eq!(run(device, Op::Advance(at)), Ok(Value::Unit));
    assert_eq!(device.now(), tick(at));
}

pub fn seed(run: &mut Run<'_>, device: &mut Vdp, address: u32, code: u8, fill: Option<u16>) {
    assert_eq!(
        run(device, Op::Seed(command(address, code), fill)),
        Ok(Value::Unit)
    );
    assert_eq!(device.command(), command(address, code));
}

pub fn busy_status(run: &mut Run<'_>, device: &mut Vdp, busy: bool) {
    let Value::Word(status) = run(device, Op::Status).unwrap() else {
        panic!("status word")
    };
    assert_eq!(status & 2, u16::from(busy) * 2);
    assert_eq!(status & 0x200, 0x200);
}

pub fn reject(run: &mut Run<'_>, device: &mut Vdp, op: Op, error: Error) {
    let saved = device.clone();
    let pointer = device.memory().vram().as_ptr();
    assert_eq!(run(device, op), Err(error));
    assert_eq!(*device, saved);
    assert_eq!(device.memory().vram().as_ptr(), pointer);
}

pub fn known(epoch: u64, at: u64, mode: u8, skip: Option<u8>, stable: bool) -> Vdp {
    let mut bank = VdpRegisterPipeline::at_h32_edge(tick(epoch));
    for index in 0..24 {
        if skip == Some(index) {
            continue;
        }
        let value = match index {
            1 => mode,
            15 => 1,
            19 => 2,
            23 => 0x80,
            _ => 0,
        };
        bank.advance_to(tick(bank.now().get() + 1)).unwrap();
        let write = bank.queue_register(index, value).unwrap();
        bank.advance_to(write.publish_at).unwrap();
    }
    if mode & 0x40 == 0 {
        let interrupts = VdpInterrupts::at_counters_zero(bank.now(), 0, false, false);
        let mut device = Vdp::from_known_state(interrupts, bank, REGION).unwrap();
        device.advance_to(tick(at), REGION).unwrap();
        return device;
    }
    bank.advance_to(tick(at)).unwrap();
    let mut interrupts = VdpInterrupts::at_counters_zero(tick(epoch), 0, false, false);
    while interrupts
        .advance_to(tick(at), REGION, &bank)
        .unwrap()
        .is_some()
    {}
    if stable {
        Vdp::from_stable_display_state(interrupts, bank, REGION).unwrap()
    } else {
        Vdp::from_known_state(interrupts, bank, REGION).unwrap()
    }
}
