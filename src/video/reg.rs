use intbits::Bits;

#[allow(clippy::struct_excessive_bools)]
#[derive(Default, Debug)]
pub struct DisplayControl {
    pub mode: u8,
    pub frame_select: u8,
    pub hblank_oam_access: bool,
    pub obj_1d: bool,
    pub forced_blank: bool,

    pub display_bg: [bool; 4],
    pub display_obj: bool,
    pub display_window: [bool; 2],
    pub display_obj_window: bool,
}

impl DisplayControl {
    pub fn lo_bits(&self) -> u8 {
        let mut bits = 0;
        bits.set_bits(..3, self.mode.bits(..3));
        bits.set_bits(4..5, self.frame_select);
        bits.set_bit(5, self.hblank_oam_access);
        bits.set_bit(6, self.obj_1d);
        bits.set_bit(7, self.forced_blank);

        bits
    }

    pub fn hi_bits(&self) -> u8 {
        let mut bits = 0;
        bits.set_bit(0, self.display_bg[0]);
        bits.set_bit(1, self.display_bg[1]);
        bits.set_bit(2, self.display_bg[2]);
        bits.set_bit(3, self.display_bg[3]);
        bits.set_bit(4, self.display_obj);

        bits.set_bit(5, self.display_window[0]);
        bits.set_bit(6, self.display_window[1]);
        bits.set_bit(7, self.display_obj_window);

        bits
    }

    pub fn set_lo_bits(&mut self, bits: u8) {
        self.mode = bits.bits(..3);
        self.frame_select = bits.bits(4..5);
        self.hblank_oam_access = bits.bit(5);
        self.obj_1d = bits.bit(6);
        self.forced_blank = bits.bit(7);
    }

    pub fn set_hi_bits(&mut self, bits: u8) {
        self.display_bg[0] = bits.bit(0);
        self.display_bg[1] = bits.bit(1);
        self.display_bg[2] = bits.bit(2);
        self.display_bg[3] = bits.bit(3);
        self.display_obj = bits.bit(4);

        self.display_window[0] = bits.bit(5);
        self.display_window[1] = bits.bit(6);
        self.display_obj_window = bits.bit(7);
    }
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Default, Debug)]
pub struct DisplayStatus {
    pub vblank_irq_enabled: bool,
    pub hblank_irq_enabled: bool,
    pub vcount_irq_enabled: bool,
    pub unused_bit7: bool,
    pub vcount_target: u8,
}

impl DisplayStatus {
    #[allow(clippy::similar_names)]
    pub fn lo_bits(&self, vblanking: bool, hblanking: bool, vcount: u8) -> u8 {
        let mut bits = 0;
        bits.set_bit(0, vblanking);
        bits.set_bit(1, hblanking);
        bits.set_bit(2, vcount == self.vcount_target);
        bits.set_bit(3, self.vblank_irq_enabled);
        bits.set_bit(4, self.hblank_irq_enabled);
        bits.set_bit(5, self.vcount_irq_enabled);
        bits.set_bit(7, self.unused_bit7);

        bits
    }

    #[allow(clippy::similar_names)]
    pub fn set_lo_bits(&mut self, bits: u8) {
        self.vblank_irq_enabled = bits.bit(3);
        self.hblank_irq_enabled = bits.bit(4);
        self.vcount_irq_enabled = bits.bit(5);
        self.unused_bit7 = bits.bit(7);
    }
}