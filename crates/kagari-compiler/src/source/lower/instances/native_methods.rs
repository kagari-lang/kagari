//! Plan selected native methods and defaults using their admitted registration contracts.
use crate::source::lower::{
    MirLoweringError,
    abi::checked_bounds,
    instances::{
        InstancePlanner,
        methods::{MethodOperation, MethodPlan},
    },
};
use kagari_common::{identity::table::DefinitionId, span::Span};
use kagari_contract::{native_import::NativeImport, types::ConcreteFunctionIdentity};
use kagari_hir::{
    aggregates::traits::MethodDefault,
    native::NativeBinding,
    semantic::signatures::FunctionImplementation,
    types::{NominalType, TypeId, TypeSubstitution as HirSubstitution, semantic::raise_type},
};
use kagari_mir::instruction::CallTarget;
use kagari_types::{callable::Signature, ty::substitution::TypeSubstitution};

impl InstancePlanner<'_> {
    fn native_method_call(
        &mut self,
        mut import: NativeImport,
        result: &TypeId,
        span: Span,
    ) -> Result<MethodPlan, MirLoweringError> {
        let target = if import.instance.arguments.iter().any(|ty| !ty.is_concrete()) {
            CallTarget::Shared(Box::new(
                self.shared_call(
                    &import.instance.declaration,
                    &import
                        .instance
                        .arguments
                        .iter()
                        .map(raise_type)
                        .collect::<Vec<_>>(),
                    import.signature,
                    span,
                )?,
            ))
        } else {
            import.callables = self.native_callables(&mut import, span)?;
            if !import.structurally_valid(&self.definitions()) {
                return Err(MirLoweringError::MissingBinding(
                    "concrete native entry application",
                ));
            }
            CallTarget::Native(Box::new(import))
        };
        Ok(MethodPlan {
            result: result.clone(),
            operation: MethodOperation::Call(target),
        })
    }

    pub(super) fn native_implementation_plan(
        &mut self,
        declaration: &DefinitionId,
        arguments: &[TypeId],
        receiver: &TypeId,
        result: &TypeId,
        span: Span,
    ) -> Result<MethodPlan, MirLoweringError> {
        let function = self
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
                self.catalog
                    .normalize_type(&self.definitions(), &p.ty.instantiate(&substitution))
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
        let applied_arguments = arguments
            .iter()
            .map(|ty| self.encode_type(ty))
            .collect::<Vec<_>>();
        for (param, argument) in function.generic_params.iter().zip(&applied_arguments) {
            bindings.bind(&param.owner, param.position, argument);
        }
        let bounds = self
            .registered_native_declaration(declaration)
            .map(|registered| registered.function.bounds.clone())
            .unwrap_or_else(|| checked_bounds(self.catalog, &self.definitions(), &function.bounds));
        let requirements = bindings
            .apply_bounds(&bounds, &self.definitions(), &self.options.cancel)
            .map_err(|_| MirLoweringError::MissingBinding("native method bounds"))?;
        self.native_method_call(
            NativeImport {
                receiver_access: self
                    .registered_native_declaration(declaration)
                    .and_then(|registered| registered.function.method_policy.receiver_access),
                result_adapter: None,
                generic: None,
                callables: vec![],
                instance: ConcreteFunctionIdentity {
                    declaration: *declaration,
                    arguments: applied_arguments,
                },
                binding: *binding,
                signature: Signature {
                    params: params.iter().map(|ty| self.encode_type(ty)).collect(),
                    result: self.encode_type(result),
                },
                requirements,
            },
            result,
            span,
        )
    }

    pub(super) fn native_default_plan(
        &mut self,
        receiver: &TypeId,
        interface: &NominalType,
        method: &DefinitionId,
        arguments: &[TypeId],
        span: Span,
    ) -> Result<MethodPlan, MirLoweringError> {
        let signature = self
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
            let import =
                self.native_default_import(receiver, interface, method, arguments, span)?;
            let result = self
                .catalog
                .normalize_type(&self.definitions(), &raise_type(&import.signature.result));
            return self.native_method_call(import, &result, span);
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
        substitution.insert_receiver(signature.owner, receiver.clone());
        let instantiate = |ty: &TypeId| {
            self.catalog.normalize_type(
                &self.definitions(),
                &ty.instantiate(&substitution)
                    .with_associated_types(interface, receiver),
            )
        };
        let params = signature
            .params
            .iter()
            .map(|p| instantiate(&p.ty))
            .collect::<Vec<_>>();
        let result = instantiate(&signature.return_type);
        self.native_method_call(
            NativeImport {
                receiver_access: signature.policy.receiver_access,
                result_adapter: None,
                generic: None,
                callables: vec![],
                instance: ConcreteFunctionIdentity {
                    declaration: *method,
                    arguments: arguments.iter().map(|ty| self.encode_type(ty)).collect(),
                },
                binding: *binding,
                signature: Signature {
                    params: params.iter().map(|ty| self.encode_type(ty)).collect(),
                    result: self.encode_type(&result),
                },
                requirements: vec![],
            },
            &result,
            span,
        )
    }
}
