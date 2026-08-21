use super::*;
use crate::save_state::{StateReader, StateWriter};

fn packet(command: u8, compression: u8, payload: &[u8]) -> Vec<u8> {
    let len = u16::try_from(payload.len()).unwrap();
    let mut bytes = vec![
        0x88,
        0x33,
        command,
        compression,
        len as u8,
        (len >> 8) as u8,
    ];
    bytes.extend_from_slice(payload);
    let checksum = bytes[2..]
        .iter()
        .fold(0u16, |sum, byte| sum.wrapping_add(u16::from(*byte)));
    bytes.extend_from_slice(&checksum.to_le_bytes());
    bytes.extend_from_slice(&[0, 0]);
    bytes
}

fn exchange(printer: &mut GameboyPrinter, bytes: &[u8]) -> Vec<u8> {
    bytes
        .iter()
        .map(|byte| printer.feed_serial_byte(*byte))
        .collect()
}

fn exchange_packet(
    printer: &mut GameboyPrinter,
    command: u8,
    compression: u8,
    payload: &[u8],
) -> Vec<u8> {
    exchange(printer, &packet(command, compression, payload))
}

fn roundtrip(printer: &GameboyPrinter) -> GameboyPrinter {
    let mut writer = StateWriter::new();
    printer.write_state(&mut writer);
    let bytes = writer.into_bytes();
    let mut reader = StateReader::new(&bytes);
    let restored = GameboyPrinter::read_state(&mut reader).unwrap();
    assert!(reader.is_exhausted());
    restored
}

#[test]
fn init_golden_transcript_returns_device_id_and_ready_status() {
    let mut printer = GameboyPrinter::new();
    let replies = exchange(&mut printer, &[0x88, 0x33, 0x01, 0, 0, 0, 0x01, 0, 0, 0]);

    assert!(replies[..8].iter().all(|reply| *reply == 0));
    assert_eq!(&replies[8..], &[DEVICE_ID, 0]);
}

#[test]
fn raw_data_end_and_print_render_a_fixed_rgba_job() {
    let mut printer = GameboyPrinter::new();
    let mut tile_row = vec![0; BYTES_PER_TILE_ROW];
    tile_row[0] = 0x80;

    let data_replies = exchange_packet(&mut printer, 0x04, 0, &tile_row);
    assert_eq!(data_replies[data_replies.len() - 2], DEVICE_ID);
    assert_eq!(data_replies.last(), Some(&0));
    let end_replies = exchange_packet(&mut printer, 0x04, 0, &[]);
    assert_eq!(end_replies.last(), Some(&STATUS_UNPROCESSED_DATA));

    let print_replies = exchange_packet(&mut printer, 0x02, 0, &[1, 0, 0xE4, 0x40]);
    assert_eq!(print_replies.last(), Some(&STATUS_UNPROCESSED_DATA));
    assert_eq!(printer.image_count(), 1);
    let image = printer.latest_image().unwrap();
    assert_eq!(image.len(), PRINTER_RGBA_SIZE);
    let first_pixel = (PRINTER_MARGIN_TOP * PRINTER_IMAGE_W + PRINTER_MARGIN_LEFT) * 4;
    assert_eq!(&image[first_pixel..first_pixel + 4], &[192, 192, 192, 255]);
    assert_eq!(&image[..4], &[255, 255, 255, 255]);
    assert!(printer.data_buffer.is_empty());
    assert_eq!(printer.status, STATUS_BUSY | STATUS_BUFFER_FULL);
    let busy_cycles = printer.busy_cycles_remaining;
    printer.step(busy_cycles - 1);
    assert_eq!(printer.status, STATUS_BUSY | STATUS_BUFFER_FULL);
    printer.step(1);
    assert_eq!(printer.status, STATUS_BUFFER_FULL);

    let status_replies = exchange_packet(&mut printer, 0x0F, 0, &[]);
    assert_eq!(status_replies.last(), Some(&STATUS_BUFFER_FULL));
}

#[test]
fn rle_literal_and_run_boundaries_decode_losslessly() {
    let mut encoded = vec![0x7F];
    encoded.extend(0u8..=127);
    encoded.extend_from_slice(&[0x80, 0xA5, 0xFF, 0x5A]);

    let mut expected: Vec<u8> = (0u8..=127).collect();
    expected.extend_from_slice(&[0xA5; 2]);
    expected.extend_from_slice(&[0x5A; 129]);
    assert_eq!(decode_rle(&encoded), Some(expected.clone()));

    let mut printer = GameboyPrinter::new();
    exchange_packet(&mut printer, 0x04, 1, &encoded);
    assert_eq!(printer.data_buffer, expected);
}

