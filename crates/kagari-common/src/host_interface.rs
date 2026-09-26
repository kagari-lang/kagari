//! Offline host declarations contain no callback, runtime slot, or business service.
use bincode::Options;
use serde::{Deserialize, Serialize};

use crate::{
    capability::CapabilitySet,
    identity::{DefinitionId, DefinitionKind, DefinitionPathSegment, ModuleIdentity, PackageId},
};

const MAGIC: [u8; 4] = *b"KHI\0";
const VERSION: u16 = 10;
const MAX_BYTES: u64 = 4 * 1024 * 1024;

mod decode_limits;
mod path;
pub use path::{
    HostIndexSegmentDeclaration, HostPathContract, HostPathDeclaration, HostPathInput,
    HostPathSegmentContract, HostPathSegmentDeclaration, HostVirtualSegmentDeclaration,
};
mod value_type;
pub use value_type::HostValueType;
mod type_declaration;
pub use type_declaration::{
    HostAssociatedTypeBinding, HostFieldDeclaration, HostMethodDeclaration, HostReflectionPolicy,
    HostTraitImplementationDeclaration, HostTraitMethodBinding, HostTypeDeclaration,
    HostTypeOwnership, PathAccess, Visibility,
};

impl HostValueType {
    pub fn opaque(symbol: &str) -> Self {
        Self::Opaque(host_type_identity(symbol))
    }
}

/// Default identity for a type in the application host namespace. Providers may
/// supply their own package/module identity independently of the export label.
pub fn host_type_identity(symbol: &str) -> DefinitionId {
    let mut id = HostFunctionDeclaration::new(symbol, Vec::new(), HostValueType::Unit).id;
    id.path
        .last_mut()
        .expect("constructor creates declaration")
        .kind = DefinitionKind::Struct;
    id
}

