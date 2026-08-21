use super::*;
use crate::cheats::CheatPatch;
use crate::cheats::CheatValue;
use crate::hardware::ppu::Lcdc;
use crate::hardware::rom_header::RomHeader;

fn make_test_bus() -> Bus {
    let mut rom = vec![0u8; 0x8000];
    for (i, byte) in rom.iter_mut().take(0x100).enumerate() {
        *byte = i as u8;
    }
    let header = RomHeader::from_rom(&rom).expect("test ROM header should parse");
    Bus::new(rom, &header, HardwareMode::DMG).expect("test bus should initialize")
}

fn make_cgb_test_bus() -> Bus {
    let mut rom = vec![0u8; 0x8000];
    for (i, byte) in rom.iter_mut().take(0x100).enumerate() {
        *byte = i as u8;
    }
    rom[0x143] = 0x80; // CGB compatible flag
    let header = RomHeader::from_rom(&rom).expect("test ROM header should parse");
    Bus::new(rom, &header, HardwareMode::CGBNormal).expect("test bus should initialize")
}

fn make_cgb_compat_test_bus() -> Bus {
    let mut rom = vec![0u8; 0x8000];
    for (i, byte) in rom.iter_mut().take(0x100).enumerate() {
        *byte = i as u8;
    }
    let header = RomHeader::from_rom(&rom).expect("test ROM header should parse");
    Bus::new(rom, &header, HardwareMode::CGBNormal).expect("test bus should initialize")
}

fn printer_packet(command: u8, payload: &[u8]) -> Vec<u8> {
    let len = u16::try_from(payload.len()).unwrap();
    let mut bytes = vec![0x88, 0x33, command, 0, len as u8, (len >> 8) as u8];
    bytes.extend_from_slice(payload);
    let checksum = bytes[2..]
        .iter()
        .fold(0u16, |sum, byte| sum.wrapping_add(u16::from(*byte)));
    bytes.extend_from_slice(&checksum.to_le_bytes());
    bytes.extend_from_slice(&[0, 0]);
    bytes
}

fn exchange_printer_packet(bus: &mut Bus, command: u8, payload: &[u8]) -> Vec<u8> {
    printer_packet(command, payload)
        .into_iter()
        .map(|byte| {
            bus.write_byte(SERIAL_SB, byte);
            bus.write_byte(SERIAL_SC, 0x81);
            bus.step_serial(4096);
            bus.read_byte(SERIAL_SB)
        })
        .collect()
}

#[test]
fn disconnected_link_preview_does_not_mutate_either_side() {
    let left = make_test_bus();
    let right = make_test_bus();
    let left_before = left.game_boy_link_replay_state();
    let right_before = right.game_boy_link_replay_state();

    assert_eq!(
        left.preview_game_boy_link_peer(&right),
        GameBoyLinkExchangePreview {
            local_action: None,
            peer_action: None,
            local_reply: left.game_boy_link_reply_to_master_start(),
            peer_reply: right.game_boy_link_reply_to_master_start(),
        }
    );
    assert_eq!(left.game_boy_link_replay_state(), left_before);
    assert_eq!(right.game_boy_link_replay_state(), right_before);
}

#[test]
fn disconnected_serial_transfer_completes_with_ff_and_interrupt() {
    let mut bus = make_test_bus();
    assert_eq!(
        bus.game_boy_serial_device(),
        crate::hardware::GameBoySerialDevice::Disconnected
    );

    bus.write_byte(SERIAL_SB, 0x42);
    bus.write_byte(SERIAL_SC, 0x81);
    bus.step_serial(4096);

    assert_eq!(bus.read_byte(SERIAL_SB), 0xFF);
    assert_eq!(bus.read_byte(SERIAL_SC) & 0x80, 0);
    assert_ne!(bus.if_reg & 0x08, 0);
}

#[test]
fn selected_printer_receives_real_serial_register_transcript() {
    let mut bus = make_test_bus();
    bus.set_game_boy_serial_device(crate::hardware::GameBoySerialDevice::Printer);
    let transcript = [0x88, 0x33, 0x01, 0, 0, 0, 0x01, 0, 0, 0];
    let replies: Vec<u8> = transcript
        .into_iter()
        .map(|byte| {
            bus.write_byte(SERIAL_SB, byte);
            bus.write_byte(SERIAL_SC, 0x81);
            bus.step_serial(4096);
            bus.read_byte(SERIAL_SB)
        })
        .collect();

    assert_eq!(&replies[replies.len() - 2..], &[0x81, 0]);
    assert_ne!(bus.if_reg & 0x08, 0);
}

#[test]
fn selected_bardigun_reader_receives_real_serial_register_transcript() {
    let mut bus = make_test_bus();
    bus.set_game_boy_serial_device(crate::hardware::GameBoySerialDevice::BardigunBarcodeReader);
    bus.queue_bardigun_barcode_scan(vec![0x81, 0x24, 0xA5])
        .unwrap();

    let replies: Vec<u8> = [0xFF; 5]
        .into_iter()
        .map(|byte| {
            bus.write_byte(SERIAL_SB, byte);
            bus.write_byte(SERIAL_SC, 0x81);
            bus.step_serial(4096);
            bus.read_byte(SERIAL_SB)
        })
        .collect();

    assert_eq!(replies, [0x81, 0x24, 0xA5, 0, 0]);
    assert_ne!(bus.if_reg & 0x08, 0);
}

#[test]
fn bardigun_bus_api_rejects_scans_while_another_device_is_selected() {
    let mut bus = make_test_bus();
    assert!(bus.queue_bardigun_barcode_scan(vec![0x81]).is_err());

    bus.set_game_boy_serial_device(crate::hardware::GameBoySerialDevice::Printer);
    assert!(bus.queue_bardigun_barcode_scan(vec![0x81]).is_err());

    bus.set_game_boy_serial_device(crate::hardware::GameBoySerialDevice::BardigunBarcodeReader);
    assert!(bus.queue_bardigun_barcode_scan(vec![0x81]).is_ok());
}

#[test]
fn camera_sized_print_has_documented_status_sequence_on_serial_registers() {
    let mut bus = make_test_bus();
    bus.set_game_boy_serial_device(crate::hardware::GameBoySerialDevice::Printer);

    let init = exchange_printer_packet(&mut bus, 0x01, &[]);
    assert_eq!(&init[init.len() - 2..], &[0x81, 0x00]);

    let band = [0; 0x280];
    for index in 0..9 {
        let data = exchange_printer_packet(&mut bus, 0x04, &band);
        assert_eq!(data[data.len() - 2], 0x81);
        assert_eq!(data[data.len() - 1], if index == 0 { 0x00 } else { 0x08 });
    }
    let data_end = exchange_printer_packet(&mut bus, 0x04, &[]);
    assert_eq!(&data_end[data_end.len() - 2..], &[0x81, 0x08]);

    let print = exchange_printer_packet(&mut bus, 0x02, &[1, 0, 0xE4, 0x40]);
    assert_eq!(&print[print.len() - 2..], &[0x81, 0x08]);
    assert_eq!(bus.printer_image_count(), 1);

    let printing = exchange_printer_packet(&mut bus, 0x0F, &[]);
    assert_eq!(&printing[printing.len() - 2..], &[0x81, 0x06]);
    bus.step_serial(100_000_000);
    let done = exchange_printer_packet(&mut bus, 0x0F, &[]);
    assert_eq!(&done[done.len() - 2..], &[0x81, 0x04]);
}

