//! Authored source resources. This package performs no lookup or registration.

/// An authored module and the names of its direct dependencies within this bundle.
/// `uri` identifies the original text independently of the host's filesystem.
pub struct ModuleSource {
    pub name: &'static str,
    pub package_alias: &'static str,
    pub uri: &'static str,
    pub text: &'static str,
    pub dependencies: &'static [&'static str],
}

pub const MODULES: &[ModuleSource] = &[
    ModuleSource {
        name: "cmp",
        package_alias: "core",
        uri: "kagari-core:///cmp.kgr",
        text: include_str!("../cmp.kgr"),
        dependencies: &["option"],
    },
    ModuleSource {
        name: "option",
        package_alias: "core",
        uri: "kagari-core:///option.kgr",
        text: include_str!("../option.kgr"),
        dependencies: &["convert", "default", "ops"],
    },
    ModuleSource {
        name: "convert",
        package_alias: "core",
        uri: "kagari-core:///convert.kgr",
        text: include_str!("../convert.kgr"),
        dependencies: &["result"],
    },
    ModuleSource {
        name: "default",
        package_alias: "core",
        uri: "kagari-core:///default.kgr",
        text: include_str!("../default.kgr"),
        dependencies: &[],
    },
    ModuleSource {
        name: "ops",
        package_alias: "core",
        uri: "kagari-core:///ops.kgr",
        text: include_str!("../ops.kgr"),
        dependencies: &["convert"],
    },
    ModuleSource {
        name: "result",
        package_alias: "core",
        uri: "kagari-core:///result.kgr",
        text: include_str!("../result.kgr"),
        dependencies: &["convert", "ops"],
    },
    ModuleSource {
        name: "future",
        package_alias: "core",
        uri: "kagari-core:///future.kgr",
        text: include_str!("../future.kgr"),
        dependencies: &[],
    },
    ModuleSource {
        name: "fmt",
        package_alias: "core",
        uri: "kagari-core:///fmt.kgr",
        text: include_str!("../fmt.kgr"),
        dependencies: &["string"],
    },
    ModuleSource {
        name: "iter",
        package_alias: "core",
        uri: "kagari-core:///iter.kgr",
        text: include_str!("../iter.kgr"),
        dependencies: &["option"],
    },
    ModuleSource {
        name: "hash",
        package_alias: "core",
        uri: "kagari-core:///hash.kgr",
        text: include_str!("../hash.kgr"),
        dependencies: &[],
    },
    ModuleSource {
        name: "str",
        package_alias: "core",
        uri: "kagari-core:///str.kgr",
        text: include_str!("../str.kgr"),
        dependencies: &["string", "result"],
    },
    ModuleSource {
        name: "string",
        package_alias: "alloc",
        uri: "kagari-alloc:///string.kgr",
        text: include_str!("../string.kgr"),
        dependencies: &[],
    },
];
