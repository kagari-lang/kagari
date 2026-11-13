//! Prepared operand classes distinguish Unit from payload-bearing bank offsets.
/// A bounded physical bank offset, or the payload-free Unit operand.
#[derive(Debug, Clone, Copy)]
pub struct OperandSlot(u32);

impl OperandSlot {
    pub(crate) fn new(index: usize, managed: bool) -> Self {
        // Prepared bank offsets are bounded before packing; this bit classifies
        // storage and never truncates a runtime owner/slot/generation identity.
        assert!(index < (1 << 31) - 1, "prepared operand index overflow");
        Self(index as u32 | if managed { 1 << 31 } else { 0 })
    }

    pub(crate) const fn unit() -> Self {
        Self(u32::MAX)
    }

    pub(crate) fn is_unit(self) -> bool {
        self.0 == u32::MAX
    }

    pub(crate) fn index(self) -> usize {
        (self.0 & !(1 << 31)) as usize
    }

    pub(crate) fn managed(self) -> bool {
        !self.is_unit() && self.0 & (1 << 31) != 0
    }

    pub(crate) fn scalar(self) -> Option<ScalarSlot> {
        (!self.managed() && !self.is_unit()).then_some(ScalarSlot(self.0))
    }
}

/// A sealed scalar-bank offset. General locations prove their class once during
/// preparation; scalar execution cannot reinterpret a managed offset as payload.
#[derive(Debug, Clone, Copy)]
pub struct ScalarSlot(u32);

impl ScalarSlot {
    #[inline]
    pub(crate) fn index(self) -> usize {
        self.0 as usize
    }
}
