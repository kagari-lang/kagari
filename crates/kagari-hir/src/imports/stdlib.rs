//! The installed package contributes its source namespace and declared prelude.

use crate::{
    hir::item::storage::Visibility,
    imports::{ImportTarget, ModuleImports, ResolvedImport, SourceCatalog},
    lower::LoweredModule,
};
use kagari_common::span::Span;
use std::{borrow::Cow, collections::HashSet};

#[cfg(test)]
mod tests;

impl SourceCatalog<'_> {
    /// Translate the installed package alias without interpreting any declarations.
    pub(super) fn source_path<'p>(&self, path: &'p str) -> Cow<'p, str> {
        if let Some((alias, member)) = path.split_once("::")
            && let Some(packages) = self.package_aliases.get(alias)
            && packages.len() == 1
        {
            let package = packages.first().expect("one installed package alias");
            return Cow::Owned(format!("{package}::{member}"));
        }
        let Some(root) = &self.standard_root else {
            return Cow::Borrowed(path);
        };
        if path == "std" {
            Cow::Owned(root.to_string())
        } else if let Some(member) = path.strip_prefix("std::") {
            Cow::Owned(format!("{}::{member}", root.package.0))
        } else {
            Cow::Borrowed(path)
        }
    }

    pub(super) fn install_standard_prelude(
        &self,
        module: &LoweredModule,
        local_names: &HashSet<&str>,
        imports: &mut ModuleImports,
    ) {
        let Some(root) = &self.standard_root else {
            return;
        };
        let Some([entry]) = self.paths.get(&root.to_string()).map(Vec::as_slice) else {
            return;
        };
        // Ordinary declarations, explicit imports and glob bindings shadow the prelude.
        let mut insert = |alias: &str, target| {
            if local_names.contains(alias)
                || imports
                    .entries
                    .iter()
                    .any(|entry| !entry.glob_root && entry.alias == alias)
            {
                return;
            }
            imports.entries.push(ResolvedImport {
                alias: alias.into(),
                span: Span::new(0, 0),
                target: Some(ImportTarget::Source(target)),
                glob_root: false,
                implicit_module: None,
                visibility: Visibility::Private,
                internal_namespace: false,
            });
        };
        insert("std", entry.target(None, module.source.module_identity()));
        let prelude_path = format!("{}::prelude", root.package.0);
        let Some([prelude]) = self.paths.get(&prelude_path).map(Vec::as_slice) else {
            return;
        };
        let prelude = prelude.target(None, module.source.module_identity());
        for (name, items) in prelude.members.iter() {
            let [item] = items.as_slice() else {
                continue;
            };
            let mut target = prelude.clone();
            target.item = Some(*item);
            insert(name, target);
        }
    }
}
