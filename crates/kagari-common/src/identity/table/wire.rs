//! Canonical identity-only portable records. No process-local table ID is encoded.
use crate::{
    decode_limits::bounded_vec,
    identity::{
        DefinitionKind, DefinitionPath, ModuleIdentity,
        table::{
            DefinitionId, DefinitionIndex, DefinitionRemap, DefinitionTable,
            DefinitionTableBuilder, DefinitionTableError, Node,
        },
    },
};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};

pub const MAX_PORTABLE_IDENTITY_RECORDS: usize = 1_000_000;

fn records<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    bounded_vec(
        deserializer,
        MAX_PORTABLE_IDENTITY_RECORDS,
        "portable identity record",
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PortableDefinitionRef(u32);

impl PortableDefinitionRef {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum PortableNode {
    Root(u32),
    Child {
        parent: PortableDefinitionRef,
        kind: DefinitionKind,
        name: u32,
        occurrence: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortableDefinitionTable {
    #[serde(deserialize_with = "records")]
    modules: Vec<ModuleIdentity>,
    #[serde(deserialize_with = "records")]
    symbols: Vec<String>,
    #[serde(deserialize_with = "records")]
    nodes: Vec<PortableNode>,
}

pub struct DefinitionEncoding {
    pub table: PortableDefinitionTable,
    owner: super::DefinitionTableId,
    references: HashMap<DefinitionIndex, PortableDefinitionRef>,
}

impl DefinitionEncoding {
    pub fn reference(
        &self,
        id: DefinitionId,
    ) -> Result<PortableDefinitionRef, DefinitionTableError> {
        if id.table() != self.owner {
            return Err(DefinitionTableError::ForeignTable);
        }
        self.references
            .get(&id.index)
            .copied()
            .ok_or(DefinitionTableError::UnmappedDefinition)
    }
}

pub struct DefinitionDecoding {
    pub table: DefinitionTable,
    references: Vec<DefinitionId>,
}

impl DefinitionDecoding {
    pub fn resolve(
        &self,
        reference: PortableDefinitionRef,
    ) -> Result<DefinitionId, DefinitionTableError> {
        self.references
            .get(reference.index())
            .copied()
            .ok_or(DefinitionTableError::InvalidIndex)
    }

    pub fn import_into(
        &self,
        target: &mut DefinitionTableBuilder,
    ) -> Result<DefinitionRemap, DefinitionTableError> {
        target.import(&self.table, self.references.iter().copied())
    }
}

impl DefinitionTable {
    /// Encode only requested definitions and ancestors, ordered by exact identity.
    pub fn encode(
        &self,
        ids: impl IntoIterator<Item = DefinitionId>,
    ) -> Result<DefinitionEncoding, DefinitionTableError> {
        let mut required = HashSet::new();
        for id in ids {
            self.resolve(id)?;
            let mut index = id.index;
            loop {
                if !required.insert(index) {
                    break;
                }
                match self.data.nodes[index.0 as usize] {
                    Node::Root(_) => break,
                    Node::Child { parent, .. } => index = parent,
                }
            }
        }
        if required.len() > MAX_PORTABLE_IDENTITY_RECORDS {
            return Err(DefinitionTableError::PortableLimit);
        }
        let mut paths: Vec<(DefinitionPath, DefinitionIndex)> = required
            .into_iter()
            .map(|index| {
                let id = DefinitionId {
                    table: self.id,
                    index,
                };
                (
                    self.resolve(id)
                        .expect("selected validated index")
                        .to_path(),
                    index,
                )
            })
            .collect();
        paths.sort_by(|left, right| left.0.cmp(&right.0));
        let modules: Vec<_> = paths
            .iter()
            .map(|(path, _)| path.module.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let symbols: Vec<_> = paths
            .iter()
            .filter_map(|(path, _)| path.path.last().map(|part| part.name.clone()))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        if modules.len() > MAX_PORTABLE_IDENTITY_RECORDS
            || symbols.len() > MAX_PORTABLE_IDENTITY_RECORDS
        {
            return Err(DefinitionTableError::PortableLimit);
        }
        let mut references = HashMap::new();
        let mut nodes = Vec::with_capacity(paths.len());
        for (path, index) in paths {
            let reference = PortableDefinitionRef(nodes.len() as u32);
            let node = match self.data.nodes[index.0 as usize] {
                Node::Root(_) => PortableNode::Root(
                    modules
                        .binary_search(&path.module)
                        .expect("selected module") as u32,
                ),
                Node::Child {
                    parent,
                    kind,
                    name,
                    occurrence,
                } => PortableNode::Child {
                    parent: *references
                        .get(&parent)
                        .expect("canonical ancestors precede children"),
                    kind,
                    name: symbols
                        .binary_search_by(|symbol| {
                            symbol.as_str().cmp(&self.data.symbols[name.0 as usize])
                        })
                        .expect("selected symbol") as u32,
                    occurrence,
                },
            };
            nodes.push(node);
            references.insert(index, reference);
        }
        Ok(DefinitionEncoding {
            table: PortableDefinitionTable {
                modules,
                symbols,
                nodes,
            },
            owner: self.id,
            references,
        })
    }
}

impl PortableDefinitionTable {
    /// Resolve only after validating all structure; callers still validate ABI semantics.
    pub fn decode(&self) -> Result<DefinitionDecoding, DefinitionTableError> {
        if self.modules.len() > MAX_PORTABLE_IDENTITY_RECORDS
            || self.symbols.len() > MAX_PORTABLE_IDENTITY_RECORDS
            || self.nodes.len() > MAX_PORTABLE_IDENTITY_RECORDS
        {
            return Err(DefinitionTableError::PortableLimit);
        }
        let mut modules = HashSet::new();
        if self
            .modules
            .iter()
            .any(|module| !module.within_path_limit() || !modules.insert(module))
        {
            return Err(DefinitionTableError::InvalidPortableRecord);
        }
        let mut symbols = HashSet::new();
        if self.symbols.iter().any(|symbol| !symbols.insert(symbol)) {
            return Err(DefinitionTableError::InvalidPortableRecord);
        }
        let mut builder = DefinitionTableBuilder::new()?;
        let mut references = Vec::with_capacity(self.nodes.len());
        let mut unique = HashSet::new();
        for node in &self.nodes {
            let id = match node {
                PortableNode::Root(module) => builder.intern_root(
                    self.modules
                        .get(*module as usize)
                        .ok_or(DefinitionTableError::InvalidPortableRecord)?,
                )?,
                PortableNode::Child {
                    parent,
                    kind,
                    name,
                    occurrence,
                } => {
                    let parent = references
                        .get(parent.index())
                        .copied()
                        .ok_or(DefinitionTableError::InvalidPortableRecord)?;
                    let name = self
                        .symbols
                        .get(*name as usize)
                        .ok_or(DefinitionTableError::InvalidPortableRecord)?;
                    builder.intern_child(parent, *kind, name, *occurrence)?
                }
            };
            if !unique.insert(id) {
                return Err(DefinitionTableError::InvalidPortableRecord);
            }
            references.push(id);
        }
        Ok(DefinitionDecoding {
            table: builder.freeze(),
            references,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bincode::Options;

    fn valid_table() -> PortableDefinitionTable {
        PortableDefinitionTable {
            modules: vec![ModuleIdentity::single_file("main")],
            symbols: vec!["Player".into(), "hp".into()],
            nodes: vec![
                PortableNode::Root(0),
                PortableNode::Child {
                    parent: PortableDefinitionRef(0),
                    kind: DefinitionKind::Struct,
                    name: 0,
                    occurrence: 0,
                },
                PortableNode::Child {
                    parent: PortableDefinitionRef(1),
                    kind: DefinitionKind::Field,
                    name: 1,
                    occurrence: 0,
                },
            ],
        }
    }

    #[test]
    fn portable_tables_reject_missing_refs_cycles_and_duplicate_identities() {
        let valid = valid_table();
        assert!(valid.decode().is_ok());
        for invalid in [
            PortableNode::Root(1),
            PortableNode::Child {
                parent: PortableDefinitionRef(2),
                kind: DefinitionKind::Field,
                name: 1,
                occurrence: 0,
            },
            PortableNode::Child {
                parent: PortableDefinitionRef(0),
                kind: DefinitionKind::Field,
                name: 2,
                occurrence: 0,
            },
            PortableNode::Root(0),
        ] {
            let mut changed = valid.clone();
            changed.nodes[2] = invalid;
            assert!(changed.decode().is_err());
        }
        let mut changed = valid.clone();
        changed.symbols.push("hp".into());
        assert!(changed.decode().is_err());
        let mut changed = valid;
        changed.modules.push(changed.modules[0].clone());
        assert!(changed.decode().is_err());
    }

    #[test]
    fn table_length_prefix_is_bounded_before_elements_are_decoded() {
        let codec = bincode::DefaultOptions::new().with_fixint_encoding();
        let mut bytes = codec.serialize(&valid_table()).unwrap();
        bytes[..8].copy_from_slice(&u64::MAX.to_le_bytes());
        let error = codec
            .deserialize::<PortableDefinitionTable>(&bytes)
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("portable identity record count limit exceeded")
        );
    }

    #[test]
    fn decoded_references_are_checked_and_can_be_imported() {
        let decoded = valid_table().decode().unwrap();
        let hp = decoded.resolve(PortableDefinitionRef(2)).unwrap();
        assert_eq!(
            decoded.table.resolve(hp).unwrap().to_path().path[1].name,
            "hp"
        );
        assert_eq!(
            decoded.resolve(PortableDefinitionRef(3)),
            Err(DefinitionTableError::InvalidIndex)
        );
        let mut target = DefinitionTableBuilder::new().unwrap();
        let mapping = decoded.import_into(&mut target).unwrap();
        assert_eq!(
            decoded.table.resolve(hp).unwrap().to_path(),
            target.resolve(mapping.map(hp).unwrap()).unwrap().to_path()
        );
    }
}
