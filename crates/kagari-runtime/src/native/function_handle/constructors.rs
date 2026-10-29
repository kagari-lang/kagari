//! Prepare public native constructors using installed executable applications.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::{arguments::TypeArgument, compatibility::TypeView},
    module::ExecutableModule,
    native::{
        binding::NativeResult,
        conversion::KagariType,
        function_handle::{Function, evidence::EntryEvidence},
    },
};
use kagari_common::identity::DefinitionKind;
use kagari_contract::types::{PublicItem, inherent::InherentTable};
use kagari_types::ty::{Ty, matching::match_receiver};

impl Runtime {
    /// Bind the checked public zero-argument `new` application for an exact nominal type.
    /// The returned route retains argument scopes and selects current bodies on calls.
    pub fn bind_constructor<R: KagariType + 'static>(
        &self,
        owner: &ExecutableModule,
        receiver: &TypeArgument,
    ) -> NativeResult<Function<(), R>> {
        self.validate_executable_module(owner)?;
        receiver.validate(self)?;
        let Ty::NativeObject(nominal) = receiver.ty() else {
            return Err(invalid());
        };
        let arguments = (0..nominal.arguments.len())
            .map(|index| receiver.parameter(self, owner, index))
            .collect::<NativeResult<Vec<_>>>()?;
        let receiver = receiver.ty().clone();
        self.gc().ensure_no_storage_borrow()?;
        self.resources().ensure_execution_allowed()?;
        self.validate_executable_module(owner)?;
        for argument in &arguments {
            argument.validate(self)?;
        }
        let mut found = None;
        for member in owner.members() {
            for item in &member.bytecode.public_items {
                let PublicItem::InherentTable(table) = item else {
                    continue;
                };
                let Some(substitution) = match_receiver(
                    &table.generic_params,
                    &table.for_type,
                    &receiver,
                    &Default::default(),
                )
                .map_err(|_| invalid())?
                else {
                    continue;
                };
                let Some(method) = table.methods.iter().find(|method| method.name == "new") else {
                    continue;
                };
                if !method.params.is_empty()
                    || !method.generic_params.is_empty()
                    || substitution
                        .apply(&method.return_type, &Default::default())
                        .map_err(|_| invalid())?
                        != receiver
                {
                    continue;
                }
                if found.is_some() {
                    return Err(RuntimeError::module_validation(
                        "ambiguous native constructor",
                    ));
                }
                let applied = constructor_arguments(table, &arguments, &member, owner)?;
                let declaration = member
                    .definitions()
                    .lookup_child(table.declaration, DefinitionKind::Method, &method.name, 0)
                    .ok_or_else(invalid)?;
                found = Some((declaration, applied));
            }
        }
        let (declaration, applied) = found.ok_or_else(invalid)?;
        let evidence = EntryEvidence::find(self, owner, declaration, &applied)?;
        if evidence.signature.result.ty() != &receiver {
            return Err(invalid());
        }
        for (index, argument) in arguments.iter().enumerate() {
            let result = evidence
                .signature
                .result
                .parameter(self, &evidence.owner, index)?;
            if !argument
                .view(owner)
                .compatible(result.view(&evidence.owner))
            {
                return Err(invalid());
            }
        }
        self.cache_function(evidence)
    }
}

/// Registered storage constructors name their key/value parameters directly.
/// Preserve these scopes instead of flattening an application to nominal IDs.
fn constructor_arguments(
    table: &InherentTable,
    arguments: &[TypeArgument],
    declaration_owner: &ExecutableModule,
    caller: &ExecutableModule,
) -> NativeResult<Vec<TypeArgument>> {
    let Ty::NativeObject(nominal) = &table.for_type else {
        return Err(invalid());
    };
    let parameters = nominal.arguments.iter();
    let mut bindings: Vec<Option<TypeArgument>> = vec![None; table.generic_params.len()];
    for (template, actual) in parameters.into_iter().zip(arguments) {
        if let Ty::Parameter { owner, position } = template {
            let index = table
                .generic_params
                .iter()
                .position(|parameter| parameter.owner == *owner && parameter.position == *position)
                .ok_or_else(invalid)?;
            if let Some(previous) = &bindings[index] {
                if !previous.view(caller).compatible(actual.view(caller)) {
                    return Err(invalid());
                }
            } else {
                bindings[index] = Some(actual.clone());
            }
        } else if !TypeView::new(template, declaration_owner, None).compatible(actual.view(caller))
        {
            return Err(invalid());
        }
    }
    bindings
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .ok_or_else(invalid)
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("native constructor requires a checked public new application")
}
