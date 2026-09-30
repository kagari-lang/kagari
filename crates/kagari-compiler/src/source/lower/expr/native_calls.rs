//! Existing engine operation lowering, selected from checked callable bindings.
//! Callback algorithm expansion remains here until the runtime continuation migration.

use crate::source::lower::{
    MirLoweringError, expr::native_contracts::NativeApplication, state::FunctionLowerer,
};
use kagari_abi::{
    callable::{EngineNativeBinding, NativeCall},
    operations::{IterOp, StringIterKind},
    representation::ValueType,
    scalar::BuiltinType,
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
        if matches!(
            intrinsic,
            StandardIntrinsic::ArrayRetain
                | StandardIntrinsic::MapRetain
                | StandardIntrinsic::SetRetain
                | StandardIntrinsic::ArraySort
                | StandardIntrinsic::ArraySortBy
                | StandardIntrinsic::ArraySortByKey
                | StandardIntrinsic::ArrayDedup
        ) {
            let base = receiver
                .or_else(|| args.first().copied())
                .ok_or(MirLoweringError::MissingBinding("collection receiver"))?;
            let receiver = self
                .analyzed
                .typed
                .type_table
                .expr_type(base)
                .ok_or(MirLoweringError::MissingExprType(base))?;
            let receiver = self
                .planner
                .arguments(&[receiver], &self.instance.substitution, span)?
                .remove(0);
            let callback_type = if intrinsic == StandardIntrinsic::ArraySortByKey {
                let site = *args
                    .last()
                    .ok_or(MirLoweringError::MissingBinding("sort callback"))?;
                let ty = self
                    .analyzed
                    .typed
                    .type_table
                    .coerced_expr_type(site)
                    .ok_or(MirLoweringError::MissingExprType(site))?;
                Some(
                    self.planner
                        .arguments(&[ty], &self.instance.substitution, span)?
                        .remove(0),
                )
            } else {
                None
            };
            return self.lower_prepared_collection(
                intrinsic,
                &receiver,
                &lowered,
                callback_type.as_ref(),
            );
        }

        if matches!(
            intrinsic,
            StandardIntrinsic::MapGetOrInsertWith | StandardIntrinsic::MapUpdate
        ) {
            let base = receiver
                .or_else(|| args.first().copied())
                .ok_or(MirLoweringError::MissingBinding("map receiver"))?;
            let receiver = self
                .analyzed
                .typed
                .type_table
                .expr_type(base)
                .ok_or(MirLoweringError::MissingExprType(base))?;
            let receiver = self
                .planner
                .arguments(&[receiver], &self.instance.substitution, span)?
                .remove(0);
            return self.lower_map_update(intrinsic, &receiver, &lowered);
        }

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
            StandardIntrinsic::ArrayCopyFrom | StandardIntrinsic::ArrayExtend
        ) {
            let source_expr = *args
                .last()
                .ok_or(MirLoweringError::MissingBinding("copy source"))?;
            let source = self
                .analyzed
                .typed
                .type_table
                .interface_coercion(source_expr)
                .map(|coercion| TypeId::Trait(coercion.interface_type.clone()))
                .or_else(|| self.analyzed.typed.type_table.expr_type(source_expr))
                .ok_or(MirLoweringError::MissingExprType(source_expr))?;
            let source = self
                .planner
                .arguments(
                    &[source],
                    &self.instance.substitution,
                    self.function.debug.source_span,
                )?
                .remove(0);
            return self.lower_list_copy(
                source,
                lowered[0],
                lowered[1],
                intrinsic == StandardIntrinsic::ArrayExtend,
            );
        }

        if matches!(
            intrinsic,
            StandardIntrinsic::ArrayCopyWithin | StandardIntrinsic::ArrayRemoveRange
        ) {
            let input = args[usize::from(receiver.is_none())];
            let source = self
                .analyzed
                .typed
                .type_table
                .expr_type(input)
                .ok_or(MirLoweringError::MissingExprType(input))?;
            let source = self
                .planner
                .arguments(&[source], &self.instance.substitution, span)?
                .remove(0);
            let mut interface = StandardTrait::RangeBounds.nominal();
            interface
                .arguments
                .push(TypeId::Builtin(BuiltinType::USize));
            let start = self.lower_applied_operator(
                interface.clone(),
                source.clone(),
                &self.protocol_method(StandardTrait::RangeBounds, 0)?,
                &[lowered[1]],
            )?;
            let end = self.lower_applied_operator(
                interface,
                source,
                &self.protocol_method(StandardTrait::RangeBounds, 1)?,
                &[lowered[1]],
            )?;
            if intrinsic == StandardIntrinsic::ArrayRemoveRange {
                let base = receiver
                    .or_else(|| args.first().copied())
                    .ok_or(MirLoweringError::MissingBinding("array receiver"))?;
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
                let TypeId::Array(item, _) = &ty else {
                    return Err(MirLoweringError::MissingBinding("array storage"));
                };
                self.emit_intrinsic(
                    StandardIntrinsic::CollectionMutationBegin,
                    &[lowered[0]],
                    ValueType::Unit,
                );
                let prepared = self.emit_intrinsic(
                    StandardIntrinsic::ArrayRemoveRangePrepare,
                    &[lowered[0], start, end],
                    ValueType::HeapObject,
                );
                let remaining = self.prepared_field(prepared, 0, &ty)?;
                let removed = self.prepared_field(prepared, 1, &ty)?;
                let result = self.readonly_array((**item).clone(), removed)?;
                self.emit_intrinsic(
                    StandardIntrinsic::CollectionMutationEnd,
                    &[lowered[0]],
                    ValueType::Unit,
                );
                self.emit_intrinsic(
                    StandardIntrinsic::ArrayReplaceStorage,
                    &[lowered[0], remaining],
                    ValueType::Unit,
                );
                return Ok(result);
            }
            return Ok(self.emit_intrinsic(
                StandardIntrinsic::ArrayCopyWithinBounds,
                &[lowered[0], start, end, lowered[2]],
                ValueType::Unit,
            ));
        }
        if matches!(
            intrinsic,
            StandardIntrinsic::ArrayListFrom
                | StandardIntrinsic::LinkedHashMapFrom
                | StandardIntrinsic::LinkedHashSetFrom
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
            let key = match &ty {
                TypeId::Map { key, .. } | TypeId::Set(key, _) => Some(&**key),
                _ => None,
            };
            if let Some(key) = key
                && self.has_custom_protocol(key)?
                && matches!(
                    intrinsic,
                    StandardIntrinsic::MapGet
                        | StandardIntrinsic::MapContainsKey
                        | StandardIntrinsic::MapInsert
                        | StandardIntrinsic::MapRemove
                        | StandardIntrinsic::SetContains
                        | StandardIntrinsic::SetInsert
                        | StandardIntrinsic::SetRemove
                )
            {
                return self.lower_key_operation(intrinsic, key, &lowered);
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
