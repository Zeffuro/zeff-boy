use crate::emulator::Emulator;
use crate::hardware::bus::Bus;

impl Emulator {
    pub fn framebuffer(&self) -> &[u8] {
        &self.bus.ppu.framebuffer[..]
    }

    pub fn framebuffer_dimensions(&self) -> (usize, usize) {
        (
            crate::hardware::ppu::SCREEN_W,
            crate::hardware::ppu::SCREEN_H,
        )
    }

    pub fn frame_ready(&self) -> bool {
        self.bus.ppu.frame_ready
    }

    pub fn clear_frame_ready(&mut self) {
        self.bus.ppu.frame_ready = false;
    }

    pub fn rom_hash(&self) -> [u8; 32] {
        self.rom_hash
    }

    pub fn rom_crc32(&self) -> u32 {
        self.rom_crc32
    }

    pub fn frame_count(&self) -> u64 {
        self.bus.ppu.frame_count + u64::from(self.bus.ppu.frame_ready)
    }

    pub fn cpu_pc(&self) -> u16 {
        self.cpu.pc
    }

    pub fn cpu_cycles(&self) -> u64 {
        self.cpu.cycles
    }

    pub fn cpu_a(&self) -> u8 {
        self.cpu.regs.a
    }

    pub fn cpu_x(&self) -> u8 {
        self.cpu.regs.x
    }

    pub fn cpu_y(&self) -> u8 {
        self.cpu.regs.y
    }

    pub fn cpu_sp(&self) -> u8 {
        self.cpu.sp
    }

    pub fn cpu_status(&self) -> u8 {
        self.cpu.regs.p.bits()
    }

    pub fn cpu_last_opcode(&self) -> u8 {
        self.cpu.last_opcode
    }

    pub fn cpu_last_step_cycles(&self) -> u64 {
        self.cpu.last_step_cycles
    }

    pub fn cpu_nmi_pending(&self) -> bool {
        self.cpu.nmi_pending
    }

    pub fn cpu_irq_line(&self) -> bool {
        self.cpu.irq_line
    }

    pub fn cpu_nmi_count(&self) -> u64 {
        self.cpu.nmi_count
    }

    pub fn cpu_irq_count(&self) -> u64 {
        self.cpu.irq_count
    }

    pub fn bus(&self) -> &Bus {
        &self.bus
    }

    pub fn bus_mut(&mut self) -> &mut Bus {
        &mut self.bus
    }

    pub fn cartridge_header(&self) -> &crate::hardware::cartridge::RomHeader {
        self.bus.cartridge.header()
    }

    pub fn cartridge_effective_mapper_label(&self) -> String {
        self.bus.cartridge.effective_mapper_label()
    }

    pub fn last_opcode_pc(&self) -> u16 {
        self.cpu.last_opcode_pc
    }

    pub fn ppu_palette_ram(&self) -> &[u8; 32] {
        &self.bus.ppu.palette_ram
    }

    pub fn ppu_oam(&self) -> &[u8; 256] {
        &self.bus.ppu.oam
    }

    pub fn ppu_nametable_ram(&self) -> &[u8; 0x1000] {
        &self.bus.ppu.nametable_ram
    }

    pub fn ppu_ctrl(&self) -> u8 {
        self.bus.ppu.regs.ctrl
    }

    pub fn ppu_mask(&self) -> u8 {
        self.bus.ppu.regs.mask
    }

    pub fn ppu_status(&self) -> u8 {
        self.bus.ppu.regs.status
    }

    pub fn ppu_scanline(&self) -> u16 {
        self.bus.ppu.scanline
    }

    pub fn ppu_dot(&self) -> u16 {
        self.bus.ppu.dot
    }

    pub fn ppu_frame_count(&self) -> u64 {
        self.bus.ppu.frame_count
    }

    pub fn ppu_in_vblank(&self) -> bool {
        self.bus.ppu.in_vblank
    }

    pub fn ppu_frame_ready(&self) -> bool {
        self.bus.ppu.frame_ready
    }

    pub fn ppu_scroll_v(&self) -> u16 {
        self.bus.ppu.v
    }

    pub fn ppu_scroll_t(&self) -> u16 {
        self.bus.ppu.t
    }

    pub fn ppu_fine_x(&self) -> u8 {
        self.bus.ppu.fine_x
    }

    pub fn ppu_tall_sprites(&self) -> bool {
        self.bus.ppu.regs.tall_sprites()
    }

    pub fn system_ram(&self) -> &[u8] {
        &self.bus.ram
    }

    pub fn chr_ram_snapshot(&mut self) -> Vec<u8> {
        let mut buf = vec![0u8; 0x2000];
        for addr in 0..0x2000u16 {
            buf[addr as usize] = self.bus.cartridge.chr_read(addr);
        }
        buf
    }

    pub fn video_ram_snapshot(&mut self) -> Vec<u8> {
        self.chr_ram_snapshot()
    }
}
