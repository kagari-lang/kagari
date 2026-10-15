//! Declaration queries do not depend on the runtime or invoke host callbacks.

use crate::{
    resolver::resolved::ResolvedName,
    types::{NominalType, TypeId},
};

use callable::{HostCallable, HostSignature};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        map::DefinitionContext,
        mapping::DefinitionMappingError,
        metadata::DefinitionMetadata,
        table::{DefinitionId, DefinitionTable},
    },
};

use kagari_types::{
    declaration::names::NameNamespace,
    host_interface::{
        HostFunctionDeclaration, HostInterface, HostInterfaceError,
        type_declaration::HostTypeDeclaration, value_type::HostValueType,
    },
    scalar::BuiltinType,
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

mod adoption;
pub mod callable;

#[cfg(test)]
mod tests;

/// Scoped to one immutable declaration input, never a runtime binding slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HostFunctionId {
    revision: u64,
    index: usize,
}

/// A module-prefix index qualified by one immutable host declaration revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HostModuleId {
    revision: u64,
    index: usize,
}

/// A host type index qualified by one immutable declaration revision, not a runtime object handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HostTypeId {
    revision: u64,
    index: usize,
}

/// Immutable offline host declarations, signature projections and lookup indexes.
///
/// [`Self::new`] validates the supplied interface, allocates a revision and
/// builds qualified path/member indexes. IDs contain that revision plus a slot;
/// function/type access returns `None` for a foreign revision or invalid slot.
///
/// ```text
/// DefinitionMetadata<HostInterface> -> HostDeclarations::new
/// +-- paths / methods -> HostFunctionId { revision, index }
/// |   `-- function(id) -> interface.functions[index]
/// +-- type_paths / type_identities -> HostTypeId -> interface.types[index]
/// +-- signatures[index] -> semantic parameter/result types
/// `-- modules[index] -> qualified namespace prefix
/// ```
///
/// These records describe callable/type/access contracts without invoking callbacks
/// or owning runtime Rust state. [`crate::host::callable::HostCallable`] presents
/// their semantic signatures to the ordinary call checker.
#[derive(Debug)]
pub struct HostDeclarations {
    revision: u64,
    interface: HostInterface,
    definitions: DefinitionTable,
    signatures: Vec<HostSignature>,
    paths: HashMap<String, HostFunctionId>,
    methods: HashMap<(DefinitionId, String), HostFunctionId>,
    type_paths: HashMap<String, HostTypeId>,
    type_identities: HashMap<DefinitionId, HostTypeId>,
    modules: Vec<String>,
}

