//! Contextual identity traversal of the owning metadata records.
use crate::instruction::{BytecodeInstruction, CallTarget, FieldRef};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{
            DefinitionMapper, DefinitionMappingError, DefinitionRecord, check_cancel, map_sequence,
        },
        reference::DefinitionReference,
    },
};

impl<I: DefinitionReference> DefinitionRecord<I> for FieldRef<I> {
    type Rebind<J: DefinitionReference> = FieldRef<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(FieldRef {
            structure: self.structure,
            arguments: map_sequence(&self.arguments, |value| (value).map_identities(mapper))?,
            slot: self.slot,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for value0 in &self.arguments {
            (value0).visit_definitions(visit, cancel)?;
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
            Self::Shared {
                module,
                target,
                contract,
            } => CallTarget::Shared {
                module: *(module),
                target: *(target),
                contract: Box::new(((contract).as_ref()).map_identities(mapper)?),
            },
            Self::ModuleFunction { module, function } => CallTarget::ModuleFunction {
                module: *(module),
                function: *(function),
            },
            Self::Function(field0) => CallTarget::Function(*(field0)),
            Self::InterfaceMethod { module, contract } => CallTarget::InterfaceMethod {
                module: *(module),
                contract: Box::new(((contract).as_ref()).map_identities(mapper)?),
            },
            Self::Native(field0) => CallTarget::Native(*(field0)),
            Self::Register(field0) => CallTarget::Register(*(field0)),
            Self::ClosureRegister {
                register,
                params,
                return_type,
            } => CallTarget::ClosureRegister {
                register: *(register),
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
            Self::Shared {
                module: _,
                target: _,
                contract,
            } => {
                ((contract).as_ref()).visit_definitions(visit, cancel)?;
            }
            Self::ModuleFunction {
                module: _,
                function: _,
            } => {}
            Self::Function(_) => {}
            Self::InterfaceMethod {
                module: _,
                contract,
            } => {
                ((contract).as_ref()).visit_definitions(visit, cancel)?;
            }
            Self::Native(_) => {}
            Self::Register(_) => {}
            Self::ClosureRegister {
                register: _,
                params: _,
                return_type: _,
            } => {}
            Self::RuntimePrimitive(_) => {}
            Self::RuntimeHelper(_) => {}
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for BytecodeInstruction<I> {
    type Rebind<J: DefinitionReference> = BytecodeInstruction<J>;

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
            } => BytecodeInstruction::Convert {
                dst: *(dst),
                src: *(src),
                conversion: *(conversion),
            },
            Self::Numeric {
                dst,
                operation,
                lhs,
                rhs,
            } => BytecodeInstruction::Numeric {
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
            } => BytecodeInstruction::MakeFuture {
                dst: *dst,
                function: *function,
                arguments: arguments.clone(),
                future: future.map_identities(mapper)?,
            },
            Self::Await { dst, value, future } => BytecodeInstruction::Await {
                dst: *dst,
                value: *value,
                future: future.map_identities(mapper)?,
            },
            Self::Iter { dst, value, ty, op } => BytecodeInstruction::Iter {
                dst: *(dst),
                value: *(value),
                ty: (ty).map_identities(mapper)?,
                op: *(op),
            },

            Self::LoadConst { dst, constant } => BytecodeInstruction::LoadConst {
                dst: *(dst),
                constant: *constant,
            },
            Self::LoadLocal { dst, local } => BytecodeInstruction::LoadLocal {
                dst: *(dst),
                local: *(local),
            },
            Self::LoadModule { dst, slot } => BytecodeInstruction::LoadModule {
                dst: *(dst),
                slot: *(slot),
            },
            Self::StoreLocal { local, src } => BytecodeInstruction::StoreLocal {
                local: *(local),
                src: *(src),
            },
            Self::StoreModule { slot, src } => BytecodeInstruction::StoreModule {
                slot: *(slot),
                src: *(src),
            },
            Self::Move { dst, src } => BytecodeInstruction::Move {
                dst: *(dst),
                src: *(src),
            },
            Self::Unary { dst, op, operand } => BytecodeInstruction::Unary {
                dst: *(dst),
                op: *(op),
                operand: *(operand),
            },
            Self::Binary { dst, op, lhs, rhs } => BytecodeInstruction::Binary {
                dst: *(dst),
                op: *(op),
                lhs: *(lhs),
                rhs: *(rhs),
            },
            Self::Call { dst, callee, args } => BytecodeInstruction::Call {
                dst: *(dst),
                callee: (callee).map_identities(mapper)?,
                args: (args).clone(),
            },
            Self::BeginIteration { collection } => BytecodeInstruction::BeginIteration {
                collection: *(collection),
            },
            Self::EndIteration => BytecodeInstruction::EndIteration,
            Self::MakeTuple { dst, elements } => BytecodeInstruction::MakeTuple {
                dst: *(dst),
                elements: (elements).clone(),
            },
            Self::RangeBound {
                dst,
                value,
                range,
                bound,
                upper,
            } => BytecodeInstruction::RangeBound {
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
            } => BytecodeInstruction::MakeRange {
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
            } => BytecodeInstruction::RepeatArray {
                element: (element).map_identities(mapper)?,
                dst: *(dst),
                value: *(value),
                count: *(count),
            },
            Self::MakeArray {
                element,
                dst,
                elements,
            } => BytecodeInstruction::MakeArray {
                element: (element).map_identities(mapper)?,
                dst: *(dst),
                elements: (elements).clone(),
            },
            Self::MakeClosure {
                dst,
                function,
                captures,
            } => BytecodeInstruction::MakeClosure {
                dst: *(dst),
                function: *(function),
                captures: (captures).clone(),
            },
            Self::MakeCell { dst, value } => BytecodeInstruction::MakeCell {
                dst: *(dst),
                value: *(value),
            },
            Self::ReadCell { dst, cell } => BytecodeInstruction::ReadCell {
                dst: *(dst),
                cell: *(cell),
            },
            Self::WriteCell { cell, value } => BytecodeInstruction::WriteCell {
                cell: *(cell),
                value: *(value),
            },
            Self::UpcastInterface {
                dst,
                value,
                source,
                target,
            } => BytecodeInstruction::UpcastInterface {
                dst: *(dst),
                value: *(value),
                source: (source).map_identities(mapper)?,
                target: (target).map_identities(mapper)?,
            },
            Self::MakeInterface {
                dst,
                value,
                module,
                implementation,
                arguments,
            } => BytecodeInstruction::MakeInterface {
                dst: *(dst),
                value: *(value),
                module: *(module),
                implementation: *(implementation),
                arguments: map_sequence(arguments, |value| (value).map_identities(mapper))?,
            },
            Self::MakeStruct {
                dst,
                structure,
                arguments,
                fields,
            } => BytecodeInstruction::MakeStruct {
                dst: *(dst),
                structure: *(structure),
                arguments: map_sequence(arguments, |value| (value).map_identities(mapper))?,
                fields: (fields).clone(),
            },
            Self::MakeEnum {
                dst,
                enumeration,
                arguments,
                variant,
                fields,
            } => BytecodeInstruction::MakeEnum {
                dst: *(dst),
                enumeration: *(enumeration),
                arguments: map_sequence(arguments, |value| (value).map_identities(mapper))?,
                variant: *(variant),
                fields: (fields).clone(),
            },
            Self::TestEnumVariant {
                dst,
                value,
                enumeration,
                arguments,
                variant,
            } => BytecodeInstruction::TestEnumVariant {
                dst: *(dst),
                value: *(value),
                enumeration: *(enumeration),
                arguments: map_sequence(arguments, |value| (value).map_identities(mapper))?,
                variant: *(variant),
            },
            Self::ReadEnumPayload {
                dst,
                value,
                enumeration,
                arguments,
                variant,
                index,
            } => BytecodeInstruction::ReadEnumPayload {
                dst: *(dst),
                value: *(value),
                enumeration: *(enumeration),
                arguments: map_sequence(arguments, |value| (value).map_identities(mapper))?,
                variant: *(variant),
                index: *(index),
            },
            Self::ReadAggregateField { dst, base, field } => {
                BytecodeInstruction::ReadAggregateField {
                    dst: *(dst),
                    base: *(base),
                    field: (field).map_identities(mapper)?,
                }
            }
            Self::WriteAggregateField { base, field, value } => {
                BytecodeInstruction::WriteAggregateField {
                    base: *(base),
                    field: (field).map_identities(mapper)?,
                    value: *(value),
                }
            }
            Self::ReadAggregateIndex { dst, base, index } => {
                BytecodeInstruction::ReadAggregateIndex {
                    dst: *(dst),
                    base: *(base),
                    index: *(index),
                }
            }
            Self::WriteAggregateIndex { base, index, value } => {
                BytecodeInstruction::WriteAggregateIndex {
                    base: *(base),
                    index: *(index),
                    value: *(value),
                }
            }
            Self::ReadPath {
                dst,
                root_or_view,
                path,
                dynamic_args,
            } => BytecodeInstruction::ReadPath {
                dst: *(dst),
                root_or_view: *(root_or_view),
                path: *(path),
                dynamic_args: (dynamic_args).clone(),
            },
            Self::SetPath {
                root_or_view,
                path,
                dynamic_args,
                value,
            } => BytecodeInstruction::SetPath {
                root_or_view: *(root_or_view),
                path: *(path),
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
            } => BytecodeInstruction::ModifyPath {
                dst: *(dst),
                root_or_view: *(root_or_view),
                path: *(path),
                dynamic_args: (dynamic_args).clone(),
                op: *(op),
                value: *(value),
            },
            Self::MakePathView {
                dst,
                root_or_view,
                path,
                dynamic_args,
            } => BytecodeInstruction::MakePathView {
                dst: *(dst),
                root_or_view: *(root_or_view),
                path: *(path),
                dynamic_args: (dynamic_args).clone(),
            },
            Self::Jump { target } => BytecodeInstruction::Jump { target: *(target) },
            Self::Branch {
                cond,
                then_target,
                else_target,
            } => BytecodeInstruction::Branch {
                cond: *(cond),
                then_target: *(then_target),
                else_target: *(else_target),
            },
            Self::Return(field0) => BytecodeInstruction::Return(*(field0)),
            Self::Unreachable => BytecodeInstruction::Unreachable,
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
            Self::Await { future, .. } | Self::MakeFuture { future, .. } => {
                future.visit_definitions(visit, cancel)?
            }
            Self::Iter {
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
                module: _,
                implementation: _,
                arguments,
            } => {
                for value0 in arguments {
                    (value0).visit_definitions(visit, cancel)?;
                }
            }
            Self::MakeStruct {
                dst: _,
                structure: _,
                arguments,
                fields: _,
            } => {
                for value0 in arguments {
                    (value0).visit_definitions(visit, cancel)?;
                }
            }
            Self::MakeEnum {
                dst: _,
                enumeration: _,
                arguments,
                variant: _,
                fields: _,
            } => {
                for value0 in arguments {
                    (value0).visit_definitions(visit, cancel)?;
                }
            }
            Self::TestEnumVariant {
                dst: _,
                value: _,
                enumeration: _,
                arguments,
                variant: _,
            } => {
                for value0 in arguments {
                    (value0).visit_definitions(visit, cancel)?;
                }
            }
            Self::ReadEnumPayload {
                dst: _,
                value: _,
                enumeration: _,
                arguments,
                variant: _,
                index: _,
            } => {
                for value0 in arguments {
                    (value0).visit_definitions(visit, cancel)?;
                }
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
                path: _,
                dynamic_args: _,
            } => {}
            Self::SetPath {
                root_or_view: _,
                path: _,
                dynamic_args: _,
                value: _,
            } => {}
            Self::ModifyPath {
                dst: _,
                root_or_view: _,
                path: _,
                dynamic_args: _,
                op: _,
                value: _,
            } => {}
            Self::MakePathView {
                dst: _,
                root_or_view: _,
                path: _,
                dynamic_args: _,
            } => {}
            Self::Jump { target: _ } => {}
            Self::Branch {
                cond: _,
                then_target: _,
                else_target: _,
            } => {}
            Self::Return(_) => {}
            Self::Unreachable => {}
        }
        Ok(())
    }
}
