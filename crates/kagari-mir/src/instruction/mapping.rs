//! Contextual identity traversal of the owning metadata records.
use crate::instruction::{
    AggregateFieldRef, CallTarget, Instruction, PathRef, SourceFunctionContract,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel, map_sequence},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};

impl RemapDefinitions for AggregateFieldRef {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_, DefinitionId, DefinitionId>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(AggregateFieldRef {
            owner: self.owner.remap_definitions(mapper)?,
            slot: self.slot,
        })
    }
}

impl DefinitionReferences for AggregateFieldRef {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.owner.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}

impl RemapDefinitions for PathRef {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_, DefinitionId, DefinitionId>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(PathRef {
            declaration: self
                .declaration
                .as_ref()
                .map(|value| (value).remap_definitions(mapper))
                .transpose()?,
            contract_fingerprint: self.contract_fingerprint,
            root_ty: self.root_ty,
            result_ty: self.result_ty,
            read_only: self.read_only,
            debug_name: self.debug_name.clone(),
        })
    }
}

impl DefinitionReferences for PathRef {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        if let Some(value0) = self.declaration.as_ref() {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}

impl RemapDefinitions for Instruction {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_, DefinitionId, DefinitionId>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(match self {
            Self::Convert {
                dst,
                src,
                conversion,
            } => Instruction::Convert {
                dst: *(dst),
                src: *(src),
                conversion: *(conversion),
            },
            Self::Numeric {
                dst,
                operation,
                lhs,
                rhs,
            } => Instruction::Numeric {
                dst: *(dst),
                operation: *(operation),
                lhs: *(lhs),
                rhs: *(rhs),
            },
            Self::MakeFuture {
                dst,
                function,
                arguments,
                future,
            } => Instruction::MakeFuture {
                dst: *dst,
                function: *function,
                arguments: arguments.clone(),
                future: future.remap_definitions(mapper)?,
            },
            Self::Await { dst, value, future } => Instruction::Await {
                dst: *dst,
                value: *value,
                future: future.remap_definitions(mapper)?,
            },
            Self::Iter { dst, value, ty, op } => Instruction::Iter {
                dst: *(dst),
                value: *(value),
                ty: (ty).remap_definitions(mapper)?,
                op: *(op),
            },

            Self::LoadConst { dst, constant } => Instruction::LoadConst {
                dst: *(dst),
                constant: (constant).clone(),
            },
            Self::LoadLocal { dst, local } => Instruction::LoadLocal {
                dst: *(dst),
                local: *(local),
            },
            Self::LoadModule { dst, slot } => Instruction::LoadModule {
                dst: *(dst),
                slot: *(slot),
            },
            Self::StoreLocal { local, src } => Instruction::StoreLocal {
                local: *(local),
                src: *(src),
            },
            Self::StoreModule { slot, src } => Instruction::StoreModule {
                slot: *(slot),
                src: *(src),
            },
            Self::Move { dst, src } => Instruction::Move {
                dst: *(dst),
                src: *(src),
            },
            Self::Unary { dst, op, operand } => Instruction::Unary {
                dst: *(dst),
                op: *(op),
                operand: *(operand),
            },
            Self::Binary { dst, op, lhs, rhs } => Instruction::Binary {
                dst: *(dst),
                op: *(op),
                lhs: *(lhs),
                rhs: *(rhs),
            },
            Self::Call { dst, callee, args } => Instruction::Call {
                dst: *(dst),
                callee: (callee).remap_definitions(mapper)?,
                args: (args).clone(),
            },
            Self::MakeTuple { dst, elements } => Instruction::MakeTuple {
                dst: *(dst),
                elements: (elements).clone(),
            },
            Self::RangeBound {
                dst,
                value,
                range,
                bound,
                upper,
            } => Instruction::RangeBound {
                dst: *(dst),
                value: *(value),
                range: (range).remap_definitions(mapper)?,
                bound: (bound).remap_definitions(mapper)?,
                upper: *(upper),
            },
            Self::MakeRange {
                dst,
                start,
                end,
                ty,
            } => Instruction::MakeRange {
                dst: *(dst),
                start: *(start),
                end: *(end),
                ty: (ty).remap_definitions(mapper)?,
            },
            Self::RepeatArray {
                element,
                dst,
                value,
                count,
            } => Instruction::RepeatArray {
                element: (element).remap_definitions(mapper)?,
                dst: *(dst),
                value: *(value),
                count: *(count),
            },
            Self::MakeArray {
                element,
                dst,
                elements,
            } => Instruction::MakeArray {
                element: (element).remap_definitions(mapper)?,
                dst: *(dst),
                elements: (elements).clone(),
            },
            Self::MakeClosure {
                dst,
                function,
                captures,
            } => Instruction::MakeClosure {
                dst: *(dst),
                function: *(function),
                captures: (captures).clone(),
            },
            Self::MakeCell { dst, value } => Instruction::MakeCell {
                dst: *(dst),
                value: *(value),
            },
            Self::ReadCell { dst, cell } => Instruction::ReadCell {
                dst: *(dst),
                cell: *(cell),
            },
            Self::WriteCell { cell, value } => Instruction::WriteCell {
                cell: *(cell),
                value: *(value),
            },
            Self::UpcastInterface {
                dst,
                value,
                source,
                target,
            } => Instruction::UpcastInterface {
                dst: *(dst),
                value: *(value),
                source: (source).remap_definitions(mapper)?,
                target: (target).remap_definitions(mapper)?,
            },
            Self::MakeInterface {
                dst,
                value,
                implementation,
                arguments,
            } => Instruction::MakeInterface {
                dst: *(dst),
                value: *(value),
                implementation: mapper.reference(implementation)?,
                arguments: map_sequence(arguments, |value| (value).remap_definitions(mapper))?,
            },
            Self::MakeStruct {
                dst,
                structure,
                fields,
            } => Instruction::MakeStruct {
                dst: *(dst),
                structure: (structure).remap_definitions(mapper)?,
                fields: (fields).clone(),
            },
            Self::MakeEnum {
                dst,
                enumeration,
                variant,
                fields,
            } => Instruction::MakeEnum {
                dst: *(dst),
                enumeration: (enumeration).remap_definitions(mapper)?,
                variant: *(variant),
                fields: (fields).clone(),
            },
            Self::TestEnumVariant {
                dst,
                value,
                enumeration,
                variant,
            } => Instruction::TestEnumVariant {
                dst: *(dst),
                value: *(value),
                enumeration: (enumeration).remap_definitions(mapper)?,
                variant: *(variant),
            },
            Self::ReadEnumPayload {
                dst,
                value,
                enumeration,
                variant,
                index,
            } => Instruction::ReadEnumPayload {
                dst: *(dst),
                value: *(value),
                enumeration: (enumeration).remap_definitions(mapper)?,
                variant: *(variant),
                index: *(index),
            },
            Self::ReadAggregateField { dst, base, field } => Instruction::ReadAggregateField {
                dst: *(dst),
                base: *(base),
                field: (field).remap_definitions(mapper)?,
            },
            Self::WriteAggregateField { base, field, value } => Instruction::WriteAggregateField {
                base: *(base),
                field: (field).remap_definitions(mapper)?,
                value: *(value),
            },
            Self::ReadAggregateIndex { dst, base, index } => Instruction::ReadAggregateIndex {
                dst: *(dst),
                base: *(base),
                index: *(index),
            },
            Self::WriteAggregateIndex { base, index, value } => Instruction::WriteAggregateIndex {
                base: *(base),
                index: *(index),
                value: *(value),
            },
            Self::ReadPath {
                dst,
                root_or_view,
                path,
                dynamic_args,
            } => Instruction::ReadPath {
                dst: *(dst),
                root_or_view: *(root_or_view),
                path: (path).remap_definitions(mapper)?,
                dynamic_args: (dynamic_args).clone(),
            },
            Self::SetPath {
                root_or_view,
                path,
                dynamic_args,
                value,
            } => Instruction::SetPath {
                root_or_view: *(root_or_view),
                path: (path).remap_definitions(mapper)?,
                dynamic_args: (dynamic_args).clone(),
                value: *(value),
            },
            Self::ModifyPath {
                dst,
                root_or_view,
                path,
                dynamic_args,
                op,
                value,
            } => Instruction::ModifyPath {
                dst: *(dst),
                root_or_view: *(root_or_view),
                path: (path).remap_definitions(mapper)?,
                dynamic_args: (dynamic_args).clone(),
                op: *(op),
                value: *(value),
            },
            Self::MakePathView {
                dst,
                root_or_view,
                path,
                dynamic_args,
            } => Instruction::MakePathView {
                dst: *(dst),
                root_or_view: *(root_or_view),
                path: (path).remap_definitions(mapper)?,
                dynamic_args: (dynamic_args).clone(),
            },
        })
    }
}