#[test]
fn malformed_rle_and_bad_checksum_report_errors_without_mutating_data() {
    let mut printer = GameboyPrinter::new();
    let malformed = exchange_packet(&mut printer, 0x04, 1, &[0x80]);
    assert_eq!(malformed.last(), Some(&STATUS_PACKET_ERROR));
    assert!(printer.data_buffer.is_empty());

    let mut bad = packet(0x04, 0, &[0x12, 0x34]);
    let checksum_high = bad.len() - 3;
    bad[checksum_high] ^= 0x80;
    let replies = exchange(&mut printer, &bad);
    assert_eq!(replies.last(), Some(&STATUS_CHECKSUM_ERROR));
    assert!(printer.data_buffer.is_empty());

    let nul = exchange_packet(&mut printer, 0x0F, 0, &[]);
    assert_eq!(nul.last(), Some(&0));
}

#[test]
fn buffer_is_bounded_to_eight_kibibytes() {
    let mut printer = GameboyPrinter::new();
    let full = vec![0xAA; BUFFER_CAPACITY];
    let replies = exchange_packet(&mut printer, 0x04, 0, &full);
    assert_eq!(printer.data_buffer.len(), BUFFER_CAPACITY);
    assert_eq!(replies.last(), Some(&0));
    assert_eq!(printer.status, STATUS_UNPROCESSED_DATA | STATUS_BUFFER_FULL);

    let overflow = exchange_packet(&mut printer, 0x04, 0, &[0xBB]);
    assert_eq!(
        overflow.last(),
        Some(&(STATUS_UNPROCESSED_DATA | STATUS_BUFFER_FULL))
    );
    assert_eq!(printer.data_buffer, full);
}

#[test]
fn invalid_second_magic_resynchronizes_on_the_next_88() {
    let mut printer = GameboyPrinter::new();
    let mut transcript = vec![0x12, 0x88, 0x44, 0x88];
    transcript.extend_from_slice(&packet(0x01, 0, &[])[1..]);
    let replies = exchange(&mut printer, &transcript);
    assert_eq!(&replies[replies.len() - 2..], &[DEVICE_ID, 0]);
    assert_eq!(printer.state, ParserState::Magic1);
}

#[test]
fn init_data_break_and_nul_have_distinct_command_semantics() {
    let mut printer = GameboyPrinter::new();
    exchange_packet(&mut printer, 0x04, 0, &[1, 2, 3]);
    printer.status |= STATUS_BUSY;
    printer.busy_cycles_remaining = 100;
    let replies = exchange_packet(&mut printer, 0x08, 0, &[]);
    assert_eq!(
        replies.last(),
        Some(&(STATUS_BUSY | STATUS_UNPROCESSED_DATA))
    );
    assert_eq!(printer.data_buffer, [1, 2, 3]);
    assert_eq!(printer.busy_cycles_remaining, 0);

    let replies = exchange_packet(&mut printer, 0x0F, 0, &[]);
    assert_eq!(replies.last(), Some(&STATUS_UNPROCESSED_DATA));
    let replies = exchange_packet(&mut printer, 0x01, 0, &[]);
    assert_eq!(replies.last(), Some(&0));
    assert!(printer.data_buffer.is_empty());
}

#[test]
fn print_busy_time_uses_image_bands_copies_and_feed() {
    let mut printer = GameboyPrinter::new();
    exchange_packet(&mut printer, 0x04, 0, &vec![0; BYTES_PER_TILE_ROW]);
    exchange_packet(&mut printer, 0x04, 0, &[]);
    let replies = exchange_packet(&mut printer, 0x02, 0, &[2, 0x21, 0xE4, 0x40]);
    assert_eq!(replies.last(), Some(&STATUS_UNPROCESSED_DATA));
    let expected_cycles = (8 * DMG_CLOCK_HZ * PRINT_BANDS_PER_SECOND_DENOMINATOR)
        .div_ceil(PRINT_BANDS_PER_SECOND_NUMERATOR);
    assert_eq!(printer.busy_cycles_remaining, expected_cycles);

    let replies = exchange_packet(&mut printer, 0x0F, 0, &[]);
    assert_eq!(replies.last(), Some(&(STATUS_BUSY | STATUS_BUFFER_FULL)));
    printer.step(expected_cycles);
    let replies = exchange_packet(&mut printer, 0x0F, 0, &[]);
    assert_eq!(replies.last(), Some(&STATUS_BUFFER_FULL));
}