#[test]
fn direct_link_takes_precedence_over_selected_printers() {
    let mut master = make_test_bus();
    let mut passive = make_test_bus();
    master.set_game_boy_serial_device(crate::hardware::GameBoySerialDevice::Printer);
    passive.set_game_boy_serial_device(crate::hardware::GameBoySerialDevice::Printer);

    master.write_byte(SERIAL_SB, 0xAB);
    passive.write_byte(SERIAL_SB, 0x34);
    master.write_byte(SERIAL_SC, 0x81);
    passive.write_byte(SERIAL_SC, 0x80);

    master.try_sync_game_boy_link_peer(&mut passive).unwrap();
    master.step_serial(4096);
    passive.step_serial(4096);

    assert_eq!(master.read_byte(SERIAL_SB), 0x34);
    assert_eq!(passive.read_byte(SERIAL_SB), 0xAB);
    assert_eq!(master.read_byte(SERIAL_SC) & 0x80, 0);
    assert_eq!(passive.read_byte(SERIAL_SC) & 0x80, 0);
    assert_eq!(master.printer_image_count(), 0);
    assert_eq!(passive.printer_image_count(), 0);
}

#[test]
fn direct_link_takes_precedence_over_selected_bardigun_readers() {
    let mut master = make_test_bus();
    let mut passive = make_test_bus();
    master.set_game_boy_serial_device(crate::hardware::GameBoySerialDevice::BardigunBarcodeReader);
    passive.set_game_boy_serial_device(crate::hardware::GameBoySerialDevice::BardigunBarcodeReader);
    master.queue_bardigun_barcode_scan(vec![0x11]).unwrap();
    passive.queue_bardigun_barcode_scan(vec![0x22]).unwrap();

    master.write_byte(SERIAL_SB, 0xAB);
    passive.write_byte(SERIAL_SB, 0x34);
    master.write_byte(SERIAL_SC, 0x81);
    passive.write_byte(SERIAL_SC, 0x80);

    master.try_sync_game_boy_link_peer(&mut passive).unwrap();
    master.step_serial(4096);
    passive.step_serial(4096);

    assert_eq!(master.read_byte(SERIAL_SB), 0x34);
    assert_eq!(passive.read_byte(SERIAL_SB), 0xAB);

    master.set_game_boy_link_peer_present(false);
    master.write_byte(SERIAL_SB, 0xFF);
    master.write_byte(SERIAL_SC, 0x81);
    master.step_serial(4096);
    assert_eq!(master.read_byte(SERIAL_SB), 0x11);
}

#[test]
fn disconnected_preview_exposes_prospective_single_master_action() {
    let mut master = make_test_bus();
    let passive = make_test_bus();
    master.write_byte(SERIAL_SB, 0xAB);
    master.write_byte(SERIAL_SC, 0x81);
    let master_before = master.game_boy_link_replay_state();
    let passive_before = passive.game_boy_link_replay_state();

    let preview = master.preview_game_boy_link_peer(&passive);
    assert_eq!(
        preview.local_action,
        Some(GameBoyLinkAction {
            out_byte: 0xAB,
            clock_period_t_cycles: 4096,
            serial_generation: master_before.serial_generation,
        })
    );
    assert_eq!(preview.peer_action, None);
    assert_eq!(master.game_boy_link_replay_state(), master_before);
    assert_eq!(passive.game_boy_link_replay_state(), passive_before);
}

#[test]
fn preview_snapshots_crossed_master_inputs_committed_by_exchange() {
    let mut left = make_test_bus();
    let mut right = make_test_bus();
    left.write_byte(SERIAL_SB, 0xAB);
    right.write_byte(SERIAL_SB, 0x34);
    left.write_byte(SERIAL_SC, 0x81);
    right.write_byte(SERIAL_SC, 0x81);

    let preview = left.preview_game_boy_link_peer(&right);
    assert!(preview.local_action.is_some());
    assert!(preview.peer_action.is_some());
    assert_eq!(preview.local_reply.out_byte, 0xAB);
    assert_eq!(preview.peer_reply.out_byte, 0x34);
    assert!(matches!(
        left.try_sync_game_boy_link_peer(&mut right),
        Ok(GameBoyLinkExchangeOutcome::Exchanged {
            local_action: Some(_),
            peer_action: Some(_),
        })
    ));
    let left_state = left.game_boy_link_replay_state();
    let right_state = right.game_boy_link_replay_state();
    assert_eq!(
        left_state.pending_master_response,
        Some(preview.peer_reply.out_byte)
    );
    assert_eq!(
        right_state.pending_master_response,
        Some(preview.local_reply.out_byte)
    );
}

#[test]
fn link_exchange_accepts_simultaneous_master_replies_before_completion() {
    let mut left = make_test_bus();
    let mut right = make_test_bus();
    assert_eq!(
        left.try_sync_game_boy_link_peer(&mut right),
        Ok(GameBoyLinkExchangeOutcome::Idle)
    );

    left.write_byte(SERIAL_SB, 0xAB);
    right.write_byte(SERIAL_SB, 0x34);
    left.write_byte(SERIAL_SC, 0x81);
    right.write_byte(SERIAL_SC, 0x81);

    let pending = GameBoyLinkTransferExchange {
        reply: GameBoyLinkReplyDisposition::AcceptedPending,
        passive_responder_scheduled: false,
    };
    assert_eq!(
        left.try_sync_game_boy_link_peer(&mut right),
        Ok(GameBoyLinkExchangeOutcome::Exchanged {
            local_action: Some(pending),
            peer_action: Some(pending),
        })
    );
    assert_eq!(left.read_byte(SERIAL_SB), 0xAB);
    assert_eq!(right.read_byte(SERIAL_SB), 0x34);

    left.step_serial(4096);
    right.step_serial(4096);
    assert_eq!(left.read_byte(SERIAL_SB), 0x34);
    assert_eq!(right.read_byte(SERIAL_SB), 0xAB);
    assert_ne!(left.if_reg & 0x08, 0);
    assert_ne!(right.if_reg & 0x08, 0);
}

#[test]
fn link_exchange_snapshots_both_replies_for_ready_masters() {
    let mut left = make_test_bus();
    let mut right = make_test_bus();
    left.try_sync_game_boy_link_peer(&mut right).unwrap();

    left.write_byte(SERIAL_SB, 0xAB);
    right.write_byte(SERIAL_SB, 0x34);
    left.write_byte(SERIAL_SC, 0x81);
    right.write_byte(SERIAL_SC, 0x81);
    left.step_serial(4096);
    right.step_serial(4096);

    let completed = GameBoyLinkTransferExchange {
        reply: GameBoyLinkReplyDisposition::Completed,
        passive_responder_scheduled: false,
    };
    assert_eq!(
        left.try_sync_game_boy_link_peer(&mut right),
        Ok(GameBoyLinkExchangeOutcome::Exchanged {
            local_action: Some(completed),
            peer_action: Some(completed),
        })
    );
    assert_eq!(left.read_byte(SERIAL_SB), 0x34);
    assert_eq!(right.read_byte(SERIAL_SB), 0xAB);
    assert_ne!(left.if_reg & 0x08, 0);
    assert_ne!(right.if_reg & 0x08, 0);
}

