use super::*;
use crate::sound_trace::physical::{Fact, Half};

fn assign<const N: usize>(bits: &mut [Origin; N], mask: u32, source: Origin) {
    for (index, bit) in bits.iter_mut().enumerate() {
        if mask & (1 << index) != 0 {
            *bit = source;
        }
    }
}
fn coherent(bus: &Bus) -> Option<Token> {
    let mut sound = None;
    for origin in bus.address.iter().chain(&bus.data).chain(&bus.controls) {
        match *origin {
            Origin::Fixed => {}
            Origin::Sound(token) if sound.is_none_or(|prior| prior == token) => sound = Some(token),
            _ => return None,
        }
    }
    sound
}
impl Half {
    fn origin(&self, source: Source) -> Origin {
        match source {
            Source::Unknown => Origin::Unknown,
            Source::Foreign => Origin::Foreign,
            Source::ArbiterHigh => self.state.vdp.arbiter,
            Source::Sound {
                logical,
                bank,
                write,
            } => {
                let active = self.state.recent[0].filter(|active| {
                    write
                        && active.binding.external
                        && active.binding.shape == super::super::Shape::Write
                        && active.binding.address == logical
                        && active.binding.bank == bank
                });
                active
                    .and_then(|active| {
                        active.id.map(|id| {
                            Origin::Sound(Token {
                                id,
                                binding: active.binding,
                                stamp: active.stamp,
                            })
                        })
                    })
                    .unwrap_or(Origin::Unknown)
            }
        }
    }
    fn acceptance(&mut self, input: Primitive) {
        let Some(token) = coherent(&self.state.vdp.wiring) else {
            return;
        };
        if input.word as u8 != (input.word >> 8) as u8 {
            self.gap();
            return;
        }
        let candidates = [
            (
                Kind::Data,
                input.ack[0]
                    && input.main[0]
                    && input.intake[0]
                    && input
                        .load
                        .is_some_and(|(word, causes)| word == input.word && causes[0]),
            ),
            (
                Kind::ControlFirst,
                input.ack[1] && input.main[0] && input.intake[1],
            ),
            (
                Kind::ControlSecond,
                input.ack[1] && input.main[0] && input.intake[2],
            ),
            (
                Kind::DisconnectedPsg,
                input.psg[0] && !input.psg[1] && input.lanes == [true, false],
            ),
        ];
        for (kind, accepted) in candidates {
            if accepted {
                self.push_for(
                    Fact::VdpAccepted(Acceptance {
                        primitive: input.ordinal,
                        source: token.stamp,
                        binding: token.binding,
                        kind,
                        address: input.address << 1,
                        bus_address: input.bus_address,
                        word: input.word,
                        lanes: input.lanes,
                    }),
                    Some(token.id),
                );
            }
        }
    }
}
impl Capture for Half {
    fn vdp(&mut self, event: Event) {
        if !self.enabled || self.overflow {
            return;
        }
        match event {
            Event::Drive { slot, mask, source } => {
                let origin = self.origin(source);
                match slot {
                    Slot::Address => assign(&mut self.state.vdp.address, mask, origin),
                    Slot::Data => assign(&mut self.state.vdp.data, mask, origin),
                }
            }
            Event::Arbiter(source) => self.state.vdp.arbiter = self.origin(source),
            Event::Outputs => self.state.vdp.outputs = self.state.vdp.arbiter,
            Event::Resolve(mux) => {
                let state = &self.state.vdp;
                let controls = if mux.released {
                    let read = if mux.held_read {
                        state.held[0]
                    } else {
                        self.origin(mux.source)
                    };
                    let upper = if mux.held_strobes {
                        state.held[1]
                    } else {
                        state.outputs
                    };
                    let lower = if mux.held_strobes {
                        state.held[2]
                    } else {
                        state.outputs
                    };
                    [
                        read,
                        if mux.held_strobes {
                            Origin::Unknown
                        } else {
                            state.outputs
                        },
                        upper,
                        lower,
                    ]
                } else {
                    [Origin::Foreign; 4]
                };
                self.state.vdp.input = Bus {
                    address: state.address,
                    data: state.data,
                    controls,
                };
                self.state.vdp.held = [controls[0], controls[2], controls[3]];
            }
            Event::Terminal => self.state.vdp.held = [Origin::Foreign; 3],
            Event::Readback => self.state.vdp.input.data = [Origin::Foreign; 16],
            Event::Capture { dma } => {
                let state = &mut self.state.vdp;
                if dma {
                    assign(&mut state.wiring.address, 0x3fffff, Origin::Foreign);
                } else {
                    for index in 0..23 {
                        state.wiring.address[index] = if 0x73ffff & (1 << index) != 0 {
                            state.input.address[index + 1]
                        } else {
                            Origin::Fixed
                        };
                    }
                }
                state.wiring.address[23] = Origin::Fixed;
                state.wiring.data = state.input.data;
                state.wiring.controls = state.input.controls;
            }
            Event::Primitive(input) => self.acceptance(input),
        }
    }
    #[cfg(test)]
    fn vdp_raw(&mut self, event: raw::Event) {
        if self.enabled
            && let Some(raw) = &mut self.raw
        {
            raw.push(raw::Record {
                at: self.state.last,
                event,
            });
        }
    }
}
