use crate::source::lower::{MirLoweringError, state::FunctionLowerer};
use kagari_abi::{
    operations::BinaryOp, representation::ValueType, standard::traits::StandardTrait,
};
use kagari_hir::{builtin::traits::StandardTraitSemantics, types::TypeId};
use kagari_mir::instruction::{Instruction, MirValue};

impl FunctionLowerer<'_, '_> {
    pub(super) fn list_call(
        &mut self,
        source: &TypeId,
        item: &TypeId,
        name: &str,
        args: &[MirValue],
    ) -> Result<MirValue, MirLoweringError> {
        let mut interface = StandardTrait::List.nominal();
        interface.arguments.push(item.clone());
        let method = self
            .planner
            .catalog
            .trait_(&interface.declaration)
            .and_then(|contract| contract.methods.iter().find(|method| method.name == name))
            .ok_or(MirLoweringError::MissingBinding("list query member"))?
            .id
            .clone();
        self.lower_applied_operator(interface, source.clone(), &method, args)
    }
    pub(super) fn query_binary(
        &mut self,
        op: BinaryOp,
        left: MirValue,
        right: MirValue,
        ty: ValueType,
    ) -> MirValue {
        let dst = self.alloc_temp(ty);
        self.emit(Instruction::Binary {
            dst,
            op,
            lhs: left,
            rhs: right,
        });
        dst
    }
    pub(super) fn query_guard(
        &mut self,
        source: &TypeId,
        value: MirValue,
    ) -> Result<(TypeId, MirValue), MirLoweringError> {
        let iterator_type = self.iteration_output(StandardTrait::Iterable, source, "Iter")?;
        let iterator = self.lower_applied_operator(
            StandardTrait::Iterable.nominal(),
            source.clone(),
            &self.protocol_method(StandardTrait::Iterable, 0)?,
            &[value],
        )?;
        self.emit(Instruction::BeginIteration {
            collection: iterator,
        });
        Ok((iterator_type, iterator))
    }
    pub(super) fn end_query_guard(&mut self, guard: (TypeId, MirValue)) {
        self.iterator_close(&guard.0, guard.1);
        self.emit(Instruction::EndIteration);
    }
}