#[test]
fn link_exchange_schedules_passive_responder_once() {
    let mut master = make_test_bus();
    let mut passive = make_test_bus();
    master.try_sync_game_boy_link_peer(&mut passive).unwrap();

    master.write_byte(SERIAL_SB, 0xAB);
    passive.write_byte(SERIAL_SB, 0x34);
    master.write_byte(SERIAL_SC, 0x81);
    passive.write_byte(SERIAL_SC, 0x80);

    assert_eq!(
        master.try_sync_game_boy_link_peer(&mut passive),
        Ok(GameBoyLinkExchangeOutcome::Exchanged {
            local_action: Some(GameBoyLinkTransferExchange {
                reply: GameBoyLinkReplyDisposition::AcceptedPending,
                passive_responder_scheduled: true,
            }),
            peer_action: None,
        })
    );
    assert_eq!(passive.read_byte(SERIAL_SB), 0x34);
    assert_ne!(passive.read_byte(SERIAL_SC) & 0x80, 0);
    assert_eq!(passive.if_reg & 0x08, 0);

    passive.step_serial(4095);
    assert_eq!(passive.read_byte(SERIAL_SB), 0x34);
    assert_ne!(passive.read_byte(SERIAL_SC) & 0x80, 0);
    assert_eq!(passive.if_reg & 0x08, 0);

    passive.step_serial(1);
    assert_eq!(passive.read_byte(SERIAL_SB), 0xAB);
    assert_eq!(passive.read_byte(SERIAL_SC) & 0x80, 0);
    assert_ne!(passive.if_reg & 0x08, 0);

    let completed_state = passive.game_boy_link_replay_state();
    passive.if_reg &= !0x08;
    passive.step_serial(4096);
    assert_eq!(passive.game_boy_link_replay_state(), completed_state);
    assert_eq!(passive.if_reg & 0x08, 0);

    master.step_serial(4096);
    assert_eq!(master.read_byte(SERIAL_SB), 0x34);
    assert_eq!(master.read_byte(SERIAL_SC) & 0x80, 0);
    assert_ne!(master.if_reg & 0x08, 0);
}

#[test]
fn prepared_exchange_schedules_responder_but_defers_master_reply() {
    let mut master = make_test_bus();
    let mut passive = make_test_bus();
    master.try_sync_game_boy_link_peer(&mut passive).unwrap();

    master.write_byte(SERIAL_SB, 0xAB);
    passive.write_byte(SERIAL_SB, 0x34);
    master.write_byte(SERIAL_SC, 0x81);
    passive.write_byte(SERIAL_SC, 0x80);
    let preview = master.preview_game_boy_link_peer(&passive);
    let prepared = master.try_prepare_game_boy_link_peer(&mut passive).unwrap();
    let transfer = prepared.local_action.unwrap();

    assert_eq!(transfer.action(), preview.local_action.unwrap());
    assert_eq!(transfer.reply(), preview.peer_reply);
    assert!(transfer.passive_responder_scheduled());
    assert_eq!(prepared.peer_action, None);
    assert_eq!(
        master.game_boy_link_replay_state().pending_master_response,
        None
    );

    passive.step_serial(4096);
    assert_eq!(passive.read_byte(SERIAL_SB), 0xAB);
    master.step_serial(4096);
    assert_ne!(master.read_byte(SERIAL_SC) & 0x80, 0);

    assert_eq!(
        master
            .try_apply_prepared_game_boy_link_reply(transfer)
            .unwrap(),
        GameBoyLinkTransferExchange {
            reply: GameBoyLinkReplyDisposition::Completed,
            passive_responder_scheduled: true,
        }
    );
    assert_eq!(master.read_byte(SERIAL_SB), 0x34);
}

#[test]
fn stale_prepared_reply_is_rejected_without_mutation() {
    let mut master = make_test_bus();
    let mut passive = make_test_bus();
    master.try_sync_game_boy_link_peer(&mut passive).unwrap();
    master.write_byte(SERIAL_SB, 0xAB);
    passive.write_byte(SERIAL_SB, 0x34);
    master.write_byte(SERIAL_SC, 0x81);
    passive.write_byte(SERIAL_SC, 0x80);
    let transfer = master
        .try_prepare_game_boy_link_peer(&mut passive)
        .unwrap()
        .local_action
        .unwrap();

    let mut stale = master.game_boy_link_replay_state();
    stale.serial_generation = stale.serial_generation.wrapping_add(1);
    master.restore_game_boy_link_replay_state(stale);
    let before = master.game_boy_link_replay_state();
    assert!(matches!(
        master.try_apply_prepared_game_boy_link_reply(transfer),
        Err(GameBoyLinkExchangeError::RejectedReply {
            side: GameBoyLinkExchangeSide::Local,
            ..
        })
    ));
    assert_eq!(master.game_boy_link_replay_state(), before);
}

#[test]
fn crossed_prepare_retains_both_preview_snapshots_without_replies() {
    let mut left = make_test_bus();
    let mut right = make_test_bus();
    left.write_byte(SERIAL_SB, 0xAB);
    right.write_byte(SERIAL_SB, 0x34);
    left.write_byte(SERIAL_SC, 0x81);
    right.write_byte(SERIAL_SC, 0x81);
    let preview = left.preview_game_boy_link_peer(&right);

    let prepared = left.try_prepare_game_boy_link_peer(&mut right).unwrap();
    let left_transfer = prepared.local_action.unwrap();
    let right_transfer = prepared.peer_action.unwrap();
    assert_eq!(left_transfer.action(), preview.local_action.unwrap());
    assert_eq!(left_transfer.reply(), preview.peer_reply);
    assert_eq!(right_transfer.action(), preview.peer_action.unwrap());
    assert_eq!(right_transfer.reply(), preview.local_reply);
    assert_eq!(
        left.game_boy_link_replay_state().pending_master_response,
        None
    );
    assert_eq!(
        right.game_boy_link_replay_state().pending_master_response,
        None
    );
}

#[test]
fn link_exchange_preflight_rejection_preserves_both_sides() {
    let mut master = make_test_bus();
    let mut passive = make_test_bus();
    master.try_sync_game_boy_link_peer(&mut passive).unwrap();

    passive.write_byte(SERIAL_SB, 0x34);
    passive.write_byte(SERIAL_SC, 0x80);
    assert!(passive.schedule_game_boy_external_link_transfer(0x55, 4096));
    master.write_byte(SERIAL_SB, 0xAB);
    master.write_byte(SERIAL_SC, 0x81);
    master.if_reg = 0x05;
    passive.if_reg = 0x12;

    let master_state = master.game_boy_link_replay_state();
    let passive_state = passive.game_boy_link_replay_state();
    let master_if = master.if_reg;
    let passive_if = passive.if_reg;

    assert_eq!(
        master.try_sync_game_boy_link_peer(&mut passive),
        Err(GameBoyLinkExchangeError::RejectedPassiveScheduling {
            responder: GameBoyLinkExchangeSide::Peer,
        })
    );
    assert_eq!(master.game_boy_link_replay_state(), master_state);
    assert_eq!(passive.game_boy_link_replay_state(), passive_state);
    assert_eq!(master.if_reg, master_if);
    assert_eq!(passive.if_reg, passive_if);
}

