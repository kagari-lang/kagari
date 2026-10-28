//! Context-owned, append-only declaration identity interning.
//!
//! Table numbers never enter portable data. A short ID is meaningful only with
//! its owning table; exact paths are imported explicitly between contexts.
pub mod module;
#[cfg(test)]
mod tests;
pub mod wire;

use crate::identity::{
    DefinitionKind, DefinitionPath, DefinitionPathSegment, MAX_IDENTITY_PATH_SEGMENTS,
    ModuleIdentity,
};
use std::{
    cmp::Ordering as Comparison,
    collections::HashMap,
    num::NonZeroU32,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
};
use thiserror::Error;

static NEXT_TABLE: AtomicU32 = AtomicU32::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DefinitionTableId(NonZeroU32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DefinitionIndex(u32);

/// Compact declaration identity, meaningful only in its explicitly owned table.
/// Process-local handles cannot be serialized without an explicit table codec.
///
/// ```compile_fail
/// use kagari_common::identity::table::DefinitionId;
/// fn serialize_handle(id: DefinitionId) {
///     let _ = bincode::serialize(&id);
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DefinitionId {
    table: DefinitionTableId,
    index: DefinitionIndex,
}

impl DefinitionId {
    pub fn table(self) -> DefinitionTableId {
        self.table
    }

    pub fn index(self) -> usize {
        self.index.0 as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Symbol(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Node {
    Root(u32),
    Child {
        parent: DefinitionIndex,
        kind: DefinitionKind,
        name: Symbol,
        occurrence: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DefinitionTableError {
    #[error("definition table identity exhausted")]
    TableIdentityExhausted,
    #[error("definition table index exhausted")]
    IndexExhausted,
    #[error("definition belongs to a different identity table")]
    ForeignTable,
    #[error("definition index is outside its owning table")]
    InvalidIndex,
    #[error("definition is not a module root")]
    InvalidModuleRoot,
    #[error("definition identity path limit exceeded")]
    PathLimit,
    #[error("definition was not included in the import mapping")]
    UnmappedDefinition,
    #[error("invalid or duplicate portable identity record")]
    InvalidPortableRecord,
    #[error("portable identity table exceeds its record limit")]
    PortableLimit,
}

#[derive(Debug, Clone, Default)]
struct TableData {
    modules: Vec<Arc<ModuleIdentity>>,
    module_indices: HashMap<Arc<ModuleIdentity>, u32>,
    package_roots: HashMap<Symbol, DefinitionIndex>,
    module_children: HashMap<(DefinitionIndex, Symbol), DefinitionIndex>,
    symbols: Vec<Arc<str>>,
    symbol_indices: HashMap<Arc<str>, Symbol>,
    nodes: Vec<Node>,
    node_indices: HashMap<Node, DefinitionIndex>,
    depths: Vec<u8>,
}

/// An immutable retained prefix of an identity context.
#[derive(Debug, Clone)]
pub struct DefinitionTable {
    id: DefinitionTableId,
    data: Arc<TableData>,
}

impl DefinitionTable {
    /// Compare exact portable identities, independently of allocation order.
    /// Both IDs must belong to this retained table prefix.
    pub fn compare(
        &self,
        left: DefinitionId,
        right: DefinitionId,
    ) -> Result<Comparison, DefinitionTableError> {
        let left = self.resolve(left)?;
        let right = self.resolve(right)?;
        Ok(left
            .module()
            .cmp(right.module())
            .then_with(|| left.segments().cmp(right.segments())))
    }

    /// Whether `owner` is `member` or one of its definition ancestors.
    /// Rejects foreign/absent IDs before inspecting the parent chain.
    pub fn contains(
        &self,
        owner: DefinitionId,
        member: DefinitionId,
    ) -> Result<bool, DefinitionTableError> {
        self.resolve(owner)?;
        self.resolve(member)?;
        let mut current = Some(member);
        while let Some(id) = current {
            if id == owner {
                return Ok(true);
            }
            current = self.parent(id)?;
        }
        Ok(false)
    }

    pub fn parent(&self, id: DefinitionId) -> Result<Option<DefinitionId>, DefinitionTableError> {
        validate_id(self.id, &self.data, id)?;
        Ok(match self.data.nodes[id.index()] {
            Node::Root(_) => None,
            Node::Child { parent, .. } => Some(DefinitionId {
                table: self.id,
                index: parent,
            }),
        })
    }

    pub fn lookup_child(
        &self,
        parent: DefinitionId,
        kind: DefinitionKind,
        name: &str,
        occurrence: u32,
    ) -> Option<DefinitionId> {
        validate_id(self.id, &self.data, parent).ok()?;
        let name = *self.data.symbol_indices.get(name)?;
        let index = *self.data.node_indices.get(&Node::Child {
            parent: parent.index,
            kind,
            name,
            occurrence,
        })?;
        Some(DefinitionId {
            table: self.id,
            index,
        })
    }

    pub fn id(&self) -> DefinitionTableId {
        self.id
    }

    pub fn len(&self) -> usize {
        self.data.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.nodes.is_empty()
    }

    pub fn symbol_count(&self) -> usize {
        self.data.symbols.len()
    }

    pub fn resolve(&self, id: DefinitionId) -> Result<DefinitionView<'_>, DefinitionTableError> {
        validate_id(self.id, &self.data, id)?;
        Ok(DefinitionView {
            data: &self.data,
            index: id.index,
        })
    }

    /// Find the interned root for an exact module identity without building a path.
    pub fn lookup_root(&self, module: &ModuleIdentity) -> Option<DefinitionId> {
        let module = *self.data.module_indices.get(module)?;
        let index = *self.data.node_indices.get(&Node::Root(module))?;
        Some(DefinitionId {
            table: self.id,
            index,
        })
    }

    pub fn lookup(&self, path: &DefinitionPath) -> Option<DefinitionId> {
        self.lookup_components(
            &path.module.package.0,
            path.module.path.iter().map(String::as_str),
            path.path.iter().map(|segment| DefinitionSegment {
                kind: segment.kind,
                name: &segment.name,
                occurrence: segment.occurrence,
            }),
        )
    }

    /// Find an exact symbolic identity from borrowed components without allocation.
    ///
    /// The result belongs to this table and retained snapshot only. Package, full
    /// module path, and each declaration's kind, name and occurrence must match.
    /// An interned identity (including a module prefix) does not imply that its
    /// declaration is installed; consumers must also check their declaration store.
    pub fn lookup_components<'a>(
        &self,
        package: &str,
        module: impl IntoIterator<Item = &'a str>,
        path: impl IntoIterator<Item = DefinitionSegment<'a>>,
    ) -> Option<DefinitionId> {
        let mut index = self
            .lookup_module_components(package, module)?
            .definition()
            .index;
        for segment in path {
            let name = *self.data.symbol_indices.get(segment.name)?;
            index = *self.data.node_indices.get(&Node::Child {
                parent: index,
                kind: segment.kind,
                name,
                occurrence: segment.occurrence,
            })?;
        }
        Some(DefinitionId {
            table: self.id,
            index,
        })
    }
}

/// Publication uses copy-on-write snapshots; published indices never change.
#[derive(Debug)]
pub struct DefinitionTableBuilder {
    id: DefinitionTableId,
    data: Arc<TableData>,
}

impl DefinitionTableBuilder {
    pub fn new() -> Result<Self, DefinitionTableError> {
        let id = NEXT_TABLE
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .map_err(|_| DefinitionTableError::TableIdentityExhausted)?;
        Ok(Self {
            id: DefinitionTableId(
                NonZeroU32::new(id).ok_or(DefinitionTableError::TableIdentityExhausted)?,
            ),
            data: Arc::new(TableData::default()),
        })
    }

    pub fn freeze(&self) -> DefinitionTable {
        DefinitionTable {
            id: self.id,
            data: self.data.clone(),
        }
    }

    pub fn resolve(&self, id: DefinitionId) -> Result<DefinitionView<'_>, DefinitionTableError> {
        validate_id(self.id, &self.data, id)?;
        Ok(DefinitionView {
            data: &self.data,
            index: id.index,
        })
    }

    pub fn intern_root(
        &mut self,
        module: &ModuleIdentity,
    ) -> Result<DefinitionId, DefinitionTableError> {
        self.intern_module(module).map(|module| module.definition())
    }

    fn intern_root_record(
        &mut self,
        module: &ModuleIdentity,
    ) -> Result<DefinitionId, DefinitionTableError> {
        if !module.within_path_limit() {
            return Err(DefinitionTableError::PathLimit);
        }
        let module_index = if let Some(index) = self.data.module_indices.get(module) {
            *index
        } else {
            let index = checked_index(self.data.modules.len())?;
            let data = Arc::make_mut(&mut self.data);
            let module = Arc::new(module.clone());
            data.modules.push(module.clone());
            data.module_indices.insert(module, index);
            index
        };
        self.intern_node(Node::Root(module_index), 0)
    }

    pub fn intern_child(
        &mut self,
        parent: DefinitionId,
        kind: DefinitionKind,
        name: &str,
        occurrence: u32,
    ) -> Result<DefinitionId, DefinitionTableError> {
        validate_id(self.id, &self.data, parent)?;
        let depth = usize::from(self.data.depths[parent.index()]) + 1;
        if depth > MAX_IDENTITY_PATH_SEGMENTS {
            return Err(DefinitionTableError::PathLimit);
        }
        let name = self.intern_symbol(name)?;
        self.intern_node(
            Node::Child {
                parent: parent.index,
                kind,
                name,
                occurrence,
            },
            depth as u8,
        )
    }

    pub fn intern_path(
        &mut self,
        path: &DefinitionPath,
    ) -> Result<DefinitionId, DefinitionTableError> {
        if !path.within_path_limit() {
            return Err(DefinitionTableError::PathLimit);
        }
        let mut id = self.intern_root(&path.module)?;
        for segment in &path.path {
            id = self.intern_child(id, segment.kind, &segment.name, segment.occurrence)?;
        }
        Ok(id)
    }

    /// Import the referenced ancestor closure once; absent definitions stay unmapped.
    pub fn import(
        &mut self,
        source: &DefinitionTable,
        ids: impl IntoIterator<Item = DefinitionId>,
    ) -> Result<DefinitionRemap, DefinitionTableError> {
        let mut mapping = DefinitionRemap {
            source: source.id,
            target: self.id,
            entries: vec![None; source.len()],
        };
        for id in ids {
            source.resolve(id)?;
            let mut pending = Vec::new();
            let mut index = id.index;
            while mapping.entries[index.0 as usize].is_none() {
                pending.push(index);
                match source.data.nodes[index.0 as usize] {
                    Node::Root(_) => break,
                    Node::Child { parent, .. } => index = parent,
                }
            }
            for index in pending.into_iter().rev() {
                let mapped = if source.id == self.id {
                    let id = DefinitionId {
                        table: self.id,
                        index,
                    };
                    validate_id(self.id, &self.data, id)?;
                    id
                } else {
                    match source.data.nodes[index.0 as usize] {
                        Node::Root(module) => {
                            self.intern_root(&source.data.modules[module as usize])?
                        }
                        Node::Child {
                            parent,
                            kind,
                            name,
                            occurrence,
                        } => {
                            let parent = mapping.entries[parent.0 as usize]
                                .ok_or(DefinitionTableError::UnmappedDefinition)?;
                            self.intern_child(
                                parent,
                                kind,
                                &source.data.symbols[name.0 as usize],
                                occurrence,
                            )?
                        }
                    }
                };
                mapping.entries[index.0 as usize] = Some(mapped);
            }
        }
        Ok(mapping)
    }

    fn intern_node(&mut self, node: Node, depth: u8) -> Result<DefinitionId, DefinitionTableError> {
        let index = if let Some(index) = self.data.node_indices.get(&node) {
            *index
        } else {
            if self.data.nodes.len() >= wire::MAX_PORTABLE_IDENTITY_RECORDS {
                return Err(DefinitionTableError::PortableLimit);
            }
            let index = DefinitionIndex(checked_index(self.data.nodes.len())?);
            let data = Arc::make_mut(&mut self.data);
            data.nodes.push(node);
            data.depths.push(depth);
            data.node_indices.insert(node, index);
            index
        };
        Ok(DefinitionId {
            table: self.id,
            index,
        })
    }
}

#[derive(Debug)]
pub struct DefinitionRemap {
    source: DefinitionTableId,
    target: DefinitionTableId,
    entries: Vec<Option<DefinitionId>>,
}

impl DefinitionRemap {
    pub fn target(&self) -> DefinitionTableId {
        self.target
    }

    pub fn map(&self, id: DefinitionId) -> Result<DefinitionId, DefinitionTableError> {
        if id.table != self.source {
            return Err(DefinitionTableError::ForeignTable);
        }
        self.entries
            .get(id.index())
            .ok_or(DefinitionTableError::InvalidIndex)?
            .ok_or(DefinitionTableError::UnmappedDefinition)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DefinitionSegment<'a> {
    pub kind: DefinitionKind,
    pub name: &'a str,
    pub occurrence: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct DefinitionView<'a> {
    data: &'a TableData,
    index: DefinitionIndex,
}

impl<'a> DefinitionView<'a> {
    /// Borrow the final definition segment; a module root has no segment.
    pub fn last(self) -> Option<DefinitionSegment<'a>> {
        match self.data.nodes[self.index.0 as usize] {
            Node::Root(_) => None,
            Node::Child {
                kind,
                name,
                occurrence,
                ..
            } => Some(DefinitionSegment {
                kind,
                name: &self.data.symbols[name.0 as usize],
                occurrence,
            }),
        }
    }

    pub fn module(self) -> &'a ModuleIdentity {
        let mut index = self.index;
        loop {
            match self.data.nodes[index.0 as usize] {
                Node::Root(module) => return &self.data.modules[module as usize],
                Node::Child { parent, .. } => index = parent,
            }
        }
    }

    pub fn segments(self) -> DefinitionSegments<'a> {
        let mut indices = [DefinitionIndex(0); MAX_IDENTITY_PATH_SEGMENTS];
        let mut count = 0;
        let mut index = self.index;
        while let Node::Child { parent, .. } = self.data.nodes[index.0 as usize] {
            indices[count] = index;
            count += 1;
            index = parent;
        }
        DefinitionSegments {
            data: self.data,
            indices,
            remaining: count,
        }
    }

    pub fn to_path(self) -> DefinitionPath {
        DefinitionPath {
            module: self.module().clone(),
            path: self
                .segments()
                .map(|segment| DefinitionPathSegment {
                    kind: segment.kind,
                    name: segment.name.to_owned(),
                    occurrence: segment.occurrence,
                })
                .collect(),
        }
    }
}

pub struct DefinitionSegments<'a> {
    data: &'a TableData,
    indices: [DefinitionIndex; MAX_IDENTITY_PATH_SEGMENTS],
    remaining: usize,
}

impl<'a> Iterator for DefinitionSegments<'a> {
    type Item = DefinitionSegment<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.remaining = self.remaining.checked_sub(1)?;
        let Node::Child {
            kind,
            name,
            occurrence,
            ..
        } = self.data.nodes[self.indices[self.remaining].0 as usize]
        else {
            unreachable!("validated child path");
        };
        Some(DefinitionSegment {
            kind,
            name: &self.data.symbols[name.0 as usize],
            occurrence,
        })
    }
}

fn validate_id(
    table: DefinitionTableId,
    data: &TableData,
    id: DefinitionId,
) -> Result<(), DefinitionTableError> {
    if id.table != table {
        return Err(DefinitionTableError::ForeignTable);
    }
    if id.index() >= data.nodes.len() {
        return Err(DefinitionTableError::InvalidIndex);
    }
    Ok(())
}

fn checked_index(index: usize) -> Result<u32, DefinitionTableError> {
    u32::try_from(index).map_err(|_| DefinitionTableError::IndexExhausted)
}
