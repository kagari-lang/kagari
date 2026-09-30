//! Encode a selected callable application without reconstructing its declaration.
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
    standard::{bindings::NativeProtocolMethod, traits::StandardTrait},
    types::{
        ConcreteFunctionIdentity, ConstraintAbi, GenericBoundAbi, substitution::TypeSubstitution,
    },
};
use kagari_common::{
    Span,
    identity::{DefinitionId, associated_type_id},
};
use kagari_hir::{
    builtin::traits::StandardTraitSemantics,
    callable::AppliedCallSignature,
    declarations::DeclarationId,
    native::NativeBinding,
    resolver::ResolvedName,
    typeck::{CallTarget, FunctionImplementation},
    types::{TypeId, TypeSubstitution as HirSubstitution, abi::lower_type},
};
use kagari_mir::instruction::{CallTarget as MirCallTarget, Instruction, MirValue};
use std::slice;

pub(super) struct NativeApplication<'a> {
    pub target: &'a CallTarget,
    pub signature: &'a AppliedCallSignature,
    pub arguments: &'a [TypeId],
}

impl FunctionLowerer<'_, '_> {
    pub(super) fn engine_native_contract(
        &mut self,
        target: &CallTarget,
        application: &AppliedCallSignature,
        arguments: &[TypeId],
        span: Span,
    ) -> Result<EngineNativeImport, MirLoweringError> {
        let invalid = || MirLoweringError::MissingBinding("checked engine native contract");
        let (declaration, function) = match target {
            CallTarget::Function(id) => {
                let DeclarationId::Definition(declaration) = &self
                    .analyzed
                    .declarations
                    .target(ResolvedName::Function(*id))
                    .ok_or_else(invalid)?
                    .id
                else {
                    return Err(invalid());
                };
                (
                    declaration.clone(),
                    self.analyzed
                        .typed
                        .functions
                        .iter()
                        .find(|function| function.id == *id)
                        .ok_or_else(invalid)?,
                )
            }
            CallTarget::SourceFunction(id) => {
                let imported = self
                    .analyzed
                    .imported_functions
                    .target(id)
                    .ok_or_else(invalid)?;
                (imported.declaration.clone(), &imported.signature)
            }
            _ => return Err(invalid()),
        };
        let FunctionImplementation::Native(NativeBinding::Engine(binding)) =
            function.implementation
        else {
            return Err(invalid());
        };
        let function = function.clone();
        let arguments = self
            .planner
            .arguments(arguments, &self.instance.substitution, span)?;
        if arguments.len() != function.generic_params.len() {
            return Err(invalid());
        }
        let arguments: Vec<_> = arguments.iter().map(lower_type).collect();
        let mut substitution = TypeSubstitution::default();
        for (parameter, argument) in function.generic_params.iter().zip(&arguments) {
            substitution.bind(&parameter.owner, parameter.position, argument);
        }
        let requirements = substitution
            .apply_bounds(
                &checked_bounds(&function.bounds),
                &self.planner.options.cancel,
            )
            .map_err(|_| invalid())?;
        let witnesses = self.native_requirement_witnesses(binding, &requirements)?;
        let params =
            self.planner
                .arguments(&application.params, &self.instance.substitution, span)?;
        let result = self.planner.arguments(
            slice::from_ref(&application.return_type),
            &self.instance.substitution,
            span,
        )?;
        let contract = EngineNativeImport {
            instance: ConcreteFunctionIdentity {
                declaration,
                arguments,
            },
            binding,
            binding_version: ENGINE_NATIVE_BINDING_VERSION,
            signature: NativeSignature {
                params: params.iter().map(lower_type).collect(),
                result: lower_type(&result[0]),
            },
            requirements,
            witnesses,
        };
        if contract.resolve().is_none() {
            return Err(invalid());
        }
        Ok(contract)
    }
    fn native_requirement_witnesses(
        &mut self,
        binding: EngineNativeBinding,
        requirements: &[GenericBoundAbi],
    ) -> Result<Vec<NativeWitness>, MirLoweringError> {
        let numeric = matches!(
            binding,
            EngineNativeBinding::Protocol(
                NativeProtocolMethod::NumericSum | NativeProtocolMethod::NumericProduct
            )
        );
        let mut witnesses = Vec::new();
        for bound in requirements {
            for constraint in &bound.constraints {
                let ConstraintAbi::Trait(interface) = constraint else {
                    continue;
                };
                let receiver = raise_type(&bound.ty);
                let applied = raise_nominal_type(interface);
                let witness = if numeric {
                    self.lower_native_witness(&receiver, &applied, &[])?
                } else {
                    let implementation = if let Some((declaration, arguments)) = self
                        .planner
                        .catalog
                        .concrete_interface_implementation(
                            &applied,
                            &receiver,
                            &Default::default(),
                            self.planner.options.max_type_nodes,
                            self.planner.options.max_type_depth,
                            &self.planner.options.cancel,
                        )
                        .map_err(|_| MirLoweringError::MissingBinding("native witness"))?
                    {
                        NativeWitnessImplementation::Table(ConcreteFunctionIdentity {
                            declaration,
                            arguments: arguments.iter().map(lower_type).collect(),
                        })
                    } else {
                        match receiver {
                            TypeId::Host(_) => NativeWitnessImplementation::Host,
                            TypeId::Trait(_) => NativeWitnessImplementation::Interface,
                            _ => NativeWitnessImplementation::Primitive,
                        }
                    };
                    NativeWitness {
                        receiver: bound.ty.clone(),
                        interface: interface.clone(),
                        implementation,
                        methods: vec![],
                    }
                };
                if !witnesses.contains(&witness) {
                    witnesses.push(witness);
                }
                if numeric
                    && StandardTrait::from_id(&interface.declaration)
                        == Some(StandardTrait::Iterable)
                {
                    let iterator =
                        self.iteration_output(StandardTrait::Iterable, &receiver, "Iter")?;
                    let mut applied = StandardTrait::Iterator.nominal();
                    let item = self.iteration_output(StandardTrait::Iterable, &receiver, "Item")?;
                    applied
                        .associated_types
                        .insert(associated_type_id(&applied.declaration, "Item"), item);
                    let witness = self.lower_native_witness(&iterator, &applied, &[])?;
                    if !witnesses.contains(&witness) {
                        witnesses.push(witness);
                    }
                }
            }
        }
        Ok(witnesses)
    }

