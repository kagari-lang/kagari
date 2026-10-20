//! Immutable proof resources and dependency-visible views of one program.
use crate::{
    layout::EnumLayout,
    types::proofs::{ProofCatalog, implementation::Implementation},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::table::{DefinitionId, DefinitionTable},
};
use kagari_types::{
    declaration::{NativeDeclaration, TraitDef},
    ty::substitution::TypeTransformError,
};
use std::{collections::BTreeMap, sync::Arc};

/// Records physically carried by one executable module, before linked queries.
///
/// The position in the program's input vector is its visibility index. A carried
/// contract can occur in multiple modules; its declaration's nominal owner alone
/// does not determine where that contract is available.
pub struct ProofModule<'a> {
    pub implementations: Vec<Implementation<'a>>,
    pub enumerations: Vec<&'a EnumLayout>,
    pub contracts: Vec<(DefinitionId, &'a TraitDef)>,
    pub native_declarations: Vec<&'a NativeDeclaration>,
}

/// Structurally checked resources prepared once for an immutable module graph.
///
/// This is not a linked proof catalog or executable seal. Each query view must
/// select the calling module's exact dependency closure; view admission checks
/// the obligations dependent on that visibility before exposing proof operations.
pub struct ProgramProofs<'a> {
    catalog: ProofCatalog<'a>,
    modules: usize,
}

impl<'a> ProgramProofs<'a> {
    pub fn new(
        modules: Vec<ProofModule<'a>>,
        definitions: &DefinitionTable,
        cancel: &CancellationToken,
    ) -> Result<Self, TypeTransformError> {
        let count = modules.len();
        let catalog = ProofCatalog::prepare(modules, definitions, cancel)?;
        Ok(Self {
            catalog,
            modules: count,
        })
    }

    /// Select exactly the supplied module slots, without rebuilding resource maps.
    pub fn view(
        &self,
        modules: impl IntoIterator<Item = usize>,
        cancel: &CancellationToken,
    ) -> Result<ProofCatalog<'a>, TypeTransformError> {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        let mut visible = vec![false; self.modules];
        for module in modules {
            cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
            *visible
                .get_mut(module)
                .ok_or(TypeTransformError::InvalidContract)? = true;
        }
        let scope: Arc<[bool]> = visible.into();
        let catalog = ProofCatalog {
            definitions: self.catalog.definitions.clone(),
            implementations: self.catalog.implementations.scoped(scope.clone()),
            enumerations: self.catalog.enumerations.scoped(scope.clone()),
            contracts: self.catalog.contracts.scoped(scope.clone()),
            native_declarations: self.catalog.native_declarations.scoped(scope),
        };
        catalog.validate_linked(cancel)?;
        Ok(catalog)
    }
}

pub(super) struct Record<T> {
    pub value: T,
    pub modules: Vec<usize>,
}

impl<T> Record<T> {
    pub fn new(value: T, module: usize) -> Self {
        Self {
            value,
            modules: vec![module],
        }
    }

    fn visible(&self, scope: Option<&[bool]>) -> bool {
        scope.is_none_or(|scope| {
            self.modules
                .iter()
                .any(|module| scope.get(*module) == Some(&true))
        })
    }
}

pub(super) struct ScopedRecords<T> {
    records: Arc<[Record<T>]>,
    scope: Option<Arc<[bool]>>,
}

impl<T> ScopedRecords<T> {
    pub fn new(records: Vec<Record<T>>) -> Self {
        Self {
            records: records.into(),
            scope: None,
        }
    }

    pub fn scoped(&self, scope: Arc<[bool]>) -> Self {
        Self {
            records: self.records.clone(),
            scope: Some(scope),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.records
            .iter()
            .filter(|record| record.visible(self.scope.as_deref()))
            .map(|record| &record.value)
    }
}

pub(super) struct ScopedMap<K, V> {
    records: Arc<BTreeMap<K, Record<V>>>,
    scope: Option<Arc<[bool]>>,
}

impl<K: Ord, V> ScopedMap<K, V> {
    pub fn new(records: BTreeMap<K, Record<V>>) -> Self {
        Self {
            records: Arc::new(records),
            scope: None,
        }
    }

    pub fn scoped(&self, scope: Arc<[bool]>) -> Self {
        Self {
            records: self.records.clone(),
            scope: Some(scope),
        }
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        self.records
            .get(key)
            .filter(|record| record.visible(self.scope.as_deref()))
            .map(|record| &record.value)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.records
            .iter()
            .filter(|(_, record)| record.visible(self.scope.as_deref()))
            .map(|(key, record)| (key, &record.value))
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.iter().map(|(_, value)| value)
    }
}
