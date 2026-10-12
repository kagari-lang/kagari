use crate::source::lower::{MirLoweringError, state::FunctionLowerer};
use bincode::{DefaultOptions, Options};
use kagari_abi::representation::ValueType;
use kagari_common::identity::{mapping::DefinitionMappingError, scoped::DefinitionReferences};
use kagari_contract::{operations::BinaryOp, standard::RuntimePrimitive};
use kagari_hir::{
    aggregates::implementations::{ImplementationSearchError, ImplementationSearchLimits},
    language::semantics::ProtocolSemantics,
    types::{TypeId, semantic::lower_nominal_type},
};
use kagari_mir::instruction::{
    CallTarget, Constant, Instruction, MirValue, Terminator, ValueBuffer,
};
use kagari_types::{declaration::wire::leaf::NominalTyRecord, language::Protocol};
use std::{collections::HashSet, fmt::Write};

impl FunctionLowerer<'_, '_> {
    pub(super) fn has_custom_protocol(&self, ty: &TypeId) -> Result<bool, MirLoweringError> {
        let mut pending = vec![ty.clone()];
        let mut seen = HashSet::new();
        while let Some(ty) = pending.pop() {
            self.planner.check()?;
            if !seen.insert(ty.clone()) {
                continue;
            }
            if seen.len() > self.planner.options.max_type_nodes {
                return Err(MirLoweringError::UnsupportedExpr(
                    "protocol composition exceeds type node limit",
                ));
            }
            match &ty {
                TypeId::Trait(_) => return Ok(true),
                TypeId::NativeObject(_)
                | TypeId::Struct(_)
                | TypeId::Enum(_)
                | TypeId::Map { .. }
                | TypeId::Set(_, _) => {
                    // Implicit identity/member equality does not import a
                    // protocol declaration. Only an actual checked override
                    // requires its method contract during call lowering.
                    if self
                        .planner
                        .catalog
                        .concrete_interface_implementation(
                            &self.planner.definitions(),
                            &Protocol::PartialEq
                                .nominal(&self.planner.definitions())
                                .expect("registered protocol identity"),
                            &ty,
                            &Default::default(),
                            ImplementationSearchLimits {
                                max_checks: self.planner.options.max_type_nodes,
                                max_depth: self.planner.options.max_type_depth,
                            },
                            &self.planner.options.cancel,
                        )
                        .map_err(|error| match error {
                            ImplementationSearchError::Cancelled => MirLoweringError::Cancelled,
                            ImplementationSearchError::LimitExceeded => {
                                MirLoweringError::UnsupportedExpr(
                                    "protocol implementation search exceeds type limits",
                                )
                            }
                        })?
                        .is_some()
                    {
                        return Ok(true);
                    }
                    if let TypeId::Enum(n) = &ty
                        && let Some(contract) = self.planner.catalog.enumeration(&n.declaration)
                    {
                        let substitution = contract
                            .generic_params
                            .iter()
                            .cloned()
                            .zip(n.arguments.iter().cloned())
                            .collect();
                        pending.extend(
                            contract.variants.iter().flat_map(|v| {
                                v.payload.iter().map(|t| t.instantiate(&substitution))
                            }),
                        );
                    }
                }
                TypeId::Tuple(members) => pending.extend(members.iter().cloned()),
                _ => {}
            }
        }
        Ok(false)
    }

    pub(crate) fn emit_intrinsic(
        &mut self,
        intrinsic: RuntimePrimitive,
        args: &[MirValue],
        ty: ValueType,
    ) -> MirValue {
        let dst = self.alloc_temp(ty);
        self.emit(Instruction::Call {
            dst: Some(dst),
            callee: CallTarget::RuntimePrimitive(intrinsic),
            args: args.iter().copied().collect(),
        });
        dst
    }

    pub(crate) fn lower_protocol(
        &mut self,
        protocol: Protocol,
        ty: &TypeId,
        args: &[MirValue],
        depth: usize,
    ) -> Result<MirValue, MirLoweringError> {
        if matches!(ty, TypeId::Tuple(_) | TypeId::Enum(_))
            && self.has_custom_protocol(ty)?
            && self
                .planner
                .catalog
                .implementation_method(
                    &self.planner.definitions(),
                    &self.protocol_method(protocol, 0)?,
                    &protocol
                        .nominal(&self.planner.definitions())
                        .expect("registered protocol identity"),
                    ty,
                )
                .is_none()
        {
            let function = self.planner.enqueue_protocol(
                &self.instance,
                protocol,
                ty,
                self.function.debug.source_span,
            )?;
            let dst = self.alloc_temp(if protocol == Protocol::PartialEq {
                ValueType::Bool
            } else {
                ValueType::I64
            });
            self.emit(Instruction::Call {
                dst: Some(dst),
                callee: CallTarget::Function(function),
                args: args.iter().copied().collect(),
            });
            Ok(dst)
        } else {
            self.lower_protocol_body(protocol, ty, args, depth)
        }
    }

