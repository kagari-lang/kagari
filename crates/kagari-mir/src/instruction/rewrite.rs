//! Operand rewriting preserves instruction policy and source evaluation order.
use crate::instruction::{CallTarget, Instruction, MirValue, Terminator};
use kagari_common::identity::reference::DefinitionReference;

impl<I: DefinitionReference> Instruction<I> {
    pub(crate) fn rewrite_inputs(&mut self, mut rewrite: impl FnMut(&mut MirValue)) {
        match self {
            Self::LoadConst { .. }
            | Self::LoadLocal { .. }
            | Self::LoadModule { .. }
            | Self::EndIteration => {}
            Self::Convert { src, .. }
            | Self::StoreLocal { src, .. }
            | Self::StoreModule { src, .. }
            | Self::Move { src, .. } => rewrite(src),
            Self::Unary { operand, .. } => rewrite(operand),
            Self::Numeric { lhs, rhs, .. } => {
                rewrite(lhs);
                if let Some(rhs) = rhs {
                    rewrite(rhs);
                }
            }
            Self::Iter { value, .. } => {
                if let Some(value) = value {
                    rewrite(value);
                }
            }
            Self::Binary { lhs, rhs, .. } => {
                rewrite(lhs);
                rewrite(rhs);
            }
            Self::Call { callee, args, .. } => {
                match callee {
                    CallTarget::Value(value) | CallTarget::Closure { value, .. } => rewrite(value),
                    _ => {}
                }
                for value in args {
                    rewrite(value);
                }
            }
            Self::BeginIteration { collection } => rewrite(collection),
            Self::MakeTuple { elements, .. }
            | Self::MakeArray { elements, .. }
            | Self::MakeEnum {
                fields: elements, ..
            }
            | Self::MakeClosure {
                captures: elements, ..
            } => {
                for value in elements {
                    rewrite(value);
                }
            }
            Self::MakeFuture { arguments, .. } => {
                for value in arguments {
                    rewrite(value);
                }
            }
            Self::Await { value, .. }
            | Self::RangeBound { value, .. }
            | Self::MakeCell { value, .. }
            | Self::MakeInterface { value, .. }
            | Self::UpcastInterface { value, .. }
            | Self::TestEnumVariant { value, .. }
            | Self::ReadEnumPayload { value, .. } => rewrite(value),
            Self::MakeRange { start, end, .. } => {
                if let Some(value) = start {
                    rewrite(value);
                }
                if let Some(value) = end {
                    rewrite(value);
                }
            }
            Self::RepeatArray { value, count, .. } => {
                rewrite(value);
                rewrite(count);
            }
            Self::ReadCell { cell, .. } => rewrite(cell),
            Self::WriteCell { cell, value } => {
                rewrite(cell);
                rewrite(value);
            }
            Self::MakeStruct { fields, .. } => {
                for field in fields {
                    rewrite(&mut field.value);
                }
            }
            Self::ReadAggregateField { base, .. } => rewrite(base),
            Self::WriteAggregateField { base, value, .. } => {
                rewrite(base);
                rewrite(value);
            }
            Self::ReadAggregateIndex { base, index, .. } => {
                rewrite(base);
                rewrite(index);
            }
            Self::WriteAggregateIndex {
                base, index, value, ..
            } => {
                rewrite(base);
                rewrite(index);
                rewrite(value);
            }
            Self::ReadPath {
                root_or_view,
                dynamic_args,
                ..
            }
            | Self::MakePathView {
                root_or_view,
                dynamic_args,
                ..
            } => {
                rewrite(root_or_view);
                for arg in dynamic_args {
                    rewrite(arg);
                }
            }
            Self::SetPath {
                root_or_view,
                dynamic_args,
                value,
                ..
            }
            | Self::ModifyPath {
                root_or_view,
                dynamic_args,
                value,
                ..
            } => {
                rewrite(root_or_view);
                for arg in dynamic_args {
                    rewrite(arg);
                }
                rewrite(value);
            }
        }
    }
}

impl Terminator {
    pub(crate) fn rewrite_input(&mut self, rewrite: impl FnOnce(&mut MirValue)) {
        match self {
            Self::Return(Some(value)) | Self::Branch { cond: value, .. } => rewrite(value),
            Self::Return(None) | Self::Jump(_) | Self::Unreachable => {}
        }
    }
}
