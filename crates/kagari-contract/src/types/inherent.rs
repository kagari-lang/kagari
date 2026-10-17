//! Public inherent members, including the declaring impl and receiver scope.
use crate::{
    slots::SemanticSlots,
    types::{ConcreteFunctionIdentity, PublicItem},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind,
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel, map_sequence},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{FnDecl, NativeDeclaration},
    ty::{GenericBound, GenericParam, Ty, substitution::TypeSubstitution},
};

/// Only publicly callable members are exported. Private and parent-visible
/// methods remain absent, independently of their executable function records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InherentTable {
    pub declaration: DefinitionId,
    pub name: String,
    pub generic_params: Vec<GenericParam>,
    pub bounds: Vec<GenericBound>,
    pub for_type: Ty,
    pub methods: Vec<FnDecl>,
}

impl RemapDefinitions for InherentTable {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(InherentTable {
            declaration: mapper.reference(&self.declaration)?,
            name: self.name.clone(),
            generic_params: map_sequence(&self.generic_params, |value| {
                value.remap_definitions(mapper)
            })?,
            bounds: map_sequence(&self.bounds, |value| value.remap_definitions(mapper))?,
            for_type: self.for_type.remap_definitions(mapper)?,
            methods: map_sequence(&self.methods, |value| value.remap_definitions(mapper))?,
        })
    }
}

impl DefinitionReferences for InherentTable {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.declaration)?;
        for parameter in &self.generic_params {
            parameter.visit_definition_ids(visit, cancel)?;
        }
        for bound in &self.bounds {
            bound.visit_definition_ids(visit, cancel)?;
        }
        self.for_type.visit_definition_ids(visit, cancel)?;
        for method in &self.methods {
            method.visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}

/// Native exports share the registered template's signature and binder scopes.
/// A public member table cannot redefine an installed callable's declaration.
pub fn native_signatures_match(
    items: &[PublicItem],
    declarations: &[NativeDeclaration],
    definitions: &DefinitionTable,
) -> bool {
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
            let Some(id) = definitions.lookup_child(
                table.declaration,
                DefinitionKind::Method,
                &method.name,
                0,
            ) else {
                return false;
            };
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
            let Ok(bounds) = GenericBound::normalize(
                table.bounds.iter().chain(&method.bounds).cloned(),
                definitions,
                &CancellationToken::default(),
            ) else {
                return false;
            };
            function.bounds = bounds;
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
    definitions: &DefinitionTable,
) -> bool {
    let Some(member) = definitions
        .resolve(identity.declaration)
        .ok()
        .and_then(|view| view.last())
    else {
        return false;
    };
    if member.kind != DefinitionKind::Method || member.occurrence != 0 {
        return true;
    }
    let Some(table) = items.iter().find_map(|item| match item {
        PublicItem::InherentTable(table)
            if definitions.parent(identity.declaration) == Ok(Some(table.declaration)) =>
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
