//! Prepare public collection constructors using installed executable applications.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::{arguments::TypeArgument, compatibility::TypeView},
    module::LoadedModule,
    native::{
        binding::NativeResult,
        collections::{map::ScriptMap, set::ScriptSet},
        conversion::KagariType,
        function_handle::{PinnedFunction, evidence::EntryEvidence},
    },
};
use kagari_common::identity::{DefinitionKind, DefinitionPathSegment, table::DefinitionId};
use kagari_contract::types::{PublicItem, inherent::InherentTable};
use kagari_types::{
    collection::CollectionAccess,
    ty::{Ty, matching::match_receiver},
};
use std::slice;

impl Runtime {
    /// Prepare the installed public `new` application, including its selected
    /// Hash/Eq operations. The returned callable can be cached and reused.
    pub fn bind_map_constructor<K: KagariType + 'static, V: KagariType + 'static>(
        &self,
        owner: &LoadedModule,
        key: &TypeArgument,
        value: &TypeArgument,
    ) -> NativeResult<PinnedFunction<(), ScriptMap<K, V>>> {
        self.bind_hash_constructor(
            owner,
            Ty::Map {
                key: Box::new(key.ty().clone()),
                value: Box::new(value.ty().clone()),
                access: CollectionAccess::Mutable,
            },
            &[key.clone(), value.clone()],
        )
    }

    /// Prepare the installed public `new` application for an exact element scope.
    pub fn bind_set_constructor<T: KagariType + 'static>(
        &self,
        owner: &LoadedModule,
        element: &TypeArgument,
    ) -> NativeResult<PinnedFunction<(), ScriptSet<T>>> {
        self.bind_hash_constructor(
            owner,
            Ty::Set(Box::new(element.ty().clone()), CollectionAccess::Mutable),
            slice::from_ref(element),
        )
    }

    fn bind_hash_constructor<R: KagariType + 'static>(
        &self,
        owner: &LoadedModule,
        receiver: Ty<DefinitionId>,
        arguments: &[TypeArgument],
    ) -> NativeResult<PinnedFunction<(), R>> {
        self.gc().ensure_no_native_borrow()?;
        self.resources().ensure_execution_allowed()?;
        self.validate_loaded_module(owner)?;
        for argument in arguments {
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
                        "ambiguous collection constructor",
                    ));
                }
                let applied = constructor_arguments(table, arguments, &member, owner)?;
                let mut declaration = member.definition(table.declaration)?.to_path();
                declaration.path.push(DefinitionPathSegment {
                    kind: DefinitionKind::Method,
                    name: method.name.clone(),
                    occurrence: 0,
                });
                found = Some((declaration, applied));
            }
        }
        let (declaration, applied) = found.ok_or_else(invalid)?;
        let evidence = EntryEvidence::find(self, owner, &declaration, &applied)?;
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
    table: &InherentTable<DefinitionId>,
    arguments: &[TypeArgument],
    declaration_owner: &LoadedModule,
    caller: &LoadedModule,
) -> NativeResult<Vec<TypeArgument>> {
    let parameters = match &table.for_type {
        Ty::Map { key, value, .. } => vec![key.as_ref(), value.as_ref()],
        Ty::Set(element, _) => vec![element.as_ref()],
        _ => return Err(invalid()),
    };
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
    RuntimeError::module_validation(
        "collection constructor requires a checked public new application",
    )
}