#[test]
fn camera_image_uses_physical_printer_band_timing() {
    let mut printer = GameboyPrinter::new();
    exchange_packet(
        &mut printer,
        0x04,
        0,
        &vec![0; PRINTER_TILE_ROWS * BYTES_PER_TILE_ROW],
    );
    exchange_packet(&mut printer, 0x04, 0, &[]);
    exchange_packet(&mut printer, 0x02, 0, &[1, 0x13, 0xE4, 0x40]);

    // Nine image bands plus one pre-feed and three post-feed bands at 1.1 bands/s.
    let expected_cycles = (13 * DMG_CLOCK_HZ * PRINT_BANDS_PER_SECOND_DENOMINATOR)
        .div_ceil(PRINT_BANDS_PER_SECOND_NUMERATOR);
    assert_eq!(printer.busy_cycles_remaining, expected_cycles);
    assert_eq!(expected_cycles, 49_569_048);
}

#[test]
fn print_requires_an_end_of_data_packet() {
    let mut printer = GameboyPrinter::new();
    exchange_packet(&mut printer, 0x04, 0, &vec![0; BYTES_PER_TILE_ROW]);

    let replies = exchange_packet(&mut printer, 0x02, 0, &[1, 0, 0xE4, 0x40]);

    assert_eq!(replies.last(), Some(&STATUS_UNPROCESSED_DATA));
    assert_eq!(printer.status, STATUS_UNPROCESSED_DATA);
    assert_eq!(printer.image_count(), 0);
    assert_eq!(printer.busy_cycles_remaining, 0);
}

#[test]
fn empty_buffer_and_zero_copy_prints_are_feed_only() {
    let mut empty = GameboyPrinter::new();
    exchange_packet(&mut empty, 0x04, 0, &[]);
    exchange_packet(&mut empty, 0x02, 0, &[5, 0x13, 0xE4, 0x40]);
    let fifteen_feeds = (15 * DMG_CLOCK_HZ * PRINT_BANDS_PER_SECOND_DENOMINATOR)
        .div_ceil(PRINT_BANDS_PER_SECOND_NUMERATOR);
    assert_eq!(empty.busy_cycles_remaining, fifteen_feeds);
    assert_eq!(empty.image_count(), 0);

    let mut zero_copy = GameboyPrinter::new();
    exchange_packet(&mut zero_copy, 0x04, 0, &vec![0; BYTES_PER_TILE_ROW]);
    exchange_packet(&mut zero_copy, 0x04, 0, &[]);
    exchange_packet(&mut zero_copy, 0x02, 0, &[0, 0x13, 0xE4, 0x40]);
    let three_feeds = (3 * DMG_CLOCK_HZ * PRINT_BANDS_PER_SECOND_DENOMINATOR)
        .div_ceil(PRINT_BANDS_PER_SECOND_NUMERATOR);
    assert_eq!(zero_copy.busy_cycles_remaining, three_feeds);
    assert_eq!(zero_copy.image_count(), 0);
}

#[test]
fn clearing_host_images_does_not_disturb_an_in_flight_packet() {
    let data = packet(0x04, 0, &[0x11, 0x22]);
    let mut printer = GameboyPrinter::new();
    printer.images.push(vec![0; PRINTER_RGBA_SIZE]);
    exchange(&mut printer, &data[..7]);
    let before_clear = roundtrip(&printer);

    printer.clear();
    assert!(printer.images.is_empty());
    assert_eq!(printer.state, before_clear.state);
    assert_eq!(printer.payload, before_clear.payload);
    let replies = exchange(&mut printer, &data[7..]);
    assert_eq!(replies[replies.len() - 2], DEVICE_ID);
    assert_eq!(printer.data_buffer, [0x11, 0x22]);
}

