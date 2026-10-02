//! Encode a selected callable application without reconstructing its declaration.
use crate::source::{
    lower::{MirLoweringError, abi::checked_bounds, state::FunctionLowerer},
    types::raise_type,
};
use kagari_abi::{
    native_import::{NativeImport, NativeSignature},
    types::{ConcreteFunctionIdentity, substitution::TypeSubstitution},
};
use kagari_common::{identity::DefinitionId, span::Span};
use kagari_hir::{
    aggregates::traits::MethodDefault,
    callable::AppliedCallSignature,
    declarations::DeclarationId,
    native::NativeBinding,
    resolver::resolved::ResolvedName,
    typeck::{FunctionImplementation, table::CallTarget},
    types::{NominalType, TypeId, TypeSubstitution as HirSubstitution, abi::lower_type},
};
use kagari_mir::instruction::{CallTarget as MirCallTarget, Instruction, MirValue};
use std::slice;

pub(super) struct NativeApplication<'a> {
    pub target: &'a CallTarget,
    pub signature: &'a AppliedCallSignature,
    pub arguments: &'a [TypeId],
}

impl FunctionLowerer<'_, '_> {
    pub(super) fn checked_native_import(
        &mut self,
        target: &CallTarget,
        application: &AppliedCallSignature,
        arguments: &[TypeId],
        span: Span,
    ) -> Result<NativeImport, MirLoweringError> {
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
        let FunctionImplementation::Native(NativeBinding::Entry(binding)) =
            &function.implementation
        else {
            return Err(invalid());
        };
        let binding = binding.clone();
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
        let bounds = self
            .planner
            .registered_native_declaration(&declaration)
            .map(|registered| registered.function.bounds.clone())
            .unwrap_or_else(|| checked_bounds(&function.bounds));
        let requirements = substitution
            .apply_bounds(&bounds, &self.planner.options.cancel)
            .map_err(|_| invalid())?;
        let params =
            self.planner
                .arguments(&application.params, &self.instance.substitution, span)?;
        let result = self.planner.arguments(
            slice::from_ref(&application.return_type),
            &self.instance.substitution,
            span,
        )?;
        let mut import = NativeImport {
            result_adapter: None,
            generic: None,
            callables: vec![],
            instance: ConcreteFunctionIdentity {
                declaration,
                arguments,
            },
            binding,
            host: None,
            signature: NativeSignature {
                params: params.iter().map(lower_type).collect(),
                result: lower_type(&result[0]),
            },
            requirements,
        };
        import.callables = self.planner.native_callables(&mut import, span)?;
        if !import.structurally_valid() {
            return Err(invalid());
        }
        Ok(import)
    }

    fn emit_native_application(
        &mut self,
        mut import: NativeImport,
        result: &TypeId,
        values: &[MirValue],
    ) -> Result<MirValue, MirLoweringError> {
        let callee = if import.instance.arguments.iter().any(|ty| !ty.is_concrete()) {
            MirCallTarget::Shared(Box::new(
                self.planner.shared_call(
                    &import.instance.declaration,
                    &import
                        .instance
                        .arguments
                        .iter()
                        .map(raise_type)
                        .collect::<Vec<_>>(),
                    import.signature,
                    self.function.debug.source_span,
                )?,
            ))
        } else {
            import.callables = self
                .planner
                .native_callables(&mut import, self.function.debug.source_span)?;
            if !import.structurally_valid() {
                return Err(MirLoweringError::MissingBinding(
                    "concrete native entry application",
                ));
            }
            MirCallTarget::Native(Box::new(import))
        };
        let dst = self.alloc_temp(self.value_type(result)?);
        self.function
            .semantic
            .registers
            .insert(dst.temp.index(), lower_type(result));
        self.emit(Instruction::Call {
            dst: Some(dst),
            callee,
            args: values.iter().copied().collect(),
        });
        Ok(dst)
    }

