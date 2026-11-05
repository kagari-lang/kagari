//! Encode an admitted native application without reconstructing its registered declaration.

use crate::source::lower::{MirLoweringError, abi::checked_bounds, instances::InstancePlanner};
use kagari_common::{identity::table::DefinitionId, span::Span};
use kagari_contract::{native_import::NativeImport, types::ConcreteFunctionIdentity};
use kagari_hir::{
    callable::AppliedCallSignature, native::NativeBinding,
    semantic::signatures::FunctionImplementation, types::TypeId,
};
use kagari_types::{callable::Signature, ty::substitution::TypeSubstitution};

impl InstancePlanner<'_> {
    pub(super) fn checked_native_import(
        &mut self,
        declaration: DefinitionId,
        application: &AppliedCallSignature,
        arguments: &[TypeId],
        span: Span,
    ) -> Result<NativeImport, MirLoweringError> {
        let invalid = || MirLoweringError::MissingBinding("checked engine native contract");
        let function = self
            .native_function(&declaration)
            .ok_or_else(invalid)?
            .clone();
        let FunctionImplementation::Native(NativeBinding::Entry(binding)) =
            &function.implementation
        else {
            return Err(invalid());
        };
        let binding = *binding;
        if arguments.len() != function.generic_params.len() {
            return Err(invalid());
        }
        let arguments: Vec<_> = arguments.iter().map(|ty| self.encode_type(ty)).collect();
        let mut binding_substitution = TypeSubstitution::default();
        for (parameter, argument) in function.generic_params.iter().zip(&arguments) {
            binding_substitution.bind(&parameter.owner, parameter.position, argument);
        }
        let bounds = self
            .registered_native_declaration(&declaration)
            .map(|registered| registered.function.bounds.clone())
            .unwrap_or_else(|| checked_bounds(self.catalog, &self.definitions(), &function.bounds));
        let requirements = binding_substitution
            .apply_bounds(&bounds, &self.definitions(), &self.options.cancel)
            .map_err(|_| invalid())?;
        let mut import = NativeImport {
            receiver_access: self
                .registered_native_declaration(&declaration)
                .and_then(|registered| registered.function.method_policy.receiver_access),
            result_adapter: None,
            generic: None,
            callables: vec![],
            instance: ConcreteFunctionIdentity {
                declaration,
                arguments,
            },
            binding,
            signature: Signature {
                params: application
                    .params
                    .iter()
                    .map(|ty| self.encode_type(ty))
                    .collect(),
                result: self.encode_type(&application.return_type),
            },
            requirements,
        };
        import.callables = self.native_callables(&mut import, span)?;
        if !import.structurally_valid(&self.definitions()) {
            return Err(invalid());
        }
        Ok(import)
    }
}
