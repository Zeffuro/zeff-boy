use super::super::domain::RenderGeometry;
use super::*;
use crate::hardware::vdp::state::{StateReader, StateWriter};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct PixelTag {
    pub field: u64,
    pub x: u16,
    pub y: u16,
    pub wide: bool,
    pub height: u16,
    pub mode5: bool,
    pub horizontal_active: bool,
    pub vertical_active: bool,
}

impl PixelTag {
    pub fn rectangular(self) -> bool {
        self.y < self.height && self.x < if self.wide { 320 } else { 256 }
    }
    pub fn visible(self) -> bool {
        self.horizontal_active && self.vertical_active && self.rectangular()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct VideoPixel {
    pub tag: PixelTag,
    pub rgb: DacDrive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Raster {
    field: u64,
    horizontal_active: bool,
    vertical_active: bool,
    start: u16,
    row: u16,
    mode5: bool,
    source: Option<PixelTag>,
    display: [[Option<PixelTag>; 8]; 2],
    active: [[Option<PixelTag>; 3]; 2],
    delayed: Option<PixelTag>,
    decoded: Option<PixelTag>,
    select: [Option<PixelTag>; 2],
    output: Option<PixelTag>,
}

impl Raster {
    pub fn software_origin() -> Self {
        Self {
            field: 0,
            horizontal_active: false,
            vertical_active: false,
            start: 0,
            row: 0,
            mode5: true,
            source: None,
            display: [[None; 8]; 2],
            active: [[None; 3]; 2],
            delayed: None,
            decoded: None,
            select: [None; 2],
            output: None,
        }
    }

    pub fn clock(
        &mut self,
        phases: PhaseLevels,
        geometry: RenderGeometry,
        mode5: bool,
    ) -> Result<()> {
        if geometry.vertical_active && !self.vertical_active {
            self.field = self
                .field
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("raster field overflow"))?;
        }
        if geometry.horizontal_active && !self.horizontal_active {
            self.start = geometry.horizontal;
            self.row = geometry.vertical;
        }
        self.horizontal_active = geometry.horizontal_active;
        self.vertical_active = geometry.vertical_active;
        if phases.horizontal_master {
            let x = geometry.horizontal.wrapping_sub(self.start) & 511;
            let tag = Some(PixelTag {
                field: self.field,
                x,
                y: self.row,
                wide: geometry.wide,
                height: geometry.height,
                mode5: geometry.mode5,
                horizontal_active: geometry.horizontal_active,
                vertical_active: geometry.vertical_active,
            });
            let display = self.display[1];
            self.display[0][1..].copy_from_slice(&display[..7]);
            self.display[0][0] = tag;
            let active = self.active[1];
            self.active[0][1..].copy_from_slice(&active[..2]);
            self.active[0][0] = self.delayed;
            self.select[0] = self.decoded;
        }
        if phases.horizontal_slave {
            self.display[1] = self.display[0];
            self.active[1] = self.active[0];
            self.select[1] = self.select[0];
        }
        self.decoded = self.active[1][2];
        self.output = self.select[1];
        self.mode5 = mode5;
        self.source = Some(self.source_tag(geometry));
        self.delayed = if mode5 {
            self.display[1][7]
        } else {
            self.source
        };
        Ok(())
    }

    fn source_tag(&self, geometry: RenderGeometry) -> PixelTag {
        PixelTag {
            field: self.field,
            x: geometry.horizontal.wrapping_sub(self.start) & 511,
            y: self.row,
            wide: geometry.wide,
            height: geometry.height,
            mode5: geometry.mode5,
            horizontal_active: geometry.horizontal_active,
            vertical_active: geometry.vertical_active,
        }
    }

    pub fn validate_source(&self, geometry: RenderGeometry) -> Result<()> {
        ensure!(
            self.mode5 == geometry.mode5,
            "raster mux mode differs from input"
        );
        ensure!(
            self.horizontal_active == geometry.horizontal_active
                && self.vertical_active == geometry.vertical_active,
            "raster source edge witness differs"
        );
        if self.source.is_some() {
            ensure!(
                self.source == Some(self.source_tag(geometry)),
                "stale direct raster source"
            );
        } else {
            ensure!(
                self.field == 0 && self.display == [[None; 8]; 2],
                "missing raster source"
            );
        }
        Ok(())
    }

    pub fn output(&self) -> Option<PixelTag> {
        self.output
    }
    pub fn field(&self) -> u64 {
        self.field
    }

    pub fn write_state(&self, w: &mut StateWriter) {
        w.write_u64(self.field);
        w.write_bool(self.horizontal_active);
        w.write_bool(self.vertical_active);
        w.write_u16(self.start);
        w.write_u16(self.row);
        w.write_bool(self.mode5);
        write_tag(w, self.source);
        for tag in self
            .display
            .into_iter()
            .flatten()
            .chain(self.active.into_iter().flatten())
            .chain([self.delayed, self.decoded])
            .chain(self.select)
            .chain([self.output])
        {
            write_tag(w, tag);
        }
    }

    pub fn read_state(r: &mut StateReader<'_>) -> Result<Self> {
        let mut state = Self::software_origin();
        state.field = r.read_u64()?;
        state.horizontal_active = r.read_bool()?;
        state.vertical_active = r.read_bool()?;
        state.start = r.read_u16()?;
        state.row = r.read_u16()?;
        state.mode5 = r.read_bool()?;
        state.source = read_tag(r)?;
        ensure!(
            state.source.is_none_or(|tag| tag.field == state.field),
            "direct raster field differs"
        );
        for tag in state
            .display
            .iter_mut()
            .flatten()
            .chain(state.active.iter_mut().flatten())
            .chain([&mut state.delayed, &mut state.decoded])
            .chain(state.select.iter_mut())
            .chain([&mut state.output])
        {
            *tag = read_tag(r)?;
            ensure!(
                tag.is_none_or(|tag| tag.field <= state.field),
                "raster tag exceeds field frontier"
            );
        }
        ensure!(
            state.start < 512 && state.row < 512,
            "invalid raster source position"
        );
        ensure!(
            state.delayed
                == (if state.mode5 {
                    state.display[1][7]
                } else {
                    state.source
                })
                && state.decoded == state.active[1][2]
                && state.output == state.select[1],
            "inconsistent raster delay history"
        );
        Ok(state)
    }
}

pub(super) fn write_tag(w: &mut StateWriter, tag: Option<PixelTag>) {
    w.write_bool(tag.is_some());
    if let Some(tag) = tag {
        w.write_u64(tag.field);
        w.write_u16(tag.x);
        w.write_u16(tag.y);
        w.write_bool(tag.wide);
        w.write_u16(tag.height);
        w.write_bool(tag.mode5);
        w.write_bool(tag.horizontal_active);
        w.write_bool(tag.vertical_active);
    }
}

pub(super) fn read_tag(r: &mut StateReader<'_>) -> Result<Option<PixelTag>> {
    if !r.read_bool()? {
        return Ok(None);
    }
    let tag = PixelTag {
        field: r.read_u64()?,
        x: r.read_u16()?,
        y: r.read_u16()?,
        wide: r.read_bool()?,
        height: r.read_u16()?,
        mode5: r.read_bool()?,
        horizontal_active: r.read_bool()?,
        vertical_active: r.read_bool()?,
    };
    ensure!(
        (tag.field != 0 || !tag.vertical_active)
            && tag.y < 512
            && tag.x < 512
            && matches!((tag.mode5, tag.height), (false, 192) | (true, 224 | 240)),
        "invalid raster tag"
    );
    Ok(Some(tag))
}
