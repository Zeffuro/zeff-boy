use super::*;

fn read(bytes: &[u8], addr: u16) -> u8 {
    bytes.get(addr as usize).copied().unwrap_or(0)
}

#[test]
fn decodes_common_sms_io_setup_sequence() {
    let bytes = [0x3E, 0x90, 0xD3, 0x7F, 0x76];

    let ld = decode_instruction(&|addr| read(&bytes, addr), 0);
    let out = decode_instruction(&|addr| read(&bytes, addr), 2);
    let halt = decode_instruction(&|addr| read(&bytes, addr), 4);

    assert_eq!(ld.bytes.as_slice(), &[0x3E, 0x90]);
    assert_eq!(ld.mnemonic.as_str(), "LD A,$90");
    assert_eq!(out.bytes.as_slice(), &[0xD3, 0x7F]);
    assert_eq!(out.mnemonic.as_str(), "OUT ($7F),A");
    assert_eq!(halt.mnemonic.as_str(), "HALT");
}

#[test]
fn decodes_indexed_displacement_and_cb_forms() {
    let indexed_ld = [0xDD, 0x36, 0xFE, 0x5A];
    let indexed_cb = [0xFD, 0xCB, 0x02, 0x46];

    let ld = decode_instruction(&|addr| read(&indexed_ld, addr), 0);
    let bit = decode_instruction(&|addr| read(&indexed_cb, addr), 0);

    assert_eq!(instruction_len(&|addr| read(&indexed_ld, addr), 0), 4);
    assert_eq!(ld.bytes.as_slice(), &[0xDD, 0x36, 0xFE, 0x5A]);
    assert_eq!(ld.mnemonic.as_str(), "LD (IX-$02),$5A");
    assert_eq!(instruction_len(&|addr| read(&indexed_cb, addr), 0), 4);
    assert_eq!(bit.mnemonic.as_str(), "BIT 0,(IY+$02)");
}

#[test]
fn decodes_ed_memory_and_block_forms() {
    let ld = [0xED, 0x43, 0x34, 0x12];
    let ldir = [0xED, 0xB0];

    let ld_line = decode_instruction(&|addr| read(&ld, addr), 0);
    let ldir_line = decode_instruction(&|addr| read(&ldir, addr), 0);

    assert_eq!(instruction_len(&|addr| read(&ld, addr), 0), 4);
    assert_eq!(ld_line.mnemonic.as_str(), "LD ($1234),BC");
    assert_eq!(instruction_len(&|addr| read(&ldir, addr), 0), 2);
    assert_eq!(ldir_line.mnemonic.as_str(), "LDIR");
}
