use super::*;
use crate::svp::{ServiceProgress, ServiceRole};
use sha2::{Digest, Sha256};

fn fresh_serviced() -> Emulator {
    let (rom, irom) = images();
    Emulator::new_synthetic_svp_serviced(
        rom,
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound: false,
        },
        irom,
    )
    .unwrap()
}

pub(super) fn contract() {
    let mut machine = fresh_serviced();
    machine.advance_synthetic_svp_service(true).unwrap();
    machine.step_instruction().unwrap();
    machine.advance_synthetic_svp_service(true).unwrap();
    let request = machine
        .synthetic_svp()
        .unwrap()
        .pending_service()
        .unwrap()
        .unwrap();
    assert_eq!(request.role, ServiceRole::InstructionWord);
    assert_eq!(
        machine.advance_synthetic_svp_service(false),
        Ok(ServiceProgress::Waiting)
    );
    machine.step_instruction().unwrap();
    assert_eq!(
        machine.synthetic_svp().unwrap().pending_service().unwrap(),
        Some(request)
    );
    assert_eq!(&machine.bus.ram[0x200..0x202], &[0x77, 0x88]);
    let bytes = machine.encode_state().unwrap();
    assert_eq!(&bytes[8..12], &1_u32.to_le_bytes());
    let mut copy = fresh_serviced();
    copy.decode_state(&bytes).unwrap();
    assert!(fresh().decode_state(&bytes).is_err());
    assert!(
        fresh_serviced()
            .decode_state(&fresh().encode_state().unwrap())
            .is_err()
    );
    let mut digest = Sha256::new();
    for _ in 0..32 {
        assert_eq!(
            machine.advance_synthetic_svp_service(true),
            copy.advance_synthetic_svp_service(true)
        );
        let bytes = machine.encode_state().unwrap();
        assert_eq!(bytes, copy.encode_state().unwrap());
        assert_eq!(&machine.bus.ram[0x200..0x202], &[0x77, 0x88]);
        digest.update(&bytes);
    }
    machine.reset();
    assert_eq!(
        machine
            .synthetic_svp()
            .unwrap()
            .pending_service()
            .unwrap()
            .unwrap()
            .role,
        ServiceRole::ResetVector
    );
    let fingerprint = format!(
        "SVP417_MD={}/32",
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
