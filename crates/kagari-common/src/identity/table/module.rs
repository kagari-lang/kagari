//! Scoped module roots and borrowed path traversal in the definition identity table.
use crate::identity::{
    ModuleIdentity, PackageId,
    table::{
        DefinitionId, DefinitionRemap, DefinitionTable, DefinitionTableBuilder,
        DefinitionTableError, Node, Symbol, checked_index, validate_id,
    },
};
use std::sync::Arc;

/// A checked module root in one definition context, not a source or runtime version.
///
/// For `game::combat::damage`, the owner stores the portable identity and this
/// value holds only its table/node ID. Retain the owner's table to resolve it.
/// Interning does not declare a module or grant visibility. Handles cannot be
/// serialized directly; export exact identities through the table boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModuleHandle(DefinitionId);

impl ModuleHandle {
    /// Returns the module's root definition ID in the same context.
    pub fn definition(self) -> DefinitionId {
        self.0
    }
}

impl DefinitionTable {
    /// Check that an existing ID is a root in this retained prefix.
    pub fn module_handle(&self, id: DefinitionId) -> Result<ModuleHandle, DefinitionTableError> {
        validate_id(self.id, &self.data, id)?;
        match self.data.nodes[id.index()] {
            Node::Root(_) => Ok(ModuleHandle(id)),
            Node::Child { .. } => Err(DefinitionTableError::InvalidModuleRoot),
        }
    }

    /// Borrow the exact identity, rejecting foreign or later-snapshot handles.
    pub fn module_identity(
        &self,
        module: ModuleHandle,
    ) -> Result<&ModuleIdentity, DefinitionTableError> {
        let module = self.module_handle(module.0)?;
        let Node::Root(index) = self.data.nodes[module.0.index()] else {
            unreachable!("checked root")
        };
        Ok(&self.data.modules[index as usize])
    }

    /// Look up an exact portable input without allocating or extending this table.
    pub fn lookup_module(&self, module: &ModuleIdentity) -> Option<ModuleHandle> {
        self.lookup_module_path(&module.package, &module.path)
    }

    /// Walk borrowed package/path components through interned module roots.
    /// An interned prefix is not evidence of an actual module declaration.
    pub fn lookup_module_path(&self, package: &PackageId, path: &[String]) -> Option<ModuleHandle> {
        let package = self.data.symbol_indices.get(package.0.as_str())?;
        let mut index = *self.data.package_roots.get(package)?;
        for name in path {
            let name = self.data.symbol_indices.get(name.as_str())?;
            index = *self.data.module_children.get(&(index, *name))?;
        }
        Some(ModuleHandle(DefinitionId {
            table: self.id,
            index,
        }))
    }

    /// Look up a module-path child, independently of declaration-node children.
    pub fn module_child(&self, parent: ModuleHandle, name: &str) -> Option<ModuleHandle> {
        self.module_handle(parent.0).ok()?;
        let name = self.data.symbol_indices.get(name)?;
        let index = *self.data.module_children.get(&(parent.0.index, *name))?;
        Some(ModuleHandle(DefinitionId {
            table: self.id,
            index,
        }))
    }

    /// Return the lexical module parent; a package root has none.
    pub fn module_parent(
        &self,
        module: ModuleHandle,
    ) -> Result<Option<ModuleHandle>, DefinitionTableError> {
        let identity = self.module_identity(module)?;
        Ok(identity.path.len().checked_sub(1).and_then(|length| {
            self.lookup_module_path(&identity.package, &identity.path[..length])
        }))
    }

    /// Find the owning module root of a checked declaration in this table.
    pub fn module_of(&self, id: DefinitionId) -> Result<ModuleHandle, DefinitionTableError> {
        validate_id(self.id, &self.data, id)?;
        let mut root = id;
        while let Node::Child { parent, .. } = self.data.nodes[root.index()] {
            root.index = parent;
        }
        Ok(ModuleHandle(root))
    }
}

impl DefinitionTableBuilder {
    /// Intern a module and its lexical prefixes in this identity context.
    /// Checks path length before mutation; prefixes do not publish declarations.
    pub fn intern_module(
        &mut self,
        module: &ModuleIdentity,
    ) -> Result<ModuleHandle, DefinitionTableError> {
        if !module.within_path_limit() {
            return Err(DefinitionTableError::PathLimit);
        }
        if let Some(index) = self
            .data
            .module_indices
            .get(module)
            .and_then(|index| self.data.node_indices.get(&Node::Root(*index)))
        {
            return Ok(ModuleHandle(DefinitionId {
                table: self.id,
                index: *index,
            }));
        }
        let mut prefix = ModuleIdentity {
            package: module.package.clone(),
            path: Vec::new(),
        };
        let mut root = self.intern_root_record(&prefix)?;
        let package = self.intern_symbol(&module.package.0)?;
        Arc::make_mut(&mut self.data)
            .package_roots
            .insert(package, root.index);
        for name in &module.path {
            prefix.path.push(name.clone());
            let child = self.intern_root_record(&prefix)?;
            let symbol = self.intern_symbol(name)?;
            Arc::make_mut(&mut self.data)
                .module_children
                .insert((root.index, symbol), child.index);
            root = child;
        }
        Ok(ModuleHandle(root))
    }

    pub(super) fn intern_symbol(&mut self, name: &str) -> Result<Symbol, DefinitionTableError> {
        if let Some(symbol) = self.data.symbol_indices.get(name) {
            return Ok(*symbol);
        }
        let symbol = Symbol(checked_index(self.data.symbols.len())?);
        let name: Arc<str> = Arc::from(name);
        let data = Arc::make_mut(&mut self.data);
        data.symbols.push(name.clone());
        data.symbol_indices.insert(name, symbol);
        Ok(symbol)
    }
}

impl DefinitionRemap {
    /// Remap a module root through an explicit, root-preserving table import.
    pub fn map_module(&self, module: ModuleHandle) -> Result<ModuleHandle, DefinitionTableError> {
        self.map(module.0).map(ModuleHandle)
    }
}
