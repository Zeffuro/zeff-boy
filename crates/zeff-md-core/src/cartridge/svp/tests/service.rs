use super::*;
use crate::svp::{ServiceProgress, ServiceResource, ServiceRole};
use sha2::{Digest, Sha256};

fn serviced() -> InstructionModel {
    let (rom, irom) = images();
    InstructionModel::new_synthetic_serviced(rom, irom).unwrap()
}

fn restored(model: &InstructionModel, digest: &mut Sha256) {
    let bytes = model.encode_state().unwrap();
    let mut copy = serviced();
    copy.decode_state(&bytes).unwrap();
    assert_eq!(copy, *model);
    assert_eq!(copy.encode_state().unwrap(), bytes);
    digest.update(&bytes);
    assert_eq!(
        copy.advance_service(true),
        model.clone().advance_service(true)
    );
}

fn malformed(model: &InstructionModel) {
    let bytes = model.encode_state().unwrap();
    let section = bytes
        .windows(8)
        .position(|part| part == b"ZSVPSVC1")
        .unwrap();
    assert!(bytes[section + 8] > 0);
    let mut target = serviced();
    let before = target.clone();
    for offset in
        [section, section + 8]
            .into_iter()
            .chain((0..usize::from(bytes[section + 8])).flat_map(|record| {
                [0, 8, 10, 11, 12, 13, 14, 18, 20, 22, 23, 25]
                    .map(move |offset| section + 9 + record * 27 + offset)
            }))
    {
        let mut corrupt = bytes.clone();
        corrupt[offset] ^= 0xff;
        assert!(target.decode_state(&corrupt).is_err(), "offset {offset}");
        assert_eq!(target, before);
    }
    let mut extra = bytes.clone();
    extra[section + 8] += 1;
    extra.extend_from_slice(&bytes[section + 9..section + 36]);
    assert!(target.decode_state(&extra).is_err());
    let mut duplicate = bytes.clone();
    duplicate.extend_from_slice(&bytes[section..]);
    assert!(target.decode_state(&duplicate).is_err());
    assert!(target.decode_state(&bytes[..section]).is_err());
    assert_eq!(target, before);
    assert!(fresh().decode_state(&bytes).is_err());
    assert!(
        target
            .decode_state(&fresh().encode_state().unwrap())
            .is_err()
    );
}