#[test]
fn save_state_restores_mid_payload_and_response_pipeline() {
    let data_packet = packet(0x04, 0, &[0x10, 0x20, 0x30, 0x40]);
    let split = 8;
    let mut original = GameboyPrinter::new();
    exchange(&mut original, &data_packet[..split]);
    let mut restored = roundtrip(&original);

    let original_replies = exchange(&mut original, &data_packet[split..]);
    let restored_replies = exchange(&mut restored, &data_packet[split..]);
    assert_eq!(original_replies, restored_replies);
    assert_eq!(original.data_buffer, restored.data_buffer);
    assert_eq!(original.status, restored.status);

    exchange_packet(&mut restored, 0x04, 0, &[]);
    let print_packet = packet(0x02, 0, &[1, 0, 0xE4, 0x40]);
    let mut printer = restored;
    exchange(&mut printer, &print_packet[..print_packet.len() - 2]);
    assert_eq!(printer.state, ParserState::DeviceId);
    let mut restored = roundtrip(&printer);
    let tail = &print_packet[print_packet.len() - 2..];
    assert_eq!(exchange(&mut printer, tail), exchange(&mut restored, tail));
    assert_eq!(printer.latest_image(), restored.latest_image());
    assert!(printer.data_buffer.is_empty());
    assert!(restored.data_buffer.is_empty());
}

#[test]
fn save_state_restores_busy_timing_and_completed_status() {
    let mut printer = GameboyPrinter::new();
    exchange_packet(&mut printer, 0x04, 0, &vec![0; BYTES_PER_TILE_ROW]);
    exchange_packet(&mut printer, 0x04, 0, &[]);
    exchange_packet(&mut printer, 0x02, 0, &[1, 0, 0xE4, 0x40]);
    assert_eq!(printer.status, STATUS_BUSY | STATUS_BUFFER_FULL);

    let mut restored = roundtrip(&printer);
    assert_eq!(restored.status, printer.status);
    assert_eq!(
        restored.busy_cycles_remaining,
        printer.busy_cycles_remaining
    );

    let busy_cycles = printer.busy_cycles_remaining;
    printer.step(busy_cycles);
    restored.step(busy_cycles);
    assert_eq!(printer.status, STATUS_BUFFER_FULL);
    assert_eq!(restored.status, STATUS_BUFFER_FULL);

    let mut completed = roundtrip(&restored);
    let expected = exchange_packet(&mut restored, 0x0F, 0, &[]);
    assert_eq!(exchange_packet(&mut completed, 0x0F, 0, &[]), expected);
    assert_eq!(expected.last(), Some(&STATUS_BUFFER_FULL));
}

#[test]
fn completed_image_queue_stays_within_the_state_limit() {
    let mut printer = GameboyPrinter::new();
    printer.images = (0..MAX_SAVED_IMAGES)
        .map(|index| vec![(index & 0xFF) as u8])
        .collect();
    printer.payload = vec![1, 0, 0xE4, 0x40];
    printer.data_buffer = vec![0; BYTES_PER_TILE_ROW];
    printer.data_end_seen = true;

    printer.print();

    assert_eq!(printer.images.len(), MAX_SAVED_IMAGES);
    assert_eq!(printer.images[0], vec![1]);
    assert_eq!(printer.images.last().unwrap().len(), PRINTER_RGBA_SIZE);
}

#[test]
fn save_state_rejects_inconsistent_parser_positions() {
    let mut printer = GameboyPrinter::new();
    printer.state = ParserState::Payload;
    printer.header_pos = 4;
    printer.payload_expected = 1;
    printer.payload_received = 1;
    printer.payload.push(0xAA);

    let mut writer = StateWriter::new();
    printer.write_state(&mut writer);
    let bytes = writer.into_bytes();
    let mut reader = StateReader::new(&bytes);
    assert!(GameboyPrinter::read_state(&mut reader).is_err());
}

#[test]
fn legacy_state_decoder_consumes_the_v3_to_v5_layout() {
    let mut writer = StateWriter::new();
    writer.write_u8(2);
    writer.write_bytes(&[4, 0, 3, 0, 0]);
    writer.write_u64(5);
    writer.write_u64(3);
    writer.write_u64(2);
    writer.write_u8(STATUS_UNPROCESSED_DATA);
    writer.write_u64(1);
    writer.write_u64(PRINTER_RGBA_SIZE as u64);
    writer.write_bytes(&vec![0xA5; PRINTER_RGBA_SIZE]);

    let bytes = writer.into_bytes();
    let mut reader = StateReader::new(&bytes);
    let printer = GameboyPrinter::read_legacy_state(&mut reader).unwrap();
    assert!(reader.is_exhausted());
    assert_eq!(printer.state, ParserState::Magic1);
    assert!(printer.data_buffer.is_empty());
    assert_eq!(printer.status, STATUS_UNPROCESSED_DATA);
    assert_eq!(printer.image_count(), 1);
}