#[test]
fn link_exchange_rejects_stale_queued_action_without_mutation() {
    let mut left = make_test_bus();
    let mut right = make_test_bus();
    let stale = zeff_emu_common::replay::ReplayGameBoyLinkState {
        peer_present: false,
        pending_master_byte: None,
        pending_master_response: None,
        pending_master_completion_ready: false,
        queued_master_action: Some(zeff_emu_common::replay::ReplayGameBoyLinkAction {
            out_byte: 0xAB,
            clock_period_t_cycles: 4096,
            serial_generation: 7,
        }),
        serial_generation: 3,
    };
    left.restore_game_boy_link_replay_state(stale);
    let left_before = left.game_boy_link_replay_state();
    let right_before = right.game_boy_link_replay_state();

    assert_eq!(
        left.try_sync_game_boy_link_peer(&mut right),
        Err(GameBoyLinkExchangeError::RejectedReply {
            side: GameBoyLinkExchangeSide::Local,
            action_generation: 7,
            serial_generation: 3,
        })
    );
    assert_eq!(left.game_boy_link_replay_state(), left_before);
    assert_eq!(right.game_boy_link_replay_state(), right_before);
}

#[test]
fn cpu_t_cycle_advance_uses_the_system_clock_domain() {
    let mut normal = make_cgb_test_bus();
    normal.advance_cpu_t_cycles(4);
    let normal_ppu_cycles = normal.ppu_cycles();
    assert_eq!(normal.advance_cpu_t_cycles(8), 8);
    assert_eq!(normal.ppu_cycles(), normal_ppu_cycles + 8);

    let mut double = make_cgb_test_bus();
    double.write_byte(0xFF4D, 0x01);
    assert!(double.maybe_switch_cgb_speed());
    double.advance_cpu_t_cycles(8);
    let double_ppu_cycles = double.ppu_cycles();
    assert_eq!(double.advance_cpu_t_cycles(8), 4);
    assert_eq!(double.ppu_cycles(), double_ppu_cycles + 4);
}

#[test]
fn speed_switch_delay_skips_normal_cpu_clocked_devices() {
    let mut bus = make_cgb_test_bus();
    bus.write_byte(PPU_DMA, 0xC0);
    bus.write_byte(0xFF4D, 0x01);
    assert!(bus.maybe_switch_cgb_speed());

    let timer_div = bus.timer_div();
    let ppu_ly = bus.ppu_ly();
    let pending_dma = bus.oam_dma_pending_source_base;
    assert_eq!(bus.advance_cgb_speed_switch_delay(), (16_400, 4_100));

    assert_eq!(bus.timer_div(), timer_div);
    assert_ne!(bus.ppu_ly(), ppu_ly);
    assert!(!bus.oam_dma_active);
    assert_eq!(bus.oam_dma_pending_source_base, pending_dma);
}

#[test]
fn oam_dma_transfers_one_byte_per_m_cycle() {
    let mut bus = make_test_bus();
    bus.oam[0] = 0xAA;
    bus.oam[1] = 0xBB;
    bus.write_byte(PPU_DMA, 0x00);

    assert!(!bus.oam_dma_active);
    assert_eq!(bus.oam[0], 0xAA);

    bus.step_oam_dma(4);
    assert!(bus.oam_dma_active);
    assert_eq!(bus.oam[0], 0xAA);

    bus.step_oam_dma(4);
    assert_eq!(bus.oam[0], 0x00);
    assert_eq!(bus.oam[1], 0xBB);

    bus.step_oam_dma(4);
    assert_eq!(bus.oam[1], 0x01);
}

#[test]
fn oam_dma_completes_after_160_m_cycles() {
    let mut bus = make_test_bus();
    bus.write_byte(PPU_DMA, 0x00);

    bus.step_oam_dma(160 * 4);
    assert!(bus.oam_dma_active);

    bus.step_oam_dma(4);
    assert!(!bus.oam_dma_active);
}

#[test]
fn oam_dma_restart_resets_progress_to_byte_zero() {
    let mut bus = make_test_bus();
    bus.write_byte(0xC000, 0x11);
    bus.write_byte(0xC001, 0x22);
    bus.write_byte(0xC100, 0xAA);
    bus.write_byte(0xC101, 0xBB);

    bus.write_byte(PPU_DMA, 0xC0);
    bus.step_oam_dma(4);
    assert!(bus.oam_dma_active);
    assert_eq!(bus.oam[0], 0x00);

    bus.step_oam_dma(4);
    assert_eq!(bus.oam[0], 0x11);
    assert_eq!(bus.oam[1], 0x00);

    bus.cpu_write_byte(PPU_DMA, 0xC1);
    assert_eq!(bus.oam_dma_pending_source_base, Some(0xC100));
    assert_eq!(bus.oam_dma_index, 1);

    bus.step_oam_dma(4);
    assert_eq!(bus.oam_dma_index, 0);
    assert_eq!(bus.oam[1], 0x22);

    bus.step_oam_dma(4);
    assert_eq!(bus.oam[0], 0xAA);
    assert_eq!(bus.oam[1], 0x22);

    bus.step_oam_dma(4);
    assert_eq!(bus.oam[1], 0xBB);
}

#[test]
fn oam_dma_source_reads_ff_from_vram_during_mode_3() {
    let mut bus = make_test_bus();
    bus.vram[0] = 0x5A;
    bus.io.ppu.lcdc |= Lcdc::LCD_ENABLE;
    bus.io.ppu.stat = (bus.io.ppu.stat & !0x03) | 0x03;
    bus.io.ppu.cycles = 100;

    bus.write_byte(PPU_DMA, 0x80);
    bus.step_oam_dma(8);

    assert_eq!(bus.oam[0], 0xFF);
}

#[test]
fn oam_dma_fe00_ff00_sources_mirror_wram_on_dmg() {
    let mut bus = make_test_bus();
    bus.write_byte(0xDE00, 0x12);
    bus.write_byte(PPU_DMA, 0xFE);
    bus.step_oam_dma(8);
    assert_eq!(bus.oam[0], 0x12);

    let mut bus = make_test_bus();
    bus.write_byte(0xDF00, 0x34);
    bus.write_byte(PPU_DMA, 0xFF);
    bus.step_oam_dma(8);
    assert_eq!(bus.oam[0], 0x34);
}

