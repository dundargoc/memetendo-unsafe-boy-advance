use std::{
    ops::{Index, IndexMut},
    slice::SliceIndex,
};

use intbits::Bits;
use strum_macros::FromRepr;

#[derive(Copy, Clone, PartialEq, Eq, FromRepr, Debug)]
#[repr(u8)]
pub(super) enum OperationMode {
    User = 0b10000,
    FastInterrupt = 0b10001,
    Interrupt = 0b10010,
    Supervisor = 0b10011,
    Abort = 0b10111,
    UndefinedInstr = 0b11011,
    System = 0b11111,
}

impl Default for OperationMode {
    fn default() -> Self {
        Self::Supervisor
    }
}

impl OperationMode {
    pub(super) fn bits(self) -> u32 {
        self as _
    }

    #[allow(clippy::cast_possible_truncation)]
    pub(super) fn from_bits(bits: u32) -> Option<Self> {
        Self::from_repr(bits.bits(..5) as _)
    }
}

#[derive(Default, Copy, Clone, PartialEq, Eq, Debug)]
pub(super) struct GeneralRegisters(pub(crate) [u32; 16]);

pub(super) const SP_INDEX: usize = 13;
pub(super) const LR_INDEX: usize = 14;
pub(super) const PC_INDEX: usize = 15;

impl<I: SliceIndex<[u32]>> Index<I> for GeneralRegisters {
    type Output = I::Output;

    fn index(&self, index: I) -> &Self::Output {
        Index::index(&self.0, index)
    }
}

impl<I: SliceIndex<[u32]>> IndexMut<I> for GeneralRegisters {
    fn index_mut(&mut self, index: I) -> &mut Self::Output {
        IndexMut::index_mut(&mut self.0, index)
    }
}

#[derive(Default, Copy, Clone, Debug)]
pub(super) struct Registers {
    pub(super) r: GeneralRegisters,
    pub(super) cpsr: StatusRegister,
    pub(super) spsr: StatusRegister,
    banks: [Bank; 6],
    fiq_r8_12_bank: [u32; 5],
}

#[derive(Default, Copy, Clone, PartialEq, Eq, Debug)]
struct Bank {
    sp: u32,
    lr: u32,
    spsr: StatusRegister,
}

impl OperationMode {
    fn bank_index(self) -> usize {
        match self {
            Self::User | Self::System => 0,
            Self::FastInterrupt => 1,
            Self::Interrupt => 2,
            Self::Supervisor => 3,
            Self::Abort => 4,
            Self::UndefinedInstr => 5,
        }
    }
}

impl Registers {
    pub(super) fn change_mode(&mut self, mode: OperationMode) {
        self.change_bank(mode);
        self.cpsr.mode = mode;
    }

    fn change_bank(&mut self, mode: OperationMode) {
        let old_bank_index = self.cpsr.mode.bank_index();
        let bank_index = mode.bank_index();
        if old_bank_index == bank_index {
            return;
        }

        if self.cpsr.mode == OperationMode::FastInterrupt || mode == OperationMode::FastInterrupt {
            self.fiq_r8_12_bank.swap_with_slice(&mut self.r[8..=12]);
        }
        self.banks[old_bank_index].sp = self.r[SP_INDEX];
        self.banks[old_bank_index].lr = self.r[LR_INDEX];
        self.banks[old_bank_index].spsr = self.spsr;

        self.r[SP_INDEX] = self.banks[bank_index].sp;
        self.r[LR_INDEX] = self.banks[bank_index].lr;
        self.spsr = self.banks[bank_index].spsr;
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[repr(u8)]
pub(super) enum OperationState {
    Arm = 0,
    Thumb = 1 << 5,
}

impl Default for OperationState {
    fn default() -> Self {
        Self::Arm
    }
}

impl OperationState {
    fn bits(self) -> u32 {
        self as _
    }

    pub(super) fn instr_size(self) -> u32 {
        match self {
            Self::Arm => 4,
            Self::Thumb => 2,
        }
    }
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Default, Copy, Clone, PartialEq, Eq, Debug)]
pub(super) struct StatusRegister {
    pub(super) signed: bool,
    pub(super) zero: bool,
    pub(super) carry: bool,
    pub(super) overflow: bool,