impl HostDeclarations {
    /// Validates an offline interface and builds immutable revision-scoped lookup tables.
    ///
    /// # Errors
    ///
    /// Rejects invalid interfaces, conflicting declarations and namespace
    /// collisions. Construction does not execute host code.
    pub fn new(
        interface: DefinitionMetadata<HostInterface>,
    ) -> Result<Arc<Self>, HostInterfaceError> {
        let (definitions, mut interface) = interface.into_parts();
        interface.validate(&definitions)?;
        let mut present = interface
            .functions
            .iter()
            .map(|f| f.id)
            .collect::<HashSet<_>>();
        for owner in &interface.types {
            for method in &owner.methods {
                if present.insert(method.id) {
                    interface
                        .functions
                        .push(owner.method_contract(&method.id, &definitions)?);
                }
            }
        }
        interface.validate(&definitions)?;
        if interface
            .functions
            .iter()
            .map(|function| &function.symbol)
            .chain(interface.types.iter().map(|ty| &ty.symbol))
            .any(|symbol| {
                symbol.split('.').any(|segment| {
                    let mut chars = segment.chars();
                    !chars
                        .next()
                        .is_some_and(|ch| ch == '_' || ch.is_alphabetic())
                        || !chars.all(|ch| ch == '_' || ch.is_alphanumeric())
                }) || symbol.split('.').next() == Some("std")
            })
        {
            return Err(HostInterfaceError::InvalidDeclaration);
        }
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let revision = NEXT
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .expect("host declaration revision exhausted");
        let paths: HashMap<_, _> = interface
            .functions
            .iter()
            .enumerate()
            .filter(|(_, declaration)| declaration.method_owner(&definitions).is_none())
            .map(|(index, declaration)| {
                (
                    declaration.symbol.replace('.', "::"),
                    HostFunctionId { revision, index },
                )
            })
            .collect();
        let methods = interface
            .functions
            .iter()
            .enumerate()
            .filter_map(|(index, declaration)| {
                Some((
                    (
                        declaration.method_owner(&definitions)?,
                        definitions
                            .resolve(declaration.id)
                            .ok()?
                            .last()?
                            .name
                            .to_owned(),
                    ),
                    HostFunctionId { revision, index },
                ))
            })
            .collect();
        let mut modules = BTreeSet::new();
        let type_paths: HashMap<_, _> = interface
            .types
            .iter()
            .enumerate()
            .map(|(index, declaration)| {
                (
                    declaration.symbol.replace('.', "::"),
                    HostTypeId { revision, index },
                )
            })
            .collect();
        let type_identities = interface
            .types
            .iter()
            .enumerate()
            .map(|(index, declaration)| (declaration.id, HostTypeId { revision, index }))
            .collect();
        for symbol in interface
            .functions
            .iter()
            .filter(|function| function.method_owner(&definitions).is_none())
            .map(|function| &function.symbol)
            .chain(interface.types.iter().map(|ty| &ty.symbol))
        {
            let path = symbol.replace('.', "::");
            for (offset, _) in path.match_indices("::") {
                modules.insert(path[..offset].to_owned());
            }
        }
        if modules.iter().any(|module| type_paths.contains_key(module)) {
            return Err(HostInterfaceError::DuplicateDeclaration);
        }
        Ok(Arc::new(Self {
            revision,
            signatures: interface.functions.iter().map(HostSignature::new).collect(),
            interface,
            definitions,
            paths,
            methods,
            type_paths,
            type_identities,
            modules: modules.into_iter().collect(),
        }))
    }

    /// Returns a shared immutable empty declaration universe with a stable revision.
    /// Fails if the identity scope cannot be allocated; no Host contract is published on failure.
    pub fn empty() -> Result<Arc<Self>, DefinitionMappingError> {
        static EMPTY: OnceLock<Result<Arc<HostDeclarations>, DefinitionMappingError>> =
            OnceLock::new();
        EMPTY
            .get_or_init(|| {
                let context = DefinitionContext::new()?;
                let interface = DefinitionMetadata::checked(
                    context.snapshot(),
                    HostInterface::default(),
                    &CancellationToken::default(),
                )?;
                Self::new(interface).map_err(|_| DefinitionMappingError::InvalidContract)
            })
            .clone()
    }

    /// Finds a free callable by its normalized `::` path; absent paths return `None`.
    pub fn resolve(&self, path: &str) -> Option<HostFunctionId> {
        self.paths.get(path).copied()
    }

    /// Finds a host method by canonical owner identity and unqualified member name.
    pub fn method(&self, owner: &DefinitionId, name: &str) -> Option<HostFunctionId> {
        self.methods.get(&(*owner, name.to_owned())).copied()
    }

    /// Finds a namespace prefix and returns an ID qualified by this revision.
    pub fn module(&self, path: &str) -> Option<HostModuleId> {
        self.modules
            .binary_search_by(|candidate| candidate.as_str().cmp(path))
            .ok()
            .map(|index| HostModuleId {
                revision: self.revision,
                index,
            })
    }

    pub(crate) fn members_of_module(&self, module: HostModuleId) -> Vec<(String, ResolvedName)> {
        if module.revision != self.revision {
            return Vec::new();
        }
        let Some(path) = self.modules.get(module.index) else {
            return Vec::new();
        };
        let prefix = format!("{path}::");
        let mut members = BTreeMap::new();
        for name in self
            .paths
            .keys()
            .chain(self.type_paths.keys())
            .chain(self.modules.iter())
        {
            if let Some(member) = name.strip_prefix(&prefix)
                && !member.contains("::")
            {
                for namespace in NameNamespace::ALL {
                    if let Some(resolved) = self.resolve_name(name, namespace) {
                        members.insert((member.to_owned(), namespace), resolved);
                    }
                }
            }
        }
        members
            .into_iter()
            .map(|((name, _), target)| (name, target))
            .collect()
    }