    pub(crate) fn lower_protocol_body(
        &mut self,
        protocol: Protocol,
        ty: &TypeId,
        args: &[MirValue],
        depth: usize,
    ) -> Result<MirValue, MirLoweringError> {
        self.planner.check()?;
        if depth > self.planner.options.max_type_depth {
            return Err(MirLoweringError::UnsupportedExpr(
                "protocol composition exceeds type depth limit",
            ));
        }
        if protocol == Protocol::PartialEq && matches!(ty, TypeId::Trait(_)) {
            return Ok(self.emit_intrinsic(RuntimePrimitive::ValueEq, args, ValueType::Bool));
        }
        let result_ty = if protocol == Protocol::PartialEq {
            ValueType::Bool
        } else {
            ValueType::I64
        };
        if !self.has_custom_protocol(ty)? {
            if protocol == Protocol::PartialEq {
                let dst = self.alloc_temp(ValueType::Bool);
                self.emit(Instruction::Binary {
                    dst,
                    op: BinaryOp::Eq,
                    lhs: args[0],
                    rhs: args[1],
                });
                return Ok(dst);
            }
            return Ok(self.emit_intrinsic(RuntimePrimitive::ValueHash, args, result_ty));
        }
        let method = &self.protocol_method(protocol, 0)?;
        let interface = protocol
            .nominal(&self.planner.definitions())
            .expect("registered protocol identity");
        if self
            .planner
            .catalog
            .implementation_method(&self.planner.definitions(), method, &interface, ty)
            .is_some()
        {
            return self.lower_applied_method(interface, ty.clone(), method, &[], args);
        }
        match ty {
            TypeId::Tuple(members) => {
                let mut inputs = Vec::new();
                for (index, member) in members.iter().enumerate() {
                    let index = self.lower_constant(Constant::I32(index as i32), ValueType::I32);
                    let mut values = Vec::new();
                    for base in args {
                        let dst = self.alloc_temp(self.value_type(member)?);
                        self.emit(Instruction::ReadAggregateIndex {
                            dst,
                            base: *base,
                            index,
                        });
                        values.push(dst);
                    }
                    inputs.push((member.clone(), values));
                }
                self.combine_protocol(protocol, &inputs, None, depth)
            }
            TypeId::Enum(nominal) => {
                let contract = self
                    .planner
                    .catalog
                    .enumeration(&nominal.declaration)
                    .ok_or(MirLoweringError::MissingBinding("enum protocol contract"))?
                    .clone();
                let substitution = contract
                    .generic_params
                    .iter()
                    .cloned()
                    .zip(nominal.arguments.iter().cloned())
                    .collect();
                let variants = contract
                    .variants
                    .iter()
                    .map(|v| {
                        (
                            v.slot,
                            v.payload
                                .iter()
                                .map(|ty| ty.instantiate(&substitution))
                                .collect::<Vec<_>>(),
                        )
                    })
                    .collect::<Vec<_>>();
                self.enum_protocol(protocol, ty, args, &variants, depth)
            }
            _ => {
                let dst = self.alloc_temp(result_ty);
                let intrinsic = if protocol == Protocol::PartialEq {
                    RuntimePrimitive::ValueEq
                } else {
                    RuntimePrimitive::ValueHash
                };
                self.emit(Instruction::Call {
                    dst: Some(dst),
                    callee: CallTarget::RuntimePrimitive(intrinsic),
                    args: args.iter().copied().collect(),
                });
                Ok(dst)
            }
        }
    }

    fn combine_protocol(
        &mut self,
        protocol: Protocol,
        inputs: &[(TypeId, Vec<MirValue>)],
        tag: Option<Constant>,
        depth: usize,
    ) -> Result<MirValue, MirLoweringError> {
        if protocol == Protocol::Hash {
            let mut hashes = ValueBuffer::new();
            if let Some(tag) = tag {
                let ty = if matches!(tag, Constant::Str(_)) {
                    ValueType::Str
                } else {
                    ValueType::I64
                };
                hashes.push(self.lower_constant(tag, ty));
            }
            for (ty, args) in inputs {
                hashes.push(self.lower_protocol(protocol, ty, args, depth + 1)?);
            }
            let tuple = self.alloc_temp(ValueType::HeapObject);
            self.emit(Instruction::MakeTuple {
                dst: tuple,
                elements: hashes,
            });
            let dst = self.alloc_temp(ValueType::I64);
            self.emit(Instruction::Call {
                dst: Some(dst),
                callee: CallTarget::RuntimePrimitive(RuntimePrimitive::ValueHash),
                args: [tuple].into_iter().collect(),
            });
            return Ok(dst);
        }
        let dst = self.alloc_temp(ValueType::Bool);
        let fail = self.new_block();
        let join = self.new_block();
        for (ty, args) in inputs {
            let cond = self.lower_protocol(protocol, ty, args, depth + 1)?;
            let next = self.new_block();
            self.set_terminator(Terminator::Branch {
                cond,
                then_block: next,
                else_block: fail,
            });
            self.switch_to_block(next);
        }
        let yes = self.lower_constant(Constant::Bool(true), ValueType::Bool);
        self.emit(Instruction::Move { dst, src: yes });
        self.set_terminator(Terminator::Jump(join));
        self.switch_to_block(fail);
        let no = self.lower_constant(Constant::Bool(false), ValueType::Bool);
        self.emit(Instruction::Move { dst, src: no });
        self.set_terminator(Terminator::Jump(join));
        self.switch_to_block(join);
        Ok(dst)
    }

