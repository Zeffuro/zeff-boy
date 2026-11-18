use super::Emulator;
use zeff_emu_common::save_state::StateReader;
use zeff_m68k::timing::PrefetchCpu;

pub(in super::super) fn restore_pending_trace(emu: &mut Emulator) {
    let mut bytes = emu.encode_state().unwrap();
    let (start, end, pending) = {
        let mut state = StateReader::new(&bytes);
        let mut magic = [0; 8];
        state.read_exact(&mut magic).unwrap();
        assert_eq!(&magic, b"ZMDRAW01");
        assert_eq!(state.read_u32().unwrap(), 1);
        state.read_exact(&mut [0; 32]).unwrap();
        state.read_u32().unwrap();
        state.read_u8().unwrap();
        state.read_bool().unwrap();
        state.read_u8().unwrap();
        state.read_u8().unwrap();
        let section = state.read_slice(128).unwrap();
        let end = state.position();
        let start = end - section.len();
        let mut cpu = StateReader::new(section);
        assert_eq!(cpu.read_u8().unwrap(), 1);
        let fields = cpu.read_slice(95).unwrap();
        assert!(cpu.is_exhausted());
        let fields_start = section.len() - fields.len();
        let mut fields = StateReader::new(fields);
        for _ in 0..18 {
            fields.read_u32().unwrap();
        }
        fields.read_u16().unwrap();
        fields.read_u64().unwrap();
        if fields.read_bool().unwrap() {
            fields.read_u32().unwrap();
            fields.read_u16().unwrap();
            fields.read_u16().unwrap();
        }
        assert!(!fields.read_bool().unwrap());
        let pending = start + fields_start + fields.position();
        assert!(!fields.read_bool().unwrap());
        fields.read_u8().unwrap();
        fields.read_bool().unwrap();
        assert!(fields.is_exhausted());
        (start, end, pending)
    };
    bytes[pending] = 1;
    let mut cpu = PrefetchCpu::default();
    cpu.read_boundary_state(&mut StateReader::new(&bytes[start..end]))
        .unwrap();
    assert!(cpu.trace_pending());
    emu.decode_state(&bytes).unwrap();
    assert_eq!(emu.encode_state().unwrap(), bytes);
}
