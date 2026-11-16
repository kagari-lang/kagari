//! Match checked executable contracts to implementation metadata without entry authority.

use crate::module::{VerifiedProgram, admission::DefinitionAdmission};
use kagari_common::identity::{
    DefinitionKind,
    table::{DefinitionId, DefinitionTable},
};
use kagari_contract::types::{PublicItem, materialization};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{FnDecl, NativeDeclaration, TraitDef},
    ty::{
        GenericBound, Ty,
        substitution::{TypeTransformError, normalize_projections},
    },
};

/// Drop only projection inputs already stated by the same function's bounds.
/// This is structural contract normalization, not trait selection or inference.
fn signature(
    function: &FnDecl,
    definitions: &DefinitionTable,
) -> Result<FnDecl, TypeTransformError> {
    let cancel = Default::default();
    let normalize = |ty: &Ty| {
        normalize_projections(
            ty,
            &|interface, receiver, member, arguments| {
                let mut reduced = interface.clone();
                for bound in &function.bounds {
                    if &bound.ty != receiver {
                        continue;
                    }
                    for constraint in &bound.constraints {
                        if constraint.declaration == interface.declaration
                            && constraint.arguments == interface.arguments
                        {
                            reduced.associated_types.retain(|id, value| {
                                constraint.associated_types.get(id) != Some(value)
                            });
                        }
                    }
                }
                Ok((reduced != *interface).then(|| Ty::Projection {
                    receiver: Box::new(receiver.clone()),
                    interface: Box::new(reduced),
                    member: *member,
                    arguments: arguments.to_vec(),
                }))
            },
            &cancel,
        )
    };
    let mut result = function.clone();
    result.bounds = GenericBound::normalize(
        function
            .bounds
            .iter()
            .map(|bound| {
                Ok(GenericBound {
                    ty: normalize(&bound.ty)?,
                    constraints: bound
                        .constraints
                        .iter()
                        .map(|constraint| {
                            let Ty::Trait(nominal) = normalize(&Ty::Trait(constraint.clone()))?
                            else {
                                return Err(TypeTransformError::InvalidContract);
                            };
                            Ok(nominal)
                        })
                        .collect::<Result<_, _>>()?,
                })
            })
            .collect::<Result<Vec<_>, TypeTransformError>>()?,
        definitions,
        &cancel,
    )?;
    for parameter in &mut result.params {
        parameter.ty = normalize(&parameter.ty)?;
    }
    result.return_type = normalize(&result.return_type)?;
    Ok(result)
}

/// Only the implementation-side selector is replaced. Every static contract field
/// remains part of the comparison, including ordered operation requirements.
pub(super) fn declaration(
    expected: &NativeDeclaration,
    actual: &NativeDeclaration,
    definitions: &DefinitionTable,
) -> bool {
    let mut expected = expected.clone();
    if matches!(
        expected.function.implementation,
        CallableImplementation::Native(_)
    ) {
        expected.function.implementation = CallableImplementation::Native(expected.declaration);
    }
    same_declaration(&expected, actual, definitions)
}

fn same_declaration(
    expected: &NativeDeclaration,
    actual: &NativeDeclaration,
    definitions: &DefinitionTable,
) -> bool {
    let mut expected = expected.clone();
    let (Ok(function), Ok(actual_function)) = (
        signature(&expected.function, definitions),
        signature(&actual.function, definitions),
    ) else {
        return false;
    };
    expected.function = function;
    let mut actual = actual.clone();
    actual.function = actual_function;
    expected == actual
}

pub(super) fn method(
    owner: DefinitionId,
    expected: &FnDecl,
    definitions: &DefinitionTable,
) -> Option<FnDecl> {
    let mut expected = signature(expected, definitions).ok()?;
    if matches!(expected.implementation, CallableImplementation::Native(_)) {
        let id = definitions.lookup_child(owner, DefinitionKind::Method, &expected.name, 0)?;
        expected.implementation = CallableImplementation::Native(id);
    }
    Some(expected)
}

pub(super) fn trait_contract(
    owner: DefinitionId,
    expected: &TraitDef,
    actual: &TraitDef,
    definitions: &DefinitionTable,
) -> bool {
    let mut expected = expected.clone();
    let Some(methods) = expected
        .methods
        .iter()
        .map(|item| method(owner, item, definitions))
        .collect()
    else {
        return false;
    };
    expected.methods = methods;
    let mut actual = actual.clone();
    let Some(methods) = actual
        .methods
        .iter()
        .map(|item| signature(item, definitions).ok())
        .collect()
    else {
        return false;
    };
    actual.methods = methods;
    expected == actual
}

/// Validate an ordinary inherited Native materialization against the installed source entry.
pub(super) fn installed(
    expected: &NativeDeclaration,
    actual: &NativeDeclaration,
    program: &VerifiedProgram,
    admission: &DefinitionAdmission,
) -> bool {
    if declaration(expected, actual, admission.target()) {
        return true;
    }
    let mut source = expected.clone();
    source.function.implementation = CallableImplementation::Native(source.declaration);
    for table in program
        .modules()
        .iter()
        .flat_map(|module| &module.public_items)
        .filter_map(|item| match item {
            PublicItem::InterfaceTable(table) => Some(table),
            _ => None,
        })
    {
        let Ok(table) = admission.runtime(table.as_ref(), &Default::default()) else {
            continue;
        };
        if let Some(origin) = table.method_origins.iter().find(|origin| {
            origin.placement == actual.declaration && origin.authored == source.declaration
        }) && let Ok(expected) = materialization::native(
            &source,
            &table,
            origin,
            admission.target(),
            &Default::default(),
        ) && same_declaration(&expected, actual, admission.target())
        {
            return true;
        }
    }
    false
}