pub(super) fn contract() {
    let mut model = serviced();
    let mut legacy = fresh();
    let mut digest = Sha256::new();
    port_paths(&mut digest);
    assert!(model.advance(true).is_err());
    assert!(legacy.advance_service(true).is_err());
    restored(&model, &mut digest);
    assert!(
        matches!(model.advance_service(true).unwrap(), ServiceProgress::Granted { request, .. } if request.role == ServiceRole::ResetVector)
    );
    legacy.advance(true).unwrap();
    model.publish_main_command(0x1234).unwrap();
    legacy.publish_main_command(0x1234).unwrap();
    let mut grants = 0;
    let mut resources = 0_u8;
    let mut port_capture_checked = false;
    for _ in 0..300 {
        let pending = model.pending_service().unwrap();
        let bytes = model.encode_state().unwrap();
        let registers = model.registers();
        let ports = model.ports.clone();
        assert_eq!(model.advance_service(false), Ok(ServiceProgress::Waiting));
        assert_eq!(model.pending_service().unwrap(), pending);
        assert_eq!(model.encode_state().unwrap(), bytes);
        restored(&model, &mut digest);
        if let Some(request) = pending {
            if !matches!(model.phase, Phase::Fetch) {
                assert_eq!(model.publish_main_command(0xbeef), Err(Error::Busy));
                assert_eq!(model.main_dram_write(0, 0xbeef, 0xffff), Err(Error::Busy));
                assert_eq!(model.set_synthetic_input_pins(0xc00), Err(Error::Busy));
            }
            if matches!(request.role, ServiceRole::PortWrite(_)) {
                assert_eq!(model.peek_dram(0), legacy.peek_dram(0));
            }
        }
        match model.advance_service(true).unwrap() {
            ServiceProgress::Granted { request, .. } => {
                grants += 1;
                assert_eq!(pending, Some(request));
                assert_eq!(model.registers(), registers);
                assert_eq!(model.ports, ports);
                resources |= match request.resource {
                    ServiceResource::ProgramIrom => 1,
                    ServiceResource::SharedRom => 2,
                    ServiceResource::SharedDram => 4,
                    ServiceResource::Controller => 8,
                    _ => 0,
                };
                if matches!(
                    request.role,
                    ServiceRole::PortRead(_) | ServiceRole::PortWrite(_)
                ) && !port_capture_checked
                {
                    malformed(&model);
                    port_capture_checked = true;
                }
            }
            ServiceProgress::Boundary(Progress::Prepared { .. }) => {
                assert_eq!(model.registers(), registers);
                assert_eq!(model.ports, ports);
            }
            ServiceProgress::Boundary(Progress::Committed { .. }) => {
                loop {
                    if matches!(legacy.advance(true).unwrap(), Progress::Committed { .. }) {
                        break;
                    }
                }
                assert_eq!(model.core, legacy.core);
                assert_eq!(model.ports, legacy.ports);
                if model.instructions == 32 {
                    break;
                }
            }
            other => panic!("unexpected service progress {other:?}"),
        }
    }
    assert_eq!(model.instructions, 32);
    assert!(grants > 50 && port_capture_checked);
    assert_eq!(resources, 15);

    let mut alias = serviced();
    alias.advance_service(true).unwrap();
    alias.core.pc = 0x400;
    alias.core.ram[0][0] = 0x400;
    let mut rom = alias.rom.to_vec();
    rom[0x400] = 0x0a10;
    let irom = alias.irom.to_vec();
    alias = InstructionModel::new_synthetic_serviced(rom.clone(), irom.clone()).unwrap();
    alias.advance_service(true).unwrap();
    alias.core.pc = 0x400;
    alias.core.ram[0][0] = 0x400;
    let opcode = alias.pending_service().unwrap().unwrap();
    alias.advance_service(true).unwrap();
    let operand = alias.pending_service().unwrap().unwrap();
    assert_eq!((opcode.address, operand.address), (0x400, 0x400));
    assert_eq!(
        (opcode.role, operand.role),
        (ServiceRole::Opcode, ServiceRole::ProgramOperand)
    );
    for _ in 0..3 {
        alias.advance_service(true).unwrap();
    }
    assert_eq!(alias.core.x, 0x0a10);
    assert_eq!(alias.core.ram[0][0], 0x401);

    let mut crossing = serviced();
    crossing.advance_service(true).unwrap();
    crossing.publish_main_command(0).unwrap();
    crossing.core.pc = 0x3ff;
    std::sync::Arc::make_mut(&mut crossing.ports.iram)[0x3ff] = 0x0810;
    crossing.advance_service(true).unwrap();
    let request = crossing.pending_service().unwrap().unwrap();
    assert_eq!(
        (request.role, request.resource, request.address),
        (
            ServiceRole::InstructionWord,
            ServiceResource::SharedRom,
            0x400
        )
    );

    let mut fault = serviced();
    fault.advance_service(true).unwrap();
    fault.publish_main_command(0).unwrap();
    fault.core.pc = 0x400;
    std::sync::Arc::make_mut(&mut fault.ports.iram)[0] = 0xffff;
    fault.core.pc = 0;
    fault.advance_service(true).unwrap();
    let saved = fault.clone();
    assert!(fault.advance_service(true).is_err());
    assert_eq!(fault, saved);
    restored(&fault, &mut digest);
    model.reset_synthetic();
    assert_eq!(model, serviced());
    let fingerprint = format!(
        "SVP417_SERVICE={}/{grants}",
        digest
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    #[cfg(not(target_arch = "wasm32"))]
    println!("{fingerprint}");
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::console_log!("{fingerprint}");
}

fn at_opcode(opcode: u16) -> InstructionModel {
    let mut model = serviced();
    model.advance_service(true).unwrap();
    model.publish_main_command(0).unwrap();
    model.core.pc = 0;
    std::sync::Arc::make_mut(&mut model.ports.iram)[0] = opcode;
    model
}

fn grant(model: &mut InstructionModel, role: ServiceRole, value: u16, digest: &mut Sha256) {
    restored(model, digest);
    let before = model.ports.clone();
    assert!(
        matches!(model.advance_service(true).unwrap(), ServiceProgress::Granted { request, response } if request.role == role && response == value)
    );
    assert_eq!(model.ports, before);
    restored(model, digest);
}

fn retire(model: &mut InstructionModel, digest: &mut Sha256) {
    assert!(matches!(
        model.advance_service(true).unwrap(),
        ServiceProgress::Boundary(Progress::Prepared { .. })
    ));
    restored(model, digest);
    assert!(matches!(
        model.advance_service(true).unwrap(),
        ServiceProgress::Boundary(Progress::Committed { .. })
    ));
    restored(model, digest);
}

fn program(model: &mut InstructionModel, writing: bool, mode: u16) {
    model.ports.write(6, 0, false, 0).unwrap();
    model.ports.write(6, 0, false, mode).unwrap();
    if writing {
        model.ports.write(4, 0, true, 0xffff).unwrap()
    } else {
        model.ports.read(4, 0, true).unwrap();
    }
}

fn port_paths(digest: &mut Sha256) {
    let mut inputs = at_opcode(0x0014);
    inputs.set_synthetic_input_pins(0x800).unwrap();
    grant(&mut inputs, ServiceRole::Opcode, 0x0014, digest);
    grant(&mut inputs, ServiceRole::StatusInputs, 0x800, digest);
    retire(&mut inputs, digest);
    assert_eq!(inputs.core.x, 0x800);

    let mut chain = at_opcode(0x00b8);
    chain.publish_main_command(0x1234).unwrap();
    grant(&mut chain, ServiceRole::Opcode, 0x00b8, digest);
    grant(&mut chain, ServiceRole::PortRead(0), 2, digest);
    let request = chain.pending_service().unwrap().unwrap();
    assert_eq!(
        (request.ordinal, request.role, request.value),
        (2, ServiceRole::PortWrite(3), 2)
    );
    grant(&mut chain, ServiceRole::PortWrite(3), 0, digest);
    retire(&mut chain, digest);
    assert_eq!((chain.peek_status(), chain.peek_result()), (1, 2));

    let mut read = at_opcode(0x001c);
    read.main_dram_write(0, 0x1234, 0xffff).unwrap();
    program(&mut read, false, 0x0818);
    grant(&mut read, ServiceRole::Opcode, 0x001c, digest);
    let request = read.pending_service().unwrap().unwrap();
    assert_eq!(
        (request.resource, request.address, request.mode),
        (ServiceResource::SharedDram, 0x180000, 0x0800)
    );
    grant(&mut read, ServiceRole::PortRead(4), 0x1234, digest);
    retire(&mut read, digest);
    assert_eq!(read.core.x, 0x1234);

    let mut overwrite = at_opcode(0x00c1);
    overwrite.core.x = 0xa0b0;
    overwrite.main_dram_write(0, 0x1234, 0xffff).unwrap();
    program(&mut overwrite, true, 0x0418);
    grant(&mut overwrite, ServiceRole::Opcode, 0x00c1, digest);
    grant(&mut overwrite, ServiceRole::PortWrite(4), 0xa2b4, digest);
    assert_eq!(overwrite.peek_dram(0), 0x1234);
    malformed(&overwrite);
    retire(&mut overwrite, digest);
    assert_eq!(overwrite.peek_dram(0), 0xa2b4);

    let mut failed = at_opcode(0x00d8);
    failed.publish_main_command(0x1234).unwrap();
    grant(&mut failed, ServiceRole::Opcode, 0x00d8, digest);
    grant(&mut failed, ServiceRole::PortRead(0), 2, digest);
    let before = failed.clone();
    assert!(failed.advance_service(true).is_err());
    assert_eq!(failed, before);
    restored(&failed, digest);
    assert_eq!((failed.peek_status(), failed.peek_result()), (2, 0x1234));
}
