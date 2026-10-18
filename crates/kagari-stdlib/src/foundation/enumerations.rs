//! Canonical enum owners and their explicit variant/public inventory.
use crate::{foundation::type_docs, namespaces};
use kagari_runtime::native::{assembly::draft::ModuleDraft, binding::NativeResult};
use kagari_types::declaration::{TypeDefKind, VariantDef};

pub(crate) struct Enumeration {
    pub name: &'static str,
    arity: usize,
    variants: &'static [(&'static str, Option<usize>)],
    export: bool,
}

pub(crate) const ENUMERATIONS: &[Enumeration] = &[
    Enumeration {
        name: "Option",
        arity: 1,
        variants: &[("Some", Some(0)), ("None", None)],
        export: true,
    },
    Enumeration {
        name: "Result",
        arity: 2,
        variants: &[("Ok", Some(0)), ("Err", Some(1))],
        export: true,
    },
    Enumeration {
        name: "Ordering",
        arity: 0,
        variants: &[("Less", None), ("Equal", None), ("Greater", None)],
        export: true,
    },
    Enumeration {
        name: "Bound",
        arity: 1,
        variants: &[
            ("Included", Some(0)),
            ("Excluded", Some(0)),
            ("Unbounded", None),
        ],
        export: true,
    },
    Enumeration {
        name: "ParseError",
        arity: 0,
        variants: &[
            ("Empty", None),
            ("InvalidDigit", None),
            ("OutOfRange", None),
            ("InvalidRadix", None),
            ("InvalidSyntax", None),
        ],
        export: false,
    },
    Enumeration {
        name: "TryFromIntError",
        arity: 0,
        variants: &[("OutOfRange", None)],
        export: false,
    },
    Enumeration {
        name: "ControlFlow",
        arity: 2,
        variants: &[("Break", Some(0)), ("Continue", Some(1))],
        export: false,
    },
    Enumeration {
        name: "Infallible",
        arity: 0,
        variants: &[],
        export: false,
    },
];

pub(super) fn declare(module: &mut ModuleDraft<'_>) -> NativeResult<()> {
    for enumeration in ENUMERATIONS {
        if *module.identity() != namespaces::type_owner(enumeration.name) {
            continue;
        }
        let reference = module.define_type(
            enumeration.name,
            TypeDefKind::Enum,
            enumeration.arity,
            |record, _| {
                record.variants = enumeration
                    .variants
                    .iter()
                    .map(|(name, slot)| VariantDef {
                        name: (*name).into(),
                        reports_failure: enumeration.name == "Result" && *name == "Err",
                        payload: slot
                            .map(|slot| vec![record.generic_params[slot].as_type()])
                            .unwrap_or_default(),
                    })
                    .collect();
                Ok(())
            },
        )?;
        if enumeration.export {
            module.export_variants(&reference)?;
        }
        let variants = enumeration
            .variants
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>();
        type_docs(module, &reference, enumeration.name, &variants)?;
    }
    Ok(())
}
