//! Canonical enum owners and their explicit variant/public inventory.
pub(super) mod documentation;
pub(super) mod value;
use crate::{foundation::type_docs, namespaces};
use kagari_runtime::native::{assembly::draft::ModuleDraft, binding::NativeResult};
use kagari_types::declaration::{TypeDefKind, VariantDef};

pub(crate) struct Enumeration {
    pub name: &'static str,
    arity: usize,
    documentation: (&'static str, &'static str),
    variants: &'static [(&'static str, Option<usize>)],
    export: bool,
}

pub(crate) const ENUMERATIONS: &[Enumeration] = &[
    Enumeration {
        name: "Option",
        documentation: (
            "An optional value: Some carries one value and None carries none. Question-mark propagation returns None early.",
            "fn main() -> bool {\n    val values = Vec::from([4, 5]);\n    values.get(0u64) == Some(4) && values.get(2u64) == None\n}",
        ),
        arity: 1,
        variants: &[("Some", Some(0)), ("None", None)],
        export: true,
    },
    Enumeration {
        name: "Result",
        documentation: (
            "A checked outcome: Ok carries the successful value and Err carries the error. Question-mark propagation returns a converted error early.",
            "use core::num::ParseError;\n\nfn main() -> bool {\n    (\n        match i32::from_str(\"42\") {\n            Ok(value) => value == 42,\n            Err(_) => false,\n        }\n    ) &&\n        (\n        match i32::from_str(\"\") {\n            Err(ParseError::Empty) => true,\n            _ => false,\n        }\n    ) &&\n        (\n        match i32::from_str(\"12x\") {\n            Err(ParseError::InvalidDigit) => true,\n            _ => false,\n        }\n    )\n}",
        ),
        arity: 2,
        variants: &[("Ok", Some(0)), ("Err", Some(1))],
        export: true,
    },
    Enumeration {
        name: "Ordering",
        documentation: (
            "The result of a total comparison: Less, Equal or Greater.",
            "use core::cmp::Ordering;\nfn main() -> bool {\n    2.cmp(3) == Ordering::Less && 3.cmp(3) == Ordering::Equal && 4.cmp(3) == Ordering::Greater\n}",
        ),
        arity: 0,
        variants: &[("Less", None), ("Equal", None), ("Greater", None)],
        export: true,
    },
    Enumeration {
        name: "Bound",
        documentation: (
            "A range endpoint: Included(value), Excluded(value) or Unbounded.",
            "use core::ops::Bound;\nfn main() -> bool {\n    (1..=4).start_bound() == Bound::Included(1) &&\n        (1..=4).end_bound() == Bound::Included(4)\n}",
        ),
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
        documentation: (
            "A numeric parsing error. Invalid input and out-of-range values are reported through Result.",
            "use core::num::ParseError;\n\nfn main() -> bool {\n    (\n        match i32::from_str(\"42\") {\n            Ok(value) => value == 42,\n            Err(_) => false,\n        }\n    ) &&\n        (\n        match i32::from_str(\"\") {\n            Err(ParseError::Empty) => true,\n            _ => false,\n        }\n    ) &&\n        (\n        match i32::from_str(\"12x\") {\n            Err(ParseError::InvalidDigit) => true,\n            _ => false,\n        }\n    )\n}",
        ),
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
        documentation: (
            "A checked numeric conversion error. Its variants distinguish the declared failure conditions.",
            "use core::num::TryFromIntError;\nfn main() -> bool {\n    (match i32::try_from(42i64) { Ok(value) => value == 42i32, Err(_) => false, }) &&\n        (match i32::try_from(2147483648i64) { Err(TryFromIntError::OutOfRange) => true, _ => false, })\n}",
        ),
        arity: 0,
        variants: &[("OutOfRange", None)],
        export: false,
    },
    Enumeration {
        name: "CharTryFromError",
        documentation: (
            "A Unicode scalar conversion error. InvalidScalar rejects surrogate values U+D800 through U+DFFF and values above U+10FFFF.",
            "use core::char::CharTryFromError;\nfn main() -> bool {\n    (match char::try_from(0xE9u32) { Ok(value) => value == 'é', Err(_) => false, }) &&\n        (match char::try_from(0xD800u32) { Err(CharTryFromError::InvalidScalar) => true, _ => false, })\n}",
        ),
        arity: 0,
        variants: &[("InvalidScalar", None)],
        export: false,
    },
    Enumeration {
        name: "ControlFlow",
        documentation: (
            "Continue contains a successful value; Break contains an early-exit value. Try uses ControlFlow<B, Infallible> as its distinct residual, preserving the break value without conversion.",
            "use core::ops::ControlFlow;\nfn main() -> bool {\n    val value: ControlFlow<String, i32> = ControlFlow::Continue(42);\n    match value {\n        ControlFlow::Continue(item) => item == 42,\n        ControlFlow::Break(_) => false,\n    }\n}",
        ),
        arity: 2,
        variants: &[("Break", Some(0)), ("Continue", Some(1))],
        export: false,
    },
    Enumeration {
        name: "Infallible",
        documentation: (
            "An uninhabited error type for conversions that cannot fail. It has no constructible variants.",
            "fn main() -> bool {\n    val converted: Result<i64, core::convert::Infallible> = i64::try_from(42i32);\n    match converted {\n        Ok(value) => value == 42i64,\n        Err(_) => false,\n    }\n}",
        ),
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
        type_docs(
            module,
            &reference,
            enumeration.name,
            enumeration.documentation,
            &variants,
        )?;
    }
    Ok(())
}

pub(super) const OPTION_MODULE_DOCS: (&str, &str) = (
    "Optional values: Some contains a value; None represents absence. Question-mark propagation preserves the enclosing Option result.",
    "fn main() -> bool {\n    val values = Vec::from([4, 5]);\n    values.get(0u64) == Some(4) && values.get(2u64) == None\n}",
);

pub(super) const RESULT_MODULE_DOCS: (&str, &str) = (
    "Success and failure values. Question-mark propagation returns an error early and uses a declared From conversion where needed.",
    "use core::num::ParseError;\n\nfn main() -> bool {\n    (\n        match i32::from_str(\"42\") {\n            Ok(value) => value == 42,\n            Err(_) => false,\n        }\n    ) &&\n        (\n        match i32::from_str(\"\") {\n            Err(ParseError::Empty) => true,\n            _ => false,\n        }\n    ) &&\n        (\n        match i32::from_str(\"12x\") {\n            Err(ParseError::InvalidDigit) => true,\n            _ => false,\n        }\n    )\n}",
);