    fn enum_protocol(
        &mut self,
        protocol: Protocol,
        ty: &TypeId,
        args: &[MirValue],
        variants: &[(usize, Vec<TypeId>)],
        depth: usize,
    ) -> Result<MirValue, MirLoweringError> {
        let dst = self.alloc_temp(if protocol == Protocol::PartialEq {
            ValueType::Bool
        } else {
            ValueType::I64
        });
        let join = self.new_block();
        let fail = self.new_block();
        for (variant, members) in variants {
            let matched = self.new_block();
            let next = self.new_block();
            let cond = self.enum_test(ty, args[0], *variant)?;
            self.set_terminator(Terminator::Branch {
                cond,
                then_block: matched,
                else_block: next,
            });
            self.switch_to_block(matched);
            if protocol == Protocol::PartialEq {
                let cond = self.enum_test(ty, args[1], *variant)?;
                let payload = self.new_block();
                self.set_terminator(Terminator::Branch {
                    cond,
                    then_block: payload,
                    else_block: fail,
                });
                self.switch_to_block(payload);
            }
            let mut inputs = Vec::new();
            for (index, member) in members.iter().enumerate() {
                let values = args
                    .iter()
                    .map(|arg| self.enum_member(ty, *arg, *variant, index, member))
                    .collect::<Result<Vec<_>, _>>()?;
                inputs.push((member.clone(), values));
            }
            // Declaration identity, not the current slot, survives variant reordering.
            let tag = if let TypeId::Enum(nominal) = ty {
                let declaration = &self
                    .planner
                    .catalog
                    .enumeration(&nominal.declaration)
                    .ok_or(MirLoweringError::MissingBinding("enum hash identity"))?
                    .variants[*variant]
                    .id;
                // Hash tags encode reachable symbolic identities canonically; process-local
                // table/index values and declaration insertion order never affect the tag.
                let nominal = lower_nominal_type(nominal);
                let definitions = self.planner.definitions();
                let cancel = &self.planner.options.cancel;
                let mut references = HashSet::from([*declaration]);
                nominal.visit_definition_ids(
                    &mut |id| {
                        references.insert(*id);
                        Ok(())
                    },
                    cancel,
                )?;
                let encoding = definitions
                    .encode(references)
                    .map_err(DefinitionMappingError::from)?;
                let nominal = NominalTyRecord::encode(&nominal, &encoding, cancel)?;
                let variant = encoding
                    .reference(*declaration)
                    .map_err(DefinitionMappingError::from)?;
                let bytes = DefaultOptions::new()
                    .with_fixint_encoding()
                    .with_little_endian()
                    .serialize(&(encoding.table, nominal, variant))
                    .expect("validated enum identity");
                let mut encoded = String::new();
                for byte in bytes {
                    write!(encoded, "{byte:02x}").expect("String write");
                }
                Constant::Str(encoded)
            } else {
                Constant::I64(*variant as i64)
            };
            let result = self.combine_protocol(protocol, &inputs, Some(tag), depth)?;
            self.emit(Instruction::Move { dst, src: result });
            self.set_terminator(Terminator::Jump(join));
            self.switch_to_block(next);
        }
        self.set_terminator(Terminator::Jump(fail));
        self.switch_to_block(fail);
        let zero = if protocol == Protocol::PartialEq {
            self.lower_constant(Constant::Bool(false), ValueType::Bool)
        } else {
            self.lower_constant(Constant::I64(0), ValueType::I64)
        };
        self.emit(Instruction::Move { dst, src: zero });
        self.set_terminator(Terminator::Jump(join));
        self.switch_to_block(join);
        Ok(dst)
    }

    fn enum_test(
        &mut self,
        ty: &TypeId,
        value: MirValue,
        variant: usize,
    ) -> Result<MirValue, MirLoweringError> {
        self.test_enum_variant(ty, value, variant)
    }

    fn enum_member(
        &mut self,
        ty: &TypeId,
        value: MirValue,
        variant: usize,
        index: usize,
        member: &TypeId,
    ) -> Result<MirValue, MirLoweringError> {
        let _ = member; // The declaration owns the payload type.
        self.read_enum_field(ty, value, variant, index)
    }
}
