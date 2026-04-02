use std::collections::HashMap;

use kagari_common::Diagnostic;
use kagari_syntax::ast::{self, Item};

#[derive(Debug, Clone, Default)]
pub struct NameTable {
    functions: HashMap<String, usize>,
}

impl NameTable {
    pub fn contains_function(&self, name: &str) -> bool {
        self.functions.contains_key(name)
    }
}

pub fn resolve_names(module: &ast::Module) -> Result<NameTable, Vec<Diagnostic>> {
    let mut names = NameTable::default();
    let mut diagnostics = Vec::new();

    for (index, item) in module.items.iter().enumerate() {
        let Item::Function(function) = item;
        if names
            .functions
            .insert(function.name.clone(), index)
            .is_some()
        {
            diagnostics.push(Diagnostic::error(format!(
                "duplicate function `{}`",
                function.name
            )));
        }
    }

    if diagnostics.is_empty() {
        Ok(names)
    } else {
        Err(diagnostics)
    }
}
