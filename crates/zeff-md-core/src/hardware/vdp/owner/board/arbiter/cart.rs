use super::*;

impl Arbiter {
    pub(crate) fn cart(&mut self) {
        let i = self.input;
        self.w[73] = self.p[32][1];
        self.rf(26, self.w[97], self.data8, self.p[32][1]);
        self.w[274] = !i.cas;
        self.sf(45, self.w[274], self.va[3], !self.w[223]);
        self.w[269] = self.d[45][3] && self.va[3] && self.w[274];
        self.w[248] = self.w[223] || self.w[269];
        self.w[208] = if self.d[26][1] {
            self.w[254]
        } else {
            self.w[248]
        };
        self.w[202] = self.w[208] || !self.cart22;
        self.w[170] = !self.d[26][1] || i.delayed[4];
        self.w[167] = self.w[170] && self.w[202];
        self.w[211] = i.address_strobe || self.va[3];
        self.w[72] = self.d[15][2] || self.w[43];
        self.w[132] = self.w[72] || self.w[211];
        self.w[69] = !(self.w[44] || !self.p[12][1]);
        self.w[168] = !(self.w[69] || !self.d[26][1]);
        self.w[169] = self.w[168] || self.w[211] || !self.cart22;
        self.rf(25, !i.vclk, !self.p[12][1], self.w[73]);
        self.w[101] = !(self.d[20][2] || self.d[25][1]);
        self.w[173] = self.w[101] || !self.d[26][1];
        self.w[172] = !(self.w[169] && self.w[167] && self.w[173]);
        self.ce_level = !self.w[172];
        self.w[301] = !(self.w[223] && i.delayed[6]);
        self.w[298] = self.w[301] && self.va[3];
        self.sf(46, self.w[274], self.d[46][2], self.w[298]);
        self.w[271] = self.d[46][3] || i.delayed[5];
        self.w[254] = self.w[223] || self.va[3];
        self.w[238] = i.cas || self.w[223];
        self.cart22 = !self.va[2];
        self.w[59] = i.address_strobe;
        self.w[54] = !(self.d[23][3] && self.va[3]);
        self.w[84] = !(self.d[23][2] && self.d[33][2] && self.w[356]);
        self.w[58] = self.w[54] && self.w[84];
        self.sf(17, i.vclk, self.w[58], self.w[73]);
        self.w[49] = self.d[17][3];
        self.w[74] = self.w[49];
        self.sf(20, !i.vclk, self.w[74], self.w[73]);
        self.w[71] = self.d[20][3] && self.w[74];
        self.sf(19, !i.vclk, self.w[71], self.w[73]);
        self.w[42] = !(self.d[19][3] && self.w[71]);
        self.w[44] = self.w[42] || self.va[3] || self.w[59];
        self.ff(16, i.vclk, self.w[44]);
        self.w[43] = self.w[44] || self.p[16][1];
        self.ff(11, !i.vclk, self.w[43]);
        self.w[27] = self.p[11][1] || self.w[44];
        self.w[41] = self.w[27] || self.d[15][3];
        self.ff(12, !i.vclk, self.w[41]);
        self.w[26] = !(self.d[15][2] && self.p[12][1]);
        self.w[83] = self.d[23][2] || self.va[3];
        self.w[40] = self.w[26] && self.w[83];
        self.sf(15, i.vclk, self.w[40], self.w[73]);
        self.w[70] = self.w[27] && self.w[71];
    }

    pub(crate) fn divider(&mut self) {
        let clock = self.input.vclk;
        self.w[286] = !(self.w[287] || !self.p[32][1]);
        self.w[289] = self.w[286];
        self.count(78, clock, self.w[289], false, self.w[289]);
        self.count(80, clock, self.w[289], false, self.c[78][2]);
        self.count(79, clock, self.w[289], false, self.c[80][2]);
        self.count(77, clock, self.w[289], false, self.c[79][2]);
        self.w[374] = self.c[77][1] && self.c[78][1] && self.c[79][1] && self.c[80][1];
        self.w[356] = self.w[374];
        self.w[266] = self.w[289] && self.w[356];
        self.count(48, clock, self.w[289], false, self.w[266]);
        self.count(54, clock, self.w[289], false, self.c[48][2]);
        self.count(53, clock, self.w[289], false, self.c[54][2]);
        self.count(55, clock, self.w[289], true, self.c[53][2]);
        self.w[309] = self.c[48][1] && self.c[53][1] && self.c[54][1] && self.c[55][1];
        self.w[287] = self.w[309] && self.w[356];
        self.w[183] = !(self.d[33][3] || self.d[23][1] || self.w[356] || !self.w[223]);
        self.w[283] = !(self.w[183] || self.w[287] || self.w[343]);
        self.sf(33, clock, self.w[283], self.p[32][1]);
        self.rf(23, !self.w[59], self.d[33][2], self.d[33][2]);
    }

    pub(crate) fn ram_oe(&mut self) {
        let i = self.input;
        self.w[279] = !i.bgack;
        self.w[302] = !(self.w[9] && (!self.d[50][1] || self.d[62][1]));
        self.w[299] = !self.w[302];
        self.rf(49, i.vclk, self.w[279], self.d[51][1]);
        self.rf(50, !i.vclk, self.w[322], self.w[9]);
        self.rf(51, self.w[279], self.w[336], self.w[299]);
        self.w[325] = i.cas && self.d[62][1];
        self.w[321] = !self.d[61][1] && i.output_enable;
        self.w[326] = self.w[321] || self.w[325];
        self.w[300] = !(self.w[326] && (!self.d[49][1] || self.d[50][1]));
        self.w[314] = !self.w[300];
        self.lower_oe_level = self.w[314];
        self.upper_oe_level = self.w[314];
        self.w[322] = self.d[62][1];
        self.w[342] = self.d[49][1];
        self.rf(61, !i.vclk, self.w[342], self.d[51][1]);
        self.rf(62, i.vclk, self.d[61][1], self.d[51][1]);
    }
}
