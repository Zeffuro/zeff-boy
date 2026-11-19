use super::{ActiveCore, CoreState};
use zeff_emu_common::memory::{MemoryRegionDescriptor, MemoryRegionKind, resolve_memory_region};
use zeff_md_core::{CartridgeHeader, ControllerKind, ControllerPort, Emulator, LoadConfig};

pub(super) fn load(data: &[u8], sample_rate: u32) -> anyhow::Result<Emulator> {
    anyhow::ensure!(
        data.len() <= zeff_md_core::constants::MAX_CARTRIDGE_SIZE,
        "Mega Drive cartridge exceeds the supported 4 MiB limit"
    );
    let console = data.get(0x100..0x110).unwrap_or_default();
    anyhow::ensure!(
        !console.windows(3).any(|bytes| bytes == b"32X")
            && !console.windows(2).any(|bytes| bytes == b"CD"),
        "Mega Drive expansion media is not supported"
    );
    let region = CartridgeHeader::preferred_region_for_rom(data).unwrap_or_default();
    let mut emu = Emulator::new(
        data.to_vec(),
        LoadConfig {
            region,
            concurrent_sound: true,
        },
    )
    .map_err(|error| anyhow::anyhow!("Invalid Mega Drive cartridge: {error:?}"))?;
    emu.set_sample_rate(sample_rate);
    Ok(emu)
}

pub(super) fn set_input(emu: &mut Emulator, port: ControllerPort, buttons: u8, dpad: u8) {
    let project = |value: u8, bits: &[(u8, u8)]| {
        bits.iter().fold(0, |mapped, &(source, target)| {
            mapped | ((value >> source & 1) << target)
        })
    };
    emu.input_mut().set_controller_raw(
        port,
        !(project(buttons, &[(6, 6), (0, 4), (1, 5), (3, 7)])
            | project(dpad, &[(0, 3), (1, 2), (2, 0), (3, 1)])),
    );
    emu.input_mut()
        .set_controller_extra_raw(port, !project(buttons, &[(2, 3), (4, 2), (7, 1), (5, 0)]));
}

pub(super) fn memory_regions(emu: &Emulator) -> Vec<MemoryRegionDescriptor> {
    let memory = emu.vdp().memory();
    let mut regions = vec![
        MemoryRegionDescriptor::read_only_cpu_address_space(24),
        MemoryRegionDescriptor::system_ram(emu.work_ram().len()),
        MemoryRegionDescriptor::video_ram(memory.vram().len()),
        MemoryRegionDescriptor::palette_ram(memory.cram_word_count() * size_of::<u16>()),
        MemoryRegionDescriptor::video_scroll_ram(size_of_val(memory.vsram())),
    ];
    if emu.save_ram_kind().has_ram() {
        regions.push(MemoryRegionDescriptor::save_ram(emu.save_ram_kind().size()));
    }
    regions.push(MemoryRegionDescriptor::framebuffer(emu.framebuffer().len()));
    regions
}

pub(super) fn copy_memory(
    emu: &Emulator,
    id: &str,
    out: &mut Vec<u8>,
) -> anyhow::Result<MemoryRegionDescriptor> {
    let region = resolve_memory_region(&memory_regions(emu), id)
        .ok_or_else(|| anyhow::anyhow!("unknown Mega Drive memory region '{id}'"))?;
    let data = match region.kind {
        MemoryRegionKind::SystemRam => emu.work_ram(),
        MemoryRegionKind::VideoRam => emu.vdp().memory().vram(),
        MemoryRegionKind::Framebuffer => {
            anyhow::ensure!(
                emu.framebuffer_available(),
                "Mega Drive framebuffer is unavailable"
            );
            emu.framebuffer()
        }
        MemoryRegionKind::PaletteRam | MemoryRegionKind::VideoScrollRam => {
            let words = if region.kind == MemoryRegionKind::PaletteRam {
                emu.vdp()
                    .memory()
                    .known_cram()
                    .ok_or_else(|| anyhow::anyhow!("Mega Drive CRAM contains unknown words"))?
            } else {
                emu.vdp().memory().vsram()
            };
            out.clear();
            out.extend(words.iter().flat_map(|word| word.to_be_bytes()));
            return Ok(region);
        }
        MemoryRegionKind::SaveRam => {
            *out = emu
                .dump_save_ram()
                .ok_or_else(|| anyhow::anyhow!("MD save RAM is unavailable"))?;
            return Ok(region);
        }
        _ => anyhow::bail!(
            "Mega Drive memory region '{}' is not a finite copyable region",
            region.id
        ),
    };
    out.clear();
    out.extend_from_slice(data);
    Ok(region)
}

impl CoreState {
    pub(crate) fn take_md_av_change(&mut self) -> Option<crate::api::retro_system_av_info> {
        if !matches!(self.core, ActiveCore::Md(_)) {
            return None;
        }
        let geometry = self.video_geometry();
        let current = (geometry.base_width, geometry.base_height, self.fps());
        if self.md_reported_av == Some(current) {
            return None;
        }
        self.md_reported_av = Some(current);
        Some(crate::api::retro_system_av_info {
            geometry,
            timing: crate::api::retro_system_timing {
                fps: self.fps(),
                sample_rate: f64::from(self.sample_rate),
            },
        })
    }

    pub(crate) fn set_port_device(&mut self, port: usize, device: u32) {
        if port >= self.port_device.len() {
            return;
        }
        self.port_device[port] = device;
        if let ActiveCore::Md(emu) = &mut self.core {
            let port = [ControllerPort::One, ControllerPort::Two][port];
            let kind = if device == crate::input::MD_SIX_BUTTON_DEVICE {
                ControllerKind::SixButton
            } else {
                ControllerKind::ThreeButton
            };
            emu.input_mut().set_controller_kind(port, kind);
        }
    }

    pub(crate) fn poll_md_input(&mut self) -> bool {
        let ActiveCore::Md(emu) = &mut self.core else {
            return false;
        };
        for (index, port) in [ControllerPort::One, ControllerPort::Two]
            .into_iter()
            .enumerate()
        {
            let (buttons, dpad) = if matches!(
                self.port_device[index],
                crate::api::RETRO_DEVICE_JOYPAD | crate::input::MD_SIX_BUTTON_DEVICE
            ) {
                crate::input::poll_md_joypad_port(index as u32)
            } else {
                (0, 0)
            };
            set_input(emu, port, buttons, dpad);
        }
        true
    }
}
