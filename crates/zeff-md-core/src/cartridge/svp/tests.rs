use super::{
    controller::Controller,
    model::Phase,
    ssp1601::{Core, Ports},
    *,
};
use zeff_emu_common::save_state::StateWriter;
use zeff_m68k::{
    Cpu, StepOutcome,
    bus::{BusAccess, ByteLanes, M68kBus},
};

fn images() -> (Vec<u16>, Vec<u16>) {
    let mut rom = vec![0; 0x420];
    let code = [
        0x0840, 0, 0x0038, 0xa800, 2, 0x4d50, 0x0402, 0x003b, 0x8800, 1, 0x0013, 0x0840, 0x0020,
        0x08e0, 0, 0x08e0, 0x0018, 0x0080, 0x0081, 0x08e0, 1, 0x08e0, 0x0018, 0x0080, 0x0081,
        0x0840, 0, 0x00b1, 0x4c00, 0x041c,
    ];
    rom[0x400..0x400 + code.len()].copy_from_slice(&code);
    let mut irom = vec![0; 1024];
    irom[0x3fc] = 0xfc08;
    irom[8..10].copy_from_slice(&[0x0860, 0x0400]);
    (rom, irom)
}

fn fresh() -> InstructionModel {
    let (rom, irom) = images();
    InstructionModel::new_synthetic(rom, irom).unwrap()
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum HarnessError {
    Address(u32, ByteLanes),
    Model(Error),
}

struct Harness {
    model: InstructionModel,
    program: Vec<u8>,
    status_samples: Vec<(ByteLanes, u16)>,
}

impl Harness {
    fn new(lower: bool) -> Self {
        let mut program = vec![0; 0x12a];
        program[..8].copy_from_slice(&[0, 0, 0x20, 0, 0, 0, 1, 0]);
        let code: [u16; 21] = [
            0x33fc,
            0x1234,
            0x00a1,
            0x5000,
            if lower { 0x1039 } else { 0x3039 },
            0x00a1,
            if lower { 0x5005 } else { 0x5004 },
            if lower { 0x0200 } else { 0x0240 },
            1,
            0x67f4,
            0x3239,
            0x00a1,
            0x5000,
            0x3439,
            0x0030,
            0,
            0x3639,
            0x0030,
            2,
            0x4e71,
            0x4e71,
        ];
        for (offset, word) in code.into_iter().enumerate() {
            program[0x100 + offset * 2..0x102 + offset * 2].copy_from_slice(&word.to_be_bytes());
        }
        Self {
            model: fresh(),
            program,
            status_samples: Vec::new(),
        }
    }
}

impl M68kBus for Harness {
    type Error = HarnessError;

    fn read(&mut self, access: BusAccess) -> Result<u16, Self::Error> {
        let address = access.bus_address;
        if address == 0xa15004 && matches!(access.lanes, ByteLanes::Both | ByteLanes::Lower) {
            let value = self
                .model
                .consume_main_status()
                .map_err(HarnessError::Model)?;
            self.status_samples.push((access.lanes, value));
            return Ok(value);
        }
        let value = match (address, access.lanes) {
            (0xa15000, ByteLanes::Both) => self.model.peek_result(),
            (0x300000, ByteLanes::Both) => self.model.peek_dram(0),
            (0x300002, ByteLanes::Both) => self.model.peek_dram(1),
            _ if (address as usize) + 1 < self.program.len() => {
                let index = address as usize;
                let word = u16::from_be_bytes([self.program[index], self.program[index + 1]]);
                match access.lanes {
                    ByteLanes::Both => word,
                    ByteLanes::Upper => word & 0xff00,
                    ByteLanes::Lower => word & 0x00ff,
                }
            }
            _ => return Err(HarnessError::Address(address, access.lanes)),
        };
        Ok(value)
    }

    fn write(&mut self, access: BusAccess, value: u16) -> Result<(), Self::Error> {
        if (access.bus_address, access.lanes) != (0xa15000, ByteLanes::Both) {
            return Err(HarnessError::Address(access.bus_address, access.lanes));
        }
        self.model
            .publish_main_command(value)
            .map_err(HarnessError::Model)
    }
}

fn main_step(cpu: &mut Cpu, bus: &mut Harness, opcode: u16) {
    assert!(
        matches!(cpu.step(bus), StepOutcome::Executed { opcode: actual, .. } if actual == opcode)
    );
}

fn busy_is_atomic(model: &InstructionModel) {
    if matches!(model.phase, Phase::Fetch) {
        return;
    }
    let mut candidate = model.clone();
    assert_eq!(candidate.publish_main_command(0x9876), Err(Error::Busy));
    assert_eq!(candidate, *model);
    assert_eq!(candidate.consume_main_status(), Err(Error::Busy));
    assert_eq!(candidate, *model);
    assert_eq!(candidate.set_synthetic_input_pins(0), Err(Error::Busy));
    assert_eq!(candidate, *model);
}

fn phase_roundtrip(model: &InstructionModel, longer: bool) {
    busy_is_atomic(model);
    let bytes = model.encode_state().unwrap();
    let mut restored = fresh();
    restored.decode_state(&bytes).unwrap();
    assert_eq!(restored, *model);
    assert_eq!(restored.advance(false), Ok(Progress::Waiting));
    assert_eq!(restored, *model);
    let mut expected = model.clone();
    let count = if longer { 18 } else { 1 };
    for _ in 0..count {
        assert_eq!(restored.advance(true), expected.advance(true));
        assert_eq!(restored, expected);
    }
}

fn rejected_state(model: &InstructionModel, bytes: &[u8]) {
    let mut candidate = model.clone();
    assert!(matches!(
        candidate.decode_state(bytes),
        Err(Error::State(_))
    ));
    assert_eq!(candidate, *model);
}

fn malformed_states(model: &InstructionModel) {
    let bytes = model.encode_state().unwrap();
    for length in [0, 7, 47, bytes.len() - 1] {
        rejected_state(model, &bytes[..length]);
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    rejected_state(model, &trailing);
    let (mut rom, irom) = images();
    rom[0x400] ^= 1;
    let mismatch = InstructionModel::new_synthetic(rom, irom).unwrap();
    rejected_state(&mismatch, &bytes);
    let (rom, mut irom) = images();
    irom[0] ^= 1;
    rejected_state(&InstructionModel::new_synthetic(rom, irom).unwrap(), &bytes);
    match &model.phase {
        Phase::Execute { .. } => {
            let mut forged = bytes;
            let offset = forged.len() - 2;
            forged[offset] ^= 1;
            rejected_state(model, &forged);
        }
        Phase::Commit { ports, .. } => {
            let mut controller = StateWriter::new();
            model.ports.write_state(&mut controller);
            let pending_core = 48 + 1063 + controller.position() + 1;
            let mut forged = bytes.clone();
            forged[pending_core + 4] ^= 1;
            rejected_state(model, &forged);
            let mut pending = StateWriter::new();
            ports.write_state(&mut pending);
            let dram = pending_core + 1063 + pending.position() - 65536 * 2;
            let mut forged = bytes;
            forged[dram] ^= 1;
            rejected_state(model, &forged);
        }
        _ => {}
    }
}

fn run_exchange(lower: bool) {
    let mut bus = Harness::new(lower);
    let mut main = Cpu::new();
    main.reset(&mut bus).unwrap();
    malformed_states(&bus.model);
    for _ in 0..4 {
        phase_roundtrip(&bus.model, false);
        bus.model.advance(true).unwrap();
    }
    assert_eq!(bus.model.registers().pc, 0x0400);
    for _ in 0..12 {
        phase_roundtrip(&bus.model, false);
        bus.model.advance(true).unwrap();
    }
    assert_eq!(
        (bus.model.registers().pc, bus.model.peek_status()),
        (0x0402, 0)
    );
    main_step(&mut main, &mut bus, 0x33fc);
    assert_eq!(
        (bus.model.peek_status(), bus.model.peek_result()),
        (2, 0x1234)
    );
    for opcode in [
        if lower { 0x1039 } else { 0x3039 },
        if lower { 0x0200 } else { 0x0240 },
        0x67f4,
    ] {
        main_step(&mut main, &mut bus, opcode);
    }
    assert_eq!(main.registers.pc, 0x108);
    let mut coverage = [false; 9];
    for _ in 0..96 {
        let long = match &bus.model.phase {
            Phase::Commit { step, ports, .. } if step.pc == 0x0402 => {
                coverage[8] = true;
                assert_eq!(ports.status(), 0);
                assert_eq!(bus.model.peek_status(), 2);
                false
            }
            Phase::Fetch if bus.model.core.pc == 0x040f => {
                coverage[0] = true;
                true
            }
            Phase::Fetch if bus.model.core.pc == 0x0411 => {
                coverage[1] = true;
                true
            }
            Phase::Commit { step, ports, .. } if step.pc == 0x0411 => {
                coverage[2] = true;
                assert_ne!(*ports, bus.model.ports);
                assert_eq!((bus.model.peek_dram(0), bus.model.peek_dram(1)), (0, 0));
                true
            }
            Phase::Commit { step, ports, .. } if step.pc == 0x0412 => {
                coverage[3] = true;
                assert_eq!(ports.dram(0), 0x1235);
                assert_eq!(bus.model.peek_dram(0), 0);
                true
            }
            Phase::Commit { step, ports, .. } if step.pc == 0x0418 => {
                coverage[4] = true;
                assert_eq!((ports.dram(0), ports.dram(1)), (0x1235, 0x1235));
                assert_eq!(bus.model.peek_dram(1), 0);
                false
            }
            Phase::Commit { step, ports, .. } if step.pc == 0x041b => {
                coverage[5] = true;
                assert_eq!((ports.status(), ports.xst()), (1, 0x1235));
                assert_eq!(
                    (bus.model.peek_status(), bus.model.peek_result()),
                    (0, 0x1234)
                );
                malformed_states(&bus.model);
                true
            }
            _ => false,
        };
        if matches!(bus.model.phase, Phase::Execute { pc: 0x0411, .. }) {
            malformed_states(&bus.model);
        }
        phase_roundtrip(&bus.model, long);
        let progress = bus.model.advance(true).unwrap();
        if matches!(progress, Progress::Committed { pc: 0x0412, .. }) {
            coverage[6] = true;
            assert_eq!(
                (bus.model.peek_dram(0), bus.model.peek_status()),
                (0x1235, 0)
            );
        }
        if matches!(progress, Progress::Committed { pc: 0x041b, .. }) {
            coverage[7] = true;
            phase_roundtrip(&bus.model, true);
            break;
        }
    }
    assert_eq!(coverage, [true; 9]);
    let snapshot = bus.model.clone();
    assert_eq!(bus.model.peek_status(), 1);
    assert_eq!(bus.model.peek_status(), 1);
    assert_eq!(bus.model, snapshot);
    for opcode in [
        if lower { 0x1039 } else { 0x3039 },
        if lower { 0x0200 } else { 0x0240 },
        0x67f4,
        0x3239,
        0x3439,
        0x3639,
    ] {
        main_step(&mut main, &mut bus, opcode);
    }
    assert_eq!(&main.registers.d[..4], &[1, 0x1235, 0x1235, 0x1235]);
    assert_eq!(main.registers.pc, 0x126);
    assert_eq!(bus.model.peek_status(), 0);
    assert_eq!(
        bus.status_samples,
        [
            (
                if lower {
                    ByteLanes::Lower
                } else {
                    ByteLanes::Both
                },
                2
            ),
            (
                if lower {
                    ByteLanes::Lower
                } else {
                    ByteLanes::Both
                },
                1
            )
        ]
    );
}

fn cpu_word(core: &mut Core, ports: &mut Controller, words: &[u16]) {
    core.pc = 0;
    core.step(
        |address| {
            words
                .get(usize::from(address))
                .copied()
                .ok_or(ssp1601::Fault::ProgramRead(address))
        },
        ports,
    )
    .unwrap();
}

fn reject_cpu_word(core: &Core, ports: &Controller, words: &[u16]) {
    let mut candidate = core.clone();
    candidate.pc = 0;
    let before = candidate.clone();
    let mut candidate_ports = ports.clone();
    assert!(
        candidate
            .step(
                |address| words
                    .get(usize::from(address))
                    .copied()
                    .ok_or(ssp1601::Fault::ProgramRead(address)),
                &mut candidate_ports
            )
            .is_err()
    );
    assert_eq!(candidate, before);
    assert_eq!(candidate_ports, *ports);
}

fn directional_programming_rollback() {
    let mut cpu = Core::synthetic(0);
    let mut ports = Controller::synthetic();
    for words in [
        &[0x0840, 0x0020][..],
        &[0x0810, 0x55aa],
        &[0x08e0, 5],
        &[0x08e0, 0x0018],
        &[0x0080],
        &[0x0081],
    ] {
        cpu_word(&mut cpu, &mut ports, words);
    }
    assert_eq!(ports.dram(5), 0x55aa);
    cpu_word(&mut cpu, &mut ports, &[0x08e0, 9]);
    reject_cpu_word(&cpu, &ports, &[0x0081]);
    reject_cpu_word(&cpu, &ports, &[0x08e0, 0xc018]);
    cpu_word(&mut cpu, &mut ports, &[0x0840, 0]);
    reject_cpu_word(&cpu, &ports, &[0x0038]);
    cpu_word(&mut cpu, &mut ports, &[0x08e0, 0x0018]);
    reject_cpu_word(&cpu, &ports, &[0x0038]);
    reject_cpu_word(&cpu, &ports, &[0x0080]);
    cpu_word(&mut cpu, &mut ports, &[0x0840, 0x0020]);
    reject_cpu_word(&cpu, &ports, &[0x0081]);
    cpu_word(&mut cpu, &mut ports, &[0x0080]);
    cpu_word(&mut cpu, &mut ports, &[0x0810, 0x1235]);
    cpu_word(&mut cpu, &mut ports, &[0x0081]);
    assert_eq!((ports.dram(5), ports.dram(9)), (0x55aa, 0x1235));
    assert_eq!(ports.read(6, 0, false).unwrap(), 9);
    assert_eq!(ports.read(6, 0, false).unwrap(), 0x0090);
    ports.write(6, 0, false, 0x1234).unwrap();
    let before = ports.clone();
    assert!(ports.read(6, 0, false).is_err());
    assert_eq!(ports, before);
    cpu.a = 0x12345678;
    cpu_word(&mut cpu, &mut ports, &[0x000f]);
    assert_eq!(cpu.a, 0x12345678);
}

fn bus_rejections() {
    use zeff_m68k::bus::{AccessKind, FunctionCode};
    let mut bus = Harness::new(false);
    let before = bus.model.clone();
    for (address, lanes) in [(0x200000, ByteLanes::Both), (0xa15004, ByteLanes::Upper)] {
        let access = BusAccess {
            logical_address: address,
            bus_address: address,
            function_code: FunctionCode::SupervisorData,
            kind: AccessKind::Data,
            lanes,
        };
        assert_eq!(bus.read(access), Err(HarnessError::Address(address, lanes)));
        assert_eq!(
            bus.write(access, 1),
            Err(HarnessError::Address(address, lanes))
        );
        assert_eq!(bus.model, before);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn scalar_owner_command_state_contract() {
    let mut exhausted = fresh();
    for _ in 0..3 {
        exhausted.advance(true).unwrap();
    }
    exhausted.instructions = u64::MAX;
    let pending = exhausted.encode_state().unwrap();
    assert!(exhausted.advance(true).is_err());
    assert_eq!(exhausted.encode_state().unwrap(), pending);
    service_tests::contract();
    dram_tests::dram_storage_contract();
    run_exchange(false);
    run_exchange(true);
    directional_programming_rollback();
    bus_rejections();
    boot_tests::controller_boot_contract();
    crate::Emulator::assert_synthetic_svp_transport();
}

#[path = "tests/boot.rs"]
mod boot_tests;
#[path = "tests/dram.rs"]
mod dram_tests;

#[path = "tests/rom_history.rs"]
mod rom_history_tests;
#[path = "tests/service.rs"]
mod service_tests;
