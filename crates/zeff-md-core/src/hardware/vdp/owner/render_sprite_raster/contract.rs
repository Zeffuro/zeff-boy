use super::super::{
    domain::{Domain, RenderBus},
    render::Renderer,
    render_timing::RenderTiming,
};
use super::{
    SpriteRaster,
    line_port::{LinePort, PortPins, ReadWord, WORD_MASK},
};
use crate::hardware::vdp::control_core::PhaseLevels;
use crate::hardware::vdp::state::{StateReader, StateWriter};

const QUIET: PhaseLevels = PhaseLevels {
    data_master: false,
    data_slave: false,
    horizontal_master: false,
    horizontal_slave: false,
};

fn pins(address: Option<u8>) -> PortPins {
    PortPins {
        address,
        enabled: false,
        data_master: false,
        writes: [false; 8],
        incoming: [0; 8],
    }
}

fn commit(port: &mut LinePort, lines: &mut [[u8; 8]; 40], address: u8) {
    port.begin_edge(pins(Some(address)), QUIET, 0, lines)
        .unwrap();
    port.finish_edge(lines).unwrap();
}

fn assert_bus(port: &LinePort, values: [u8; 8], masks: [u8; 8]) {
    assert_eq!(port.bus(true).lanes(), (values, masks));
}

fn edge_history() {
    let mut port = LinePort::legacy_unknown();
    let mut lines = [[0; 8]; 40];
    lines[2] = [0x5a; 8];
    lines[3] = [0x33; 8];
    port.begin_edge(pins(Some(2)), QUIET, 0, &lines).unwrap();
    assert_eq!(port.bus(true), ReadWord::UNKNOWN);
    assert_eq!(port.bus(false).lanes(), ([127; 8], [127; 8]));
    port.finish_edge(&mut lines).unwrap();
    assert_bus(&port, [0x5a; 8], [127; 8]);
    let write = PortPins {
        address: Some(2),
        enabled: true,
        data_master: true,
        writes: [true, false, false, false, false, false, false, false],
        incoming: [0x24; 8],
    };
    port.begin_edge(
        write,
        PhaseLevels {
            data_master: true,
            ..QUIET
        },
        8,
        &lines,
    )
    .unwrap();
    let pending = port.clone();
    assert!(port.begin_edge(write, QUIET, 0, &lines).is_err());
    assert_eq!(port, pending);
    for _ in 0..4 {
        assert_bus(&port, [0x5a; 8], [127; 8]);
        assert_eq!(lines[2], [0x5a; 8]);
    }
    port.finish_edge(&mut lines).unwrap();
    assert_eq!(lines[2], [0x24, 0x5a, 0x5a, 0x5a, 0x5a, 0x5a, 0x5a, 0x5a]);
    assert_bus(&port, [0x5a; 8], [127; 8]);
    assert_eq!(port.rows, [Some(1), None]);
    let settled = port.clone();
    assert!(port.finish_edge(&mut lines).is_err());
    assert_eq!(port, settled);
    commit(&mut port, &mut lines, 3);
    assert_bus(&port, [0x33; 8], [127; 8]);
    commit(&mut port, &mut lines, 40);
    assert_bus(&port, [0x12; 8], [127; 8]);
    commit(&mut port, &mut lines, 41);
    assert_bus(&port, [0x12; 8], [127; 8]);
}

fn unknown_recovery() {
    let mut port = LinePort::legacy_unknown();
    let mut lines = [[0; 8]; 40];
    lines[2] = [0x11; 8];
    commit(&mut port, &mut lines, 2);
    commit(&mut port, &mut lines, 41);
    assert_bus(&port, [0; 8], [0x6e; 8]);
    commit(&mut port, &mut lines, 3);
    commit(&mut port, &mut lines, 40);
    assert_bus(&port, [0; 8], [127; 8]);
    assert_eq!(port.parity[1].known, WORD_MASK);
    let mut zero = LinePort::legacy_unknown();
    commit(&mut zero, &mut lines, 0);
    commit(&mut zero, &mut lines, 41);
    assert_bus(&zero, [0; 8], [127; 8]);
    let mut unknown = LinePort::legacy_unknown();
    let before = unknown.clone();
    let old_lines = lines;
    assert!(
        unknown
            .begin_edge(
                PortPins {
                    address: None,
                    enabled: true,
                    data_master: true,
                    writes: [true; 8],
                    incoming: [7; 8],
                },
                PhaseLevels {
                    data_master: true,
                    ..QUIET
                },
                16,
                &lines
            )
            .is_err()
    );
    assert_eq!(unknown, before);
    assert_eq!(lines, old_lines);
    unknown.begin_edge(pins(None), QUIET, 0, &lines).unwrap();
    unknown.finish_edge(&mut lines).unwrap();
    assert_eq!(unknown.common, ReadWord::UNKNOWN);
    unknown
        .begin_edge(
            pins(Some(0)),
            PhaseLevels {
                data_slave: true,
                ..QUIET
            },
            8,
            &lines,
        )
        .unwrap();
    unknown.finish_edge(&mut lines).unwrap();
    assert_eq!(unknown.rows, [None, None]);
    unknown
        .begin_edge(
            pins(Some(0)),
            PhaseLevels {
                data_master: true,
                ..QUIET
            },
            8,
            &lines,
        )
        .unwrap();
    unknown.finish_edge(&mut lines).unwrap();
    assert_eq!(unknown.rows, [Some(1), None]);
    unknown
        .begin_edge(
            pins(Some(0)),
            PhaseLevels {
                data_slave: true,
                ..QUIET
            },
            16,
            &lines,
        )
        .unwrap();
    unknown.finish_edge(&mut lines).unwrap();
    assert_eq!(unknown.rows, [Some(1), Some(1)]);
}

