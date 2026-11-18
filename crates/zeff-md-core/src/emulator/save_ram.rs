use super::Emulator;
use anyhow::{Result, bail};
use zeff_emu_common::save_ram::SaveRamKind;

impl Emulator {
    pub fn save_ram_kind(&self) -> SaveRamKind {
        if let Some(save) = &self.bus.cartridge.lock_on_save {
            return SaveRamKind::known_nonvolatile(save.bytes().len());
        }
        if let Some(board) = &self.bus.cartridge.sf001 {
            return SaveRamKind::known_nonvolatile(board.save_bytes().len());
        }
        if self.bus.cartridge.sf004.is_some() {
            return SaveRamKind::known_nonvolatile(crate::cartridge::sf004::SAVE_BYTES);
        }
        if let Some(save) = &self.bus.cartridge.save {
            if save.reserved_switched() {
                SaveRamKind::none()
            } else {
                SaveRamKind::known_nonvolatile(save.bytes.len())
            }
        } else {
            SaveRamKind::none()
        }
    }

    pub fn dump_save_ram(&self) -> Option<Vec<u8>> {
        if let Some(save) = &self.bus.cartridge.lock_on_save {
            return Some(save.bytes().to_vec());
        }
        if let Some(board) = &self.bus.cartridge.sf001 {
            return Some(board.save_bytes().to_vec());
        }
        if let Some(board) = &self.bus.cartridge.sf004 {
            return board.save_bytes().map(<[u8]>::to_vec);
        }
        self.bus
            .cartridge
            .save
            .as_ref()
            .filter(|save| !save.reserved_switched())
            .map(|save| save.bytes.to_vec())
    }

    pub fn load_save_ram(&mut self, bytes: &[u8]) -> Result<()> {
        if let Some(save) = &mut self.bus.cartridge.lock_on_save {
            save.load(bytes)?;
        } else if let Some(board) = &mut self.bus.cartridge.sf001 {
            board.load_save(bytes)?;
        } else if let Some(board) = &mut self.bus.cartridge.sf004 {
            board.load_save(bytes)?;
        } else if let Some(save) = &mut self.bus.cartridge.save {
            save.load(bytes)?;
        } else {
            bail!("MD cartridge has no supported save RAM");
        }
        self.invalidate_audio_trace(
            zeff_emu_common::audio_trace::AudioTraceInvalidation::ExternalMutation,
        );
        Ok(())
    }
}

