//! Authored source resources. This package performs no lookup or registration.

/// A module in the `kagari-core` package and its direct in-package dependencies.
/// `uri` identifies the original text independently of the host's filesystem.
pub struct ModuleSource {
    pub name: &'static str,
    pub uri: &'static str,
    pub text: &'static str,
    pub dependencies: &'static [&'static str],
}

pub const MODULES: &[ModuleSource] = &[
    ModuleSource {
        name: "cmp",
        uri: "kagari-core:///cmp.kgr",
        text: include_str!("../cmp.kgr"),
        dependencies: &["option"],
    },
    ModuleSource {
        name: "option",
        uri: "kagari-core:///option.kgr",
        text: include_str!("../option.kgr"),
        dependencies: &["convert", "default", "ops"],
    },
    ModuleSource {
        name: "convert",
        uri: "kagari-core:///convert.kgr",
        text: include_str!("../convert.kgr"),
        dependencies: &["result"],
    },
    ModuleSource {
        name: "default",
        uri: "kagari-core:///default.kgr",
        text: include_str!("../default.kgr"),
        dependencies: &[],
    },
    ModuleSource {
        name: "ops",
        uri: "kagari-core:///ops.kgr",
        text: include_str!("../ops.kgr"),
        dependencies: &["convert"],
    },
    ModuleSource {
        name: "result",
        uri: "kagari-core:///result.kgr",
        text: include_str!("../result.kgr"),
        dependencies: &["convert", "ops"],
    },
];