#[test]
fn oam_dma_blocks_cpu_access_except_hram() {
    let mut bus = make_test_bus();
    bus.write_byte(PPU_DMA, 0x00);

    assert_eq!(bus.cpu_read_byte(0x0001), 0x01);
    bus.step_oam_dma(4);

    assert_eq!(bus.cpu_read_byte(0x0001), 0xFF);
    bus.ie = 0x1F;
    assert_eq!(bus.cpu_read_byte(IE_ADDR), 0xFF);

    bus.cpu_write_byte(0xC000, 0x12);
    assert_ne!(bus.read_byte(0xC000), 0x12);

    bus.cpu_write_byte(IE_ADDR, 0x00);
    assert_eq!(bus.ie, 0x1F);

    bus.cpu_write_byte(PPU_DMA, 0xC0);
    assert_eq!(bus.oam_dma_pending_source_base, Some(0xC000));

    bus.cpu_write_byte(HRAM_START, 0x34);
    assert_eq!(bus.cpu_read_byte(HRAM_START), 0x34);
}

#[test]
fn oam_dma_blocks_only_conflicting_cpu_bus_and_oam_destination() {
    let mut bus = make_test_bus();
    bus.write_byte(PPU_DMA, 0x80);
    bus.step_oam_dma(4);

    assert_eq!(bus.cpu_read_byte(0xC000), 0x00);
    assert_eq!(bus.cpu_read_byte(0x8000), 0xFF);
    assert_eq!(bus.cpu_read_byte(OAM_START), 0xFF);
}

#[test]
fn read_rom_bank_0() {
    let bus = make_test_bus();
    assert_eq!(bus.read_byte_raw(0x0000), 0x00);
    assert_eq!(bus.read_byte_raw(0x0010), 0x10);
    assert_eq!(bus.read_byte_raw(0x00FF), 0xFF);
}

#[test]
fn read_wram_bank_0() {
    let mut bus = make_test_bus();
    bus.wram[0] = 0xAB;
    bus.wram[0x0FFF] = 0xCD;
    assert_eq!(bus.read_byte_raw(WRAM_0_START), 0xAB);
    assert_eq!(bus.read_byte_raw(WRAM_0_END), 0xCD);
}

#[test]
fn read_wram_bank_n() {
    let mut bus = make_test_bus();
    bus.wram[WRAM_SIZE] = 0x11;
    bus.wram[WRAM_SIZE + 0x0FFF] = 0x22;
    assert_eq!(bus.read_byte_raw(WRAM_N_START), 0x11);
    assert_eq!(bus.read_byte_raw(WRAM_N_END), 0x22);
}

#[test]
fn read_write_hram() {
    let mut bus = make_test_bus();
    bus.write_byte(HRAM_START, 0xDE);
    bus.write_byte(HRAM_END, 0xAD);
    assert_eq!(bus.read_byte_raw(HRAM_START), 0xDE);
    assert_eq!(bus.read_byte_raw(HRAM_END), 0xAD);
}

#[test]
fn read_write_ie() {
    let mut bus = make_test_bus();
    assert_eq!(bus.read_byte_raw(IE_ADDR), 0x00);
    bus.write_byte(IE_ADDR, 0x1F);
    assert_eq!(bus.read_byte_raw(IE_ADDR), 0x1F);
}

#[test]
fn unused_io_reads_return_ff_after_writes() {
    let mut bus = make_test_bus();

    for addr in [
        0xFF03, 0xFF08, 0xFF0E, 0xFF15, 0xFF1F, 0xFF27, 0xFF2F, 0xFF4C, 0xFF7F,
    ] {
        bus.write_byte(addr, 0x00);
        assert_eq!(bus.read_byte_raw(addr), 0xFF, "addr={addr:04X}");
    }
}

#[test]
fn cgb_undocumented_io_registers_have_expected_unused_bits() {
    let mut bus = make_cgb_test_bus();

    bus.write_byte(CGB_UNDOC_FF72, 0x00);
    assert_eq!(bus.read_byte_raw(CGB_UNDOC_FF72), 0x00);
    bus.write_byte(CGB_UNDOC_FF72, 0xFF);
    assert_eq!(bus.read_byte_raw(CGB_UNDOC_FF72), 0xFF);

    bus.write_byte(CGB_UNDOC_FF73, 0x00);
    assert_eq!(bus.read_byte_raw(CGB_UNDOC_FF73), 0x00);
    bus.write_byte(CGB_UNDOC_FF73, 0xFF);
    assert_eq!(bus.read_byte_raw(CGB_UNDOC_FF73), 0xFF);

    bus.write_byte(CGB_UNDOC_FF75, 0x00);
    assert_eq!(bus.read_byte_raw(CGB_UNDOC_FF75), 0x8F);
    bus.write_byte(CGB_UNDOC_FF75, 0x70);
    assert_eq!(bus.read_byte_raw(CGB_UNDOC_FF75), 0xFF);
}

#[test]
fn dmg_reads_cgb_undocumented_io_registers_as_unused() {
    let mut bus = make_test_bus();

    for addr in [CGB_UNDOC_FF72, CGB_UNDOC_FF73, CGB_UNDOC_FF75] {
        bus.write_byte(addr, 0x00);
        assert_eq!(bus.read_byte_raw(addr), 0xFF, "addr={addr:04X}");
    }
}

#[test]
fn cgb_dmg_compat_exposes_cgb_hardware_but_not_native_features() {
    let mut bus = make_cgb_compat_test_bus();

    assert!(bus.is_cgb_hardware());
    assert!(!bus.is_cgb_mode());

    bus.write_byte(CGB_UNDOC_FF72, 0x00);
    assert_eq!(bus.read_byte_raw(CGB_UNDOC_FF72), 0x00);
    bus.write_byte(CGB_UNDOC_FF75, 0x70);
    assert_eq!(bus.read_byte_raw(CGB_UNDOC_FF75), 0xFF);

    assert_eq!(bus.read_byte_raw(CGB_KEY1), 0xFF);
    bus.write_byte(CGB_KEY1, 0x01);
    assert_eq!(bus.key1 & 0x01, 0x00);

    bus.write_byte(SERIAL_SC, 0x00);
    assert_eq!(bus.read_byte_raw(SERIAL_SC) & 0x7E, 0x7E);

    bus.write_byte(CGB_BCPD, 0x00);
    assert_eq!(bus.read_byte_raw(CGB_BCPD), 0xFF);
    assert_eq!(bus.read_byte_raw(CGB_PCM12), 0x00);
}

#[test]
fn read_not_usable_returns_ff() {
    let bus = make_test_bus();
    assert_eq!(bus.read_byte_raw(NOT_USABLE_START), 0xFF);
    assert_eq!(bus.read_byte_raw(NOT_USABLE_END), 0xFF);
}

#[test]
fn read_write_oam() {
    let mut bus = make_test_bus();
    bus.io.ppu.lcdc &= !Lcdc::LCD_ENABLE;
    bus.write_byte(OAM_START, 0x42);
    bus.write_byte(OAM_START + 0x9F, 0x99);
    assert_eq!(bus.read_byte_raw(OAM_START), 0x42);
    assert_eq!(bus.read_byte_raw(OAM_START + 0x9F), 0x99);
}