    pub(super) fn lower_native_implementation(
        &mut self,
        declaration: &DefinitionId,
        arguments: &[TypeId],
        result: &TypeId,
        values: &[MirValue],
    ) -> Result<MirValue, MirLoweringError> {
        let invalid = || MirLoweringError::MissingBinding("selected native implementation");
        let function = self
            .planner
            .native_function(declaration)
            .ok_or_else(invalid)?
            .clone();
        let FunctionImplementation::Native(NativeBinding::Engine(binding)) =
            function.implementation
        else {
            return Err(invalid());
        };
        if arguments.len() != function.generic_params.len() {
            return Err(invalid());
        }
        let substitution: HirSubstitution = function
            .generic_params
            .iter()
            .cloned()
            .zip(arguments.iter().cloned())
            .collect();
        let params = function
            .params
            .iter()
            .map(|parameter| {
                self.planner
                    .catalog
                    .normalize_type(&parameter.ty.instantiate(&substitution))
            })
            .collect::<Vec<_>>();
        let arguments: Vec<_> = arguments.iter().map(lower_type).collect();
        let mut substitution = TypeSubstitution::default();
        for (parameter, argument) in function.generic_params.iter().zip(&arguments) {
            substitution.bind(&parameter.owner, parameter.position, argument);
        }
        let requirements = substitution
            .apply_bounds(
                &checked_bounds(&function.bounds),
                &self.planner.options.cancel,
            )
            .map_err(|_| invalid())?;
        let witnesses = self.native_requirement_witnesses(binding, &requirements)?;
        let contract = EngineNativeImport {
            instance: ConcreteFunctionIdentity {
                declaration: declaration.clone(),
                arguments,
            },
            binding,
            binding_version: ENGINE_NATIVE_BINDING_VERSION,
            signature: NativeSignature {
                params: params.iter().map(lower_type).collect(),
                result: lower_type(result),
            },
            requirements,
            witnesses,
        };
        if contract.resolve().is_none() {
            return Err(invalid());
        }
        let dst = self.alloc_temp(self.value_type(result)?);
        self.function
            .semantic
            .registers
            .insert(dst.temp.index(), lower_type(result));
        self.emit(Instruction::Call {
            dst: Some(dst),
            callee: MirCallTarget::Native(NativeCall::Engine(Box::new(contract))),
            args: values.iter().copied().collect(),
        });
        Ok(dst)
    }
}
