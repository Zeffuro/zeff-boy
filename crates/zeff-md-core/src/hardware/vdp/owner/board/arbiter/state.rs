use super::*;

const PLAIN: &[usize] = &[
    10, 11, 12, 13, 16, 18, 21, 22, 24, 27, 28, 29, 30, 32, 34, 44, 59, 64, 66, 70, 73, 75,
];
const DUAL: &[usize] = &[
    14, 15, 17, 19, 20, 23, 25, 26, 31, 33, 45, 46, 47, 49, 50, 51, 52, 56, 57, 58, 60, 61, 62, 63,
    65, 67, 68, 69, 71, 72, 74, 76,
];
const COUNT: &[usize] = &[48, 53, 54, 55, 77, 78, 79, 80];
const WIRES: &[usize] = &[
    9, 12, 16, 26, 27, 31, 33, 34, 36, 40, 41, 42, 43, 44, 45, 46, 48, 49, 50, 51, 53, 54, 58, 59,
    63, 65, 66, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 83, 84, 86, 90, 91, 94, 95, 96, 97, 99,
    101, 103, 106, 111, 112, 113, 118, 119, 122, 124, 126, 128, 129, 130, 131, 132, 133, 134, 137,
    142, 143, 146, 149, 164, 166, 167, 168, 169, 170, 172, 173, 174, 175, 176, 178, 182, 183, 185,
    188, 199, 200, 202, 207, 208, 211, 220, 222, 223, 238, 248, 249, 254, 255, 257, 258, 266, 268,
    269, 271, 272, 273, 274, 279, 283, 286, 287, 289, 298, 299, 300, 301, 302, 307, 308, 309, 314,
    321, 322, 325, 326, 328, 331, 332, 333, 334, 336, 339, 341, 342, 343, 344, 346, 348, 354, 356,
    362, 363, 374,
];

impl Arbiter {
    pub fn write_state(&self, writer: &mut StateWriter) {
        write_input(writer, self.input);
        write_input(writer, self.old_input);
        for delay in &self.delay {
            delay.write(writer);
        }
        for &index in WIRES {
            writer.write_bool(self.w[index]);
        }
        for &index in PLAIN {
            write_bits(writer, self.p[index]);
        }
        for &index in DUAL {
            write_bits(writer, self.d[index]);
        }
        for &index in COUNT {
            write_bits(writer, self.c[index]);
        }
        write_bits(writer, self.fc);
        write_bits(writer, self.va);
        writer.write_u16(self.sound_address_high);
        write_bits(
            writer,
            [
                self.reset_released,
                self.reset_sync,
                self.z_to_v,
                self.v_to_z,
                self.data8,
                self.cart22,
                self.br_level,
                self.bgack_level,
                self.dtack_level,
                self.wait_level,
                self.vpa_level,
                self.ce_level,
                self.lower_oe_level,
                self.upper_oe_level,
                self.ram_address14,
                self.strobe_direction,
                self.read_direction,
                self.as_level,
                self.upper_level,
                self.lower_level,
                self.bus_status_read,
            ],
        );
    }

    pub fn read_state(reader: &mut StateReader<'_>, half: u64) -> Result<Self> {
        let mut value = Self::software_origin();
        value.input = read_input(reader)?;
        value.old_input = read_input(reader)?;
        for (delay, length) in value.delay.iter_mut().zip([2, 2, 7, 2, 3, 7, 7, 2]) {
            *delay = Delay::read(reader, length)?;
        }
        for &index in WIRES {
            value.w[index] = reader.read_bool()?;
        }
        for &index in PLAIN {
            value.p[index] = read_bits(reader)?;
        }
        for &index in DUAL {
            value.d[index] = read_bits(reader)?;
        }
        for &index in COUNT {
            value.c[index] = read_bits(reader)?;
        }
        value.fc = read_bits(reader)?;
        value.va = read_bits(reader)?;
        value.sound_address_high = reader.read_u16()?;
        [
            value.reset_released,
            value.reset_sync,
            value.z_to_v,
            value.v_to_z,
            value.data8,
            value.cart22,
            value.br_level,
            value.bgack_level,
            value.dtack_level,
            value.wait_level,
            value.vpa_level,
            value.ce_level,
            value.lower_oe_level,
            value.upper_oe_level,
            value.ram_address14,
            value.strobe_direction,
            value.read_direction,
            value.as_level,
            value.upper_level,
            value.lower_level,
            value.bus_status_read,
        ] = read_bits(reader)?;
        value.validate(half)?;
        Ok(value)
    }

