//! Canonical enum owners and their explicit variant/public inventory.
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
            "fn first(values: Vec<i32>) -> Option<i32> { values.get(0) }",
        ),
        arity: 1,
        variants: &[("Some", Some(0)), ("None", None)],
        export: true,
    },
    Enumeration {
        name: "Result",
        documentation: (
            "A checked outcome: Ok carries the successful value and Err carries the error. Question-mark propagation returns a converted error early.",
            "fn read(text: String) -> Result<i32, core::num::ParseError> { i32::from_str(text) }",
        ),
        arity: 2,
        variants: &[("Ok", Some(0)), ("Err", Some(1))],
        export: true,
    },
    Enumeration {
        name: "Ordering",
        documentation: (
            "The result of a total comparison: Less, Equal or Greater.",
            "fn compare(left: i32, right: i32) -> core::cmp::Ordering { left.cmp(right) }",
        ),
        arity: 0,
        variants: &[("Less", None), ("Equal", None), ("Greater", None)],
        export: true,
    },
    Enumeration {
        name: "Bound",
        documentation: (
            "A range endpoint: Included(value), Excluded(value) or Unbounded.",
            "fn lower() -> core::ops::Bound<i32> { (1..4).start_bound() }",
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
            "fn number(text: String) -> Result<i32, core::num::ParseError> { i32::from_str(text) }",
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
            "fn narrow(value: i64) -> Result<i32, core::num::TryFromIntError> { i32::try_from(value) }",
        ),
        arity: 0,
        variants: &[("OutOfRange", None)],
        export: false,
    },
    Enumeration {
        name: "CharTryFromError",
        documentation: (
            "A Unicode scalar conversion error. InvalidScalar rejects surrogate values U+D800 through U+DFFF and values above U+10FFFF.",
            "fn scalar(value: u32) -> Result<char, core::char::CharTryFromError> { char::try_from(value) }",
        ),
        arity: 0,
        variants: &[("InvalidScalar", None)],
        export: false,
    },
    Enumeration {
        name: "ControlFlow",
        documentation: (
            "Continue contains a successful value; Break contains an early-exit value. Try uses ControlFlow<B, Infallible> as its distinct residual, preserving the break value without conversion.",
            "fn success() -> core::ops::ControlFlow<String, i32> { core::ops::ControlFlow::Continue(42) }",
        ),
        arity: 2,
        variants: &[("Break", Some(0)), ("Continue", Some(1))],
        export: false,
    },
    Enumeration {
        name: "Infallible",
        documentation: (
            "An uninhabited error type for conversions that cannot fail. It has no constructible variants.",
            "fn widen(value: i32) -> i64 { i64::from(value) }",
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
    "fn first(values: Vec<i32>) -> Option<i32> { values.get(0) }",
);

pub(super) const RESULT_MODULE_DOCS: (&str, &str) = (
    "Success and failure values. Question-mark propagation returns an error early and uses a declared From conversion where needed.",
    "fn parse(text: String) -> Result<i32, core::num::ParseError> { i32::from_str(text) }",
);
