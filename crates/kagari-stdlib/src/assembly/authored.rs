//! The std facade surface over ordinary authored modules. These are export names,
//! not recipes: the seed and the ordinary sealer supply and check their targets.
use kagari_common::identity::DefinitionKind;

pub(super) struct Facade {
    pub path: &'static str,
    pub docs: (&'static str, &'static str),
    pub exports: &'static [(DefinitionKind, &'static [&'static str])],
}

pub(super) const FACADES: &[Facade] = &[
    Facade {
        path: "cmp",
        docs: (
            "Equality and ordering contracts.",
            "fn same(left: i32, right: i32) -> bool { left == right }",
        ),
        exports: &[
            (
                DefinitionKind::Trait,
                &["PartialEq", "Eq", "PartialOrd", "Ord"],
            ),
            (DefinitionKind::Enum, &["Ordering"]),
        ],
    },
    Facade {
        path: "convert",
        docs: (
            "Explicit infallible and checked conversions.",
            "fn widen(value: i32) -> i64 { i64::from(value) }",
        ),
        exports: &[
            (
                DefinitionKind::Trait,
                &["From", "Into", "TryFrom", "TryInto"],
            ),
            (DefinitionKind::Enum, &["Infallible"]),
        ],
    },
    Facade {
        path: "default",
        docs: (
            "Explicit implementation-defined construction through Default.",
            "fn absent<T>() -> Option<T> { Option<T>::default() }",
        ),
        exports: &[(DefinitionKind::Trait, &["Default"])],
    },
    Facade {
        path: "ops",
        docs: (
            "Operator contracts, range values and boundary policies.",
            "fn plus(left: i32, right: i32) -> i32 { left + right }",
        ),
        exports: &[
            (
                DefinitionKind::Trait,
                &[
                    "Add",
                    "Sub",
                    "Mul",
                    "Div",
                    "Rem",
                    "BitAnd",
                    "BitOr",
                    "BitXor",
                    "Shl",
                    "Shr",
                    "Neg",
                    "Not",
                    "Index",
                    "Fn",
                    "RangeBounds",
                    "Try",
                    "FromResidual",
                ],
            ),
            (DefinitionKind::Enum, &["Bound", "ControlFlow"]),
            (
                DefinitionKind::AssociatedType,
                &[
                    "Range",
                    "RangeInclusive",
                    "RangeFrom",
                    "RangeTo",
                    "RangeToInclusive",
                    "RangeFull",
                ],
            ),
        ],
    },
    Facade {
        path: "option",
        docs: (
            "Optional values: Some contains a value; None represents absence.",
            "fn absent() -> Option<i32> { None }",
        ),
        exports: &[(DefinitionKind::Enum, &["Option"])],
    },
    Facade {
        path: "result",
        docs: (
            "Success and failure values, with early error propagation.",
            "fn parse(text: String) -> Result<i32, core::num::ParseError> { i32::from_str(text) }",
        ),
        exports: &[(DefinitionKind::Enum, &["Result"])],
    },
];