#[test]
fn read_write_vram() {
    let mut bus = make_test_bus();
    bus.write_byte(VRAM_START, 0xAA);
    bus.write_byte(VRAM_END, 0xBB);
    assert_eq!(bus.read_byte_raw(VRAM_START), 0xAA);
    assert_eq!(bus.read_byte_raw(VRAM_END), 0xBB);
}

#[test]
fn echo_ram_read_mirrors_wram_bank_0() {
    let mut bus = make_test_bus();
    bus.wram[0] = 0x77;
    bus.wram[0x0FFF] = 0x88;
    assert_eq!(bus.read_byte_raw(ECHO_RAM_START), 0x77);
    assert_eq!(bus.read_byte_raw(0xEFFF), 0x88);
}

#[test]
fn echo_ram_read_mirrors_wram_bank_n() {
    let mut bus = make_test_bus();
    bus.wram[WRAM_SIZE] = 0xAA;
    bus.wram[WRAM_SIZE + 0x0DFF] = 0xBB;
    assert_eq!(bus.read_byte_raw(0xF000), 0xAA);
    assert_eq!(bus.read_byte_raw(ECHO_RAM_END), 0xBB);
}

#[test]
fn echo_ram_write_mirrors_to_wram() {
    let mut bus = make_test_bus();
    bus.write_byte(ECHO_RAM_START, 0x55);
    assert_eq!(bus.wram[0], 0x55);
    assert_eq!(bus.read_byte_raw(WRAM_0_START), 0x55);

    bus.write_byte(0xF000, 0x66);
    assert_eq!(bus.wram[WRAM_SIZE], 0x66);
    assert_eq!(bus.read_byte_raw(WRAM_N_START), 0x66);
}

#[test]
fn echo_ram_boundary_bank_0_to_n() {
    let mut bus = make_test_bus();
    bus.wram[0x0FFF] = 0x12;
    assert_eq!(bus.read_byte_raw(0xEFFF), 0x12);
    bus.wram[WRAM_SIZE] = 0x34;
    assert_eq!(bus.read_byte_raw(0xF000), 0x34);
}

#[test]
fn vram_read_returns_ff_during_mode_3() {
    let mut bus = make_test_bus();
    bus.vram[0] = 0x5A;
    bus.io.ppu.lcdc |= Lcdc::LCD_ENABLE;
    bus.io.ppu.stat = (bus.io.ppu.stat & !0x03) | 0x03;
    bus.io.ppu.cycles = 100;
    assert_eq!(bus.read_byte_raw(VRAM_START), 0xFF);
}

#[test]
fn vram_write_blocked_during_mode_3() {
    let mut bus = make_test_bus();
    bus.io.ppu.lcdc |= Lcdc::LCD_ENABLE;
    bus.io.ppu.stat = (bus.io.ppu.stat & !0x03) | 0x03;
    bus.io.ppu.cycles = 100;
    bus.write_byte(VRAM_START, 0xAB);
    assert_eq!(bus.vram[0], 0x00);
}

#[test]
fn vram_accessible_when_lcd_off() {
    let mut bus = make_test_bus();
    bus.io.ppu.lcdc &= !Lcdc::LCD_ENABLE;
    bus.io.ppu.stat = (bus.io.ppu.stat & !0x03) | 0x03;
    bus.write_byte(VRAM_START, 0xCC);
    assert_eq!(bus.read_byte_raw(VRAM_START), 0xCC);
}

#[test]
fn oam_read_returns_ff_during_mode_2() {
    let mut bus = make_test_bus();
    bus.oam[0] = 0x42;
    bus.io.ppu.lcdc |= Lcdc::LCD_ENABLE;
    bus.io.ppu.stat = (bus.io.ppu.stat & !0x03) | 0x02;
    assert_eq!(bus.read_byte_raw(OAM_START), 0xFF);
}

#[test]
fn oam_read_returns_ff_during_mode_3() {
    let mut bus = make_test_bus();
    bus.oam[0] = 0x42;
    bus.io.ppu.lcdc |= Lcdc::LCD_ENABLE;
    bus.io.ppu.stat = (bus.io.ppu.stat & !0x03) | 0x03;
    assert_eq!(bus.read_byte_raw(OAM_START), 0xFF);
}

#[test]
fn oam_write_blocked_during_mode_2() {
    let mut bus = make_test_bus();
    bus.io.ppu.lcdc |= Lcdc::LCD_ENABLE;
    bus.io.ppu.stat = (bus.io.ppu.stat & !0x03) | 0x02;
    bus.write_byte(OAM_START, 0xEE);
    assert_eq!(bus.oam[0], 0x00);
}

#[test]
fn oam_accessible_during_mode_0_and_1() {
    let mut bus = make_test_bus();
    bus.io.ppu.lcdc |= Lcdc::LCD_ENABLE;
    bus.io.ppu.stat &= !0x03;
    bus.io.ppu.cycles = 300;
    bus.write_byte(OAM_START, 0x11);
    assert_eq!(bus.read_byte_raw(OAM_START), 0x11);
    bus.io.ppu.stat = (bus.io.ppu.stat & !0x03) | 0x01;
    bus.io.ppu.ly = 144;
    bus.write_byte(OAM_START, 0x22);
    assert_eq!(bus.read_byte_raw(OAM_START), 0x22);
}

#[test]
fn cgb_wram_bank_switching() {
    let mut bus = make_cgb_test_bus();
    bus.wram_bank = 1;
    bus.write_byte(WRAM_N_START, 0xAA);
    assert_eq!(bus.read_byte_raw(WRAM_N_START), 0xAA);
    bus.wram_bank = 2;
    assert_eq!(bus.read_byte_raw(WRAM_N_START), 0x00);
    bus.write_byte(WRAM_N_START, 0xBB);
    bus.wram_bank = 1;
    assert_eq!(bus.read_byte_raw(WRAM_N_START), 0xAA);
    bus.wram_bank = 2;
    assert_eq!(bus.read_byte_raw(WRAM_N_START), 0xBB);
}

#[test]
fn cgb_wram_bank_0_maps_to_bank_1() {
    let mut bus = make_cgb_test_bus();
    bus.wram_bank = 0;
    bus.write_byte(WRAM_N_START, 0xCC);

    bus.wram_bank = 1;
    assert_eq!(bus.read_byte_raw(WRAM_N_START), 0xCC);
}

#[test]
fn cgb_vram_bank_switching() {
    let mut bus = make_cgb_test_bus();
    bus.vram_bank = 0;
    bus.write_byte(VRAM_START, 0x11);
    bus.vram_bank = 1;
    assert_eq!(bus.read_byte_raw(VRAM_START), 0x00);
    bus.write_byte(VRAM_START, 0x22);
    bus.vram_bank = 0;
    assert_eq!(bus.read_byte_raw(VRAM_START), 0x11);
    bus.vram_bank = 1;
    assert_eq!(bus.read_byte_raw(VRAM_START), 0x22);
}

