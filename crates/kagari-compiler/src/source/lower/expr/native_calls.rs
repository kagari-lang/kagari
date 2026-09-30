//! Existing engine operation lowering, selected from checked callable bindings.
//! Callback algorithm expansion remains here until the runtime continuation migration.

use crate::source::lower::{
    MirLoweringError, expr::native_contracts::NativeApplication, state::FunctionLowerer,
};
use kagari_abi::{
    callable::{EngineNativeBinding, NativeCall},
    operations::{IterOp, StringIterKind},
    representation::ValueType,
    standard::{StandardIntrinsic, surface::StandardEnum, traits::StandardTrait},
};
use kagari_hir::{
    builtin::traits::StandardTraitSemantics,
    callable::AppliedCallSignature,
    hir,
    native::NativeBinding,
    typeck::{CallTarget, FunctionImplementation},
    types::TypeId,
};
use kagari_mir::instruction::{CallTarget as MirCallTarget, Instruction, MirValue, ValueBuffer};

impl FunctionLowerer<'_, '_> {
    pub(super) fn engine_intrinsic_for_call(
        &self,
        target: &CallTarget,
        application: Option<&AppliedCallSignature>,
    ) -> Result<Option<StandardIntrinsic>, MirLoweringError> {
        let signature = match target {
            CallTarget::Function(id) => self
                .analyzed
                .typed
                .functions
                .iter()
                .find(|function| function.id == *id)
                .ok_or(MirLoweringError::MissingTypedFunction(*id))?,
            CallTarget::SourceFunction(id) => {
                &self
                    .analyzed
                    .imported_functions
                    .target(id)
                    .ok_or(MirLoweringError::MissingBinding("checked source callable"))?
                    .signature
            }
            _ => return Ok(None),
        };
        let binding = match signature.implementation {
            FunctionImplementation::Script => return Ok(None),
            FunctionImplementation::Required => {
                return Err(MirLoweringError::MissingBinding(
                    "unimplemented callable requirement",
                ));
            }
            FunctionImplementation::Native(NativeBinding::Engine(binding)) => binding,
            FunctionImplementation::Native(NativeBinding::Host(_)) => {
                return Err(MirLoweringError::MissingBinding(
                    "source callable has a host binding",
                ));
            }
        };
        let application = application.ok_or(MirLoweringError::MissingBinding(
            "checked native callable application",
        ))?;
        let intrinsic = match binding {
            EngineNativeBinding::Intrinsic(intrinsic) => intrinsic,
            EngineNativeBinding::Integer(method) => {
                let Some(TypeId::Builtin(scalar)) = application.params.first() else {
                    return Err(MirLoweringError::MissingBinding("checked integer receiver"));
                };
                if scalar.integer_layout().is_none() {
                    return Err(MirLoweringError::MissingBinding("checked integer receiver"));
                }
                StandardIntrinsic::Integer(method, *scalar)
            }
            EngineNativeBinding::ParseRadix => {
                let TypeId::StandardEnum {
                    kind: StandardEnum::Result,
                    args,
                } = &application.return_type
                else {
                    return Err(MirLoweringError::MissingBinding("checked radix result"));
                };
                let Some(TypeId::Builtin(scalar)) = args.first() else {
                    return Err(MirLoweringError::MissingBinding("checked radix result"));
                };
                if scalar.integer_layout().is_none() {
                    return Err(MirLoweringError::MissingBinding("checked radix result"));
                }
                StandardIntrinsic::ParseRadix(*scalar)
            }
            EngineNativeBinding::TraitDefault(_) | EngineNativeBinding::Protocol(_) => {
                return Err(MirLoweringError::MissingBinding(
                    "native trait callable witness",
                ));
            }
        };
        Ok(Some(intrinsic))
    }

    pub(super) fn lower_engine_intrinsic(
        &mut self,
        expr: hir::ExprId,
        intrinsic: StandardIntrinsic,
        receiver: Option<hir::ExprId>,
        args: &[hir::ExprId],
        lowered: ValueBuffer,
        application: NativeApplication<'_>,
    ) -> Result<MirValue, MirLoweringError> {
        let span = self.analyzed.lowered.source_map.expr_span(expr);
        if intrinsic == StandardIntrinsic::StringParse {
            let output = self
                .analyzed
                .typed
                .type_table
                .expr_type(expr)
                .ok_or(MirLoweringError::MissingExprType(expr))?;
            let output = self
                .planner
                .arguments(&[output], &self.instance.substitution, span)?
                .remove(0);
            let TypeId::StandardEnum { args: members, .. } = output else {
                return Err(MirLoweringError::MissingBinding("parse result"));
            };
            return self.lower_applied_operator(
                StandardTrait::FromStr.nominal(),
                members[0].clone(),
                &self.protocol_method(StandardTrait::FromStr, 0)?,
                &lowered,
            );
        }

        let string_iteration = match intrinsic {
            StandardIntrinsic::StringBytes => Some(StringIterKind::Bytes),
            StandardIntrinsic::StringCharIndices => Some(StringIterKind::CharIndices),
            StandardIntrinsic::StringSplit => Some(StringIterKind::Split),
            StandardIntrinsic::StringSplitN => Some(StringIterKind::SplitN),
            StandardIntrinsic::StringSplitWhitespace => Some(StringIterKind::Whitespace),
            StandardIntrinsic::StringLines => Some(StringIterKind::Lines),
            _ => None,
        };
        if let Some(kind) = string_iteration {
            let source = self.alloc_temp(ValueType::HeapObject);
            self.emit(Instruction::MakeTuple {
                dst: source,
                elements: lowered,
            });
            let dst = self.alloc_temp(ValueType::HeapObject);
            self.emit(Instruction::Iter {
                dst,
                value: Some(source),
                ty: kind.source_type(),
                op: IterOp::String(kind),
            });
            return Ok(dst);
        }

        if matches!(
            intrinsic,
            StandardIntrinsic::LinkedHashMapFrom | StandardIntrinsic::LinkedHashSetFrom
        ) {
            return self.lower_collection_factory(expr, lowered[0]);
        }
        let base = receiver.or_else(|| args.first().copied());
        if let Some(base) = base {
            let ty = self
                .analyzed
                .typed
                .type_table
                .expr_type(base)
                .ok_or(MirLoweringError::MissingExprType(base))?;
            let ty = self
                .planner
                .arguments(&[ty], &self.instance.substitution, span)?
                .remove(0);
            if intrinsic == StandardIntrinsic::DebugAssertEq {
                let equal = self.lower_protocol(StandardTrait::PartialEq, &ty, &lowered[..2], 0)?;
                return Ok(self.emit_intrinsic(
                    StandardIntrinsic::DebugAssert,
                    &[equal, lowered[2]],
                    ValueType::Unit,
                ));
            }
        }
        let contract = self.engine_native_contract(
            application.target,
            application.signature,
            application.arguments,
            span,
        )?;
        let dst = self.alloc_temp(self.expr_type(expr)?);
        self.emit(Instruction::Call {
            dst: Some(dst),
            callee: MirCallTarget::Native(NativeCall::Engine(Box::new(contract))),
            args: lowered,
        });
        Ok(dst)
    }
}
