//! Offline host declarations contain no callback, runtime slot, or business service.
use crate::host_interface::{
    path::HostPathDeclaration, type_declaration::HostTypeDeclaration, value_type::HostValueType,
};
use bincode::{DefaultOptions, Options};
use kagari_common::identity::{
    DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity, PackageId,
    map::DefinitionContext,
    mapping::DefinitionMappingError,
    metadata::DefinitionMetadata,
    reference::DefinitionReference,
    table::{DefinitionId, DefinitionTable, DefinitionTableError},
};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, iter, sync::OnceLock};

const MAGIC: [u8; 4] = *b"KHI\0";
const VERSION: u16 = 12;
const MAX_BYTES: u64 = 4 * 1024 * 1024;

pub mod constraints;
mod decode_limits;
pub mod path;

pub mod value_type;

pub mod type_declaration;

/// Exact external query for a type in the application Host namespace.
/// This creates no declaration; semantic consumers resolve it in their own table.
pub fn host_type_identity(symbol: &str) -> DefinitionPath {
    let (module, name) = host_namespace(symbol);
    DefinitionPath {
        module,
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Struct,
            name,
            occurrence: 0,
        }],
    }
}

fn host_namespace(symbol: &str) -> (ModuleIdentity, String) {
    let mut path = symbol.split('.').map(str::to_owned).collect::<Vec<_>>();
    let name = path.pop().unwrap_or_default();
    (
        ModuleIdentity {
            package: PackageId("host".into()),
            path,
        },
        name,
    )
}

fn register_host_definition(
    context: &DefinitionContext,
    symbol: &str,
    kind: DefinitionKind,
) -> Result<DefinitionId, DefinitionTableError> {
    let (module, name) = host_namespace(symbol);
    let root = context.intern_root(&module)?;
    context.intern_child(root, kind, &name, 0)
}

pub fn validate_host_type_identity(id: &DefinitionPath) -> Result<(), HostInterfaceError> {
    validate_host_type_identity_in(id, None)
}