impl DefinitionReferences for Instruction {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        match self {
            Self::Convert {
                dst: _,
                src: _,
                conversion: _,
            } => {}
            Self::Numeric {
                dst: _,
                operation: _,
                lhs: _,
                rhs: _,
            } => {}
            Self::Await { future, .. } | Self::MakeFuture { future, .. } => {
                future.visit_definition_ids(visit, cancel)?
            }
            Self::Iter {
                dst: _,
                value: _,
                ty,
                op: _,
            } => {
                (ty).visit_definition_ids(visit, cancel)?;
            }

            Self::LoadConst {
                dst: _,
                constant: _,
            } => {}
            Self::LoadLocal { dst: _, local: _ } => {}
            Self::LoadModule { dst: _, slot: _ } => {}
            Self::StoreLocal { local: _, src: _ } => {}
            Self::StoreModule { slot: _, src: _ } => {}
            Self::Move { dst: _, src: _ } => {}
            Self::Unary {
                dst: _,
                op: _,
                operand: _,
            } => {}
            Self::Binary {
                dst: _,
                op: _,
                lhs: _,
                rhs: _,
            } => {}
            Self::Call {
                dst: _,
                callee,
                args: _,
            } => {
                (callee).visit_definition_ids(visit, cancel)?;
            }
            Self::MakeTuple {
                dst: _,
                elements: _,
            } => {}
            Self::RangeBound {
                dst: _,
                value: _,
                range,
                bound,
                upper: _,
            } => {
                (range).visit_definition_ids(visit, cancel)?;
                (bound).visit_definition_ids(visit, cancel)?;
            }
            Self::MakeRange {
                dst: _,
                start: _,
                end: _,
                ty,
            } => {
                (ty).visit_definition_ids(visit, cancel)?;
            }
            Self::RepeatArray {
                element,
                dst: _,
                value: _,
                count: _,
            } => {
                (element).visit_definition_ids(visit, cancel)?;
            }
            Self::MakeArray {
                element,
                dst: _,
                elements: _,
            } => {
                (element).visit_definition_ids(visit, cancel)?;
            }
            Self::MakeClosure {
                dst: _,
                function: _,
                captures: _,
            } => {}
            Self::MakeCell { dst: _, value: _ } => {}
            Self::ReadCell { dst: _, cell: _ } => {}
            Self::WriteCell { cell: _, value: _ } => {}
            Self::UpcastInterface {
                dst: _,
                value: _,
                source,
                target,
            } => {
                (source).visit_definition_ids(visit, cancel)?;
                (target).visit_definition_ids(visit, cancel)?;
            }
            Self::MakeInterface {
                dst: _,
                value: _,
                implementation,
                arguments,
            } => {
                check_cancel(cancel)?;
                visit(implementation)?;
                for value0 in arguments {
                    (value0).visit_definition_ids(visit, cancel)?;
                }
            }
            Self::MakeStruct {
                dst: _,
                structure,
                fields: _,
            } => {
                (structure).visit_definition_ids(visit, cancel)?;
            }
            Self::MakeEnum {
                dst: _,
                enumeration,
                variant: _,
                fields: _,
            } => {
                (enumeration).visit_definition_ids(visit, cancel)?;
            }
            Self::TestEnumVariant {
                dst: _,
                value: _,
                enumeration,
                variant: _,
            } => {
                (enumeration).visit_definition_ids(visit, cancel)?;
            }
            Self::ReadEnumPayload {
                dst: _,
                value: _,
                enumeration,
                variant: _,
                index: _,
            } => {
                (enumeration).visit_definition_ids(visit, cancel)?;
            }
            Self::ReadAggregateField {
                dst: _,
                base: _,
                field,
            } => {
                (field).visit_definition_ids(visit, cancel)?;
            }
            Self::WriteAggregateField {
                base: _,
                field,
                value: _,
            } => {
                (field).visit_definition_ids(visit, cancel)?;
            }
            Self::ReadAggregateIndex {
                dst: _,
                base: _,
                index: _,
            } => {}
            Self::WriteAggregateIndex {
                base: _,
                index: _,
                value: _,
            } => {}
            Self::ReadPath {
                dst: _,
                root_or_view: _,
                path,
                dynamic_args: _,
            } => {
                (path).visit_definition_ids(visit, cancel)?;
            }
            Self::SetPath {
                root_or_view: _,
                path,
                dynamic_args: _,
                value: _,
            } => {
                (path).visit_definition_ids(visit, cancel)?;
            }
            Self::ModifyPath {
                dst: _,
                root_or_view: _,
                path,
                dynamic_args: _,
                op: _,
                value: _,
            } => {
                (path).visit_definition_ids(visit, cancel)?;
            }
            Self::MakePathView {
                dst: _,
                root_or_view: _,
                path,
                dynamic_args: _,
            } => {
                (path).visit_definition_ids(visit, cancel)?;
            }
        }
        Ok(())
    }
}

