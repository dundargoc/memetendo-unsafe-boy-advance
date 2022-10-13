use std::mem::replace;

use intbits::Bits;

use crate::{
    bus::{AlignedExt, Bus},
    cart::Cartridge,
    irq::{Interrupt, Irq},
};

#[derive(Debug, Default, Eq, PartialEq)]
enum State {
    #[default]
    None,
    StartingTransfer,
    Transferring,
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Default)]
struct Channel {
    initial_src_addr: u32,
    initial_dst_addr: u32,
    initial_blocks: u32,
    src_addr_ctrl: u8,
    dst_addr_ctrl: u8,
    repeat: bool,
    transfer_word: bool,
    cart_drq: bool,
    timing_mode: u8,
    irq_enabled: bool,
    enabled: bool,
    cached_dmacnt_hi_bits: u16,

    src_addr: u32,
    dst_addr: u32,
    rem_blocks: u32,
    state: State,
}

#[derive(Debug, Default)]
pub struct Dma([Channel; 4]);

impl Dma {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn in_audio_fifo_mode(&self, chan_idx: usize) -> bool {
        (1..=2).contains(&chan_idx) && self.0[chan_idx].timing_mode == 3
    }

    fn start_transfer(&mut self, chan_idx: usize) {
        let audio_fifo = self.in_audio_fifo_mode(chan_idx);
        let chan = &mut self.0[chan_idx];
        if !chan.enabled || chan.state != State::None {
            return;
        }

        if audio_fifo {
            chan.rem_blocks = 4;
        } else {
            let max_blocks = if chan_idx == 3 { 0x1_0000 } else { 0x4000 };
            if chan.rem_blocks == 0 || chan.rem_blocks > max_blocks {
                chan.rem_blocks = max_blocks;
            }
        }
        chan.state = State::StartingTransfer;
    }

    #[must_use]
    pub fn step<B: Bus>(
        &mut self,
        irq: &mut Irq,
        cart: &mut Cartridge,
        cycles: u8,
    ) -> Option<impl Fn(&mut B)> {
        // TODO: proper cycle transfer timings, cart DRQ, special timing modes
        for chan_idx in 0..self.0.len() {
            if !self.0[chan_idx].enabled || self.0[chan_idx].state == State::None {
                continue;
            }

            let audio_fifo = self.in_audio_fifo_mode(chan_idx);
            let chan = &mut self.0[chan_idx];

            let dst_addr = chan.dst_addr;
            if chan.state == State::StartingTransfer
                && dst_addr >= 0x0800_0000
                && cart.is_eeprom_offset(dst_addr - 0x0800_0000)
            {
                cart.notify_eeprom_dma(chan.rem_blocks);
            }
            chan.state = State::Transferring;

            let src_addr = chan.src_addr;
            let src_addr_ctrl = chan.src_addr_ctrl;
            let dst_addr_ctrl = if audio_fifo { 2 } else { chan.dst_addr_ctrl };
            let blocks = chan.rem_blocks.min(cycles.into());
            let transfer_word = audio_fifo || chan.transfer_word;
            let stride = if transfer_word { 4 } else { 2 };

            let update_addr = |addr: &mut u32, ctrl, offset| {
                match ctrl {
                    0 | 3 => *addr = addr.wrapping_add(offset),
                    1 => *addr = addr.wrapping_sub(offset),
                    2 => {}
                    _ => unreachable!(),
                };
            };
            update_addr(&mut chan.src_addr, src_addr_ctrl, stride * blocks);
            update_addr(&mut chan.dst_addr, dst_addr_ctrl, stride * blocks);

            chan.rem_blocks -= blocks;
            if chan.rem_blocks == 0 {
                chan.state = State::None;
                chan.enabled = chan.repeat;
                if chan.repeat {
                    if chan.dst_addr_ctrl == 3 {
                        chan.dst_addr = chan.initial_dst_addr;
                    }
                    chan.rem_blocks = chan.initial_blocks;
                }

                if chan.irq_enabled {
                    irq.request(match chan_idx {
                        0 => Interrupt::Dma0,
                        1 => Interrupt::Dma1,
                        2 => Interrupt::Dma2,
                        3 => Interrupt::Dma3,
                        _ => unreachable!(),
                    });
                }
            }

            return Some(move |bus: &mut B| {
                let mut src_addr = src_addr;
                let mut dst_addr = dst_addr;

                for _ in 0..blocks {
                    if transfer_word {
                        let value = bus.read_word_aligned(src_addr);
                        bus.write_word_aligned(dst_addr, value);
                    } else {
                        let value = bus.read_hword_aligned(src_addr);
                        bus.write_hword_aligned(dst_addr, value);
                    }
                    update_addr(&mut src_addr, src_addr_ctrl, stride);
                    update_addr(&mut dst_addr, dst_addr_ctrl, stride);
                }
            });
        }

        None
    }