pub fn validate_host_type_identity_in<I: DefinitionReference>(
    id: &I,
    table: Option<&DefinitionTable>,
) -> Result<(), HostInterfaceError> {
    if !id.within_path_limit() {
        return Err(HostInterfaceError::TooLarge);
    }
    let view = id
        .describe(table)
        .map_err(|_| HostInterfaceError::InvalidDeclaration)?;
    if view.module().package.0.is_empty()
        || view.module().path.iter().any(String::is_empty)
        || view.segments().any(|part| part.name.is_empty())
        || view
            .last()
            .is_none_or(|part| part.kind != DefinitionKind::Struct)
    {
        return Err(HostInterfaceError::InvalidDeclaration);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HostPassingStyle {
    Owned,
    SharedBorrow,
    UniqueBorrow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostParameter {
    pub name: String,
    pub ty: HostValueType,
    pub passing: HostPassingStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct HostFunctionEffects {
    pub may_allocate: bool,
    pub may_trap: bool,
    /// Reads only host-provided immutable configuration, with value-only inputs
    /// and results. This does not authorize general service access or mutation.
    pub may_read_immutable_configuration: bool,
    pub may_call_host_services: bool,
    pub may_mutate_host_state: bool,
    pub may_suspend: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostFunctionDeclaration {
    pub id: DefinitionId,
    /// The binding/export label is distinct from nominal declaration identity.
    pub symbol: String,
    pub params: Vec<HostParameter>,
    pub return_type: HostValueType,

    pub effects: HostFunctionEffects,
    pub documentation: String,
}

impl HostFunctionDeclaration {
    /// Registers a function in the explicit context's application Host namespace.
    ///
    /// Parameter/result IDs must belong to this context before publishing the
    /// interface. Construction allocates identity only; `validate` checks the
    /// contract, and metadata sealing checks its reference scope. Providers may
    /// replace `id` with another identity registered in the same context.
    ///
    /// # Errors
    /// Returns identity allocation or path-limit failures.
    pub fn new(
        context: &DefinitionContext,
        symbol: impl Into<String>,
        params: Vec<HostParameter>,
        return_type: HostValueType,
    ) -> Result<Self, DefinitionTableError> {
        let symbol = symbol.into();
        Ok(Self {
            id: register_host_definition(context, &symbol, DefinitionKind::Function)?,
            symbol,
            params,
            return_type,
            effects: Default::default(),
            documentation: String::new(),
        })
    }
}

impl HostFunctionDeclaration {
    pub fn matches_binding(&self, actual: &Self) -> bool {
        self.id == actual.id
            && self.symbol == actual.symbol
            && self.params == actual.params
            && self.return_type == actual.return_type
            && self.effects == actual.effects
    }
}

impl HostFunctionDeclaration {
    pub fn method_owner(&self, definitions: &DefinitionTable) -> Option<DefinitionId> {
        if definitions.resolve(self.id).ok()?.last()?.kind != DefinitionKind::Method {
            return None;
        }
        definitions.parent(self.id).ok().flatten()
    }

    /// Canonical file-local identity encoding, domain-separated FNV-1a-64.
    /// Documentation is carried offline but does not alter the call ABI.
    pub fn fingerprint(&self, definitions: &DefinitionTable) -> Result<u64, HostInterfaceError> {
        self.validate(definitions)?;
        let mut abi = self.clone();
        abi.documentation.clear();
        wire::fingerprint(b"kagari-host-function-v3\0", &abi, definitions)
    }

    pub fn validate(&self, definitions: &DefinitionTable) -> Result<(), HostInterfaceError> {
        let identity = definitions
            .resolve(self.id)
            .map_err(|_| HostInterfaceError::InvalidDeclaration)?;
        if self.symbol.is_empty()
            || self.symbol.split('.').any(str::is_empty)
            || identity.last().is_none_or(|p| {
                !matches!(p.kind, DefinitionKind::Function | DefinitionKind::Method)
                    || p.name.is_empty()
            })
        {
            return Err(HostInterfaceError::InvalidDeclaration);
        }
        if identity
            .last()
            .is_some_and(|part| part.kind == DefinitionKind::Method)
        {
            let owner = definitions
                .parent(self.id)
                .map_err(|_| HostInterfaceError::InvalidDeclaration)?
                .ok_or(HostInterfaceError::InvalidDeclaration)?;
            validate_host_type_identity_in(&owner, Some(definitions))?;
            if self.params.first().is_none_or(|receiver| {
                receiver.name != "self" || receiver.ty != HostValueType::Opaque(owner)
            }) {
                return Err(HostInterfaceError::InvalidDeclaration);
            }
        }
        validate_signature(&self.params, &self.return_type, definitions)?;
        if self.effects.may_read_immutable_configuration {
            let mut pending = vec![&self.return_type];
            for parameter in &self.params {
                if parameter.passing != HostPassingStyle::Owned {
                    return Err(HostInterfaceError::InvalidDeclaration);
                }
                pending.push(&parameter.ty);
            }
            while let Some(ty) = pending.pop() {
                match ty {
                    HostValueType::Tuple(elements) => pending.extend(elements),
                    HostValueType::Option(_, element) => pending.push(element),
                    HostValueType::Result { ok, error, .. } => {
                        pending.extend([ok.as_ref(), error.as_ref()])
                    }
                    HostValueType::Opaque(_)
                    | HostValueType::Array(_)
                    | HostValueType::Vec(..)
                    | HostValueType::List(..)
                    | HostValueType::Map { .. }
                    | HostValueType::Set(_, _) => {
                        return Err(HostInterfaceError::InvalidDeclaration);
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }
}

fn adoption_error(error: DefinitionMappingError) -> HostInterfaceError {
    match error {
        DefinitionMappingError::LimitExceeded
        | DefinitionMappingError::Identity(
            DefinitionTableError::PathLimit
            | DefinitionTableError::IndexExhausted
            | DefinitionTableError::TableIdentityExhausted,
        ) => HostInterfaceError::TooLarge,
        _ => HostInterfaceError::InvalidDeclaration,
    }
}

fn validate_signature(
    params: &[HostParameter],
    return_type: &HostValueType,
    definitions: &DefinitionTable,
) -> Result<(), HostInterfaceError> {
    if params.len() > decode_limits::MAX_MEMBERS {
        return Err(HostInterfaceError::TooLarge);
    }
    let mut names = HashSet::new();
    for param in params {
        if param.name.is_empty() || !names.insert(&param.name) {
            return Err(HostInterfaceError::InvalidDeclaration);
        }
        if param.passing != HostPassingStyle::Owned
            && !matches!(param.ty, HostValueType::Opaque(_))
            && !(param.passing == HostPassingStyle::SharedBorrow
                && param.ty == HostValueType::String)
        {
            return Err(HostInterfaceError::InvalidDeclaration);
        }
    }
    for ty in params.iter().map(|p| &p.ty).chain(iter::once(return_type)) {
        ty.validate_in(definitions)?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HostInterface {
    pub paths: Vec<HostPathDeclaration>,
    pub types: Vec<HostTypeDeclaration>,
    pub functions: Vec<HostFunctionDeclaration>,
}

impl HostInterface {
    pub fn validate(&self, definitions: &DefinitionTable) -> Result<(), HostInterfaceError> {
        if [self.types.len(), self.functions.len(), self.paths.len()]
            .into_iter()
            .any(|count| count > decode_limits::MAX_DECLARATIONS)
            || self
                .paths
                .iter()
                .any(|path| path.segments.len() > decode_limits::MAX_MEMBERS)
        {
            return Err(HostInterfaceError::TooLarge);
        }
        let mut ids = HashSet::new();
        let mut symbols = HashSet::new();
        for ty in &self.types {
            ty.validate(definitions)?;
            if !ids.insert(&ty.id) || !symbols.insert(&ty.symbol) {
                return Err(HostInterfaceError::DuplicateDeclaration);
            }
        }
        let type_ids = self.types.iter().map(|ty| &ty.id).collect::<HashSet<_>>();
        for function in &self.functions {
            function.validate(definitions)?;
            if let Some(owner) = function.method_owner(definitions) {
                let declaration = self
                    .types
                    .iter()
                    .find(|ty| ty.id == owner)
                    .ok_or(HostInterfaceError::InvalidDeclaration)?;
                if !declaration
                    .method_contract(&function.id, definitions)?
                    .matches_binding(function)
                {
                    return Err(HostInterfaceError::InvalidDeclaration);
                }
            }
            if !ids.insert(&function.id) || !symbols.insert(&function.symbol) {
                return Err(HostInterfaceError::DuplicateDeclaration);
            }
            for ty in function
                .params
                .iter()
                .map(|param| &param.ty)
                .chain(iter::once(&function.return_type))
            {
                if ty
                    .nominal_references()
                    .into_iter()
                    .any(|id| !type_ids.contains(id))
                {
                    return Err(HostInterfaceError::InvalidDeclaration);
                }
            }
        }
        for declaration in &self.types {
            for ty in declaration.value_types() {
                if ty
                    .nominal_references()
                    .into_iter()
                    .any(|id| !type_ids.contains(id))
                {
                    return Err(HostInterfaceError::InvalidDeclaration);
                }
            }
        }
        let mut paths = HashSet::new();
        for path in &self.paths {
            self.resolve_path(path, definitions)?;
            if !paths.insert(path) {
                return Err(HostInterfaceError::DuplicateDeclaration);
            }
        }
        Ok(())
    }

    pub fn to_bytes(&self, definitions: &DefinitionTable) -> Result<Vec<u8>, HostInterfaceError> {
        self.validate(definitions)?;
        wire::encode(self, definitions)
    }

    /// Decode file references into a fresh scope, then validate all Host contracts.
    pub fn from_bytes(bytes: &[u8]) -> Result<DefinitionMetadata<Self>, HostInterfaceError> {
        wire::decode(bytes)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("host interface: {self:?}")]
pub enum HostInterfaceError {
    Cancelled,
    InvalidDeclaration,
    DuplicateDeclaration,
    Version,
    TooLarge,
    Encoding,
}

impl From<HostInterfaceError> for DefinitionMappingError {
    fn from(error: HostInterfaceError) -> Self {
        match error {
            HostInterfaceError::Cancelled => Self::Cancelled,
            HostInterfaceError::TooLarge => Self::LimitExceeded,
            _ => Self::InvalidContract,
        }
    }
}

fn codec() -> impl Options {
    DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
        .reject_trailing_bytes()
        .with_limit(MAX_BYTES)
}

/// The scoped declaration for the registered `print` entry and CLI log binding.
///
/// This immutable contract owns one retained scope. Consumers combining it with
/// other metadata must import it into their context. Repeated calls share that
/// scope; they neither allocate a new identity universe nor intern into a global pool.
/// Allocation failure is returned before the contract can enter semantic processing.
pub fn standard_log() -> Result<DefinitionMetadata<HostFunctionDeclaration>, DefinitionMappingError>
{
    static LOG: OnceLock<
        Result<DefinitionMetadata<HostFunctionDeclaration>, DefinitionMappingError>,
    > = OnceLock::new();
    LOG.get_or_init(|| {
        let context = DefinitionContext::new()?;
        let mut declaration = HostFunctionDeclaration::new(
            &context,
            "host.log",
            vec![HostParameter {
                name: "message".into(),
                ty: HostValueType::String,
                passing: HostPassingStyle::SharedBorrow,
            }],
            HostValueType::Unit,
        )?;
        declaration.effects = HostFunctionEffects {
            may_trap: true,
            may_call_host_services: true,
            may_mutate_host_state: true,
            ..Default::default()
        };
        declaration.documentation = "Write a message to the host log.".into();
        DefinitionMetadata::checked(context.snapshot(), declaration, &Default::default())
    })
    .clone()
}

fn hash(bytes: impl IntoIterator<Item = u8>) -> u64 {
    bytes.into_iter().fold(0xcbf29ce484222325, |h, byte| {
        (h ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}

mod mapping;

/// Bounded Host wire records shared by artifact codec owners.
/// Semantic callers use the scoped declarations and publication APIs.
#[doc(hidden)]
pub mod wire;

#[cfg(test)]
mod scoped_tests;

#[cfg(test)]
mod test_support;
