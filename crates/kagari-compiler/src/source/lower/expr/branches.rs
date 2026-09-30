use crate::source::lower::{MirLoweringError, state::FunctionLowerer};
use kagari_hir::types::TypeId;
use kagari_mir::instruction::{Instruction, MirValue, Terminator};

impl FunctionLowerer<'_, '_> {
    pub(super) fn branch_value(
        &mut self,
        condition: MirValue,
        output: &TypeId,
        yes: impl FnOnce(&mut Self) -> Result<MirValue, MirLoweringError>,
        no: impl FnOnce(&mut Self) -> Result<MirValue, MirLoweringError>,
    ) -> Result<MirValue, MirLoweringError> {
        let yes_block = self.new_block();
        let no_block = self.new_block();
        let join = self.new_block();
        let dst = self.alloc_temp(self.value_type(output)?);
        self.set_terminator(Terminator::Branch {
            cond: condition,
            then_block: yes_block,
            else_block: no_block,
        });
        self.switch_to_block(yes_block);
        let value = yes(self)?;
        self.emit(Instruction::Move { dst, src: value });
        self.set_terminator(Terminator::Jump(join));
        self.switch_to_block(no_block);
        let value = no(self)?;
        self.emit(Instruction::Move { dst, src: value });
        self.set_terminator(Terminator::Jump(join));
        self.switch_to_block(join);
        Ok(dst)
    }
}
