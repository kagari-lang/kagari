//! Offline host declarations contain no callback, runtime slot, or business service.
use bincode::Options;
use serde::{Deserialize, Serialize};

use crate::{
    capability::CapabilitySet,
    identity::{DefinitionId, DefinitionKind, DefinitionPathSegment, ModuleIdentity, PackageId},
};

const MAGIC: [u8; 4] = *b"KHI\0";
const VERSION: u16 = 1;
const MAX_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HostValueType {
    Unit,
    Bool,
    I32,
    I64,
    F32,
    F64,
    String,
    /// An opaque type is identified by its declaration, never a registry slot.
    Opaque(DefinitionId),
}

impl HostValueType {
    pub fn opaque(symbol: &str) -> Self {
        let mut id = HostFunctionDeclaration::new(symbol, Vec::new(), Self::Unit).id;
        id.path
            .last_mut()
            .expect("constructor creates declaration")
            .kind = DefinitionKind::Struct;
        Self::Opaque(id)
    }
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
    pub may_call_host_services: bool,
    pub may_mutate_host_state: bool,
    pub may_suspend: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostFunctionDeclaration {
    pub id: DefinitionId,
    /// The binding/export label is distinct from nominal declaration identity.
    pub symbol: String,
    pub params: Vec<HostParameter>,
    pub return_type: HostValueType,
    pub capability_requirements: CapabilitySet,
    pub resource_cost_hint: Option<u64>,
    pub effects: HostFunctionEffects,
    pub documentation: String,
}

impl HostFunctionDeclaration {
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
        if self.symbol.is_empty()
            || self.symbol.split('.').any(str::is_empty)
            || self
                .id
                .path
                .last()
                .is_none_or(|p| p.kind != DefinitionKind::Function || p.name.is_empty())
            || self.params.len() > u16::MAX as usize
        {
            return Err(HostInterfaceError::InvalidDeclaration);
        }
        let mut names = std::collections::HashSet::new();
        for param in &self.params {
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
        for ty in self
            .params
            .iter()
            .map(|p| &p.ty)
            .chain(std::iter::once(&self.return_type))
        {
            if let HostValueType::Opaque(id) = ty
                && id.path.is_empty()
            {
                return Err(HostInterfaceError::InvalidDeclaration);
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
            b"kagari-host-function-v1\0".iter().copied().chain(bytes),
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostInterface {
    pub functions: Vec<HostFunctionDeclaration>,
}

impl HostInterface {
    pub fn validate(&self) -> Result<(), HostInterfaceError> {
        let mut ids = std::collections::HashSet::new();
        let mut symbols = std::collections::HashSet::new();
        for function in &self.functions {
            function.validate()?;
            if !ids.insert(&function.id) || !symbols.insert(&function.symbol) {
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
        codec()
            .serialize(&(MAGIC, VERSION, functions))
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
        let (_, _, functions): ([u8; 4], u16, Vec<HostFunctionDeclaration>) = codec()
            .deserialize(bytes)
            .map_err(|_| HostInterfaceError::Encoding)?;
        let interface = Self { functions };
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
