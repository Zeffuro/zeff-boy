use super::*;
use crate::hardware::vdp::state::{StateReader, StateWriter};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct PalettePixel {
    pub index: u8,
    pub palette: u8,
    pub priority: bool,
}

impl PalettePixel {
    fn packed(self) -> Result<u8> {
        ensure!(
            self.index < 16 && self.palette < 4,
            "invalid VDP palette pixel"
        );
        Ok(self.index | self.palette << 4 | u8::from(self.priority) << 6)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct PaletteSignals {
    pub planes: [PalettePixel; 2],
    pub sprite: PalettePixel,
    pub priorities: [bool; 3],
    pub nonzero: [bool; 3],
    pub sprite_palette_three: bool,
    pub sprite_index_fourteen: bool,
    pub sprite_index_fifteen: bool,
    pub display_active: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Palette {
    selected: u8,
    select: [[bool; 2]; 4],
    decoded: [bool; 4],
    display_delay: [u8; 2],
    active_delay: [u8; 2],
    delayed_active: bool,
    special_sprite: bool,
    highlight: bool,
    shadow: bool,
    mode5: bool,
    direct_active: bool,
    raster: Raster,
}

impl Palette {
    pub fn validate_geometry(&self, geometry: super::super::domain::RenderGeometry) -> Result<()> {
        self.raster.validate_source(geometry)
    }
    pub fn software_origin() -> Self {
        Self {
            selected: 0,
            select: [[false; 2]; 4],
            decoded: [false; 4],
            display_delay: [0; 2],
            active_delay: [0; 2],
            delayed_active: false,
            special_sprite: false,
            highlight: false,
            shadow: false,
            mode5: true,
            direct_active: false,
            raster: Raster::software_origin(),
        }
    }

    pub fn address(&self) -> u8 {
        self.selected & 63
    }
    pub fn intensity(&self) -> (bool, bool) {
        (self.highlight, self.shadow)
    }
    pub fn tag(&self) -> Option<PixelTag> {
        self.raster.output()
    }
    pub fn field(&self) -> u64 {
        self.raster.field()
    }

    pub fn validate_input(&self, mode5: bool, display_active: bool) -> Result<()> {
        ensure!(
            (self.mode5, self.direct_active) == (mode5, display_active),
            "stale VDP palette input provenance"
        );
        Ok(())
    }

    pub fn evaluate(
        &mut self,
        phases: PhaseLevels,
        input: PaletteSignals,
        registers: [u8; 24],
        geometry: super::super::domain::RenderGeometry,
    ) -> Result<()> {
        let mode5 = registers[1] & 4 != 0;
        self.raster.clock(phases, geometry, mode5)?;
        let colors = [
            input.planes[0].packed()?,
            input.sprite.packed()?,
            input.planes[1].packed()?,
            (registers[7] & 15) | if mode5 { registers[7] & 48 } else { 16 },
        ];
        if phases.horizontal_master {
            for (pair, decoded) in self.select.iter_mut().zip(self.decoded) {
                pair[0] = decoded;
            }
            self.display_delay[0] = (self.display_delay[1] << 1) | u8::from(input.display_active);
            self.active_delay[0] =
                ((self.active_delay[1] << 1) | u8::from(self.delayed_active)) & 7;
        }
        if phases.horizontal_slave {
            for pair in &mut self.select {
                pair[1] = pair[0];
            }
            self.display_delay[1] = self.display_delay[0];
            self.active_delay[1] = self.active_delay[0];
        }
        let intensity = registers[12] & 8 != 0 && mode5;
        let operator = intensity && input.sprite_palette_three && self.special_sprite;
        let [a, sprite, b] = input.priorities;
        let first = if a { sprite } else { !b };
        let second = !a && sprite && b;
        let third = a && !sprite && !b;
        let fourth = !a && !sprite && b;
        let fifth = a && !sprite && b;
        let absent_sprite = operator || !input.nonzero[1];
        let absent_a = !input.nonzero[0] && (mode5 || input.nonzero[1]);
        let absent_b = !mode5 || !input.nonzero[2];
        let active = self.active_delay[1] & 4 != 0;
        let sprite_choice = (absent_a && third)
            || (fourth && absent_b)
            || (absent_a && absent_b && fifth)
            || first
            || second;
        let sprite_present = sprite_choice && input.nonzero[1] && active;
        let selected_operator = sprite_present && operator;
        let a_choice = (absent_sprite && fourth && absent_b)
            || (absent_sprite && absent_b && second)
            || (absent_sprite && first)
            || fifth
            || third;
        let b_choice = (absent_a && absent_sprite && third)
            || (absent_a && fifth)
            || (absent_sprite && second)
            || (absent_a && absent_sprite && first)
            || fourth;
        self.decoded = [
            a_choice && !absent_a && active,
            sprite_present && !operator,
            b_choice && !absent_b && active,
            absent_a && absent_sprite && absent_b || !active,
        ];
        let low_priority_all = intensity && !a && !sprite && !b;
        let low_priority_sprite = intensity && !a && sprite && !b;
        let ordinary = !(low_priority_all || low_priority_sprite);
        self.highlight = ordinary && input.sprite_index_fourteen && selected_operator;
        self.shadow = ((selected_operator && input.sprite_index_fifteen)
            || (!input.sprite_index_fourteen && low_priority_all)
            || (!ordinary && !self.decoded[1] && !input.sprite_index_fourteen))
            && self.active_delay[1] & 4 != 0;
        self.special_sprite = input.sprite_index_fourteen || input.sprite_index_fifteen;
        let count = self.select.iter().filter(|pair| pair[1]).count();
        if count != 0 {
            self.selected = 127;
            for (pair, color) in self.select.iter().zip(colors) {
                if pair[1] {
                    self.selected &= color;
                }
            }
        }
        self.mode5 = mode5;
        self.direct_active = input.display_active;
        self.delayed_active = if mode5 {
            self.display_delay[1] & 128 != 0
        } else {
            self.direct_active
        };
        Ok(())
    }

    pub fn write_state(&self, writer: &mut StateWriter) {
        writer.write_u8(self.selected);
        for pair in self.select {
            for bit in pair {
                writer.write_bool(bit);
            }
        }
        for bit in self.decoded {
            writer.write_bool(bit);
        }
        writer.write_bytes(&self.display_delay);
        writer.write_bytes(&self.active_delay);
        writer.write_bool(self.delayed_active);
        writer.write_bool(self.special_sprite);
        writer.write_bool(self.highlight);
        writer.write_bool(self.shadow);
        writer.write_bool(self.mode5);
        writer.write_bool(self.direct_active);
        self.raster.write_state(writer);
    }

    pub fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        let selected = reader.read_u8()?;
        let mut select = [[false; 2]; 4];
        for pair in &mut select {
            for bit in pair {
                *bit = reader.read_bool()?;
            }
        }
        let mut decoded = [false; 4];
        for bit in &mut decoded {
            *bit = reader.read_bool()?;
        }
        let display_delay = [reader.read_u8()?, reader.read_u8()?];
        let active_delay = [reader.read_u8()?, reader.read_u8()?];
        let value = Self {
            selected,
            select,
            decoded,
            display_delay,
            active_delay,
            delayed_active: reader.read_bool()?,
            special_sprite: reader.read_bool()?,
            highlight: reader.read_bool()?,
            shadow: reader.read_bool()?,
            mode5: reader.read_bool()?,
            direct_active: reader.read_bool()?,
            raster: Raster::read_state(reader)?,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.selected < 128 && self.active_delay.into_iter().all(|value| value < 8),
            "invalid VDP palette pipeline"
        );
        ensure!(
            self.decoded.into_iter().filter(|bit| *bit).count() <= 1,
            "invalid VDP palette decoder"
        );
        for phase in 0..2 {
            ensure!(
                self.select.iter().filter(|pair| pair[phase]).count() <= 1,
                "invalid VDP palette selection"
            );
        }
        ensure!(
            self.delayed_active
                == if self.mode5 {
                    self.display_delay[1] & 128 != 0
                } else {
                    self.direct_active
                },
            "invalid delayed VDP display gate"
        );
        Ok(())
    }
}
