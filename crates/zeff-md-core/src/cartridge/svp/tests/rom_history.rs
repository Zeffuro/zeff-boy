use super::*;
use crate::cartridge::Cartridge;
use crate::svp::{CurrentRom, ServiceProgress, ServiceResource, ServiceRole};
use zeff_emu_common::cheats::{CheatPatch, CheatValue};

fn patch(address: u32, value: u8) -> CheatPatch {
    CheatPatch::WideRomWrite {
        address,
        value: CheatValue::Constant(value),
    }
}

fn fixture() -> (InstructionModel, Cartridge) {
    let mut bytes = vec![0; 0x20000];
    bytes[..8].copy_from_slice(&[0, 0xff, 0x10, 0, 0, 0, 2, 0]);
    bytes[0x200..0x204].copy_from_slice(&[0x60, 0xfe, 0x4e, 0x71]);
    let words = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
        .collect();
    let mut irom = vec![0; 1024];
    irom[0x3fc] = 0x400;
    let mut model = InstructionModel::new_synthetic_serviced(words, irom).unwrap();
    model.advance_service(true).unwrap();
    (model, Cartridge::new(bytes).unwrap())
}

fn grant(model: &mut InstructionModel, cart: &Cartridge, role: ServiceRole, response: u16) {
    let pending = model.pending_service().unwrap().unwrap();
    assert_eq!(pending.role, role);
    assert_eq!(
        model
            .advance_service_current(true, Some(CurrentRom::new(&cart.bytes, &cart.rom_overlay)))
            .unwrap(),
        ServiceProgress::Granted {
            request: pending,
            response
        }
    );
}

