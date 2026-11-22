//! The std facade surface over ordinary authored modules. These are export names,
//! not recipes: the seed and the ordinary sealer supply and check their targets.
use crate::{StandardLibrary, namespaces};
use kagari_bytecode::program::verified::VerifiedBytecodeProgram;
use kagari_common::identity::{DefinitionKind, ModuleIdentity};
use kagari_runtime::native::{binding::NativeResult, catalog::DeclarationCatalog};

pub(super) struct Facade {
    pub path: &'static str,
    pub docs: (&'static str, &'static str),
    pub exports: &'static [(DefinitionKind, &'static [&'static str])],
}

impl Facade {
    pub(super) fn owner(&self) -> ModuleIdentity {
        match self.path {
            "string" => namespaces::type_owner("String"),
            path => namespaces::module("core", path),
        }
    }
}

impl StandardLibrary {
    /// Compose against only the authored owners required by the standard facades.
    /// Missing or incompatible owners fail construction; application modules in the
    /// verified program do not contribute declarations to the bundle.
    pub fn from_verified_program(program: &VerifiedBytecodeProgram) -> NativeResult<Self> {
        let owners: Vec<_> = FACADES.iter().map(Facade::owner).collect();
        let seed = DeclarationCatalog::default().with_verified_modules(program, &owners)?;
        Self::new(&seed)
    }
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
        exports: &[(
            DefinitionKind::Trait,
            &["From", "Into", "TryFrom", "TryInto"],
        )],
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
        path: "fmt",
        docs: (
            "Diagnostic and user-facing formatting.",
            "fn show<T: std::fmt::Display>(v: T) -> String { v.display() }",
        ),
        exports: &[(DefinitionKind::Trait, &["Debug", "Display"])],
    },
    Facade {
        path: "hash",
        docs: (
            "Hash codes for key-based lookup.",
            "fn code<T: std::hash::Hash>(v: T) -> i64 { v.hash() }",
        ),
        exports: &[(DefinitionKind::Trait, &["Hash"])],
    },
    Facade {
        path: "iter",
        docs: (
            "Shared cursors and lazy traversal.",
            "fn count<I: Iterator>(it: I) -> u64 { it.count() }",
        ),
        exports: &[
            (
                DefinitionKind::Trait,
                &["Iterable", "Iterator", "FromIterator", "Sum", "Product"],
            ),
            (
                DefinitionKind::Struct,
                &[
                    "MapIterator",
                    "FilterIterator",
                    "TakeIterator",
                    "FilterMapIterator",
                    "SkipIterator",
                    "EnumerateIterator",
                    "ZipIterator",
                    "ChainIterator",
                ],
            ),
        ],
    },
    Facade {
        path: "str",
        docs: (
            "Parsing text into a value.",
            "fn parse(text: String) -> Result<i32, core::num::ParseError> { i32::from_str(text) }",
        ),
        exports: &[(DefinitionKind::Trait, &["FromStr"])],
    },
    Facade {
        path: "string",
        docs: (
            "Immutable UTF-8 strings and standard text operations.",
            "fn clean(text: String) -> String { text.trim() }",
        ),
        exports: &[(DefinitionKind::AssociatedType, &["String"])],
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