#[test]
fn cgb_echo_ram_uses_active_wram_bank() {
    let mut bus = make_cgb_test_bus();
    bus.wram_bank = 3;
    bus.write_byte(WRAM_N_START, 0x55);
    assert_eq!(bus.read_byte_raw(0xF000), 0x55);
    bus.wram_bank = 4;
    assert_eq!(bus.read_byte_raw(0xF000), 0x00);
    bus.write_byte(0xF000, 0x66);
    assert_eq!(bus.read_byte_raw(WRAM_N_START), 0x66);
}

#[test]
fn gdma_transfers_all_blocks_immediately() {
    let mut bus = make_cgb_test_bus();
    bus.hdma1 = 0xC0;
    bus.hdma2 = 0x00;
    bus.hdma3 = 0x00;
    bus.hdma4 = 0x00;
    for i in 0..0x20u16 {
        bus.wram[i as usize] = (i + 1) as u8;
    }
    let t_cycles = bus.execute_hdma_transfer(0x01);
    assert!(!bus.hdma_active);
    assert_eq!(bus.hdma5, 0xFF);
    assert_eq!(t_cycles, 2 * 32);
    for i in 0..0x20u16 {
        assert_eq!(bus.vram[i as usize], (i + 1) as u8);
    }
}

#[test]
fn gdma_advances_source_and_dest_pointers() {
    let mut bus = make_cgb_test_bus();
    bus.hdma1 = 0xC0;
    bus.hdma2 = 0x00;
    bus.hdma3 = 0x00;
    bus.hdma4 = 0x00;
    bus.execute_hdma_transfer(0x00);
    assert_eq!(bus.hdma1, 0xC0);
    assert_eq!(bus.hdma2, 0x10);
    assert_eq!(bus.hdma3, 0x00);
    assert_eq!(bus.hdma4, 0x10);
}

#[test]
fn gdma_double_speed_uses_64_t_per_block() {
    let mut bus = make_cgb_test_bus();
    bus.hardware_mode = HardwareMode::CGBDouble;
    bus.hdma1 = 0xC0;
    bus.hdma2 = 0x00;
    bus.hdma3 = 0x00;
    bus.hdma4 = 0x00;

    let t_cycles = bus.execute_hdma_transfer(0x03);
    assert_eq!(t_cycles, 4 * 64);
}

#[test]
fn hblank_hdma_setup_in_mode_2_does_not_transfer_immediately() {
    let mut bus = make_cgb_test_bus();
    bus.io.ppu.lcdc |= Lcdc::LCD_ENABLE;
    bus.io.ppu.ly = 0;
    bus.io.ppu.stat = (bus.io.ppu.stat & !0x03) | 0x02;
    bus.hdma1 = 0xC0;
    bus.hdma2 = 0x00;
    bus.hdma3 = 0x00;
    bus.hdma4 = 0x00;
    let t_cycles = bus.execute_hdma_transfer(0x81);

    assert!(bus.hdma_active);
    assert!(bus.hdma_hblank);
    assert_eq!(bus.hdma_blocks_left, 2);
    assert_eq!(t_cycles, 0);
}

#[test]
fn hblank_hdma_with_lcd_off_transfers_one_block_immediately() {
    let mut bus = make_cgb_test_bus();
    bus.io.ppu.lcdc &= !Lcdc::LCD_ENABLE;
    bus.hdma1 = 0xC0;
    bus.hdma2 = 0x00;
    bus.hdma3 = 0x00;
    bus.hdma4 = 0x00;
    bus.wram[0] = 0xAA;
    bus.wram[0x10] = 0xBB;

    let t_cycles = bus.execute_hdma_transfer(0x83);

    assert!(bus.hdma_active);
    assert!(bus.hdma_hblank);
    assert_eq!(bus.hdma_blocks_left, 3);
    assert_eq!(bus.hdma5, 0x02);
    assert_eq!(t_cycles, 0);
    assert_eq!(bus.vram[0], 0xAA);
    assert_eq!(bus.vram[0x10], 0x00);
}

#[test]
fn hblank_hdma_started_in_mode_0_transfers_one_block_immediately() {
    let mut bus = make_cgb_test_bus();
    bus.io.ppu.lcdc |= Lcdc::LCD_ENABLE;
    bus.io.ppu.ly = 0;
    bus.io.ppu.stat &= !0x03;
    bus.hdma1 = 0xC0;
    bus.hdma2 = 0x00;
    bus.hdma3 = 0x00;
    bus.hdma4 = 0x00;
    bus.wram[0] = 0xCC;

    bus.execute_hdma_transfer(0x83);

    assert_eq!(bus.hdma_blocks_left, 3);
    assert_eq!(bus.hdma5, 0x02);
    assert_eq!(bus.vram[0], 0xCC);
}

#[test]
fn hblank_hdma_transfers_one_block_per_mode_0_transition() {
    let mut bus = make_cgb_test_bus();
    bus.io.ppu.lcdc |= Lcdc::LCD_ENABLE;
    bus.io.ppu.ly = 0;
    bus.hdma1 = 0xC0;
    bus.hdma2 = 0x00;
    bus.hdma3 = 0x00;
    bus.hdma4 = 0x00;

    bus.wram[0] = 0xAA;
    bus.wram[0x10] = 0xBB;
    bus.execute_hdma_transfer(0x81);
    assert_eq!(bus.hdma_blocks_left, 2);
    bus.maybe_step_hblank_hdma(3, 0);
    assert_eq!(bus.hdma_blocks_left, 1);
    assert!(bus.hdma_active);
    assert_eq!(bus.vram[0], 0xAA);
    bus.maybe_step_hblank_hdma(3, 0);
    assert!(!bus.hdma_active);
    assert_eq!(bus.hdma5, 0xFF);
    assert_eq!(bus.vram[0x10], 0xBB);
}

#[test]
fn hblank_hdma_ignores_non_mode_0_transitions() {
    let mut bus = make_cgb_test_bus();
    bus.io.ppu.lcdc |= Lcdc::LCD_ENABLE;
    bus.io.ppu.ly = 0;
    bus.hdma1 = 0xC0;
    bus.hdma2 = 0x00;
    bus.hdma3 = 0x00;
    bus.hdma4 = 0x00;

    bus.execute_hdma_transfer(0x80);
    let blocks_before = bus.hdma_blocks_left;
    bus.maybe_step_hblank_hdma(0, 2);
    assert_eq!(bus.hdma_blocks_left, blocks_before);
    bus.maybe_step_hblank_hdma(2, 3);
    assert_eq!(bus.hdma_blocks_left, blocks_before);
}

#[test]
fn hblank_hdma_skipped_during_vblank() {
    let mut bus = make_cgb_test_bus();
    bus.io.ppu.lcdc |= Lcdc::LCD_ENABLE;
    bus.io.ppu.ly = 144;
    bus.hdma1 = 0xC0;
    bus.hdma2 = 0x00;
    bus.hdma3 = 0x00;
    bus.hdma4 = 0x00;

    bus.execute_hdma_transfer(0x80);
    let blocks_before = bus.hdma_blocks_left;

    bus.maybe_step_hblank_hdma(3, 0);
    assert_eq!(bus.hdma_blocks_left, blocks_before);
}

