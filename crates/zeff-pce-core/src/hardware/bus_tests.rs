use super::cpu::{Cpu, IrqPort, TimerPort, VdcPort};
use super::{
    BaseBus, BaseBusDevices, HUCARD_ROM_REGION_LEN, OPEN_BUS_VALUE, PhysicalRegion, PsgPort,
    VcePort, decode_physical_region,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Call {
    ReadVdc(VdcPort),
    WriteVdc(VdcPort, u8),
    DirectVdc(VdcPort, u8),
    ReadVce(VcePort),
    WriteVce(VcePort, u8),
    ReadPsg(PsgPort),
    WritePsg(PsgPort, u8),
    ReadTimer,
    WriteTimer(TimerPort, u8),
    ReadController,
    WriteController(u8),
    ReadIrq(IrqPort),
    WriteIrq(IrqPort, u8),
}

#[derive(Default)]
struct Devices {
    calls: Vec<Call>,
}

impl BaseBusDevices for Devices {
    fn read_vdc(&mut self, port: VdcPort) -> u8 {
        self.calls.push(Call::ReadVdc(port));
        0x10 | port.offset()
    }

    fn write_vdc(&mut self, port: VdcPort, value: u8) {
        self.calls.push(Call::WriteVdc(port, value));
    }

    fn write_vdc_direct(&mut self, port: VdcPort, value: u8) {
        self.calls.push(Call::DirectVdc(port, value));
    }

    fn read_vce(&mut self, port: VcePort) -> u8 {
        self.calls.push(Call::ReadVce(port));
        0x20 | port.offset()
    }

    fn write_vce(&mut self, port: VcePort, value: u8) {
        self.calls.push(Call::WriteVce(port, value));
    }

    fn read_psg(&mut self, port: PsgPort) -> u8 {
        self.calls.push(Call::ReadPsg(port));
        0x40 | port.offset()
    }

    fn write_psg(&mut self, port: PsgPort, value: u8) {
        self.calls.push(Call::WritePsg(port, value));
    }

    fn read_timer_counter(&mut self) -> u8 {
        self.calls.push(Call::ReadTimer);
        0x50
    }

    fn write_timer(&mut self, port: TimerPort, value: u8) {
        self.calls.push(Call::WriteTimer(port, value));
    }

    fn read_controller(&mut self) -> u8 {
        self.calls.push(Call::ReadController);
        0x60
    }

    fn write_controller(&mut self, value: u8) {
        self.calls.push(Call::WriteController(value));
    }

    fn read_irq(&mut self, port: IrqPort) -> u8 {
        self.calls.push(Call::ReadIrq(port));
        match port {
            IrqPort::Disable => 0x70,
            IrqPort::Request => 0x71,
        }
    }

    fn write_irq(&mut self, port: IrqPort, value: u8) {
        self.calls.push(Call::WriteIrq(port, value));
    }
}

#[test]
fn physical_decoder_covers_base_boundaries_and_mirrors() {
    assert_eq!(decode_physical_region(0), PhysicalRegion::HuCard(0));
    assert_eq!(
        decode_physical_region(0x0F_FFFF),
        PhysicalRegion::HuCard(0x0F_FFFF)
    );
    assert_eq!(decode_physical_region(0x10_0000), PhysicalRegion::Unmapped);
    assert_eq!(
        decode_physical_region(0x1F_0000),
        PhysicalRegion::WorkRam(0)
    );
    assert_eq!(
        decode_physical_region(0x1F_1FFF),
        PhysicalRegion::WorkRam(0x1FFF)
    );
    assert_eq!(
        decode_physical_region(0x1F_2000),
        PhysicalRegion::WorkRam(0)
    );
    assert_eq!(
        decode_physical_region(0x1F_7FFF),
        PhysicalRegion::WorkRam(0x1FFF)
    );
    assert_eq!(decode_physical_region(0x1F_8000), PhysicalRegion::Unmapped);
    assert_eq!(
        decode_physical_region(0x1F_E000),
        PhysicalRegion::Vdc(VdcPort::SelectOrStatus)
    );
    assert_eq!(
        decode_physical_region(0x1F_E3FE),
        PhysicalRegion::Vdc(VdcPort::DataLow)
    );
    assert_eq!(
        decode_physical_region(0x1F_E3FF),
        PhysicalRegion::Vdc(VdcPort::DataHigh)
    );
    assert_eq!(decode_physical_region(0x1F_E001), PhysicalRegion::Unmapped);
    assert_eq!(
        decode_physical_region(0x1F_E407),
        PhysicalRegion::Vce(VcePort::from_offset(7))
    );
    assert_eq!(
        decode_physical_region(0x1F_EBFF),
        PhysicalRegion::Psg(PsgPort::from_offset(15))
    );
    assert_eq!(
        decode_physical_region(0x1F_EC00),
        PhysicalRegion::Timer(TimerPort::CounterReload)
    );
    assert_eq!(
        decode_physical_region(0x1F_EFFF),
        PhysicalRegion::Timer(TimerPort::Control)
    );
    assert_eq!(
        decode_physical_region(0x1F_F3FF),
        PhysicalRegion::Controller
    );
    assert_eq!(
        decode_physical_region(0x1F_F402),
        PhysicalRegion::Irq(IrqPort::Disable)
    );
    assert_eq!(
        decode_physical_region(0x1F_F7FF),
        PhysicalRegion::Irq(IrqPort::Request)
    );
    assert_eq!(decode_physical_region(0x1F_F400), PhysicalRegion::Unmapped);
    assert_eq!(decode_physical_region(0x1F_F800), PhysicalRegion::Unmapped);
    assert_eq!(decode_physical_region(0x20_0000), PhysicalRegion::HuCard(0));
}

#[test]
fn plain_hucard_and_unmapped_accesses_are_bounded() {
    let mut bus = BaseBus::new(vec![0x12, 0x34], ()).unwrap();

    assert_eq!(bus.read(0), 0x12);
    assert_eq!(bus.read(1), 0x34);
    assert_eq!(bus.read(2), OPEN_BUS_VALUE);
    assert_eq!(bus.read(0x0F_FFFF), OPEN_BUS_VALUE);
    assert_eq!(bus.read(0x10_0000), OPEN_BUS_VALUE);
    assert_eq!(bus.read(0x20_0000), 0x12);

    bus.write(0, 0xA5);
    bus.write(0x10_0000, 0xA5);
    assert_eq!(bus.read(0), 0x12);

    let error = BaseBus::new(vec![0; HUCARD_ROM_REGION_LEN + 1], ()).unwrap_err();
    assert_eq!(error.rom_len(), HUCARD_ROM_REGION_LEN + 1);
}

#[test]
fn eight_kibibytes_of_work_ram_repeat_across_all_four_cer_banks() {
    let mut bus = BaseBus::new(Vec::new(), ()).unwrap();

    bus.write(0x1F_0005, 0xA5);
    assert_eq!(bus.read(0x1F_2005), 0xA5);
    assert_eq!(bus.read(0x1F_4005), 0xA5);
    assert_eq!(bus.read(0x1F_6005), 0xA5);

    bus.write(0x1F_7FFF, 0x5A);
    assert_eq!(bus.work_ram()[0x1FFF], 0x5A);
    assert_eq!(bus.read(0x1F_1FFF), 0x5A);
}

#[test]
fn device_windows_deliver_typed_mirrored_ports() {
    let mut bus = BaseBus::new(Vec::new(), Devices::default()).unwrap();

    assert_eq!(bus.read(0x1F_E3FE), 0x12);
    assert_eq!(bus.read(0x1F_E407), 0x27);
    assert_eq!(bus.read(0x1F_EBFF), 0x4F);
    assert_eq!(bus.read(0x1F_EC00), 0x50);
    assert_eq!(bus.read(0x1F_EFFF), OPEN_BUS_VALUE);
    assert_eq!(bus.read(0x1F_F3FF), 0x60);
    assert_eq!(bus.read(0x1F_F402), 0x70);
    assert_eq!(bus.read(0x1F_F7FF), 0x71);
    assert_eq!(bus.read(0x1F_E001), OPEN_BUS_VALUE);
    assert_eq!(bus.read(0x1F_F400), OPEN_BUS_VALUE);

    bus.write(0x1F_E003, 1);
    bus.write(0x1F_E406, 2);
    bus.write(0x1F_E80F, 3);
    bus.write(0x1F_EC01, 4);
    bus.write(0x1F_F000, 5);
    bus.write(0x1F_F406, 6);
    bus.write(0x1F_F403, 7);

    assert_eq!(
        bus.devices().calls,
        [
            Call::ReadVdc(VdcPort::DataLow),
            Call::ReadVce(VcePort::from_offset(7)),
            Call::ReadPsg(PsgPort::from_offset(15)),
            Call::ReadTimer,
            Call::ReadController,
            Call::ReadIrq(IrqPort::Disable),
            Call::ReadIrq(IrqPort::Request),
            Call::WriteVdc(VdcPort::DataHigh, 1),
            Call::WriteVce(VcePort::from_offset(6), 2),
            Call::WritePsg(PsgPort::from_offset(15), 3),
            Call::WriteTimer(TimerPort::Control, 4),
            Call::WriteController(5),
            Call::WriteIrq(IrqPort::Disable, 6),
            Call::WriteIrq(IrqPort::Request, 7),
        ]
    );
}

#[test]
fn cpu_fetches_rom_and_writes_ram_through_mprs() {
    let mut rom = vec![0xFF; 0x4002];
    rom[0x4000] = 0x62;
    rom[0x4001] = 0xEA;
    let mut bus = BaseBus::new(rom, ()).unwrap();
    let mut cpu = Cpu::new();
    cpu.registers_mut().pc = 0x8000;
    cpu.registers_mut().a = 0xA5;
    cpu.set_mapping_register(4, 2);

    let step = cpu.step(&mut bus).unwrap();

    assert_eq!(step.opcode, 0x62);
    assert_eq!(cpu.registers().pc, 0x8001);
    assert_eq!(cpu.registers().a, 0);

    let mut bus = BaseBus::new(vec![0x85, 0x05], ()).unwrap();
    let mut cpu = Cpu::new();
    cpu.registers_mut().a = 0x5A;
    cpu.set_mapping_register(1, 0xF8);

    cpu.step(&mut bus).unwrap();

    assert_eq!(bus.work_ram()[5], 0x5A);
    assert_eq!(bus.read(0x1F_6005), 0x5A);
}

#[test]
fn direct_vdc_stores_bypass_the_physical_decoder() {
    let rom = vec![0x03, 0xA5, 0x13, 0xB6, 0x23, 0xC7];
    let mut bus = BaseBus::new(rom, Devices::default()).unwrap();
    let mut cpu = Cpu::new();

    cpu.step(&mut bus).unwrap();
    cpu.step(&mut bus).unwrap();
    cpu.step(&mut bus).unwrap();

    assert_eq!(
        bus.devices().calls,
        [
            Call::DirectVdc(VdcPort::SelectOrStatus, 0xA5),
            Call::DirectVdc(VdcPort::DataLow, 0xB6),
            Call::DirectVdc(VdcPort::DataHigh, 0xC7),
        ]
    );
}
