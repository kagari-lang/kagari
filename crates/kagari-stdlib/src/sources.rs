//! Bundled, version-controlled standard declaration sources. No Host export or
//! filesystem access is needed to obtain the text.
use kagari_common::identity::{ModuleIdentity, PackageId};

/// One declaration-mode source owned by an exact standard module identity.
pub struct BundledDeclaration {
    pub package: &'static str,
    pub module: &'static [&'static str],
    pub path: &'static str,
    pub text: &'static str,
}

impl BundledDeclaration {
    /// Exact registered owner; the relative path is only a presentation location.
    pub fn identity(&self) -> ModuleIdentity {
        ModuleIdentity {
            package: PackageId(self.package.into()),
            path: self.module.iter().map(|part| (*part).into()).collect(),
        }
    }
}

pub static DECLARATIONS: &[BundledDeclaration] = &[
    BundledDeclaration {
        package: "kagari-alloc",
        module: &["string"],
        path: "alloc/string.kgr",
        text: include_str!("../declarations/alloc/string.kgr"),
    },
    BundledDeclaration {
        package: "kagari-alloc",
        module: &["vec"],
        path: "alloc/vec.kgr",
        text: include_str!("../declarations/alloc/vec.kgr"),
    },
    BundledDeclaration {
        package: "kagari-core",
        module: &["array"],
        path: "core/array.kgr",
        text: include_str!("../declarations/core/array.kgr"),
    },
    BundledDeclaration {
        package: "kagari-core",
        module: &["char"],
        path: "core/char.kgr",
        text: include_str!("../declarations/core/char.kgr"),
    },
    BundledDeclaration {
        package: "kagari-core",
        module: &["cmp"],
        path: "core/cmp.kgr",
        text: include_str!("../declarations/core/cmp.kgr"),
    },
    BundledDeclaration {
        package: "kagari-core",
        module: &["convert"],
        path: "core/convert.kgr",
        text: include_str!("../declarations/core/convert.kgr"),
    },
    BundledDeclaration {
        package: "kagari-core",
        module: &["default"],
        path: "core/default.kgr",
        text: include_str!("../declarations/core/default.kgr"),
    },
    BundledDeclaration {
        package: "kagari-core",
        module: &["fmt"],
        path: "core/fmt.kgr",
        text: include_str!("../declarations/core/fmt.kgr"),
    },
    BundledDeclaration {
        package: "kagari-core",
        module: &["future"],
        path: "core/future.kgr",
        text: include_str!("../declarations/core/future.kgr"),
    },
    BundledDeclaration {
        package: "kagari-core",
        module: &["hash"],
        path: "core/hash.kgr",
        text: include_str!("../declarations/core/hash.kgr"),
    },
    BundledDeclaration {
        package: "kagari-core",
        module: &["host"],
        path: "core/host.kgr",
        text: include_str!("../declarations/core/host.kgr"),
    },
    BundledDeclaration {
        package: "kagari-core",
        module: &["iter"],
        path: "core/iter.kgr",
        text: include_str!("../declarations/core/iter.kgr"),
    },
    BundledDeclaration {
        package: "kagari-core",
        module: &["num"],
        path: "core/num.kgr",
        text: include_str!("../declarations/core/num.kgr"),
    },
    BundledDeclaration {
        package: "kagari-core",
        module: &["ops"],
        path: "core/ops.kgr",
        text: include_str!("../declarations/core/ops.kgr"),
    },
    BundledDeclaration {
        package: "kagari-core",
        module: &["option"],
        path: "core/option.kgr",
        text: include_str!("../declarations/core/option.kgr"),
    },
    BundledDeclaration {
        package: "kagari-core",
        module: &["result"],
        path: "core/result.kgr",
        text: include_str!("../declarations/core/result.kgr"),
    },
    BundledDeclaration {
        package: "kagari-core",
        module: &["str"],
        path: "core/str.kgr",
        text: include_str!("../declarations/core/str.kgr"),
    },
    BundledDeclaration {
        package: "kagari-core",
        module: &["task"],
        path: "core/task.kgr",
        text: include_str!("../declarations/core/task.kgr"),
    },
    BundledDeclaration {
        package: "std",
        module: &["array"],
        path: "std/array.kgr",
        text: include_str!("../declarations/std/array.kgr"),
    },
    BundledDeclaration {
        package: "std",
        module: &["char"],
        path: "std/char.kgr",
        text: include_str!("../declarations/std/char.kgr"),
    },
    BundledDeclaration {
        package: "std",
        module: &["cmp"],
        path: "std/cmp.kgr",
        text: include_str!("../declarations/std/cmp.kgr"),
    },
    BundledDeclaration {
        package: "std",
        module: &["collections"],
        path: "std/collections.kgr",
        text: include_str!("../declarations/std/collections.kgr"),
    },
    BundledDeclaration {
        package: "std",
        module: &["convert"],
        path: "std/convert.kgr",
        text: include_str!("../declarations/std/convert.kgr"),
    },
    BundledDeclaration {
        package: "std",
        module: &["default"],
        path: "std/default.kgr",
        text: include_str!("../declarations/std/default.kgr"),
    },
    BundledDeclaration {
        package: "std",
        module: &["fmt"],
        path: "std/fmt.kgr",
        text: include_str!("../declarations/std/fmt.kgr"),
    },
    BundledDeclaration {
        package: "std",
        module: &["future"],
        path: "std/future.kgr",
        text: include_str!("../declarations/std/future.kgr"),
    },
    BundledDeclaration {
        package: "std",
        module: &["hash"],
        path: "std/hash.kgr",
        text: include_str!("../declarations/std/hash.kgr"),
    },
    BundledDeclaration {
        package: "std",
        module: &["host"],
        path: "std/host.kgr",
        text: include_str!("../declarations/std/host.kgr"),
    },
    BundledDeclaration {
        package: "std",
        module: &["iter"],
        path: "std/iter.kgr",
        text: include_str!("../declarations/std/iter.kgr"),
    },
    BundledDeclaration {
        package: "std",
        module: &["num"],
        path: "std/num.kgr",
        text: include_str!("../declarations/std/num.kgr"),
    },
    BundledDeclaration {
        package: "std",
        module: &["ops"],
        path: "std/ops.kgr",
        text: include_str!("../declarations/std/ops.kgr"),
    },
    BundledDeclaration {
        package: "std",
        module: &["option"],
        path: "std/option.kgr",
        text: include_str!("../declarations/std/option.kgr"),
    },
    BundledDeclaration {
        package: "std",
        module: &["prelude"],
        path: "std/prelude.kgr",
        text: include_str!("../declarations/std/prelude.kgr"),
    },
    BundledDeclaration {
        package: "std",
        module: &["result"],
        path: "std/result.kgr",
        text: include_str!("../declarations/std/result.kgr"),
    },
    BundledDeclaration {
        package: "std",
        module: &["str"],
        path: "std/str.kgr",
        text: include_str!("../declarations/std/str.kgr"),
    },
    BundledDeclaration {
        package: "std",
        module: &["string"],
        path: "std/string.kgr",
        text: include_str!("../declarations/std/string.kgr"),
    },
    BundledDeclaration {
        package: "std",
        module: &["task"],
        path: "std/task.kgr",
        text: include_str!("../declarations/std/task.kgr"),
    },
    BundledDeclaration {
        package: "std",
        module: &["vec"],
        path: "std/vec.kgr",
        text: include_str!("../declarations/std/vec.kgr"),
    },
];

/// Looks up only explicitly bundled standard modules. Equal short names grant
/// no authority to a separately registered Host provider.
pub fn declaration(identity: &ModuleIdentity) -> Option<&'static BundledDeclaration> {
    DECLARATIONS.iter().find(|source| {
        source.package == identity.package.0
            && source
                .module
                .iter()
                .copied()
                .eq(identity.path.iter().map(String::as_str))
    })
}
