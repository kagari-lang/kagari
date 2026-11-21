//! Semantic projections own symbolic enum handles, never runtime objects or roots.
use kagari_common::identity::{DefinitionKind, DefinitionPath, metadata::DefinitionMetadata};
use kagari_runtime::{
    error::RuntimeError,
    native::{
        assembly::DeclarationSnapshot, binding::NativeResult, catalog::DeclarationCatalog,
        types::TypeRef,
    },
};
use kagari_types::declaration::{TypeDefKind, module::ModuleDecl};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

/// Independently retained declarations and enum handles from one sealed snapshot.
/// StandardDeclarations queries this value, not pending work in the parent Bundle.
/// Handles contain only declaration identity/metadata; allocation still resolves
/// the invoking runtime's generation-pinned layout through its checked call API.
#[derive(Debug)]
pub struct Semantic {
    catalog: DeclarationCatalog,
    shared: Vec<Arc<DefinitionMetadata<ModuleDecl>>>,
    enumerations: BTreeMap<DefinitionPath, TypeRef>,
    // Preserve the existing short-name API without scanning modules on enum calls.
    // Ambiguous names remain valid declarations, but cannot use this convenience index.
    enum_names: BTreeMap<String, Option<DefinitionPath>>,
}

impl Semantic {
    pub(crate) fn new(snapshot: DeclarationSnapshot) -> NativeResult<Self> {
        let mut enumerations = BTreeMap::new();
        let mut enum_names = BTreeMap::new();
        let mut paths = BTreeSet::new();
        for module in snapshot.modules() {
            let definitions = module.definitions();
            let records = module.records();
            for ty in &records.types {
                if ty.kind == TypeDefKind::Enum {
                    let id = records
                        .definition(DefinitionKind::Enum, &ty.name, definitions)
                        .map_err(conflict)?;
                    paths.insert(definitions.resolve(id).map_err(conflict)?.to_path());
                }
            }
            // A facade may expose an authored enum without owning its declaration.
            for id in records.exports.values() {
                let target = definitions.resolve(*id).map_err(conflict)?;
                if target
                    .last()
                    .is_some_and(|segment| segment.kind == DefinitionKind::Enum)
                {
                    paths.insert(target.to_path());
                }
            }
        }
        for path in paths {
            let declaration = snapshot.catalog().type_reference(&path)?;
            enum_names
                .entry(path.path.last().expect("enum path").name.clone())
                .and_modify(|entry| *entry = None)
                .or_insert_with(|| Some(path.clone()));
            enumerations.insert(path, declaration);
        }
        let shared = snapshot.modules().iter().cloned().map(Arc::new).collect();
        Ok(Self {
            catalog: snapshot.catalog().clone(),
            shared,
            enumerations,
            enum_names,
        })
    }

    pub fn catalog(&self) -> &DeclarationCatalog {
        &self.catalog
    }

    pub fn shared(&self) -> &[Arc<DefinitionMetadata<ModuleDecl>>] {
        &self.shared
    }

    /// Resolve an owned enum by canonical identity, without entering any cache.
    pub fn enumeration(&self, path: &DefinitionPath) -> NativeResult<&TypeRef> {
        self.enumerations
            .get(path)
            .ok_or_else(|| conflict("stdlib enum is not declared by this semantic snapshot"))
    }

    /// Resolve an unambiguous short enum name from this immutable snapshot.
    pub fn enumeration_named(&self, name: &str) -> NativeResult<&TypeRef> {
        let path = self
            .enum_names
            .get(name)
            .and_then(Option::as_ref)
            .ok_or_else(|| conflict("unknown or ambiguous standard enum"))?;
        self.enumeration(path)
    }
}

fn conflict(error: impl ToString) -> RuntimeError {
    RuntimeError::metadata_conflict(error.to_string())
}