#[test]
fn hblank_hdma_cancel() {
    let mut bus = make_cgb_test_bus();
    bus.hdma1 = 0xC0;
    bus.hdma2 = 0x00;
    bus.hdma3 = 0x00;
    bus.hdma4 = 0x00;
    bus.execute_hdma_transfer(0x83);
    assert!(bus.hdma_active);
    assert!(bus.hdma_hblank);
    let t_cycles = bus.execute_hdma_transfer(0x00);
    assert!(!bus.hdma_active);
    assert!(!bus.hdma_hblank);
    assert_eq!(t_cycles, 0);
    assert_eq!(bus.hdma5, 0x80);
}

#[test]
fn game_genie_rom_write_overrides_read() {
    let mut bus = make_test_bus();
    assert_eq!(bus.read_byte(0x0010), 0x10);

    bus.game_genie_patches.push(CheatPatch::RomWrite {
        address: 0x0010,
        value: CheatValue::Constant(0xFF),
    });

    assert_eq!(bus.read_byte(0x0010), 0xFF);
    assert_eq!(bus.read_byte(0x0011), 0x11);
}

#[test]
fn game_genie_rom_write_if_equals_conditional() {
    let mut bus = make_test_bus();
    bus.game_genie_patches.push(CheatPatch::RomWriteIfEquals {
        address: 0x0020,
        value: CheatValue::Constant(0xAA),
        compare: CheatValue::Constant(0x20),
    });

    assert_eq!(bus.read_byte(0x0020), 0xAA);
}

#[test]
fn game_genie_rom_write_if_equals_no_match() {
    let mut bus = make_test_bus();
    bus.game_genie_patches.push(CheatPatch::RomWriteIfEquals {
        address: 0x0020,
        value: CheatValue::Constant(0xAA),
        compare: CheatValue::Constant(0x99),
    });
    assert_eq!(bus.read_byte(0x0020), 0x20);
}

#[test]
fn game_genie_empty_patches_fast_path() {
    let bus = make_test_bus();
    assert!(bus.game_genie_patches.is_empty());
    assert_eq!(bus.read_byte(0x0010), 0x10);
}

#[test]
fn game_genie_non_rom_reads_unaffected() {
    let mut bus = make_test_bus();
    bus.wram[0] = 0x42;
    bus.game_genie_patches.push(CheatPatch::RomWrite {
        address: 0xC000,
        value: CheatValue::Constant(0xFF),
    });
    assert_eq!(bus.read_byte(WRAM_0_START), 0x42);
}

#[test]
fn game_genie_multiple_patches_first_match_wins() {
    let mut bus = make_test_bus();
    bus.game_genie_patches.push(CheatPatch::RomWrite {
        address: 0x0010,
        value: CheatValue::Constant(0xAA),
    });
    bus.game_genie_patches.push(CheatPatch::RomWrite {
        address: 0x0010,
        value: CheatValue::Constant(0xBB),
    });
    assert_eq!(bus.read_byte(0x0010), 0xAA);
}

#[test]
fn read_byte_raw_bypasses_game_genie() {
    let mut bus = make_test_bus();
    bus.game_genie_patches.push(CheatPatch::RomWrite {
        address: 0x0010,
        value: CheatValue::Constant(0xFF),
    });
    assert_eq!(bus.read_byte(0x0010), 0xFF);
    assert_eq!(bus.read_byte_raw(0x0010), 0x10);
}

#[test]
fn write_to_wram_stores_correctly() {
    let mut bus = make_test_bus();
    bus.write_byte(WRAM_0_START, 0x11);
    bus.write_byte(WRAM_0_START + 1, 0x22);
    bus.write_byte(WRAM_N_START, 0x33);
    assert_eq!(bus.wram[0], 0x11);
    assert_eq!(bus.wram[1], 0x22);
    assert_eq!(bus.wram[WRAM_SIZE], 0x33);
}

#[test]
fn write_returns_zero_extra_cycles_for_simple_regions() {
    let mut bus = make_test_bus();
    assert_eq!(bus.write_byte(WRAM_0_START, 0x00), 0);
    assert_eq!(bus.write_byte(HRAM_START, 0x00), 0);
    assert_eq!(bus.write_byte(IE_ADDR, 0x00), 0);
    assert_eq!(bus.write_byte(OAM_START, 0x00), 0);
    assert_eq!(bus.write_byte(VRAM_START, 0x00), 0);
}

#[test]
fn if_register_read_write() {
    let mut bus = make_test_bus();
    assert_eq!(bus.read_byte_raw(INTERRUPT_IF), 0xE1);
    bus.if_reg = 0x00;
    assert_eq!(bus.read_byte_raw(INTERRUPT_IF), 0xE0);
}

#[test]
fn cgb_speed_switch_toggles_hardware_mode() {
    let mut bus = make_cgb_test_bus();
    assert!(matches!(bus.hardware_mode, HardwareMode::CGBNormal));

    bus.key1 = (bus.key1 & 0x80) | 0x01 | 0x7E;
    assert!(bus.maybe_switch_cgb_speed());
    assert!(matches!(bus.hardware_mode, HardwareMode::CGBDouble));

    assert_eq!(bus.key1 & 0x80, 0x80);
    assert_eq!(bus.key1 & 0x01, 0x00);

    bus.key1 = (bus.key1 & 0x80) | 0x01 | 0x7E;
    assert!(bus.maybe_switch_cgb_speed());
    assert!(matches!(bus.hardware_mode, HardwareMode::CGBNormal));
    assert_eq!(bus.key1 & 0x80, 0x00);
}

#[test]
fn cgb_speed_switch_ignored_without_prepare_bit() {
    let mut bus = make_cgb_test_bus();

    assert!(!bus.maybe_switch_cgb_speed());
    assert!(matches!(bus.hardware_mode, HardwareMode::CGBNormal));
}

#[test]
fn cgb_speed_switch_ignored_in_dmg_mode() {
    let mut bus = make_test_bus();
    bus.key1 |= 0x01;
    assert!(!bus.maybe_switch_cgb_speed());
}

#[test]
fn cgb_svbk_read_returns_masked_bank() {
    let mut bus = make_cgb_test_bus();
    bus.wram_bank = 5;
    let val = io_bus::read_io(&bus, CGB_SVBK);
    assert_eq!(val & 0x07, 5);
    assert_eq!(val & 0xF8, 0xF8);
}

#[test]
fn cgb_vbk_read_returns_masked_bank() {
    let mut bus = make_cgb_test_bus();
    bus.vram_bank = 1;
    let val = io_bus::read_io(&bus, PPU_VBK);
    assert_eq!(val, 0xFF);
    bus.vram_bank = 0;
    let val = io_bus::read_io(&bus, PPU_VBK);
    assert_eq!(val, 0xFE);
}

#[test]
fn cgb_key1_write_only_affects_bit_0() {
    let mut bus = make_cgb_test_bus();
    let original_key1 = bus.key1;
    io_bus::write_io(&mut bus, CGB_KEY1, 0xFF);

    assert_eq!(bus.key1 & 0x01, 0x01);
    assert_eq!(bus.key1 & 0x7E, 0x7E);
    assert_eq!(bus.key1 & 0x80, original_key1 & 0x80);
}
