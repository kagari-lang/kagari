//! Source-free presentation of retained identities, semantic types and execution facts.

mod escaping;
mod types;

#[cfg(test)]
mod tests;

use crate::disassembly::presentation::escaping::{name, push_quoted};
use kagari_abi::representation::ValueType;
use kagari_common::identity::{
    ModuleIdentity,
    table::{DefinitionId, DefinitionTable},
};
use kagari_contract::effects::EffectSet;
use std::fmt::Write;

/// Borrows the verified program's retained identity scope for one output operation.
///
/// All definition handles, including those nested in types, must belong to this
/// retained table. Presentation neither imports identities nor reads source files.
pub(crate) struct DisplayContext<'a> {
    pub definitions: &'a DefinitionTable,
}

impl DisplayContext<'_> {
    /// Displays a portable path, preserving nonzero declaration occurrences.
    ///
    /// Ordinary path components remain unquoted; other components are quoted and
    /// escaped independently so artifact text cannot inject output structure.
    /// Panics if the caller supplies a handle outside the retained verified scope.
    pub(crate) fn definition(&self, id: DefinitionId) -> String {
        let view = self
            .definitions
            .resolve(id)
            .expect("disassembly definition belongs to the retained verified scope");
        let mut output = self.module(view.module());
        for segment in view.segments() {
            output.push_str("::");
            output.push_str(&name(segment.name));
            if segment.occurrence != 0 {
                write!(output, "#{}", segment.occurrence).expect("writing to a String");
            }
        }
        output
    }

    /// Displays the package and module path without any process-local table ID.
    pub(crate) fn module(&self, module: &ModuleIdentity) -> String {
        let ModuleIdentity { package, path } = module;
        let mut output = name(&package.0);
        for component in path {
            output.push_str("::");
            output.push_str(&name(component));
        }
        output
    }
}

/// Names the stored physical representation, not an inferred semantic type.
pub(crate) fn physical(ty: ValueType) -> &'static str {
    match ty {
        ValueType::Never => "never",
        ValueType::Unit => "unit",
        ValueType::Bool => "bool",
        ValueType::I32 => "i32",
        ValueType::I64 => "i64",
        ValueType::U64 => "u64",
        ValueType::F32 => "f32",
        ValueType::F64 => "f64",
        ValueType::Str => "str",
        ValueType::HeapObject => "heap_object",
        ValueType::HostHandle => "host_handle",
        ValueType::Generic => "generic",
        ValueType::Char => "char",
    }
}

/// Lists set recorded effects in declaration-field order, or `[]` when none are set.
///
/// These are stored summary bits, not recomputed transitive effects. An absent
/// bit, including an entirely empty summary, does not establish purity.
pub(crate) fn effects(effects: EffectSet) -> String {
    let EffectSet {
        reads_local,
        writes_local,
        reads_module,
        writes_module,
        reads_aggregate,
        writes_aggregate,
        reads_path,
        writes_path,
        allocates,
        calls,
        touches_runtime,
        may_trap,
        may_suspend,
    } = effects;
    let fields = [
        ("reads_local", reads_local),
        ("writes_local", writes_local),
        ("reads_module", reads_module),
        ("writes_module", writes_module),
        ("reads_aggregate", reads_aggregate),
        ("writes_aggregate", writes_aggregate),
        ("reads_path", reads_path),
        ("writes_path", writes_path),
        ("allocates", allocates),
        ("calls", calls),
        ("touches_runtime", touches_runtime),
        ("may_trap", may_trap),
        ("may_suspend", may_suspend),
    ];
    let mut output = String::from("[");
    for (name, set) in fields {
        if set {
            if output.len() > 1 {
                output.push_str(", ");
            }
            output.push_str(name);
        }
    }
    output.push(']');
    output
}

/// Quotes artifact text as one line while retaining ordinary UTF-8 characters.
///
/// Escapes quotes, backslashes, control characters, Unicode format controls and
/// line/paragraph separators, including bidi controls and terminal escapes.
pub(crate) fn quoted(text: &str) -> String {
    let mut output = String::new();
    push_quoted(&mut output, text);
    output
}
