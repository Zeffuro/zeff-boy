use super::domain::RenderContext;
use crate::hardware::vdp::control_core::RegisterStage;
use crate::hardware::vdp::state::{StateReader, StateWriter};
use anyhow::{Result, ensure};

#[derive(Clone, Copy)]
#[repr(u8)]
pub(super) enum Event {
    NameA,
    NameB,
    PatternA,
    PatternB,
    SpriteAttribute,
    SpriteScan,
    Scroll,
    VerticalStep,
    Odd,
    EndN,
    Zero,
    Column,
    PixelsA,
    PixelsB,
    SpriteSerial,
    BlankSprite,
    PixelResetB,
    PixelResetA,
    DisplayStart,
    DisplayEnd,
    SpriteAddress,
    Mode4SpriteSerial,
    Mode4SpritePhase,
    Mode4SpritePattern,
}

const COUNT: u32 = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RenderTiming {
    gates: [u32; 2],
    horizontal_active: bool,
    scan_end: bool,
    display_active: bool,
    memory_pixels: bool,
}

impl RenderTiming {
    pub fn software_origin() -> Self {
        Self {
            gates: [0; 2],
            horizontal_active: false,
            scan_end: false,
            display_active: false,
            memory_pixels: false,
        }
    }

    pub fn gate(self, event: Event) -> bool {
        self.gates[1] & (1 << event as u8) != 0
    }

    pub fn scan_end(self) -> bool {
        self.scan_end
    }
    pub fn display_active(self) -> bool {
        self.display_active
    }
    pub fn memory_pixels(self) -> bool {
        self.memory_pixels
    }
    pub fn horizontal_active(self) -> bool {
        self.horizontal_active
    }

    pub fn evaluate(&mut self, context: RenderContext) {
        let regs = context.registers.at(RegisterStage::Control);
        let wide = regs[12] & 1 != 0;
        let m5 = regs[1] & 4 != 0;
        if context.phases.horizontal_master {
            let h = context.before_hv.h as u16;
            let active = context.memory_active_before_hv;
            let attribute_common = matches!(h, 484 | 460 | 458)
                || any(h, &[(505, 344), (509, 360), (505, 352), (505, 336)]);
            let attribute_narrow = h == 290 || any(h, &[(509, 292), (505, 280), (505, 272)]);
            let scan_common = h == 486 || any(h, &[(503, 498), (505, 488), (509, 480), (497, 464)]);
            let attribute = scan_common && h != 486
                || if wide {
                    attribute_common || h == 364 || any(h, &[(505, 328), (509, 324)])
                } else {
                    attribute_narrow || any(h, &[(505, 264), (509, 260)])
                };
            let scan = scan_common
                || matches!(h, 502 | 508 | 500 | 504 | 496)
                || if wide {
                    attribute_common || h == 364
                } else {
                    attribute_narrow
                };
            let events = [
                (
                    Event::NameA,
                    active
                        && (h & 271 == 0
                            || !m5 && h & 271 == 8
                            || m5 && (h == 496 || wide && h & 463 == 256)),
                ),
                (
                    Event::NameB,
                    active && (h & 271 == 8 || m5 && (h == 504 || wide && h & 463 == 264)),
                ),
                (
                    Event::PatternA,
                    active
                        && (h & 263 == 4
                            || m5 && (matches!(h, 508 | 500) || wide && h & 455 == 260)),
                ),
                (
                    Event::PatternB,
                    active
                        && (h & 263 == 6
                            || m5 && (matches!(h, 510 | 502) || wide && h & 455 == 262)),
                ),
                (
                    Event::SpriteAttribute,
                    active
                        && if m5 {
                            attribute
                        } else {
                            any(h, &[(509, 484), (509, 472), (509, 280), (509, 268)])
                        },
                ),
                (
                    Event::SpriteScan,
                    active
                        && if m5 {
                            scan
                        } else {
                            any(h, &[(279, 18), (287, 10), (497, 496)])
                        },
                ),
                (Event::Scroll, active && m5 && h == 486),
                (
                    Event::VerticalStep,
                    h != if !m5 {
                        488
                    } else if wide {
                        328
                    } else {
                        264
                    },
                ),
                (Event::Odd, h & 1 == 0),
                (Event::EndN, h != if wide { 322 } else { 258 }),
                (Event::Zero, h == 0),
                (Event::Column, h & 7 == 0),
                (Event::PixelsA, h & 15 == 7 || !m5 && h & 15 == 15),
                (Event::PixelsB, h & 15 == 15),
                (
                    Event::SpriteSerial,
                    m5 && (h & 271 == 13 || wide && h & 463 == 269),
                ),
                (
                    Event::BlankSprite,
                    !active && h & 259 == 0
                        || active
                            && !m5
                            && any(h, &[(509, 480), (509, 468), (509, 276), (509, 264)]),
                ),
                (Event::PixelResetB, h == 507),
                (Event::PixelResetA, h & 15 == 3),
                (Event::DisplayStart, h == 18 || regs[0] & 32 == 0 && h == 10),
                (Event::DisplayEnd, h == if wide { 330 } else { 266 }),
                (
                    Event::SpriteAddress,
                    active
                        && if m5 {
                            h & 271 == 10 || wide && h & 463 == 266
                        } else {
                            any(h, &[(509, 480), (509, 468), (509, 276), (509, 264)])
                        },
                ),
                (
                    Event::Mode4SpriteSerial,
                    !m5 && matches!(h, 483 | 471 | 279 | 267),
                ),
                (
                    Event::Mode4SpritePattern,
                    active && !m5 && any(h, &[(509, 488), (509, 476), (509, 284), (509, 272)]),
                ),
                (
                    Event::Mode4SpritePhase,
                    active
                        && !m5
                        && (matches!(h, 488 | 484)
                            || any(h, &[(507, 472), (503, 272), (495, 268)])),
                ),
            ];
            self.gates[0] = events.into_iter().fold(0, |bits, (event, set)| {
                bits | (u32::from(set) << event as u8)
            });
        }
        if context.phases.horizontal_slave {
            self.gates[1] =
                self.gates[0] ^ (1 << Event::VerticalStep as u8) ^ (1 << Event::Odd as u8);
            self.scan_end = !self.gate(Event::EndN);
        }
        self.display_active =
            regs[1] & 64 != 0 && self.horizontal_active && context.vertical_display_before_hv;
        self.memory_pixels = self.horizontal_active && context.memory_active_before_hv;
        if self.gate(Event::DisplayStart) {
            self.horizontal_active = true;
        } else if self.gate(Event::DisplayEnd) {
            self.horizontal_active = false;
        }
    }

    pub fn write_state(self, writer: &mut StateWriter) {
        writer.write_u8(2);
        for gates in self.gates {
            writer.write_u32(gates);
        }
        for value in [
            self.horizontal_active,
            self.scan_end,
            self.display_active,
            self.memory_pixels,
        ] {
            writer.write_bool(value);
        }
    }

    pub fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        ensure!(reader.read_u8()? == 2, "unsupported render timing state");
        let gates = [reader.read_u32()?, reader.read_u32()?];
        ensure!(
            gates.into_iter().all(|value| value < 1 << COUNT),
            "invalid render timing gates"
        );
        Ok(Self {
            gates,
            horizontal_active: reader.read_bool()?,
            scan_end: reader.read_bool()?,
            display_active: reader.read_bool()?,
            memory_pixels: reader.read_bool()?,
        })
    }
}

fn any(h: u16, terms: &[(u16, u16)]) -> bool {
    terms.iter().any(|&(mask, value)| h & mask == value)
}
