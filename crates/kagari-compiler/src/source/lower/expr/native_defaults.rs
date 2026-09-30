//! Lower native trait defaults from their checked method application.
use crate::source::{
    lower::{MirLoweringError, abi::checked_bounds, state::FunctionLowerer},
    types::{raise_nominal_type, raise_type},
};
use kagari_abi::{
    callable::{EngineNativeBinding, NativeCall},
    native_import::{
        ENGINE_NATIVE_BINDING_VERSION, EngineNativeImport, NativeSignature, NativeWitness,
        NativeWitnessImplementation,
    },
    standard::{bindings::NativeDefaultMethod, traits::StandardTrait},
    types::{
        ConcreteFunctionIdentity, ConstraintAbi, substitution::TypeSubstitution as AbiSubstitution,
    },
};
use kagari_common::{
    collection::CollectionAccess,
    identity::{DefinitionId, associated_type_id},
};
use kagari_hir::{
    aggregates::MethodDefault,
    builtin::traits::StandardTraitSemantics,
    types::{
        NominalType, TypeId, TypeSubstitution,
        abi::{lower_nominal_type, lower_type},
    },
};
use kagari_mir::instruction::{CallTarget, Instruction, MirValue};

impl FunctionLowerer<'_, '_> {
    pub(super) fn lower_native_default(
        &mut self,
        receiver: &TypeId,
        interface: &NominalType,
        method: &DefinitionId,
        arguments: &[TypeId],
        values: &[MirValue],
    ) -> Result<MirValue, MirLoweringError> {
        let invalid = || MirLoweringError::MissingBinding("checked native trait default");
        let signature = self
            .planner
            .catalog
            .trait_method(method)
            .ok_or_else(invalid)?
            .clone();
        let owner = self
            .planner
            .catalog
            .trait_(&signature.owner)
            .ok_or_else(invalid)?
            .clone();
        let Some(MethodDefault::Native { binding, .. }) = signature.default else {
            return Err(invalid());
        };
        let all_arguments: Vec<_> = interface
            .arguments
            .iter()
            .chain(arguments)
            .cloned()
            .collect();
        if all_arguments.len() != signature.generic_params.len() {
            return Err(invalid());
        }
        let mut substitution: TypeSubstitution = signature
            .generic_params
            .iter()
            .cloned()
            .zip(all_arguments.iter().cloned())
            .collect();
        substitution.insert_receiver(owner.id.clone(), receiver.clone());
        let instantiate = |ty: &TypeId| {
            ty.with_self(&owner.id, receiver)
                .instantiate(&substitution)
                .with_associated_types(interface)
        };
        let span = self.function.debug.source_span;
        let params = self.planner.arguments(
            &signature
                .params
                .iter()
                .map(|param| instantiate(&param.ty))
                .collect::<Vec<_>>(),
            &Default::default(),
            span,
        )?;
        let result = self
            .planner
            .arguments(
                &[instantiate(&signature.return_type)],
                &Default::default(),
                span,
            )?
            .remove(0);
        let mut abi_substitution = AbiSubstitution::default();
        let abi_receiver = lower_type(receiver);
        let abi_arguments: Vec<_> = all_arguments.iter().map(lower_type).collect();
        abi_substitution.bind_receiver(&owner.id, &abi_receiver);
        for (parameter, argument) in signature.generic_params.iter().zip(&abi_arguments) {
            abi_substitution.bind(&parameter.owner, parameter.position, argument);
        }
        let mut requirements = abi_substitution
            .apply_bounds(
                &checked_bounds(&signature.bounds),
                &self.planner.options.cancel,
            )
            .map_err(|_| invalid())?;
        // Resolve associated outputs while still consuming checked HIR facts.
        for bound in &mut requirements {
            bound.ty = lower_type(&self.planner.catalog.normalize_type(&raise_type(&bound.ty)));
            for constraint in &mut bound.constraints {
                if let ConstraintAbi::Trait(applied) = constraint {
                    let TypeId::Trait(ty) = self
                        .planner
                        .catalog
                        .normalize_type(&TypeId::Trait(raise_nominal_type(applied)))
                    else {
                        return Err(invalid());
                    };
                    *applied = lower_nominal_type(&ty);
                }
            }
        }
        let mut applied = interface.clone();
        for id in owner.associated_types.keys() {
            let output = self.planner.catalog.normalize_type(&TypeId::Projection {
                receiver: Box::new(receiver.clone()),
                interface: Box::new(interface.clone()),
                member: id.clone(),
                arguments: vec![],
            });
            applied.associated_types.insert(id.clone(), output);
        }
        let mut witnesses = vec![self.lower_native_witness(receiver, &applied, &[])?];
        for bound in &requirements {
            for constraint in &bound.constraints {
                if let ConstraintAbi::Trait(interface) = constraint {
                    let receiver = raise_type(&bound.ty);
                    let interface = raise_nominal_type(interface);
                    if !witnesses.iter().any(|witness| {
                        witness.receiver == bound.ty
                            && witness.interface == lower_nominal_type(&interface)
                    }) {
                        let kind = StandardTrait::from_id(&interface.declaration);
                        let method_arguments = if kind.is_some_and(StandardTrait::aggregation)
                            || binding == NativeDefaultMethod::Collect
                                && kind == Some(StandardTrait::FromIterator)
                        {
                            params[..1].to_vec()
                        } else if kind == Some(StandardTrait::FromIterator) {
                            let [item] = interface.arguments.as_slice() else {
                                return Err(invalid());
                            };
                            vec![TypeId::Array(
                                Box::new(item.clone()),
                                CollectionAccess::Mutable,
                            )]
                        } else {
                            vec![]
                        };
                        witnesses.push(self.lower_native_witness(
                            &receiver,
                            &interface,
                            &method_arguments,
                        )?);
                    }
                }
            }
        }
        if matches!(
            binding,
            NativeDefaultMethod::ListLast
                | NativeDefaultMethod::ListBinarySearch
                | NativeDefaultMethod::ListContains
                | NativeDefaultMethod::ListStartsWith
                | NativeDefaultMethod::ListEndsWith
        ) {
            let sources: Vec<_> = if matches!(
                binding,
                NativeDefaultMethod::ListStartsWith | NativeDefaultMethod::ListEndsWith
            ) {
                vec![receiver.clone(), params[1].clone()]
            } else {
                vec![receiver.clone()]
            };
            for (slot, source) in sources.into_iter().enumerate() {
                if slot == 1
                    && let TypeId::Trait(list) = &source
                {
                    let witness = self.lower_native_witness(&source, list, &[])?;
                    if !witnesses.contains(&witness) {
                        witnesses.push(witness);
                    }
                }
                let mut iterable = StandardTrait::Iterable.nominal();
                for name in ["Item", "Iter"] {
                    let output = self.iteration_output(StandardTrait::Iterable, &source, name)?;
                    iterable
                        .associated_types
                        .insert(associated_type_id(&iterable.declaration, name), output);
                }
                let witness = self.lower_native_witness(&source, &iterable, &[])?;
                if !witnesses.contains(&witness) {
                    witnesses.push(witness);
                }
            }
        }
        if matches!(
            binding,
            NativeDefaultMethod::MapKeysView
                | NativeDefaultMethod::MapValuesView
                | NativeDefaultMethod::MapEntriesView
        ) {
            let mut iterable = StandardTrait::Iterable.nominal();
            for name in ["Item", "Iter"] {
                let output = self.iteration_output(StandardTrait::Iterable, receiver, name)?;
                iterable
                    .associated_types
                    .insert(associated_type_id(&iterable.declaration, name), output);
            }
            witnesses.push(self.lower_native_witness(receiver, &iterable, &[])?);
            let iterator = self.iteration_output(StandardTrait::Iterable, receiver, "Iter")?;
            let mut next = StandardTrait::Iterator.nominal();
            let item = self.iteration_output(StandardTrait::Iterable, receiver, "Item")?;
            next.associated_types
                .insert(associated_type_id(&next.declaration, "Item"), item);
            witnesses.push(self.lower_native_witness(&iterator, &next, &[])?);
            witnesses.push(self.native_list_result(&result)?);
        }
        if binding == NativeDefaultMethod::ListJoin {
            let iterator = self.iteration_output(StandardTrait::Iterable, receiver, "Iter")?;
            let item = self.iteration_output(StandardTrait::Iterable, receiver, "Item")?;
            let mut next = StandardTrait::Iterator.nominal();
            next.associated_types
                .insert(associated_type_id(&next.declaration, "Item"), item);
            let witness = self.lower_native_witness(&iterator, &next, &[])?;
            if !witnesses.contains(&witness) {
                witnesses.push(witness);
            }
        }
        self.native_lazy_witnesses(binding, &params, &result, &mut witnesses)?;
        let engine_binding = EngineNativeBinding::TraitDefault(binding);
        self.native_destinations(engine_binding, &params, &result, &mut witnesses)?;
        self.native_set_sources(engine_binding, &params, &mut witnesses)?;
        self.native_key_witnesses(engine_binding, &params, &result, &mut witnesses)?;
        let contract = EngineNativeImport {
            instance: ConcreteFunctionIdentity {
                declaration: method.clone(),
                arguments: all_arguments.iter().map(lower_type).collect(),
            },
            binding: EngineNativeBinding::TraitDefault(binding),
            binding_version: ENGINE_NATIVE_BINDING_VERSION,
            signature: NativeSignature {
                params: params.iter().map(lower_type).collect(),
                result: lower_type(&result),
            },
            requirements,
            witnesses,
        };
        if contract.resolve().is_none() {
            return Err(invalid());
        }
        let dst = self.alloc_temp(self.value_type(&result)?);
        self.function
            .semantic
            .registers
            .insert(dst.temp.index(), lower_type(&result));
        self.emit(Instruction::Call {
            dst: Some(dst),
            callee: CallTarget::Native(NativeCall::Engine(Box::new(contract))),
            args: values.iter().copied().collect(),
        });
        Ok(dst)
    }
    pub(super) fn lower_native_witness(
        &mut self,
        receiver: &TypeId,
        interface: &NominalType,
        method_arguments: &[TypeId],
    ) -> Result<NativeWitness, MirLoweringError> {
        let invalid = || MirLoweringError::MissingBinding("checked native protocol witness");
        let owner = self
            .planner
            .catalog
            .trait_(&interface.declaration)
            .ok_or_else(invalid)?
            .clone();
        let span = self.function.debug.source_span;
        let mut methods = Vec::new();
        let implementation = if let Some((declaration, table_arguments)) = self
            .planner
            .catalog
            .concrete_interface_implementation(
                interface,
                receiver,
                &Default::default(),
                self.planner.options.max_type_nodes,
                self.planner.options.max_type_depth,
                &self.planner.options.cancel,
            )
            .map_err(|_| invalid())?
        {
            let native_iterator = matches!(receiver, TypeId::Iter(_))
                && StandardTrait::from_id(&interface.declaration) == Some(StandardTrait::Iterator);
            if !native_iterator {
                for required in &owner.methods {
                    if required.default.is_none() {
                        let (target, mut arguments) = self
                            .planner
                            .catalog
                            .implementation_method(&required.id, interface, receiver)
                            .ok_or_else(invalid)?;
                        if self.planner.native_function(&target).is_some() {
                            continue;
                        }
                        arguments.extend_from_slice(method_arguments);
                        methods.push(ConcreteFunctionIdentity {
                            declaration: target.clone(),
                            arguments: arguments.iter().map(lower_type).collect(),
                        });
                        if target.module == *self.planner.owner().lowered.source.module_identity() {
                            self.planner.enqueue_declaration(&target, arguments, span)?;
                        }
                        // Foreign instances are demanded in their defining
                        // module from the carried method application.
                    }
                }
            }
            NativeWitnessImplementation::Table(ConcreteFunctionIdentity {
                declaration,
                arguments: table_arguments.iter().map(lower_type).collect(),
            })
        } else if matches!(
            StandardTrait::from_id(&interface.declaration),
            Some(StandardTrait::PartialEq | StandardTrait::Hash)
        ) && matches!(
            receiver,
            TypeId::Tuple(_) | TypeId::Enum(_) | TypeId::StandardEnum { .. }
        ) && self.has_custom_protocol(receiver)?
        {
            let id = self.planner.enqueue_protocol(
                &self.instance,
                StandardTrait::from_id(&interface.declaration).ok_or_else(invalid)?,
                receiver,
                span,
            )?;
            let instance = &self.planner.instances[id.index()];
            methods.push(ConcreteFunctionIdentity {
                declaration: instance.key.declaration.clone(),
                arguments: instance.key.arguments.iter().map(lower_type).collect(),
            });
            NativeWitnessImplementation::Derived
        } else {
            match receiver {
                TypeId::Trait(_) => NativeWitnessImplementation::Interface,
                TypeId::Host(_) => return Err(invalid()),
                _ => NativeWitnessImplementation::Primitive,
            }
        };
        Ok(NativeWitness {
            receiver: lower_type(receiver),
            interface: lower_nominal_type(interface),
            implementation,
            methods,
        })
    }
}
