//! Canonical declaration ownership and public package spelling of the bundled library.
use kagari_common::identity::{ModuleIdentity, PackageId};
use kagari_types::{scalar::BuiltinType, ty::Ty};

pub fn module(package: &str, path: &str) -> ModuleIdentity {
    ModuleIdentity {
        package: PackageId(
            match package {
                "core" => "kagari-core",
                "alloc" => "kagari-alloc",
                other => other,
            }
            .into(),
        ),
        path: path.split("::").map(str::to_owned).collect(),
    }
}

pub fn source_module(identity: &ModuleIdentity) -> String {
    let package = match identity.package.0.as_str() {
        "kagari-core" => "core",
        "kagari-alloc" => "alloc",
        other => other,
    };
    format!("{package}::{}", identity.path.join("::"))
}

pub fn trait_owner(name: &str) -> ModuleIdentity {
    match name {
        "Iterator" | "Iterable" | "FromIterator" | "Sum" | "Product" => module("core", "iter"),
        "PartialEq" | "Eq" | "PartialOrd" | "Ord" => module("core", "cmp"),
        "Hash" => module("core", "hash"),
        "Debug" | "Display" => module("core", "fmt"),
        "From" | "Into" | "TryFrom" | "TryInto" => module("core", "convert"),
        "FromStr" => module("core", "str"),
        "Add" | "Sub" | "Mul" | "Div" | "Rem" | "BitAnd" | "BitOr" | "BitXor" | "Shl" | "Shr"
        | "Neg" | "Not" | "Index" | "Fn" | "RangeBounds" => module("core", "ops"),
        "List" | "MutableList" | "Map" | "MutableMap" | "Set" | "MutableSet" => {
            module("std", "collections")
        }
        _ => panic!("unknown bundled trait {name}"),
    }
}

pub fn type_owner(name: &str) -> ModuleIdentity {
    match name {
        "String" => module("alloc", "string"),
        "Vec" => module("alloc", "vec"),
        "Option" => module("core", "option"),
        "Result" => module("core", "result"),
        "Ordering" => module("core", "cmp"),
        "Infallible" => module("core", "convert"),
        "ParseError" | "TryFromIntError" => module("core", "num"),
        "Bound" | "Range" | "RangeInclusive" | "RangeFrom" | "RangeTo" | "RangeToInclusive"
        | "RangeFull" => module("core", "ops"),
        "CollectionCursor" => module("core", "iter"),
        "HashMap" | "HashSet" => module("std", "collections"),
        _ => panic!("unknown bundled type {name}"),
    }
}

pub fn receiver_owner(ty: &Ty) -> Option<ModuleIdentity> {
    Some(match ty {
        Ty::NativeObject(nominal) | Ty::Struct(nominal) | Ty::Enum(nominal) => {
            nominal.declaration.module.clone()
        }
        Ty::Builtin(BuiltinType::String) => type_owner("String"),
        Ty::Builtin(_) => module("core", "num"),
        Ty::Array(..) => type_owner("Vec"),
        Ty::Map { .. } | Ty::Set(..) => module("std", "collections"),
        Ty::Iter(_) => module("core", "iter"),
        Ty::Range(..) => module("core", "ops"),
        _ => return None,
    })
}

pub fn is_language_module(identity: &ModuleIdentity) -> bool {
    ["iter", "cmp", "hash", "fmt", "convert", "ops"]
        .iter()
        .any(|path| *identity == module("core", path))
}

pub fn prelude() -> ModuleIdentity {
    module("std", "prelude")
}