    #[must_use]
    pub fn transfer_in_progress(&self) -> bool {
        self.0.iter().any(|chan| chan.state != State::None)
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum Event {
    VBlank,
    HBlank,
    AudioFifoA,
    AudioFifoB,
}

impl Dma {
    pub fn notify(&mut self, event: Event) {
        let event_timing_mode = match event {
            Event::VBlank => 1,
            Event::HBlank => 2,
            Event::AudioFifoA | Event::AudioFifoB => 3,
        };

        for chan_idx in 0..self.0.len() {
            if !self.0[chan_idx].enabled || self.0[chan_idx].timing_mode != event_timing_mode {
                continue;
            }
            let fifo_addr = match event {
                Event::AudioFifoA => Some(0x0400_00a0),
                Event::AudioFifoB => Some(0x0400_00a4),
                _ => None,
            };
            if let Some(fifo_addr) = fifo_addr {
                if !self.in_audio_fifo_mode(chan_idx)
                    || self.0[chan_idx].initial_dst_addr != fifo_addr
                {
                    continue;
                }
            }

            self.start_transfer(chan_idx);
        }
    }
}

impl Bus for Dma {
    fn read_byte(&mut self, addr: u32) -> u8 {
        assert!((0xb0..0xe0).contains(&addr), "IO register address OOB");

        let chan = &mut self.0[(addr as usize - 0xb0) / 12];
        #[allow(clippy::cast_possible_truncation)]
        match (addr as usize - 0xb0) % 12 {
            // DMAXCNT
            10 => chan.cached_dmacnt_hi_bits as u8,
            11 => chan
                .cached_dmacnt_hi_bits
                .with_bit(15, chan.enabled)
                .bits(8..) as u8,
            _ => 0,
        }
    }

    fn write_byte(&mut self, addr: u32, value: u8) {
        assert!((0xb0..0xe0).contains(&addr), "IO register address OOB");

        let chan_idx = (addr as usize - 0xb0) / 12;
        let chan = &mut self.0[chan_idx];
        let offset = (addr as usize - 0xb0) % 12;

        let set_addr_byte = |addr: &mut u32, i, value: u8| match i {
            0..=2 => addr.set_bits((i * 8)..(i * 8) + 8, value.into()),
            3 if chan_idx == 0 => addr.set_bits(24.., value.bits(..3).into()),
            3 => addr.set_bits(24.., value.bits(..4).into()),
            _ => unreachable!(),
        };

        match offset {
            // DMAXSAD
            0..=3 => set_addr_byte(&mut chan.initial_src_addr, offset & 3, value),
            // DMAXDAD
            4..=7 => set_addr_byte(&mut chan.initial_dst_addr, offset & 3, value),
            // DMAXCNT
            8 => chan.initial_blocks.set_bits(..8, value.into()),
            9 => chan.initial_blocks.set_bits(8.., value.into()),
            10 => {
                chan.cached_dmacnt_hi_bits.set_bits(..8, value.into());
                chan.dst_addr_ctrl = value.bits(5..7);
                chan.src_addr_ctrl.set_bit(0, value.bit(7));
            }
            11 => {
                chan.cached_dmacnt_hi_bits.set_bits(8.., value.into());
                chan.src_addr_ctrl.set_bit(1, value.bit(0));
                chan.repeat = value.bit(1);
                chan.transfer_word = value.bit(2);
                chan.cart_drq = value.bit(3);
                chan.timing_mode = value.bits(4..6);
                chan.irq_enabled = value.bit(6);

                if !replace(&mut chan.enabled, value.bit(7)) && chan.enabled {
                    chan.src_addr = chan.initial_src_addr;
                    chan.dst_addr = chan.initial_dst_addr;
                    chan.rem_blocks = chan.initial_blocks;

                    if chan.timing_mode == 0 {
                        self.start_transfer(chan_idx);
                    }
                }
            }
            _ => unreachable!(),
        }
    }
}
