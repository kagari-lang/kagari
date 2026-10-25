//! Bounded source-independent validation of authored declaration contracts.

use crate::{
    decode_limits::{MAX_NESTED_RECORDS, MAX_TABLE_RECORDS},
    layout::LayoutValidationError,
    slots::FunctionTypeFacts,
    types::{
        ConcreteFunctionIdentity, PublicItem,
        named::{NamedContract, NamedDeclaration},
        proofs::ProofCatalog,
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, ModuleIdentity,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_types::{
    declaration::verify::{Parameters, bounds_valid, function_valid, parameters, type_valid},
    ty::{GenericParam, Ty, substitution::TypeSubstitution},
    visibility::Visibility,
};
use std::collections::{BTreeMap, HashMap};

mod bounds;

/// Declaration signature checking before and after dependency proof admission.
#[derive(Clone, Copy)]
pub enum SignatureScope<'a, 'catalog> {
    /// Check local shape and bindings; applied projections require linked proofs.
    Local,
    /// Normalize projections through the caller's admitted dependency closure.
    Linked(&'a ProofCatalog<'catalog>),
}

impl SignatureScope<'_, '_> {
    fn matches(self, expected: &Ty, actual: &Ty, cancel: &CancellationToken) -> bool {
        if expected == actual {
            return true;
        }
        match self {
            Self::Local => expected.contains_projection(),
            Self::Linked(catalog) => {
                catalog
                    .normalize(expected, cancel)
                    .ok()
                    .is_some_and(|expected| {
                        catalog.normalize(actual, cancel).ok().as_ref() == Some(&expected)
                    })
            }
        }
    }
}

/// Check ownership, uniqueness, binder scopes, types and public declaration agreement.
/// Source-template completeness is a compiler obligation; supplied declarations do
/// not authenticate unavailable source. Public records cannot omit their inventory.
/// Cancellation and malformed/oversized records fail before publication.
pub fn validate(
    records: &[NamedDeclaration],
    public: &[PublicItem],
    module: &ModuleIdentity,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<(), LayoutValidationError> {
    let invalid = LayoutValidationError::Invalid;
    if records.len() > MAX_TABLE_RECORDS {
        return Err(LayoutValidationError::Limit {
            resource: "named declarations",
            limit: MAX_TABLE_RECORDS,
        });
    }
    let mut declarations = HashMap::new();
    for record in records {
        cancel
            .check()
            .map_err(|_| LayoutValidationError::Cancelled)?;
        let view = definitions
            .resolve(record.declaration)
            .map_err(|_| invalid.clone())?;
        let part = view.last().ok_or_else(|| invalid.clone())?;
        if view.module() != module || declarations.insert(record.declaration, record).is_some() {
            return Err(invalid);
        }
        let valid = match &record.contract {
            NamedContract::Function(function) => {
                let mut owners = BTreeMap::<_, Vec<_>>::new();
                for parameter in &function.generic_params {
                    owners
                        .entry(parameter.owner)
                        .or_default()
                        .push(parameter.position);
                }
                let mut outer = Parameters::new();
                let mut valid = function.params.len() <= MAX_NESTED_RECORDS
                    && function.generic_params.len() <= MAX_NESTED_RECORDS
                    && function.bounds.len() <= MAX_NESTED_RECORDS;
                for (owner, positions) in owners {
                    valid &= positions.iter().copied().eq(0..positions.len())
                        && definitions.contains(owner, record.declaration) == Ok(true);
                    if owner != record.declaration {
                        outer.extend(positions.into_iter().map(|position| (owner, position)));
                    }
                }
                let mut signature = function.clone();
                signature
                    .generic_params
                    .retain(|param| param.owner == record.declaration);
                let mut parent = definitions
                    .parent(record.declaration)
                    .map_err(|_| invalid.clone())?;
                let mut self_owner = None;
                while let Some(id) = parent {
                    if definitions
                        .resolve(id)
                        .ok()
                        .and_then(|view| view.last())
                        .is_some_and(|part| part.kind == DefinitionKind::Trait)
                    {
                        self_owner = Some(id);
                        break;
                    }
                    parent = definitions.parent(id).map_err(|_| invalid.clone())?;
                }
                valid
                    && function_valid(
                        &signature,
                        record.declaration,
                        &outer,
                        self_owner.as_ref(),
                        cancel,
                        definitions,
                    )
            }
            NamedContract::Constant(value) => {
                part.kind == DefinitionKind::Const
                    && part.name == value.name
                    && value.value.len() <= 64 * 1024 * 1024
                    && type_valid(&value.ty, &Parameters::new(), None, cancel, definitions)
            }
            NamedContract::Alias(value) => {
                part.kind == DefinitionKind::TypeAlias
                    && part.name == value.name
                    && part.occurrence == 0
                    && value.generic_params.len() <= MAX_NESTED_RECORDS
                    && value.bounds.len() <= MAX_NESTED_RECORDS
                    && parameters(
                        &value.generic_params,
                        &record.declaration,
                        &Parameters::new(),
                    )
                    .is_some_and(|params| {
                        bounds_valid(&value.bounds, &params, cancel, definitions)
                            && value.target.within_wire_limits()
                            && type_valid(&value.target, &params, None, cancel, definitions)
                    })
            }
            NamedContract::Implementation(value) => {
                part.kind == DefinitionKind::Impl
                    && value.bridge.is_none()
                    && view.segments().count() == 1
                    && value.generic_params.len() <= MAX_NESTED_RECORDS
                    && value.bounds.len() <= MAX_NESTED_RECORDS
                    && parameters(
                        &value.generic_params,
                        &record.declaration,
                        &Parameters::new(),
                    )
                    .is_some_and(|params| {
                        bounds_valid(&value.bounds, &params, cancel, definitions)
                            && type_valid(&value.receiver, &params, None, cancel, definitions)
                            && value.trait_type.as_ref().is_none_or(|ty| {
                                matches!(ty, Ty::Trait(_))
                                    && type_valid(ty, &params, None, cancel, definitions)
                            })
                    })
            }
        };
        cancel
            .check()
            .map_err(|_| LayoutValidationError::Cancelled)?;
        if !valid {
            return Err(invalid);
        }
    }
    let mut locals = HashMap::new();
    for record in records {
        let parent = definitions
            .parent(record.declaration)
            .map_err(|_| invalid.clone())?;
        let local = matches!(&record.contract, NamedContract::Function(_))
            && parent.is_some_and(|id| {
                definitions
                    .resolve(id)
                    .ok()
                    .and_then(|view| view.last())
                    .is_some_and(|part| {
                        matches!(
                            part.kind,
                            DefinitionKind::Function
                                | DefinitionKind::Method
                                | DefinitionKind::Static
                                | DefinitionKind::Const
                        )
                    })
            });
        if local {
            let NamedContract::Function(function) = &record.contract else {
                unreachable!()
            };
            locals
                .entry((parent.unwrap(), function.name.as_str()))
                .or_insert_with(Vec::new)
                .push(record);
        } else if record.ambiguous_local_body.is_some() {
            return Err(invalid);
        }
    }
    for group in locals.values() {
        let body = &group[0].ambiguous_local_body;
        if (group.len() > 1) != body.is_some()
            || body
                .as_ref()
                .is_some_and(|body| body.is_empty() || body.len() > 64 * 1024 * 1024)
            || group
                .iter()
                .any(|record| &record.ambiguous_local_body != body)
        {
            return Err(invalid);
        }
    }
    let root = definitions.lookup_root(module);
    for item in public {
        let (kind, expected) = match item {
            PublicItem::Function(value) => (
                DefinitionKind::Function,
                NamedContract::Function(value.clone()),
            ),
            PublicItem::Const(value) => (
                DefinitionKind::Const,
                NamedContract::Constant(value.clone()),
            ),
            PublicItem::InterfaceTable(table) if table.bridge.is_some() => continue,
            PublicItem::InterfaceTable(table) => {
                let record = declarations
                    .get(&table.declaration)
                    .ok_or_else(|| invalid.clone())?;
                let NamedContract::Implementation(header) = &record.contract else {
                    return Err(invalid);
                };
                if header.generic_params != table.generic_params
                    || header.bounds != table.bounds
                    || header.receiver != table.for_type
                    || header.trait_type.as_ref() != Some(&table.trait_type)
                {
                    return Err(invalid);
                }
                continue;
            }
            PublicItem::InherentTable(table) => {
                let record = declarations
                    .get(&table.declaration)
                    .ok_or_else(|| invalid.clone())?;
                let NamedContract::Implementation(header) = &record.contract else {
                    return Err(invalid);
                };
                if header.generic_params != table.generic_params
                    || header.bounds != table.bounds
                    || header.receiver != table.for_type
                    || header.trait_type.is_some()
                {
                    return Err(invalid);
                }
                continue;
            }
            _ => continue,
        };
        let record = root
            .and_then(|root| definitions.lookup_child(root, kind, item.name(), 0))
            .and_then(|id| declarations.get(&id))
            .ok_or_else(|| invalid.clone())?;
        if record.visibility != Visibility::Public || record.contract != expected {
            return Err(invalid);
        }
    }
    Ok(())
}

/// Check a claimed named implementation against its checked applied signature.
/// Generated bodies absent from this inventory retain their existing verifier;
/// absence does not confer named/current-callable authority. Specialized receiver
/// arguments precede shared entry symbols, whose body-owned positions are retained
/// rather than shifted by the specialized prefix. Executable witness requirements
/// must be a weakening of instantiated authored function and exact ancestor
/// function/impl bounds; an unrelated implementation never adds authority.
/// Bound records/constraints may be reordered or deduplicated and unused requirements
/// omitted. Generic binder order stays exact. Scope and forwarded-operation validity
/// remain with the body verifier.
pub fn signature_matches(
    scope: SignatureScope<'_, '_>,
    records: &[NamedDeclaration],
    identity: &ConcreteFunctionIdentity,
    semantic: &impl FunctionTypeFacts,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> bool {
    signature_matches_with_lookup(scope, identity, semantic, definitions, cancel, &|id| {
        records.iter().find(|record| record.declaration == id)
    })
}

pub(super) fn signature_matches_with_lookup<'a>(
    scope: SignatureScope<'_, '_>,
    identity: &ConcreteFunctionIdentity,
    semantic: &impl FunctionTypeFacts,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
    lookup: &impl Fn(DefinitionId) -> Option<&'a NamedDeclaration>,
) -> bool {
    let Some(record) = lookup(identity.declaration) else {
        return true;
    };
    let NamedContract::Function(function) = &record.contract else {
        return false;
    };
    let arguments = identity
        .arguments
        .iter()
        .cloned()
        .chain(
            semantic
                .generic()
                .into_iter()
                .flat_map(|body| body.parameters.iter().map(GenericParam::as_type)),
        )
        .collect::<Vec<_>>();
    if arguments.len() != function.generic_params.len()
        || function.params.len() != semantic.params().len()
    {
        return false;
    }
    let mut substitution = TypeSubstitution::default();
    for (parameter, argument) in function.generic_params.iter().zip(&arguments) {
        substitution.bind(&parameter.owner, parameter.position, argument);
    }
    let apply = |ty| substitution.apply(ty, cancel).ok();
    function
        .params
        .iter()
        .enumerate()
        .all(|(index, parameter)| {
            apply(&parameter.ty)
                .zip(semantic.params().get(&index))
                .is_some_and(|(expected, actual)| scope.matches(&expected, actual, cancel))
        })
        && apply(&function.return_type)
            .zip(semantic.result())
            .is_some_and(|(expected, actual)| scope.matches(&expected, actual, cancel))
        && semantic.generic().is_none_or(|body| {
            bounds::requirements_match(
                record,
                &body.bounds,
                &substitution,
                definitions,
                cancel,
                lookup,
            ) == Ok(true)
        })
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod bound_tests;

#[cfg(test)]
mod projection_tests;
