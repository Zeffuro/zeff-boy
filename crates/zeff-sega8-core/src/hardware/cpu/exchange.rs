use super::*;

impl Cpu {
    pub(super) fn exchange_af_with_shadow(&mut self) {
        std::mem::swap(&mut self.regs.a, &mut self.shadow.a);
        std::mem::swap(&mut self.regs.f, &mut self.shadow.f);
    }

    pub(super) fn exchange_shadow_registers(&mut self) {
        std::mem::swap(&mut self.regs.b, &mut self.shadow.b);
        std::mem::swap(&mut self.regs.c, &mut self.shadow.c);
        std::mem::swap(&mut self.regs.d, &mut self.shadow.d);
        std::mem::swap(&mut self.regs.e, &mut self.shadow.e);
        std::mem::swap(&mut self.regs.h, &mut self.shadow.h);
        std::mem::swap(&mut self.regs.l, &mut self.shadow.l);
    }

    pub(super) fn exchange_de_hl(&mut self) {
        std::mem::swap(&mut self.regs.d, &mut self.regs.h);
        std::mem::swap(&mut self.regs.e, &mut self.regs.l);
    }

    pub(super) fn exchange_stack_with_hl(&mut self, bus: &mut Bus) {
        let stack_value = self.read_mem_u16(bus, self.regs.sp);
        self.write_mem_u16(bus, self.regs.sp, self.regs.hl());
        self.regs.set_hl(stack_value);
    }
}