impl RemapDefinitions for CallTarget {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_, DefinitionId, DefinitionId>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(match self {
            Self::Shared(field0) => {
                CallTarget::Shared(Box::new(((field0).as_ref()).remap_definitions(mapper)?))
            }
            Self::SourceFunction(field0) => {
                CallTarget::SourceFunction(Box::new(((field0).as_ref()).remap_definitions(mapper)?))
            }
            Self::Function(field0) => CallTarget::Function(*(field0)),
            Self::InterfaceMethod(field0) => CallTarget::InterfaceMethod(Box::new(
                ((field0).as_ref()).remap_definitions(mapper)?,
            )),
            Self::Native(field0) => {
                CallTarget::Native(Box::new(((field0).as_ref()).remap_definitions(mapper)?))
            }
            Self::Value(field0) => CallTarget::Value(*(field0)),
            Self::Closure {
                value,
                params,
                return_type,
            } => CallTarget::Closure {
                value: *(value),
                params: (params).clone(),
                return_type: *(return_type),
            },
            Self::RuntimePrimitive(field0) => CallTarget::RuntimePrimitive(*(field0)),
            Self::RuntimeHelper(field0) => CallTarget::RuntimeHelper((field0).clone()),
        })
    }
}

impl DefinitionReferences for CallTarget {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        match self {
            Self::Shared(field0) => {
                ((field0).as_ref()).visit_definition_ids(visit, cancel)?;
            }
            Self::SourceFunction(field0) => {
                ((field0).as_ref()).visit_definition_ids(visit, cancel)?;
            }
            Self::Function(_) => {}
            Self::InterfaceMethod(field0) => {
                ((field0).as_ref()).visit_definition_ids(visit, cancel)?;
            }
            Self::Native(field0) => {
                ((field0).as_ref()).visit_definition_ids(visit, cancel)?;
            }
            Self::Value(_) => {}
            Self::Closure {
                value: _,
                params: _,
                return_type: _,
            } => {}
            Self::RuntimePrimitive(_) => {}
            Self::RuntimeHelper(_) => {}
        }
        Ok(())
    }
}

impl RemapDefinitions for SourceFunctionContract {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_, DefinitionId, DefinitionId>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(SourceFunctionContract {
            declaration: mapper.reference(&self.declaration)?,
            arguments: map_sequence(&self.arguments, |value| (value).remap_definitions(mapper))?,
            params: self.params.clone(),
            return_type: self.return_type,
        })
    }
}

impl DefinitionReferences for SourceFunctionContract {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.declaration)?;
        for value0 in &self.arguments {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}
