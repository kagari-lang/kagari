//! A linked Native method delegates its one external object loan to the path service.
use crate::{
    error::RuntimeError,
    host::{HostCallContext, HostPathDescriptorId},
    native::binding::NativeResult,
    value::view::ValueView,
};
use kagari_bytecode::instruction::BinaryOp;
use kagari_types::declaration::path::{
    HostPathSegmentDeclaration, NativePathOperation, NativePathOperationKind,
};

#[derive(Debug, Clone)]
pub(crate) struct LinkedPathOperation {
    descriptor: HostPathDescriptorId,
    kind: NativePathOperationKind,
    /// Argument position (after the receiver) for each descriptor slot.
    indices: Box<[usize]>,
}

impl LinkedPathOperation {
    pub(crate) fn link(
        operation: &NativePathOperation,
        descriptor: HostPathDescriptorId,
    ) -> NativeResult<Self> {
        let mut indices: Vec<_> = operation
            .path
            .segments
            .iter()
            .filter_map(|segment| match segment {
                HostPathSegmentDeclaration::Index(index) => Some(index.slot),
                _ => None,
            })
            .enumerate()
            .map(|(position, slot)| (slot, position))
            .collect();
        indices.sort_unstable();
        if indices
            .iter()
            .enumerate()
            .any(|(slot, (declared, _))| *declared as usize != slot)
        {
            return Err(RuntimeError::module_validation(
                "native path argument slots are not a permutation",
            ));
        }
        Ok(Self {
            descriptor,
            kind: operation.kind,
            indices: indices.into_iter().map(|(_, position)| position).collect(),
        })
    }

    pub(crate) fn invoke<'call>(
        &self,
        cx: &mut HostCallContext<'call>,
        values: &[ValueView<'call>],
    ) -> NativeResult<ValueView<'call>> {
        cx.runtime().reject_candidate_external_access()?;
        let invalid = || RuntimeError::module_validation("native path operation argument count");
        let (root, args) = values.split_first().ok_or_else(invalid)?;
        let (indices, rhs) = args
            .split_at_checked(self.indices.len())
            .ok_or_else(invalid)?;
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(indices.len())
            .map_err(|_| RuntimeError::resource_limit("native path arguments"))?;
        slots.extend(self.indices.iter().map(|&position| indices[position]));
        match (self.kind, rhs) {
            (NativePathOperationKind::Read, []) => cx.read_path(root, self.descriptor, &slots),
            (NativePathOperationKind::Set, [rhs]) => {
                cx.set_path(root, self.descriptor, &slots, rhs)?;
                Ok(ValueView::unit())
            }
            (NativePathOperationKind::Add, [rhs]) => {
                cx.modify_path(root, self.descriptor, &slots, BinaryOp::Add, rhs)
            }
            _ => Err(invalid()),
        }
    }
}