#[cfg(test)]
impl Emulator {
    pub(crate) fn expansion_save_contract() {
        use zeff_emu_common::save_state::StateReader;
        use zeff_emu_common::time::Reset;
        use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, FunctionCode};
        use zeff_m68k::timing::ClockedBus;
        let rom = crate::cartridge::ram::expansion_save_fixture();
        for concurrent_sound in [false, true] {
            let config = crate::LoadConfig {
                concurrent_sound,
                ..Default::default()
            };
            let mut source = Self::new(rom.clone(), config).unwrap();
            assert_eq!(
                source.save_ram_kind(),
                SaveRamKind::known_nonvolatile(65536)
            );
            assert_eq!(source.dump_save_ram(), Some(vec![0xff; 65536]));
            for size in [32768, 65537] {
                let before = source.clone();
                assert!(source.load_save_ram(&vec![0; size]).is_err());
                assert!(
                    source == before,
                    "save import rejection mutated the emulator"
                );
            }
            source.load_save_ram(&vec![0x47; 65536]).unwrap();
            for _ in 0..3 {
                source.step_instruction().unwrap();
            }
            for (address, value) in [
                (0x400000, 0x12),
                (0x400001, 0x34),
                (0x400004, 0x56),
                (0x400005, 0x78),
            ] {
                assert_eq!(source.bus.peek(address), Some(value));
                assert_eq!(source.bus.cartridge.rom_source_offset(address), None);
            }
            assert_eq!(source.bus.peek(0x410000), None);
            let access = |address, lanes| BusAccess {
                logical_address: address,
                bus_address: address & !1,
                kind: AccessKind::Data,
                function_code: FunctionCode::SupervisorData,
                lanes,
            };
            let mut bus = source.bus.clone();
            for lanes in [ByteLanes::Upper, ByteLanes::Lower, ByteLanes::Both] {
                let address = 0x400004 + u32::from(lanes == ByteLanes::Lower);
                let result = bus.read_cycle(access(address, lanes), bus.cpu_clock);
                assert_eq!(result.result, Ok(0x5678));
                assert_eq!(result.wait_clocks, 0);
            }
            for (address, lanes, value) in [
                (0x40fffe, ByteLanes::Upper, 0xab00),
                (0x40ffff, ByteLanes::Lower, 0x00cd),
            ] {
                assert_eq!(
                    bus.write_cycle(access(address, lanes), value, bus.cpu_clock)
                        .result,
                    Ok(())
                );
            }
            assert_eq!(bus.peek(0x40fffe), Some(0xab));
            assert_eq!(bus.peek(0x40ffff), Some(0xcd));
            let raw = source.encode_state().unwrap();
            let front = source.encode_frontend_state().unwrap();
            let mut cloned = source.clone();
            let mut restored = Self::new(rom.clone(), config).unwrap();
            restored.decode_state(&raw).unwrap();
            let mut frontend = Self::new(rom.clone(), config).unwrap();
            frontend.decode_frontend_state(&front).unwrap();
            assert!(
                restored.encode_state().unwrap() == raw,
                "raw save mapping roundtrip differs"
            );
            assert!(
                frontend.encode_frontend_state().unwrap() == front,
                "frontend save mapping roundtrip differs"
            );
            let mut reader = StateReader::new(&raw);
            reader.set_position(crate::save_state::STATE_MAGIC.len() + 4 + 32 + 4 + 1 + 1 + 2);
            for _ in 0..3 {
                reader
                    .read_slice(crate::save_state::MAX_STATE_BYTES)
                    .unwrap();
            }
            reader.read_bool().unwrap();
            let header = reader.position();
            let length = reader.read_u32().unwrap() as usize;
            let payload = reader.position();
            assert_eq!(raw[payload], 1);
            let mut old = raw[..header].to_vec();
            old.extend_from_slice(&1u32.to_le_bytes());
            old.push(0);
            old.extend_from_slice(&raw[payload + length..]);
            let before = restored.clone();
            assert!(restored.decode_state(&old).is_err());
            assert!(
                restored == before,
                "old save owner rejection mutated the emulator"
            );
            for machine in [&mut source, &mut cloned, &mut restored, &mut frontend] {
                for _ in 0..3 {
                    machine.step_instruction().unwrap();
                }
                assert_eq!(machine.cpu.registers.d[0] & 0xffff, 0x78);
            }
            let endpoint = source.encode_state().unwrap();
            for machine in [&cloned, &restored, &frontend] {
                assert!(
                    machine.encode_state().unwrap() == endpoint,
                    "save mapping future differs"
                );
            }
            let backing = source.dump_save_ram().unwrap();
            source.reset();
            assert_eq!(source.dump_save_ram(), Some(backing));
        }
    }

    pub(crate) fn reserved_switched_api_contract() {
        let mut rom = vec![0; 0x400000];
        rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
        let mut machine = Self::new(rom, super::LoadConfig::default()).unwrap();
        machine.bus.cartridge.save =
            Some(crate::cartridge::SaveMemory::reserved_switched_for_test());
        assert_eq!(machine.save_ram_kind(), SaveRamKind::none());
        assert!(!machine.save_ram_kind().has_ram());
        assert!(!machine.save_ram_kind().is_persistent());
        assert_eq!(machine.dump_save_ram(), None);
        for size in [0, 1, 8192, 32768] {
            let before = machine.clone();
            assert!(machine.load_save_ram(&vec![0; size]).is_err());
            assert_eq!(machine, before);
        }
    }
}

#[cfg(test)]
impl Emulator {
    pub(crate) fn bounded_bytes_api_contract() {
        let mut rom = vec![0; 0x340000];
        rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
        let mut machine = Self::new(rom, super::LoadConfig::default()).unwrap();
        machine.bus.cartridge.save = Some(crate::cartridge::SaveMemory::bounded_bytes_for_test());
        assert_eq!(
            machine.save_ram_kind(),
            SaveRamKind::known_nonvolatile(2048)
        );
        assert!(machine.save_ram_kind().has_ram());
        assert!(machine.save_ram_kind().is_persistent());
        assert_eq!(machine.dump_save_ram(), Some(vec![0xff; 2048]));
        for size in [0, 2047, 2049, 4097, 8192] {
            let before = machine.clone();
            assert!(machine.load_save_ram(&vec![0; size]).is_err());
            assert_eq!(machine, before);
        }
        let mut imported = vec![0x47; 2048];
        imported[..2].copy_from_slice(&[0xfb, 0x5a]);
        machine.load_save_ram(&imported).unwrap();
        assert_eq!(machine.dump_save_ram(), Some(imported));
    }
}
