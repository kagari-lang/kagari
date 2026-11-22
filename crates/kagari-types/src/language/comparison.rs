//! Language requirements for source-owned public comparison declarations.
use super::{Protocol, binding, requirements::required_method};
use crate::{
    declaration::TraitDef,
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
};
use kagari_common::identity::table::{DefinitionId, DefinitionTable};

fn bare(declaration: DefinitionId) -> NominalTy {
    NominalTy {
        declaration,
        arguments: vec![],
        associated_types: Default::default(),
    }
}

/// Required binary operation, shared with implicit callable verification.
pub(super) fn operation(kind: Protocol, table: &DefinitionTable) -> Option<(&'static str, Ty)> {
    let ordering = || {
        Some(Ty::Enum(bare(
            table.lookup(&binding::ordering_declaration())?,
        )))
    };
    match kind {
        Protocol::PartialEq => Some(("eq", Ty::Builtin(BuiltinType::Bool))),
        Protocol::PartialOrd => Some(("partial_cmp", binding::option(table, ordering()?)?)),
        Protocol::Ord => Some(("cmp", ordering()?)),
        _ => None,
    }
}

/// Check the exact portable declaration of a comparison role. The caller must
/// establish public membership; an interned trait or member is not a declaration.
pub fn trait_shape_valid(owner: DefinitionId, actual: &TraitDef, table: &DefinitionTable) -> bool {
    let Some(role) = Protocol::from_definition(&owner, table) else {
        return false;
    };
    let parents: &[Protocol] = match role {
        Protocol::PartialEq => &[],
        Protocol::Eq | Protocol::PartialOrd => &[Protocol::PartialEq],
        Protocol::Ord => &[Protocol::Eq, Protocol::PartialOrd],
        _ => return false,
    };
    if actual.name != role.name()
        || !actual.generic_params.is_empty()
        || !actual.bounds.is_empty()
        || !actual.associated_types.is_empty()
        || !actual.associated_consts.is_empty()
        || actual.conversion_adapter.is_some()
        || actual.supertraits.len() != parents.len()
        || !parents
            .iter()
            .zip(&actual.supertraits)
            .all(|(role, parent)| {
                role.definition(table) == Some(parent.declaration)
                    && parent.arguments.is_empty()
                    && parent.associated_types.is_empty()
            })
    {
        return false;
    }
    if role == Protocol::Eq {
        return actual.methods.is_empty();
    }
    let ([method], Some((name, result))) = (actual.methods.as_slice(), operation(role, table))
    else {
        return false;
    };
    required_method(
        owner,
        table,
        method,
        name,
        &[
            ("self", Ty::SelfType(owner)),
            ("other", Ty::SelfType(owner)),
        ],
        &result,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        callable::{CallableImplementation, MethodPolicy},
        declaration::{FnDecl, Param},
        language,
        reference::ReferenceAccess,
    };
    use kagari_common::identity::{DefinitionKind, map::DefinitionContext};

    #[test]
    fn partial_order_requires_the_declared_member_and_exact_result_application() {
        let context = DefinitionContext::new().unwrap();
        let owner = context
            .intern(&language::identity(Protocol::PartialOrd))
            .unwrap();
        let parent = context
            .intern(&language::identity(Protocol::PartialEq))
            .unwrap();
        let option = context.intern(&binding::option_declaration()).unwrap();
        let ordering = context.intern(&binding::ordering_declaration()).unwrap();
        let before_member = context.snapshot();
        context
            .intern_child(owner, DefinitionKind::Method, "partial_cmp", 0)
            .unwrap();
        let table = context.snapshot();
        let mut contract = TraitDef {
            name: "PartialOrd".into(),
            supertraits: vec![bare(parent)],
            generic_params: vec![],
            bounds: vec![],
            associated_consts: vec![],
            associated_types: vec![],
            conversion_adapter: None,
            methods: vec![FnDecl {
                name: "partial_cmp".into(),
                params: ["self", "other"]
                    .map(|name| Param {
                        name: name.into(),
                        ty: Ty::SelfType(owner),
                        mutable: false,
                    })
                    .into(),
                return_type: Ty::Enum(NominalTy {
                    arguments: vec![Ty::Enum(bare(ordering))],
                    ..bare(option)
                }),
                generic_params: vec![],
                bounds: vec![],
                method_policy: MethodPolicy::default(),
                host_policy: None,
                implementation: CallableImplementation::Required,
            }],
        };
        assert!(!trait_shape_valid(owner, &contract, &before_member));
        assert!(trait_shape_valid(owner, &contract, &table));
        contract.methods[0].method_policy.receiver_access = Some(ReferenceAccess::Shared);
        assert!(!trait_shape_valid(owner, &contract, &table));
        contract.methods[0].method_policy.receiver_access = None;
        contract.methods[0].implementation = CallableImplementation::Script;
        assert!(!trait_shape_valid(owner, &contract, &table));
        contract.methods[0].implementation = CallableImplementation::Required;
        contract.methods[0].return_type = Ty::Enum(bare(ordering));
        assert!(!trait_shape_valid(owner, &contract, &table));
    }
}