fn roundtrip(model: &InstructionModel) {
    let bytes = model.encode_state().unwrap();
    assert!(bytes.starts_with(b"ZSINST02"));
    let mut copy = fixture().0;
    copy.decode_state(&bytes).unwrap();
    assert_eq!(copy, *model);
    assert_eq!(copy.encode_state().unwrap(), bytes);
    let mut expected = model.clone();
    assert_eq!(copy.advance_service(true), expected.advance_service(true));
    assert_eq!(copy, expected);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn serviced_rom_same_offset_interleaving_retains_execute_and_commit_inputs() {
    let (mut model, mut cart) = fixture();
    model.core.ram[0][0] = 0x400;
    cart.rom_overlay = cart
        .prepare_rom_overlay(&[patch(0x800, 0x0a), patch(0x801, 0x10)])
        .unwrap();
    grant(&mut model, &cart, ServiceRole::Opcode, 0x0a10);
    roundtrip(&model);
    let mut other_branch = model.clone();
    cart.rom_overlay = cart
        .prepare_rom_overlay(&[patch(0x800, 0x22), patch(0x801, 0x22)])
        .unwrap();
    grant(&mut model, &cart, ServiceRole::ProgramOperand, 0x2222);
    assert_eq!(model.rom_history.len(), 2);
    assert_eq!(
        (model.rom_history[0].ordinal, model.rom_history[1].ordinal),
        (0, 1)
    );
    assert_eq!(model.rom_history[0].address, model.rom_history[1].address);
    cart.rom_overlay = cart.prepare_rom_overlay(&[]).unwrap();
    roundtrip(&model);
    assert!(matches!(
        model
            .advance_service_current(true, Some(CurrentRom::new(&cart.bytes, &cart.rom_overlay)))
            .unwrap(),
        ServiceProgress::Boundary(Progress::Prepared { .. })
    ));
    assert_eq!(model.services.as_ref().unwrap().len(), 2);
    roundtrip(&model);
    model.advance_service(true).unwrap();
    assert_eq!(model.core.x, 0x2222);
    assert_eq!(model.core.ram[0][0], 0x401);
    assert!(model.rom_history.is_empty() && model.services.as_ref().unwrap().is_empty());
    grant(&mut other_branch, &cart, ServiceRole::ProgramOperand, 0);
    assert_eq!(other_branch.rom_history.len(), 1);
    assert_eq!(model.core.x, 0x2222);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn serviced_shared_rom_port_replay_preserves_channel_and_fault_prefix() {
    let (mut model, mut cart) = fixture();
    model.core.st = 0x20;
    model.ports.write(6, 0, false, 0x500).unwrap();
    model.ports.write(6, 0, false, 0x0800).unwrap();
    model.ports.read(4, 0, true).unwrap();
    cart.rom_overlay = cart.prepare_rom_overlay(&[patch(0x801, 0x1c)]).unwrap();
    grant(&mut model, &cart, ServiceRole::Opcode, 0x001c);
    let request = model.pending_service().unwrap().unwrap();
    assert_eq!(
        (request.resource, request.address, request.ordinal),
        (ServiceResource::SharedRom, 0x500, 1)
    );
    cart.rom_overlay = cart
        .prepare_rom_overlay(&[patch(0xa00, 0xab), patch(0xa01, 0xcd)])
        .unwrap();
    grant(&mut model, &cart, ServiceRole::PortRead(4), 0xabcd);
    cart.rom_overlay = cart.prepare_rom_overlay(&[]).unwrap();
    roundtrip(&model);
    model.advance_service(true).unwrap();
    roundtrip(&model);
    model.advance_service(true).unwrap();
    assert_eq!(model.core.x, 0xabcd);
    model.ports.read(4, 0x20, false).unwrap();
    assert_eq!(
        model.ports.service_target(4, 0x20, false, false).unwrap().1,
        0x502
    );

    let (mut fault, mut cart) = fixture();
    cart.rom_overlay = cart
        .prepare_rom_overlay(&[patch(0x800, 0xff), patch(0x801, 0xff)])
        .unwrap();
    grant(&mut fault, &cart, ServiceRole::Opcode, 0xffff);
    let before = fault.clone();
    assert!(fault.advance_service(true).is_err());
    assert_eq!(fault, before);
    roundtrip(&fault);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn historical_component_rejects_malformed_owner_lane_order_and_legacy_fallback() {
    let (mut model, mut cart) = fixture();
    model.core.ram[0][0] = 0x400;
    cart.rom_overlay = cart
        .prepare_rom_overlay(&[patch(0x800, 0x0a), patch(0x801, 0x10)])
        .unwrap();
    grant(&mut model, &cart, ServiceRole::Opcode, 0x0a10);
    grant(&mut model, &cart, ServiceRole::ProgramOperand, 0x0a10);
    let saved = model.encode_state().unwrap();
    let at = saved
        .windows(8)
        .position(|part| part == b"ZSVPROM2")
        .unwrap();
    let captures = saved
        .windows(8)
        .position(|part| part == b"ZSVPSVC1")
        .unwrap();
    let mut offsets = vec![0, 7, at, at + 8, at + 9, at + 10, at + 14, at + 15, at + 16];
    for record in 0..2 {
        offsets.extend(
            [0, 8, 10, 11, 12, 13, 14, 18, 20, 22, 23, 25]
                .map(|offset| captures + 9 + 27 * record + offset),
        );
    }
    for offset in offsets {
        let mut bad = saved.clone();
        bad[offset] ^= 0xff;
        let mut target = fixture().0;
        let before = target.clone();
        assert!(target.decode_state(&bad).is_err(), "offset {offset}");
        assert_eq!(target, before);
    }
    let mut legacy = saved.clone();
    legacy[..8].copy_from_slice(b"ZSINST01");
    legacy.truncate(at);
    let mut target = fixture().0;
    let before = target.clone();
    assert!(target.decode_state(&legacy).is_err());
    assert_eq!(target, before);
    let mut trailing = saved.clone();
    trailing.push(0);
    assert!(target.decode_state(&trailing).is_err());
    for end in [7, at, saved.len() - 1] {
        assert!(target.decode_state(&saved[..end]).is_err());
    }
    assert_eq!(target, before);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn rejected_shared_rom_port_grant_keeps_only_the_accepted_semantic_prefix() {
    let (mut model, mut cart) = fixture();
    model.core.st = 0x40;
    model.ports.write(6, 0, false, 0x500).unwrap();
    model.ports.write(6, 0, false, 0x0800).unwrap();
    model.ports.read(4, 0, true).unwrap();
    cart.rom_overlay = cart.prepare_rom_overlay(&[patch(0x801, 0x1c)]).unwrap();
    grant(&mut model, &cart, ServiceRole::Opcode, 0x001c);
    let request = model.pending_service().unwrap().unwrap();
    assert_eq!(
        (
            request.role,
            request.resource,
            request.address,
            request.status,
            request.ordinal
        ),
        (
            ServiceRole::PortRead(4),
            ServiceResource::SharedRom,
            0x500,
            0x40,
            1
        )
    );
    assert_eq!(model.rom_history.len(), 1);
    assert_eq!(model.services.as_ref().unwrap().len(), 1);
    let before = model.clone();
    let saved = model.encode_state().unwrap();
    cart.rom_overlay = cart
        .prepare_rom_overlay(&[patch(0xa00, 0xab), patch(0xa01, 0xcd)])
        .unwrap();
    for masked in [true, false] {
        if !masked {
            cart.rom_overlay = cart.prepare_rom_overlay(&[]).unwrap();
        }
        let error = model
            .advance_service_current(true, Some(CurrentRom::new(&cart.bytes, &cart.rom_overlay)))
            .unwrap_err();
        assert!(
            matches!(error, Error::Cpu(ref text) if text.contains("unqualified external port mode"))
        );
        assert_eq!(model, before);
        assert_eq!(model.encode_state().unwrap(), saved);
        assert_eq!(model.pending_service().unwrap(), Some(request));
        roundtrip(&model);
    }
}
