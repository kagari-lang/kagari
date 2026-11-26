//! Canonical enum owners and their explicit variant/public inventory.
pub(super) mod value;
use crate::{foundation::type_docs, namespaces};
use kagari_runtime::native::{assembly::draft::ModuleDraft, binding::NativeResult};
use kagari_types::declaration::{TypeDefKind, VariantDef};

pub(crate) struct Enumeration {
    pub name: &'static str,
    documentation: (&'static str, &'static str),
    variants: &'static [&'static str],
}

pub(crate) const ENUMERATIONS: &[Enumeration] = &[
    Enumeration {
        name: "ParseError",
        documentation: (
            "A numeric parsing error. Invalid input and out-of-range values are reported through Result.",
            "fn number(text: String) -> Result<i32, core::num::ParseError> { i32::from_str(text) }",
        ),
        variants: &[
            "Empty",
            "InvalidDigit",
            "OutOfRange",
            "InvalidRadix",
            "InvalidSyntax",
        ],
    },
    Enumeration {
        name: "TryFromIntError",
        documentation: (
            "A checked numeric conversion error. Its variants distinguish the declared failure conditions.",
            "fn narrow(value: i64) -> Result<i32, core::num::TryFromIntError> { i32::try_from(value) }",
        ),
        variants: &["OutOfRange"],
    },
    Enumeration {
        name: "CharTryFromError",
        documentation: (
            "A Unicode scalar conversion error. InvalidScalar rejects surrogate values U+D800 through U+DFFF and values above U+10FFFF.",
            "fn scalar(value: u32) -> Result<char, core::char::CharTryFromError> { char::try_from(value) }",
        ),
        variants: &["InvalidScalar"],
    },
];

pub(super) fn declare(module: &mut ModuleDraft<'_>) -> NativeResult<()> {
    for enumeration in ENUMERATIONS {
        if *module.identity() != namespaces::type_owner(enumeration.name) {
            continue;
        }
        let reference =
            module.define_type(enumeration.name, TypeDefKind::Enum, 0, |record, _| {
                record.variants = enumeration
                    .variants
                    .iter()
                    .map(|name| VariantDef {
                        name: (*name).into(),
                        reports_failure: false,
                        shape: Default::default(),
                        payload: vec![],
                    })
                    .collect();
                Ok(())
            })?;
        type_docs(
            module,
            &reference,
            enumeration.name,
            enumeration.documentation,
            enumeration.variants,
        )?;
    }
    Ok(())
}