    /// Borrows a function declaration only if revision and index belong to this input.
    pub fn function(&self, id: HostFunctionId) -> Option<&HostFunctionDeclaration> {
        (id.revision == self.revision)
            .then(|| self.interface.functions.get(id.index))
            .flatten()
    }

    /// Borrows a function's original contract and precomputed semantic signature; rejects foreign/invalid IDs.
    pub fn callable(&self, id: HostFunctionId) -> Option<HostCallable<'_>> {
        let declaration = self.function(id)?;
        Some(HostCallable {
            id,
            declaration,
            signature: self.signatures.get(id.index)?,
        })
    }

    pub(crate) fn resolve_name_in(
        &self,
        module: HostModuleId,
        path: &str,
        namespace: NameNamespace,
    ) -> Option<ResolvedName> {
        if module.revision != self.revision {
            return None;
        }
        let path = format!("{}::{path}", self.modules.get(module.index)?);
        self.resolve_name(&path, namespace)
    }

    pub(crate) fn resolve_name(
        &self,
        path: &str,
        namespace: NameNamespace,
    ) -> Option<ResolvedName> {
        match namespace {
            NameNamespace::Value => self.resolve(path).map(ResolvedName::HostFunction),
            NameNamespace::Type => self
                .resolve_type(path)
                .map(ResolvedName::HostType)
                .or_else(|| self.module(path).map(ResolvedName::HostModule)),
        }
    }

    /// Returns the identity of this immutable host input, used for analysis cache invalidation.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Canonical compilation input, excluding tooling locations and process revisions.
    pub(crate) fn compiler_input_bytes(&self) -> Result<Vec<u8>, HostInterfaceError> {
        self.interface.to_bytes(&self.definitions)
    }

    /// Finds a host type by its normalized `::` path.
    pub fn resolve_type(&self, path: &str) -> Option<HostTypeId> {
        self.type_paths.get(path).copied()
    }

    /// Finds a host type slot by canonical definition identity.
    pub fn nominal_type(&self, declaration: &DefinitionId) -> Option<HostTypeId> {
        self.type_identities.get(declaration).copied()
    }

    /// Borrows a type declaration only if revision and index belong to this input.
    pub fn type_declaration(&self, id: HostTypeId) -> Option<&HostTypeDeclaration> {
        (id.revision == self.revision)
            .then(|| self.interface.types.get(id.index))
            .flatten()
    }
}

pub(crate) fn signature_type(ty: &HostValueType) -> TypeId {
    match ty {
        HostValueType::Tuple(types) => TypeId::Tuple(types.iter().map(signature_type).collect()),
        HostValueType::Array(element) => TypeId::Array(Box::new(signature_type(element))),
        HostValueType::Vec(declaration, item) | HostValueType::List(declaration, item) => {
            let nominal = NominalType {
                declaration: *declaration,
                arguments: vec![signature_type(item)],
                associated_types: Default::default(),
            };
            if matches!(ty, HostValueType::Vec(..)) {
                TypeId::NativeObject(nominal)
            } else {
                TypeId::Trait(nominal)
            }
        }
        HostValueType::Map { key, value, access } => TypeId::Map {
            key: Box::new(signature_type(key)),
            value: Box::new(signature_type(value)),
            access: *access,
        },
        HostValueType::Set(element, access) => {
            TypeId::Set(Box::new(signature_type(element)), *access)
        }
        HostValueType::Option(declaration, element) => TypeId::Enum(NominalType {
            declaration: *declaration,
            arguments: vec![signature_type(element)],
            associated_types: Default::default(),
        }),
        HostValueType::Result {
            declaration,
            ok,
            error,
        } => TypeId::Enum(NominalType {
            declaration: *declaration,
            arguments: vec![signature_type(ok), signature_type(error)],
            associated_types: Default::default(),
        }),
        HostValueType::Opaque(id) => TypeId::Host(*id),
        scalar => TypeId::Builtin(match scalar {
            HostValueType::Unit => BuiltinType::Unit,
            HostValueType::Bool => BuiltinType::Bool,
            HostValueType::I32 => BuiltinType::I32,
            HostValueType::I64 => BuiltinType::I64,
            HostValueType::F32 => BuiltinType::F32,
            HostValueType::F64 => BuiltinType::F64,
            HostValueType::String => BuiltinType::String,
            _ => unreachable!("composite handled above"),
        }),
    }
}