    pub fn validate(&self, half: u64) -> Result<()> {
        if half == 0 {
            ensure!(
                *self == Self::software_origin(),
                "invalid arbiter software origin"
            );
            return Ok(());
        }
        let cycle = half
            .checked_add(3)
            .ok_or_else(|| anyhow::anyhow!("arbiter clock overflow"))?;
        ensure!(
            self.input == self.old_input && self.input.sound_bank < 512 && self.input.m3,
            "invalid arbiter input frontier"
        );
        ensure!(
            self.delay.iter().all(|delay| delay.cycle == cycle),
            "invalid arbiter delay frontier"
        );
        for &index in DUAL {
            let cell = self.d[index];
            ensure!(
                cell[2] == !cell[1] && cell[3] == cell[1],
                "invalid arbiter dual cell"
            );
        }
        ensure!(!self.d[26][1], "unsupported MAIN DRAM memory-mode bank");
        ensure!(
            self.reset_released && self.ram_address14 == !self.va[0],
            "invalid arbiter normal-MD selection"
        );
        let address = self.input.sound_address;
        let high = if address & 0xff00 == 0x7f00 {
            0xc000
        } else {
            ((address & 0x7f00) >> 8) | self.input.sound_bank << 7
        };
        ensure!(
            self.w[86] == (address & 0xff00 != 0x7f00) && self.sound_address_high == high,
            "invalid SOUND MAIN address mux"
        );
        ensure!(
            self.br_level == self.p[10][1]
                && self.lower_oe_level == self.w[314]
                && self.upper_oe_level == self.w[314]
                && self.bus_status_read == !self.w[12],
            "invalid arbiter held outputs"
        );
        Ok(())
    }
}

fn write_input(writer: &mut StateWriter, input: Inputs) {
    writer.write_u16(input.data);
    writer.write_u32(input.address);
    writer.write_u16(input.sound_address);
    writer.write_u16(input.sound_bank);
    write_bits(
        writer,
        [
            input.vclk,
            input.zclk,
            input.bgack,
            input.bg,
            input.read,
            input.upper,
            input.address_strobe,
            input.dtack,
            input.lower,
            input.cas,
            input.output_enable,
            input.fc[0],
            input.fc[1],
            input.wait_level,
            input.sound_bus_acknowledge_level,
            input.sound_read_level,
            input.sound_write_level,
            input.sound_memory_level,
            input.sound_io_level,
            input.m1,
            input.m3,
        ],
    );
    write_bits(writer, input.delayed);
}

fn read_input(reader: &mut StateReader<'_>) -> Result<Inputs> {
    let mut input = Inputs::zero();
    input.data = reader.read_u16()?;
    input.address = reader.read_u32()?;
    input.sound_address = reader.read_u16()?;
    input.sound_bank = reader.read_u16()?;
    [
        input.vclk,
        input.zclk,
        input.bgack,
        input.bg,
        input.read,
        input.upper,
        input.address_strobe,
        input.dtack,
        input.lower,
        input.cas,
        input.output_enable,
        input.fc[0],
        input.fc[1],
        input.wait_level,
        input.sound_bus_acknowledge_level,
        input.sound_read_level,
        input.sound_write_level,
        input.sound_memory_level,
        input.sound_io_level,
        input.m1,
        input.m3,
    ] = read_bits(reader)?;
    input.delayed = read_bits(reader)?;
    ensure!(
        input.address <= 0x7fffff && input.sound_bank < 512,
        "invalid arbiter bus address"
    );
    Ok(input)
}