fn domain_half_schedule() {
    let mut render = Renderer::software_origin(false);
    let raster = render.fixture_raster();
    raster.lines[2] = [0x5a; 8];
    raster.display[1] = 2;
    raster.horizontal[1] = 1;
    raster.port.rows = [Some(3), Some(2)];
    raster.positions[1].address = 2;
    raster.positions[1].valid = true;
    raster.writes[0] = true;
    let dirty = Domain::with_render_half_schedule(render, |render, half, finished| {
        let raster = render.fixture_raster();
        let (value, known) = raster.port.bus(true).lanes();
        match (half, finished) {
            (2, false) => {
                assert_eq!((value, known), ([0; 8], [0; 8]));
                assert_eq!(raster.lines[2], [0x5a; 8]);
            }
            (2, true) | (3, false) => {
                assert_eq!((value, known), ([0x5a; 8], [127; 8]));
                assert_eq!(
                    raster.lines[2],
                    [0, 0x5a, 0x5a, 0x5a, 0x5a, 0x5a, 0x5a, 0x5a]
                );
                assert_eq!(raster.port.rows, [Some(61), Some(2)]);
            }
            (3, true) | (4, false) => {
                assert_eq!(value, [0, 0x5a, 0x5a, 0x5a, 0x5a, 0x5a, 0x5a, 0x5a]);
                assert_eq!(known, [127; 8]);
                assert_eq!(raster.port.rows, [Some(61), Some(2)]);
            }
            (4, true) => {
                assert_eq!(value, [0, 0x5a, 0x5a, 0x5a, 0x5a, 0x5a, 0x5a, 0x5a]);
                assert_eq!(known, [127; 8]);
                assert_eq!(raster.port.rows, [Some(61), Some(61)]);
                (raster.output, raster.port.output_known) = raster.port.bus(true).lanes();
                assert_eq!(
                    raster
                        .load_output(1, 0, true, RenderTiming::software_origin(), QUIET)
                        .unwrap(),
                    0x5a
                );
            }
            _ => unreachable!(),
        }
    })
    .unwrap();
    assert_eq!(dirty, [true; 3]);
}

fn consumers() {
    let mut raster = SpriteRaster::software_origin();
    raster.output = [0; 8];
    raster.port.output_known = [0; 8];
    let timing = RenderTiming::software_origin();
    assert!(raster.occupied(0, true, timing, QUIET).is_err());
    raster.output[0] = 2;
    raster.port.output_known[0] = 2;
    assert!(raster.occupied(0, true, timing, QUIET).unwrap());
    raster.output[0] = 0;
    assert_eq!(
        raster.load_output(0, 127, true, timing, QUIET).unwrap(),
        127
    );
    assert!(raster.load_output(0, 0, true, timing, QUIET).is_err());
    raster.write_disabled = true;
    raster.prepare_writes([15; 8], true, timing, QUIET).unwrap();
    assert_eq!(raster.writes, [false; 8]);
    assert!(!raster.collision[0]);
    raster.write_disabled = false;
    raster.clear = true;
    raster.prepare_writes([0; 8], true, timing, QUIET).unwrap();
    assert_eq!(raster.writes, [true; 8]);
    assert!(!raster.collision[0]);
    assert!(raster.prepare_writes([15; 8], true, timing, QUIET).is_err());
}

fn encoded(raster: &SpriteRaster) -> Vec<u8> {
    let mut w = StateWriter::new();
    raster.write_state(&mut w);
    raster.write_line_tail(&mut w).unwrap();
    w.into_bytes()
}

fn decoded(bytes: &[u8]) -> anyhow::Result<SpriteRaster> {
    let mut r = StateReader::new(bytes);
    let mut raster = SpriteRaster::read_state(&mut r)?;
    raster.read_line_tail(&mut r)?;
    assert!(r.is_exhausted());
    Ok(raster)
}

fn legacy_tail() {
    let mut legacy = SpriteRaster::software_origin();
    legacy.output = [0x75; 8];
    let mut w = StateWriter::new();
    legacy.write_state(&mut w);
    let prefix = w.into_bytes();
    assert_eq!(encoded(&legacy), prefix);
    let copy = decoded(&prefix).unwrap();
    assert_eq!(copy, legacy);
    assert_eq!(encoded(&copy), prefix);
    assert_eq!(copy.port.common, ReadWord::UNKNOWN);
    let mut partial = copy;
    partial.port.common = ReadWord { value: 1, known: 3 };
    partial.port.rows = [Some(1), None];
    (partial.output, partial.port.output_known) = partial.port.bus(true).lanes();
    let bytes = encoded(&partial);
    assert_eq!(bytes.len(), prefix.len() + 73);
    assert_eq!(decoded(&bytes).unwrap(), partial);
    assert_eq!(encoded(&decoded(&bytes).unwrap()), bytes);
    let mut bad = bytes.clone();
    bad[prefix.len() + 12] = 4;
    assert!(decoded(&bad).is_err());
    let mut duplicate = bytes.clone();
    duplicate.extend_from_slice(&bytes[prefix.len()..]);
    assert!(decoded(&duplicate).is_err());
    assert!(decoded(&bytes[..bytes.len() - 1]).is_err());
    let mut in_progress = partial;
    in_progress
        .port
        .begin_edge(pins(Some(0)), QUIET, 0, &in_progress.lines)
        .unwrap();
    assert!(
        in_progress
            .write_line_tail(&mut StateWriter::new())
            .is_err()
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn line_port_edge_history_and_legacy_tail() {
    edge_history();
    domain_half_schedule();
    unknown_recovery();
    consumers();
    legacy_tail();
}
