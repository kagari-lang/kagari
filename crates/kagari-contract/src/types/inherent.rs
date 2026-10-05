//! Public inherent members, including the declaring impl and receiver scope.
use crate::{
    slots::SemanticSlots,
    types::{ConcreteFunctionIdentity, PublicItem},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, DefinitionPath,
        mapping::{
            DefinitionMapper, DefinitionMappingError, DefinitionRecord, check_cancel, map_sequence,
        },
        reference::DefinitionReference,
    },
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{FnDecl, NativeDeclaration, module::ModuleDecl},
    ty::{Constraint, GenericBound, GenericParam, Ty, substitution::TypeSubstitution},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Only publicly callable members are exported. Private and parent-visible
/// methods remain absent, independently of their executable function records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + Serialize",
    deserialize = "I: DefinitionReference + Deserialize<'de>"
))]
pub struct InherentTable<I = DefinitionPath> {
    pub declaration: I,
    pub name: String,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub generic_params: Vec<GenericParam<I>>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub bounds: Vec<GenericBound<I>>,
    pub for_type: Ty<I>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub methods: Vec<FnDecl<I>>,
}

impl<I: DefinitionReference> DefinitionRecord<I> for InherentTable<I> {
    type Rebind<J: DefinitionReference> = InherentTable<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(InherentTable {
            declaration: mapper.reference(&self.declaration)?,
            name: self.name.clone(),
            generic_params: map_sequence(&self.generic_params, |value| {
                value.map_identities(mapper)
            })?,
            bounds: map_sequence(&self.bounds, |value| value.map_identities(mapper))?,
            for_type: self.for_type.map_identities(mapper)?,
            methods: map_sequence(&self.methods, |value| value.map_identities(mapper))?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.declaration)?;
        for parameter in &self.generic_params {
            parameter.visit_definitions(visit, cancel)?;
        }
        for bound in &self.bounds {
            bound.visit_definitions(visit, cancel)?;
        }
        self.for_type.visit_definitions(visit, cancel)?;
        for method in &self.methods {
            method.visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}

/// Native exports share the registered template's signature and binder scopes.
/// A public member table cannot redefine an installed callable's declaration.
pub fn native_signatures_match(items: &[PublicItem], declarations: &[NativeDeclaration]) -> bool {
    items.iter().all(|item| {
        let PublicItem::InherentTable(table) = item else {
            return true;
        };
        table.methods.iter().all(|method| {
            if matches!(method.implementation, CallableImplementation::Script) {
                return true;
            }
            if !matches!(method.implementation, CallableImplementation::Native(_)) {
                return false;
            }
            let id = ModuleDecl::method_id(&table.declaration, &method.name);
            let Some(declaration) = declarations.iter().find(|item| item.declaration == id) else {
                return false;
            };
            let mut function = method.clone();
            function.generic_params = table
                .generic_params
                .iter()
                .chain(&method.generic_params)
                .cloned()
                .collect();
            let mut bounds = BTreeMap::<Ty, BTreeSet<Constraint>>::new();
            for bound in table.bounds.iter().chain(&method.bounds) {
                bounds
                    .entry(bound.ty.clone())
                    .or_default()
                    .extend(bound.constraints.clone());
            }
            function.bounds = bounds
                .into_iter()
                .map(|(ty, constraints)| GenericBound {
                    ty,
                    constraints: constraints.into_iter().collect(),
                })
                .collect();
            function == declaration.function
        })
    })
}

/// Compare an exported method with its executable body after applying the
/// impl-owned and method-owned binders in their original order.
pub fn executable_signature_matches(
    items: &[PublicItem],
    identity: &ConcreteFunctionIdentity,
    semantic: &SemanticSlots,
) -> bool {
    let Some(member) = identity.declaration.path.last() else {
        return true;
    };
    if member.kind != DefinitionKind::Method || member.occurrence != 0 {
        return true;
    }
    let Some(table) = items.iter().find_map(|item| match item {
        PublicItem::InherentTable(table)
            if table.declaration.module == identity.declaration.module
                && identity.declaration.path.len() == table.declaration.path.len() + 1
                && identity
                    .declaration
                    .path
                    .starts_with(&table.declaration.path) =>
        {
            Some(table)
        }
        _ => None,
    }) else {
        return true;
    };
    let Some(method) = table
        .methods
        .iter()
        .find(|method| method.name == member.name)
    else {
        return true;
    };
    if !matches!(method.implementation, CallableImplementation::Script) {
        return false;
    }
    let parameters = table
        .generic_params
        .iter()
        .chain(&method.generic_params)
        .collect::<Vec<_>>();
    let arguments = match &semantic.generic {
        Some(body) if identity.arguments.is_empty() => {
            body.parameters.iter().map(GenericParam::as_type).collect()
        }
        _ => identity.arguments.clone(),
    };
    if parameters.len() != arguments.len() {
        return false;
    }
    let mut substitution = TypeSubstitution::default();
    for (parameter, argument) in parameters.into_iter().zip(&arguments) {
        substitution.bind(&parameter.owner, parameter.position, argument);
    }
    let apply = |ty| substitution.apply(ty, &CancellationToken::default()).ok();
    method.params.len() == semantic.params.len()
        && method
            .params
            .iter()
            .enumerate()
            .all(|(index, param)| apply(&param.ty).as_ref() == semantic.params.get(&index))
        && apply(&method.return_type).as_ref() == semantic.result.as_ref()
}
