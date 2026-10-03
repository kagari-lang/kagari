//! Contextual identity traversal of the owning metadata records.
use crate::instruction::{
    AggregateFieldRef, CallTarget, Instruction, PathRef, SourceFunctionContract,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{
            DefinitionMapper, DefinitionMappingError, DefinitionRecord, check_cancel, map_sequence,
        },
        reference::DefinitionReference,
    },
};

impl<I: DefinitionReference> DefinitionRecord<I> for AggregateFieldRef<I> {
    type Rebind<J: DefinitionReference> = AggregateFieldRef<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(AggregateFieldRef {
            owner: self.owner.map_identities(mapper)?,
            slot: self.slot,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.owner.visit_definitions(visit, cancel)?;
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for PathRef<I> {
    type Rebind<J: DefinitionReference> = PathRef<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(PathRef {
            declaration: self
                .declaration
                .as_ref()
                .map(|value| (value).map_identities(mapper))
                .transpose()?,
            contract_fingerprint: self.contract_fingerprint,
            root_ty: self.root_ty,
            result_ty: self.result_ty,
            read_only: self.read_only,
            debug_name: self.debug_name.clone(),
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        if let Some(value0) = self.declaration.as_ref() {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for Instruction<I> {
    type Rebind<J: DefinitionReference> = Instruction<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
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
            Self::MapResultError {
                dst,
                original,
                error,
                ty,
            } => Instruction::MapResultError {
                dst: *(dst),
                original: *(original),
                error: *(error),
                ty: (ty).map_identities(mapper)?,
            },
            Self::Iter { dst, value, ty, op } => Instruction::Iter {
                dst: *(dst),
                value: *(value),
                ty: (ty).map_identities(mapper)?,
                op: *(op),
            },
            Self::StandardEnum { dst, value, ty, op } => Instruction::StandardEnum {
                dst: *(dst),
                value: *(value),
                ty: (ty).map_identities(mapper)?,
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
                callee: (callee).map_identities(mapper)?,
                args: (args).clone(),
            },
            Self::BeginIteration { collection } => Instruction::BeginIteration {
                collection: *(collection),
            },
            Self::EndIteration => Instruction::EndIteration,
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
                range: (range).map_identities(mapper)?,
                bound: (bound).map_identities(mapper)?,
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
                ty: (ty).map_identities(mapper)?,
            },
            Self::RepeatArray {
                element,
                dst,
                value,
                count,
            } => Instruction::RepeatArray {
                element: (element).map_identities(mapper)?,
                dst: *(dst),
                value: *(value),
                count: *(count),
            },
            Self::MakeArray {
                element,
                dst,
                elements,
            } => Instruction::MakeArray {
                element: (element).map_identities(mapper)?,
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
                source: (source).map_identities(mapper)?,
                target: (target).map_identities(mapper)?,
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
                arguments: map_sequence(arguments, |value| (value).map_identities(mapper))?,
            },
            Self::MakeStruct {
                dst,
                structure,
                fields,
            } => Instruction::MakeStruct {
                dst: *(dst),
                structure: (structure).map_identities(mapper)?,
                fields: (fields).clone(),
            },
            Self::MakeEnum {
                dst,
                enumeration,
                variant,
                fields,
            } => Instruction::MakeEnum {
                dst: *(dst),
                enumeration: (enumeration).map_identities(mapper)?,
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
                enumeration: (enumeration).map_identities(mapper)?,
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
                enumeration: (enumeration).map_identities(mapper)?,
                variant: *(variant),
                index: *(index),
            },
            Self::ReadAggregateField { dst, base, field } => Instruction::ReadAggregateField {
                dst: *(dst),
                base: *(base),
                field: (field).map_identities(mapper)?,
            },
            Self::WriteAggregateField { base, field, value } => Instruction::WriteAggregateField {
                base: *(base),
                field: (field).map_identities(mapper)?,
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
                path: (path).map_identities(mapper)?,
                dynamic_args: (dynamic_args).clone(),
            },
            Self::SetPath {
                root_or_view,
                path,
                dynamic_args,
                value,
            } => Instruction::SetPath {
                root_or_view: *(root_or_view),
                path: (path).map_identities(mapper)?,
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
                path: (path).map_identities(mapper)?,
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
                path: (path).map_identities(mapper)?,
                dynamic_args: (dynamic_args).clone(),
            },
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
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
            Self::MapResultError {
                dst: _,
                original: _,
                error: _,
                ty,
            } => {
                (ty).visit_definitions(visit, cancel)?;
            }
            Self::Iter {
                dst: _,
                value: _,
                ty,
                op: _,
            } => {
                (ty).visit_definitions(visit, cancel)?;
            }
            Self::StandardEnum {
                dst: _,
                value: _,
                ty,
                op: _,
            } => {
                (ty).visit_definitions(visit, cancel)?;
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
                (callee).visit_definitions(visit, cancel)?;
            }
            Self::BeginIteration { collection: _ } => {}
            Self::EndIteration => {}
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
                (range).visit_definitions(visit, cancel)?;
                (bound).visit_definitions(visit, cancel)?;
            }
            Self::MakeRange {
                dst: _,
                start: _,
                end: _,
                ty,
            } => {
                (ty).visit_definitions(visit, cancel)?;
            }
            Self::RepeatArray {
                element,
                dst: _,
                value: _,
                count: _,
            } => {
                (element).visit_definitions(visit, cancel)?;
            }
            Self::MakeArray {
                element,
                dst: _,
                elements: _,
            } => {
                (element).visit_definitions(visit, cancel)?;
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
                (source).visit_definitions(visit, cancel)?;
                (target).visit_definitions(visit, cancel)?;
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
                    (value0).visit_definitions(visit, cancel)?;
                }
            }
            Self::MakeStruct {
                dst: _,
                structure,
                fields: _,
            } => {
                (structure).visit_definitions(visit, cancel)?;
            }
            Self::MakeEnum {
                dst: _,
                enumeration,
                variant: _,
                fields: _,
            } => {
                (enumeration).visit_definitions(visit, cancel)?;
            }
            Self::TestEnumVariant {
                dst: _,
                value: _,
                enumeration,
                variant: _,
            } => {
                (enumeration).visit_definitions(visit, cancel)?;
            }
            Self::ReadEnumPayload {
                dst: _,
                value: _,
                enumeration,
                variant: _,
                index: _,
            } => {
                (enumeration).visit_definitions(visit, cancel)?;
            }
            Self::ReadAggregateField {
                dst: _,
                base: _,
                field,
            } => {
                (field).visit_definitions(visit, cancel)?;
            }
            Self::WriteAggregateField {
                base: _,
                field,
                value: _,
            } => {
                (field).visit_definitions(visit, cancel)?;
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
                (path).visit_definitions(visit, cancel)?;
            }
            Self::SetPath {
                root_or_view: _,
                path,
                dynamic_args: _,
                value: _,
            } => {
                (path).visit_definitions(visit, cancel)?;
            }
            Self::ModifyPath {
                dst: _,
                root_or_view: _,
                path,
                dynamic_args: _,
                op: _,
                value: _,
            } => {
                (path).visit_definitions(visit, cancel)?;
            }
            Self::MakePathView {
                dst: _,
                root_or_view: _,
                path,
                dynamic_args: _,
            } => {
                (path).visit_definitions(visit, cancel)?;
            }
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for CallTarget<I> {
    type Rebind<J: DefinitionReference> = CallTarget<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(match self {
            Self::Shared(field0) => {
                CallTarget::Shared(Box::new(((field0).as_ref()).map_identities(mapper)?))
            }
            Self::SourceFunction(field0) => {
                CallTarget::SourceFunction(Box::new(((field0).as_ref()).map_identities(mapper)?))
            }
            Self::Function(field0) => CallTarget::Function(*(field0)),
            Self::InterfaceMethod(field0) => {
                CallTarget::InterfaceMethod(Box::new(((field0).as_ref()).map_identities(mapper)?))
            }
            Self::Native(field0) => {
                CallTarget::Native(Box::new(((field0).as_ref()).map_identities(mapper)?))
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

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        match self {
            Self::Shared(field0) => {
                ((field0).as_ref()).visit_definitions(visit, cancel)?;
            }
            Self::SourceFunction(field0) => {
                ((field0).as_ref()).visit_definitions(visit, cancel)?;
            }
            Self::Function(_) => {}
            Self::InterfaceMethod(field0) => {
                ((field0).as_ref()).visit_definitions(visit, cancel)?;
            }
            Self::Native(field0) => {
                ((field0).as_ref()).visit_definitions(visit, cancel)?;
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

impl<I: DefinitionReference> DefinitionRecord<I> for SourceFunctionContract<I> {
    type Rebind<J: DefinitionReference> = SourceFunctionContract<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(SourceFunctionContract {
            declaration: mapper.reference(&self.declaration)?,
            arguments: map_sequence(&self.arguments, |value| (value).map_identities(mapper))?,
            params: self.params.clone(),
            return_type: self.return_type,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        check_cancel(cancel)?;
        visit(&self.declaration)?;
        for value0 in &self.arguments {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}
