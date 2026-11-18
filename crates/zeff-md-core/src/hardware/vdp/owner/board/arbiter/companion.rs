use super::*;

impl Arbiter {
    pub(crate) fn z_to_main(&mut self) {
        let i = self.input;
        self.w[143] = self.w[220] || i.sound_address & 0x8000 == 0;
        self.w[185] = self.w[86] || self.w[220];
        self.w[188] = self.w[185] && self.w[143];
        self.ff(34, i.zclk, i.delayed[1]);
        self.w[182] = self.w[188] && self.p[34][1];
        self.w[255] = !(i.dtack || self.w[79]);
        self.w[258] = !(self.w[255] || self.w[182]);
        self.wait_level = !self.w[258];
        self.w[78] = !self.w[79] || self.w[182] || !self.reset_released;
        self.w[79] = self.p[21][1] || self.w[182] || !self.reset_released;
        self.ff(10, i.vclk, self.w[78]);
        self.br_level = self.p[10][1];
        self.ff(28, i.vclk, self.w[79]);
        self.w[111] = self.p[28][1] || self.w[182];
        self.ff(22, i.vclk, self.w[111]);
        self.w[77] = self.p[22][1] || self.w[182];
        self.ff(18, i.vclk, self.w[77]);
        self.w[50] = self.w[77] || i.sound_read_level;
        self.w[51] = self.p[18][1] || i.sound_write_level;
        self.w[53] = self.w[50] && self.w[51];
        self.upper_level = self.w[53] || i.sound_address & 1 != 0;
        self.lower_level = self.w[53] || i.sound_address & 1 == 0;
        self.as_level = self.w[77];
        self.w[149] = !self.w[146];
        self.w[76] = !self.reset_released || self.p[21][1];
        self.z_to_v = self.w[76];
        self.w[106] = self.w[76];
        self.w[175] = !i.address_strobe;
        self.w[176] = !i.bgack;
        self.w[174] = self.w[175] || self.w[176] || self.w[182] || i.bg;
        self.w[178] = self.w[174] && self.w[79];
        self.ff(21, !i.vclk, self.w[178]);
        self.w[146] = self.w[76];
        self.w[268] = !self.w[146];
        self.read_direction = self.w[146];
        self.strobe_direction = !self.w[268];
        self.bgack_level = !self.w[149];
        self.w[45] = self.w[46] && self.z_to_v;
        self.w[46] = self.w[45] || i.bgack;
        self.w[48] = !self.w[45];
    }

    pub(crate) fn reset_counter(&mut self) {
        self.w[16] = !(self.d[33][2] || self.w[346]);
        self.rf(60, !self.w[16], self.d[69][1], self.reset_sync);
        self.w[334] = !(self.d[60][1] || !self.d[69][1]);
        self.rf(68, !self.w[16], !self.d[68][1], self.p[32][1]);
        self.rf(71, !self.d[68][1], !self.d[71][1], self.p[32][1]);
        self.rf(72, !self.d[71][1], !self.d[72][1], self.p[32][1]);
        self.rf(76, !self.d[72][1], !self.d[76][1], self.p[32][1]);
        self.w[362] = self.w[363] || self.d[76][1];
        self.rf(63, !self.w[362], !self.d[63][1], self.p[32][1]);
        self.rf(52, !self.d[63][1], !self.d[52][1], self.p[32][1]);
        self.rf(65, !self.d[52][1], !self.d[65][1], self.p[32][1]);
        self.rf(67, !self.d[65][1], !self.d[67][1], self.p[32][1]);
        self.rf(74, !self.d[67][1], !self.d[74][1], self.reset_sync);
        self.sf(56, self.d[74][1], self.va[3], self.w[332]);
        self.rf(57, self.d[74][1], self.reset_sync, self.reset_sync);
        self.rf(58, self.d[74][1], self.d[57][1], self.reset_sync);
        self.rf(69, self.d[74][1], false, self.reset_sync);
        self.w[328] = self.d[58][1] && !self.w[334];
    }

    pub(crate) fn sound_clock(&mut self) {
        let i = self.input;
        self.w[113] = i.sound_io_level || i.m3 || !i.m1;
        self.ff(29, i.zclk, i.delayed[3]);
        self.w[112] = self.w[113] && self.p[29][1];
        self.ff(27, i.zclk, i.delayed[0]);
        self.ff(30, i.zclk, self.p[27][1]);
        self.w[199] = self.p[30][1];
        self.w[207] = self.w[199];
        self.ff(44, !i.zclk, self.w[207]);
        self.w[220] = i.sound_memory_level || !self.p[44][1];
    }

    pub(crate) fn main_to_z(&mut self) {
        let i = self.input;
        self.w[63] = !(self.w[99] && !self.w[122] && !i.upper);
        self.w[12] = !i.read || self.w[63];
        self.w[36] = self.w[63] || i.read;
        self.rf(14, self.w[36], self.data8, self.p[32][1]);
        self.w[33] = i.sound_bus_acknowledge_level;
        self.w[34] = self.w[33] || !self.d[14][1];
        self.w[257] = !(i.lower && i.upper);
        self.ff(59, i.vclk, self.w[257]);
        self.w[331] = self.w[257] && !self.p[66][1] && self.p[59][1];
        self.w[66] = self.w[119] || i.address_strobe;
        self.w[354] = !(self.w[339] && self.p[70][1]);
        self.ff(75, !i.vclk, self.w[354]);
        self.w[348] = !(self.w[339] && self.p[70][1] && !self.p[75][1]);
        self.ff(66, !i.vclk, self.w[348]);
        self.ff(73, i.vclk, i.address_strobe);
        self.w[344] = self.p[66][1] || i.address_strobe || self.p[73][1];
        self.ff(64, i.vclk, self.w[344]);
        self.w[341] = !(!self.p[64][1] && self.w[336]);
        self.w[134] = self.w[66] || self.w[341];
        self.v_to_z = self.w[119] || self.w[33];
        self.w[308] = !(self.w[287] || self.w[343]);
        self.w[307] = !(self.w[308] || !self.p[32][1]);
        self.w[272] = !(self.d[47][3] || i.address_strobe);
        self.w[273] = !(self.w[272] || self.w[307]);
        self.w[164] = !(self.w[34] && self.w[166]);
        self.sf(47, !i.vclk, self.w[273], self.w[164]);
        self.w[339] = !(i.address_strobe && !self.d[47][1]);
        self.ff(70, !i.vclk, self.w[339]);
    }
}
