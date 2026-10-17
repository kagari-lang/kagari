//! Inherent String methods belong to the language declaration owner.
use crate::catalog::{
    contracts, identities,
    key::{self, RegistrationTrait},
};
use kagari_common::identity::{DefinitionKind, map::DefinitionContext};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{
        Param,
        module::{DeclarationError, ImplDecl, ModuleDecl},
    },
    scalar::BuiltinType,
    ty::Ty,
};

pub(super) fn declare(
    ids: &DefinitionContext,
    module: &mut ModuleDecl,
) -> Result<(), DeclarationError> {
    let string = Ty::Builtin(BuiltinType::String);
    let owner = identities::implementation(ids, module.implementations.len())?;
    let mut methods = vec![];
    for (name, parameters, result, documentation) in [
        (
            "len",
            vec![],
            contracts::usize_type(),
            "Return the UTF-8 byte length.",
        ),
        (
            "is_empty",
            vec![],
            contracts::boolean(),
            "Return whether the string has no bytes.",
        ),
        (
            "contains",
            vec![("pattern", string.clone())],
            contracts::boolean(),
            "Test for a literal substring; the empty pattern matches.",
        ),
        (
            "starts_with",
            vec![("pattern", string.clone())],
            contracts::boolean(),
            "Test for a literal prefix; the empty pattern matches.",
        ),
        (
            "ends_with",
            vec![("pattern", string.clone())],
            contracts::boolean(),
            "Test for a literal suffix; the empty pattern matches.",
        ),
        (
            "find",
            vec![("pattern", string.clone())],
            contracts::option(ids, contracts::usize_type())?,
            "Return the first matching byte offset, or None. An empty pattern matches at zero.",
        ),
        (
            "slice",
            vec![
                ("start", contracts::usize_type()),
                ("end", contracts::usize_type()),
            ],
            string.clone(),
            "Copy the half-open byte range. Invalid order, bounds or UTF-8 boundaries trap.",
        ),
        (
            "trim",
            vec![],
            string.clone(),
            "Remove Unicode whitespace from both ends.",
        ),
        (
            "trim_start",
            vec![],
            string.clone(),
            "Remove leading Unicode whitespace.",
        ),
        (
            "trim_end",
            vec![],
            string.clone(),
            "Remove trailing Unicode whitespace.",
        ),
        (
            "replace",
            vec![("from", string.clone()), ("to", string.clone())],
            string.clone(),
            "Replace all non-overlapping literal matches. An empty pattern inserts at Unicode scalar boundaries, including both ends.",
        ),
        (
            "split",
            vec![("separator", string.clone())],
            Ty::Trait(key::applied(
                ids,
                RegistrationTrait::List,
                vec![string.clone()],
            )?),
            "Return an eager List<String> of literal-separated fields, preserving endpoint empties. An empty separator splits at Unicode scalar boundaries with endpoint empties.",
        ),
    ] {
        let mut method = contracts::method(name, vec![string.clone()], result);
        for (name, ty) in parameters {
            method.params.push(Param {
                name: name.into(),
                ty,
                mutable: false,
            });
        }
        method.implementation = CallableImplementation::Native(identities::definition(
            ids,
            DefinitionKind::Function,
            &format!("$foundation_string_{name}"),
        )?);
        let id = identities::method(ids, &owner, name)?;
        module.documentation.insert(id, documentation.into());
        if name == "split" {
            module
                .concrete_results
                .insert(id, contracts::vec_type(ids, string.clone())?);
        }
        methods.push(method);
    }
    module.implementations.push(ImplDecl {
        generic_params: vec![],
        bounds: vec![],
        trait_type: None,
        for_type: string,
        methods,
    });

    Ok(())
}
