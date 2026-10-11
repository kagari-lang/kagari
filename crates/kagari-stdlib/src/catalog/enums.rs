//! Library enum declarations use the ordinary nominal authoring model.
use crate::catalog::identities;
use kagari_common::identity::{DefinitionKind, map::DefinitionContext};
use kagari_types::{
    declaration::{
        TypeDef, TypeDefKind, VariantDef,
        module::{DeclarationError, ModuleDecl},
    },
    ty::GenericParam,
};

pub(super) fn declare(
    ids: &DefinitionContext,
    module: &mut ModuleDecl,
) -> Result<(), DeclarationError> {
    for (name, parameters, variants, export) in [
        (
            "Option",
            &["T"][..],
            &[("Some", Some(0)), ("None", None)][..],
            true,
        ),
        (
            "Result",
            &["T", "E"][..],
            &[("Ok", Some(0)), ("Err", Some(1))][..],
            true,
        ),
        (
            "Ordering",
            &[][..],
            &[("Less", None), ("Equal", None), ("Greater", None)][..],
            true,
        ),
        (
            "Bound",
            &["T"][..],
            &[
                ("Included", Some(0)),
                ("Excluded", Some(0)),
                ("Unbounded", None),
            ][..],
            true,
        ),
        (
            "ParseError",
            &[][..],
            &[
                ("Empty", None),
                ("InvalidDigit", None),
                ("OutOfRange", None),
                ("InvalidRadix", None),
                ("InvalidSyntax", None),
            ][..],
            false,
        ),
        (
            "TryFromIntError",
            &[][..],
            &[("OutOfRange", None)][..],
            false,
        ),
        (
            "ControlFlow",
            &["B", "C"][..],
            &[("Break", Some(0)), ("Continue", Some(1))][..],
            false,
        ),
        ("Infallible", &[][..], &[][..], false),
    ] {
        let owner = identities::definition(ids, DefinitionKind::Enum, name)?;
        let generic_params: Vec<_> = parameters
            .iter()
            .enumerate()
            .map(|(position, _)| GenericParam { owner, position })
            .collect();
        module.types.push(TypeDef {
            name: name.into(),
            kind: TypeDefKind::Enum,
            bounds: vec![],
            fields: vec![],
            variants: variants
                .iter()
                .map(|(variant_name, slot)| VariantDef {
                    reports_failure: name == "Result" && *variant_name == "Err",
                    name: (*variant_name).into(),
                    payload: slot
                        .map(|slot| vec![generic_params[slot].as_type()])
                        .unwrap_or_default(),
                })
                .collect(),
            generic_params,
        });
        if export {
            module.variant_exports.insert(name.into());
        }
        module
            .documentation
            .insert(owner, format!("Core library {name} enum."));
    }

    Ok(())
}
