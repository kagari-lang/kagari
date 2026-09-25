//! Resolve facade targets once, before declaration/name/signature consumers run.
use super::*;
use crate::resolver::ResolvedName;

impl ModuleGraph {
    pub(super) fn bind_exports(&mut self, cancel: &CancellationToken) -> Result<(), Cancelled> {
        let mut modules = Vec::new();
        for (identity, node) in &self.nodes {
            let mut bindings = HashMap::new();
            for (index, import) in node.imports.entries.iter().enumerate() {
                cancel.check()?;
                let target = match &import.target {
                    Some(ImportTarget::Source(source)) => {
                        self.resolve_export(source.clone(), cancel)?
                    }
                    target => target.clone(),
                };
                let Some(target) = target else {
                    continue;
                };
                if let ImportTarget::Source(source) = &target
                    && source.item.is_none()
                {
                    for items in source.members.values() {
                        cancel.check()?;
                        let [item] = items.as_slice() else {
                            continue;
                        };
                        let mut member = source.clone();
                        member.item = Some(*item);
                        if let Some(target) = self.resolve_export(member, cancel)? {
                            bindings.insert(
                                ResolvedName::SourceItem {
                                    import: index,
                                    item: *item,
                                },
                                target,
                            );
                        }
                    }
                }
                bindings.insert(ResolvedName::SourceImport(index), target);
            }
            modules.push((identity.clone(), bindings));
        }
        for (identity, bindings) in modules {
            cancel.check()?;
            let node = self.nodes.get_mut(&identity).expect("existing module");
            Arc::make_mut(&mut node.imports).bindings = bindings;
        }
        Ok(())
    }
}

impl ModuleImports {
    pub(crate) fn binding(&self, key: ResolvedName) -> Option<&ImportTarget> {
        self.bindings.get(&key)
    }

    pub(crate) fn resolved_name(&self, key: ResolvedName) -> Option<ResolvedName> {
        Some(match self.binding(key)? {
            ImportTarget::Source(_) => key,
            ImportTarget::HostFunction(function) => ResolvedName::HostFunction(*function),
            ImportTarget::HostType(ty) => ResolvedName::HostType(*ty),
            ImportTarget::HostModule(module) => ResolvedName::HostModule(*module),
            ImportTarget::StandardFunction(function) => ResolvedName::StandardFunction(*function),
            ImportTarget::StandardModule(module) => ResolvedName::StandardModule(*module),
        })
    }

    pub(crate) fn resolve_member(
        &self,
        import: usize,
        path: &str,
        hosts: &HostDeclarations,
    ) -> Option<ResolvedName> {
        let ImportTarget::Source(source) = self.binding(ResolvedName::SourceImport(import))? else {
            return None;
        };
        if source.item.is_some() {
            return None;
        }
        let (name, suffix) = path
            .split_once("::")
            .map_or((path, None), |(name, suffix)| (name, Some(suffix)));
        let [item] = source.members.get(name)?.as_slice() else {
            return None;
        };
        let key = ResolvedName::SourceItem {
            import,
            item: *item,
        };
        let resolved = self.resolved_name(key)?;
        match suffix {
            None => Some(resolved),
            Some(member) => match resolved {
                ResolvedName::SourceItem { .. } => {
                    let ImportTarget::Source(next) = self.binding(key)? else {
                        return None;
                    };
                    if next.item.is_some() {
                        return None;
                    }
                    let namespace = *self.namespace_entries.get(&next.module)?;
                    self.resolve_member(namespace, member, hosts)
                }
                ResolvedName::HostModule(module) => hosts.resolve_name_in(module, member),
                ResolvedName::StandardModule(module) => surface::standard_function(module, member)
                    .map(|f| ResolvedName::StandardFunction(f.intrinsic)),
                _ => None,
            },
        }
    }
}