    pub(super) irq_disabled: bool,
    pub(super) fiq_disabled: bool,
    pub(super) state: OperationState,
    pub(super) mode: OperationMode,
}

impl StatusRegister {
    pub(super) fn bits(self) -> u32 {
        let mut psr = 0;

        psr.set_bit(31, self.signed);
        psr.set_bit(30, self.zero);
        psr.set_bit(29, self.carry);
        psr.set_bit(28, self.overflow);

        psr.set_bit(7, self.irq_disabled);
        psr.set_bit(6, self.fiq_disabled);
        psr |= self.state.bits();
        psr |= self.mode.bits();

        psr
    }

    pub(super) fn set_flags_from_bits(&mut self, bits: u32) {
        self.signed = bits.bit(31);
        self.zero = bits.bit(30);
        self.carry = bits.bit(29);
        self.overflow = bits.bit(28);
    }

    pub(super) fn set_control_from_bits(&mut self, bits: u32) -> Result<OperationMode, ()> {
        self.irq_disabled = bits.bit(7);
        self.fiq_disabled = bits.bit(6);
        self.mode = OperationMode::from_bits(bits).ok_or(())?;

        Ok(self.mode)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usr_and_sys_mode_same_bank_index() {
        assert_eq!(
            OperationMode::User.bank_index(),
            OperationMode::System.bank_index()
        );
    }

    #[test]
    fn change_mode_works() {
        let mut reg = Registers::default();
        reg.change_mode(OperationMode::User);

        assert_eq!(OperationMode::User, reg.cpsr.mode);

        reg.r.0 = [1337; 16];
        reg.change_mode(OperationMode::UndefinedInstr);

        assert_eq!(OperationMode::UndefinedInstr, reg.cpsr.mode);

        let old_bank = reg.banks[OperationMode::User.bank_index()];
        assert_eq!(1337, old_bank.sp);
        assert_eq!(1337, old_bank.lr);

        reg.r[13..=14].fill(1234);
        let undef_spsr_zero = reg.spsr.zero;
        reg.spsr.zero = !reg.spsr.zero;
        reg.change_mode(OperationMode::FastInterrupt);

        assert_eq!(OperationMode::FastInterrupt, reg.cpsr.mode);

        let old_bank = reg.banks[OperationMode::UndefinedInstr.bank_index()];
        assert_eq!(1234, old_bank.sp);
        assert_eq!(1234, old_bank.lr);
        assert_ne!(undef_spsr_zero, old_bank.spsr.zero);
        // Should have temporarily saved r8-r12 for later restoration
        assert_eq!([1337; 5], reg.fiq_r8_12_bank);

        reg.r[8..=12].fill(0xeeee);
        reg.r[13..=14].fill(0xaaaa);
        reg.change_mode(OperationMode::User);

        // Been in usr mode already, so should also have the register values from when we started
        assert_eq!(OperationMode::User, reg.cpsr.mode);
        assert_eq!([1337; 2], reg.r[13..=14]);
        assert_eq!([0xeeee; 5], reg.fiq_r8_12_bank);

        let old_bank = reg.banks[OperationMode::FastInterrupt.bank_index()];
        assert_eq!(0xaaaa, old_bank.sp);
        assert_eq!(0xaaaa, old_bank.lr);

        // No need to do banking when switching to the same mode, or when switching between usr and
        // sys modes (they share the same "bank", which is actually no bank; that's an impl detail)
        reg.change_mode(OperationMode::System);

        assert_eq!(OperationMode::System, reg.cpsr.mode);
        assert_eq!([1337; 2], reg.r[13..=14]);

        let bank = reg.banks[OperationMode::System.bank_index()];
        assert_eq!(1337, bank.sp);
        assert_eq!(1337, bank.lr);
    }
}