    pub(super) fn lower_native_implementation(
        &mut self,
        declaration: &DefinitionId,
        arguments: &[TypeId],
        receiver: &TypeId,
        result: &TypeId,
        values: &[MirValue],
    ) -> Result<MirValue, MirLoweringError> {
        let function = self
            .planner
            .native_function(declaration)
            .ok_or(MirLoweringError::MissingBinding(
                "selected native entry declaration",
            ))?
            .clone();
        let FunctionImplementation::Native(NativeBinding::Entry(binding)) =
            &function.implementation
        else {
            return Err(MirLoweringError::MissingBinding(
                "selected native entry implementation",
            ));
        };
        let substitution: HirSubstitution = function
            .generic_params
            .iter()
            .cloned()
            .zip(arguments.iter().cloned())
            .collect();
        let mut params = function
            .params
            .iter()
            .map(|p| {
                self.planner
                    .catalog
                    .normalize_type(&p.ty.instantiate(&substitution))
            })
            .collect::<Vec<_>>();
        if function
            .params
            .first()
            .is_some_and(|param| param.name == "self")
            && let Some(first) = params.first_mut()
        {
            *first = receiver.clone();
        }
        let mut bindings = TypeSubstitution::default();
        let applied_arguments = arguments.iter().map(lower_type).collect::<Vec<_>>();
        for (param, argument) in function.generic_params.iter().zip(&applied_arguments) {
            bindings.bind(&param.owner, param.position, argument);
        }
        let bounds = self
            .planner
            .registered_native_declaration(declaration)
            .map(|registered| registered.function.bounds.clone())
            .unwrap_or_else(|| checked_bounds(&function.bounds));
        let requirements = bindings
            .apply_bounds(&bounds, &self.planner.options.cancel)
            .map_err(|_| MirLoweringError::MissingBinding("native method bounds"))?;
        self.emit_native_application(
            NativeImport {
                result_adapter: None,
                generic: None,
                callables: vec![],
                instance: ConcreteFunctionIdentity {
                    declaration: declaration.clone(),
                    arguments: applied_arguments,
                },
                binding: binding.clone(),
                host: None,
                signature: NativeSignature {
                    params: params.iter().map(lower_type).collect(),
                    result: lower_type(result),
                },
                requirements,
            },
            result,
            values,
        )
    }

    pub(super) fn lower_native_default(
        &mut self,
        receiver: &TypeId,
        interface: &NominalType,
        method: &DefinitionId,
        arguments: &[TypeId],
        values: &[MirValue],
    ) -> Result<MirValue, MirLoweringError> {
        let signature = self
            .planner
            .catalog
            .trait_method(method)
            .ok_or(MirLoweringError::MissingBinding(
                "selected native entry default",
            ))?
            .clone();
        if matches!(
            signature.default,
            Some(MethodDefault::Native(NativeBinding::Default(_)))
        ) {
            let import = self.planner.native_default_import(
                receiver,
                interface,
                method,
                arguments,
                self.function.debug.source_span,
            )?;
            let result = self
                .planner
                .catalog
                .normalize_type(&raise_type(&import.signature.result));
            return self.emit_native_application(import, &result, values);
        }
        let Some(MethodDefault::Native(NativeBinding::Entry(binding))) = &signature.default else {
            return Err(MirLoweringError::MissingBinding(
                "native entry default implementation",
            ));
        };
        let arguments = interface
            .arguments
            .iter()
            .chain(arguments)
            .cloned()
            .collect::<Vec<_>>();
        let mut substitution: HirSubstitution = signature
            .generic_params
            .iter()
            .cloned()
            .zip(arguments.iter().cloned())
            .collect();
        substitution.insert_receiver(signature.owner.clone(), receiver.clone());
        let instantiate = |ty: &TypeId| {
            self.planner.catalog.normalize_type(
                &ty.instantiate(&substitution)
                    .with_associated_types(interface),
            )
        };
        let params = signature
            .params
            .iter()
            .map(|p| instantiate(&p.ty))
            .collect::<Vec<_>>();
        let result = instantiate(&signature.return_type);
        self.emit_native_application(
            NativeImport {
                result_adapter: None,
                generic: None,
                callables: vec![],
                instance: ConcreteFunctionIdentity {
                    declaration: method.clone(),
                    arguments: arguments.iter().map(lower_type).collect(),
                },
                binding: binding.clone(),
                host: None,
                signature: NativeSignature {
                    params: params.iter().map(lower_type).collect(),
                    result: lower_type(&result),
                },
                requirements: vec![],
            },
            &result,
            values,
        )
    }
}