pub fn validate_host_type_identity(id: &DefinitionId) -> Result<(), HostInterfaceError> {
    if !id.within_path_limit() {
        return Err(HostInterfaceError::TooLarge);
    }
    if id.module.package.0.is_empty()
        || id.module.path.iter().any(String::is_empty)
        || id.path.iter().any(|part| part.name.is_empty())
        || id
            .path
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostFunctionDeclaration {
    pub id: DefinitionId,
    /// The binding/export label is distinct from nominal declaration identity.
    pub symbol: String,
    #[serde(deserialize_with = "decode_limits::members")]
    pub params: Vec<HostParameter>,
    pub return_type: HostValueType,
    pub capability_requirements: CapabilitySet,
    pub resource_cost_hint: Option<u64>,
    pub effects: HostFunctionEffects,
    pub documentation: String,
}

impl HostFunctionDeclaration {
    pub fn method_owner(&self) -> Option<DefinitionId> {
        if self.id.path.last()?.kind != DefinitionKind::Method {
            return None;
        }
        let mut owner = self.id.clone();
        owner.path.pop();
        Some(owner)
    }
    pub fn matches_binding(&self, actual: &Self) -> bool {
        self.id == actual.id
            && self.symbol == actual.symbol
            && self.params == actual.params
            && self.return_type == actual.return_type
            && self.capability_requirements == actual.capability_requirements
            && self.resource_cost_hint == actual.resource_cost_hint
            && self.effects == actual.effects
    }
    /// The application host namespace is a logical package. Providers can replace
    /// `id` with their own package/module declaration before publishing the interface.
    pub fn new(
        symbol: impl Into<String>,
        params: Vec<HostParameter>,
        return_type: HostValueType,
    ) -> Self {
        let symbol = symbol.into();
        let mut path = symbol.split('.').map(str::to_owned).collect::<Vec<_>>();
        let name = path.pop().unwrap_or_default();
        Self {
            id: DefinitionId {
                module: ModuleIdentity {
                    package: PackageId("host".into()),
                    path,
                },
                path: vec![DefinitionPathSegment {
                    kind: DefinitionKind::Function,
                    name,
                    occurrence: 0,
                }],
            },
            symbol,
            params,
            return_type,
            capability_requirements: Default::default(),
            resource_cost_hint: None,
            effects: Default::default(),
            documentation: String::new(),
        }
    }

    pub fn validate(&self) -> Result<(), HostInterfaceError> {
        if !self.id.within_path_limit() {
            return Err(HostInterfaceError::TooLarge);
        }
        if self.symbol.is_empty()
            || self.symbol.split('.').any(str::is_empty)
            || self.id.path.last().is_none_or(|p| {
                !matches!(p.kind, DefinitionKind::Function | DefinitionKind::Method)
                    || p.name.is_empty()
            })
        {
            return Err(HostInterfaceError::InvalidDeclaration);
        }
        if let Some(owner) = self.method_owner() {
            validate_host_type_identity(&owner)?;
            if self.params.first().is_none_or(|receiver| {
                receiver.name != "self" || receiver.ty != HostValueType::Opaque(owner)
            }) {
                return Err(HostInterfaceError::InvalidDeclaration);
            }
        }
        validate_signature(&self.params, &self.return_type)?;
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
                    HostValueType::Option(element) => pending.push(element),
                    HostValueType::Result { ok, error } => {
                        pending.extend([ok.as_ref(), error.as_ref()])
                    }
                    HostValueType::Opaque(_)
                    | HostValueType::Array(_)
                    | HostValueType::Map { .. }
                    | HostValueType::Set(_) => {
                        return Err(HostInterfaceError::InvalidDeclaration);
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    /// Fixed-width little-endian, declaration-order encoding, domain-separated
    /// FNV-1a-64. Documentation is carried offline but does not alter the call ABI.
    pub fn fingerprint(&self) -> Result<u64, HostInterfaceError> {
        self.validate()?;
        let bytes = codec()
            .serialize(&(
                &self.id,
                &self.symbol,
                &self.params,
                &self.return_type,
                self.capability_requirements,
                self.resource_cost_hint,
                self.effects,
            ))
            .map_err(|_| HostInterfaceError::Encoding)?;
        Ok(hash(
            b"kagari-host-function-v3\0".iter().copied().chain(bytes),
        ))
    }
}

fn validate_signature(
    params: &[HostParameter],
    return_type: &HostValueType,
) -> Result<(), HostInterfaceError> {
    if params.len() > decode_limits::MAX_MEMBERS {
        return Err(HostInterfaceError::TooLarge);
    }
    let mut names = std::collections::HashSet::new();
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
    for ty in params
        .iter()
        .map(|p| &p.ty)
        .chain(std::iter::once(return_type))
    {
        ty.validate()?;
    }
    Ok(())
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostInterface {
    #[serde(deserialize_with = "decode_limits::declarations")]
    pub paths: Vec<HostPathDeclaration>,
    #[serde(deserialize_with = "decode_limits::declarations")]
    pub types: Vec<HostTypeDeclaration>,
    #[serde(deserialize_with = "decode_limits::declarations")]
    pub functions: Vec<HostFunctionDeclaration>,
}

#[derive(Deserialize)]
struct HostInterfaceWire {
    _magic: [u8; 4],
    _version: u16,
    #[serde(deserialize_with = "decode_limits::declarations")]
    types: Vec<HostTypeDeclaration>,
    #[serde(deserialize_with = "decode_limits::declarations")]
    functions: Vec<HostFunctionDeclaration>,
    #[serde(deserialize_with = "decode_limits::declarations")]
    paths: Vec<HostPathDeclaration>,
}

impl HostInterface {
    pub fn validate(&self) -> Result<(), HostInterfaceError> {
        if [self.types.len(), self.functions.len(), self.paths.len()]
            .into_iter()
            .any(|count| count > decode_limits::MAX_DECLARATIONS)
            || self.paths.iter().any(|path| {
                path.segments.len() > decode_limits::MAX_MEMBERS
                    || !path.root.within_path_limit()
                    || path.segments.iter().any(|step| match step {
                        HostPathSegmentDeclaration::Field(id) => !id.within_path_limit(),
                        _ => false,
                    })
            })
        {
            return Err(HostInterfaceError::TooLarge);
        }
        let mut ids = std::collections::HashSet::new();
        let mut symbols = std::collections::HashSet::new();
        for ty in &self.types {
            ty.validate()?;
            if !ids.insert(&ty.id) || !symbols.insert(&ty.symbol) {
                return Err(HostInterfaceError::DuplicateDeclaration);
            }
        }
        let type_ids = self
            .types
            .iter()
            .map(|ty| &ty.id)
            .collect::<std::collections::HashSet<_>>();
        for function in &self.functions {
            function.validate()?;
            if let Some(owner) = function.method_owner() {
                let declaration = self
                    .types
                    .iter()
                    .find(|ty| ty.id == owner)
                    .ok_or(HostInterfaceError::InvalidDeclaration)?;
                if !declaration
                    .method_contract(&function.id)?
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
                .chain(std::iter::once(&function.return_type))
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
        let mut paths = std::collections::BTreeSet::new();
        for path in &self.paths {
            self.resolve_path(path)?;
            if !paths.insert(
                codec()
                    .serialize(path)
                    .map_err(|_| HostInterfaceError::Encoding)?,
            ) {
                return Err(HostInterfaceError::DuplicateDeclaration);
            }
        }
        Ok(())
    }
    pub fn to_bytes(&self) -> Result<Vec<u8>, HostInterfaceError> {
        self.validate()?;
        // Canonical declaration order is independent of registration order.
        let mut functions = self.functions.iter().collect::<Vec<_>>();
        functions.sort_by(|a, b| a.id.cmp(&b.id));
        let mut types = self.types.iter().collect::<Vec<_>>();
        types.sort_by(|a, b| a.id.cmp(&b.id));
        let mut paths = self
            .paths
            .iter()
            .map(|path| {
                codec()
                    .serialize(path)
                    .map(|key| (key, path))
                    .map_err(|_| HostInterfaceError::Encoding)
            })
            .collect::<Result<Vec<_>, _>>()?;
        paths.sort_by(|a, b| a.0.cmp(&b.0));
        let paths = paths.into_iter().map(|(_, path)| path).collect::<Vec<_>>();
        codec()
            .serialize(&(MAGIC, VERSION, types, functions, paths))
            .map_err(|_| HostInterfaceError::Encoding)
    }
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, HostInterfaceError> {
        if bytes.len() as u64 > MAX_BYTES {
            return Err(HostInterfaceError::TooLarge);
        }
        if bytes.get(..4) != Some(MAGIC.as_slice())
            || bytes.get(4..6) != Some(VERSION.to_le_bytes().as_slice())
        {
            return Err(HostInterfaceError::Version);
        }
        let HostInterfaceWire {
            types,
            functions,
            paths,
            ..
        } = codec()
            .deserialize(bytes)
            .map_err(|_| HostInterfaceError::Encoding)?;
        let interface = Self {
            types,
            functions,
            paths,
        };
        interface.validate()?;
        Ok(interface)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostInterfaceError {
    InvalidDeclaration,
    DuplicateDeclaration,
    Version,
    TooLarge,
    Encoding,
}

impl std::fmt::Display for HostInterfaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "host interface: {self:?}")
    }
}
impl std::error::Error for HostInterfaceError {}

fn codec() -> impl Options {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
        .reject_trailing_bytes()
        .with_limit(MAX_BYTES)
}

/// The declaration for the current source `print` entry and CLI log binding.
pub fn standard_log() -> HostFunctionDeclaration {
    let mut declaration = HostFunctionDeclaration::new(
        "host.log",
        vec![HostParameter {
            name: "message".into(),
            ty: HostValueType::String,
            passing: HostPassingStyle::SharedBorrow,
        }],
        HostValueType::Unit,
    );
    declaration.effects.may_trap = true;
    declaration.effects.may_call_host_services = true;
    declaration.effects.may_mutate_host_state = true;
    declaration.documentation = "Write a message to the host log.".into();
    declaration
}
fn hash(bytes: impl IntoIterator<Item = u8>) -> u64 {
    bytes.into_iter().fold(0xcbf29ce484222325, |h, byte| {
        (h ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}
