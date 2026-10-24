//! Runtime-independent schema, DTO values and validated snapshots.

use crate::state::{
    error::{SnapshotError, SnapshotErrorKind, SnapshotResult, SnapshotStage},
    limits::{Budget, SnapshotOptions, validate_snapshot},
    wire,
};
use serde::Serialize;

/// Logical application schema identity, unrelated to runtime or deployment IDs.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct SchemaVersion {
    pub id: String,
    pub version: u32,
}

impl SchemaVersion {
    /// Construct a nonempty logical ID with a nonzero version.
    pub fn new(id: impl Into<String>, version: u32) -> SnapshotResult<Self> {
        let id = id.into();
        if id.is_empty() || version == 0 {
            return Err(SnapshotError::new(
                SnapshotStage::Validate,
                SnapshotErrorKind::InvalidSchema,
                "schema ID must be nonempty and version nonzero",
            ));
        }
        Ok(Self { id, version })
    }
}

/// Host-supplied provenance; these fields confer no authorization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SnapshotHeader {
    pub schema: SchemaVersion,
    pub source_deployment: String,
    pub state_owner: String,
    pub state_revision: u64,
}

/// Complete closed schema. Definitions are ordered by their stable nominal key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StateSchema {
    pub version: SchemaVersion,
    pub root: StateType,
    pub definitions: Vec<StateDefinition>,
}

/// Nominal recursion refers to definitions rather than embedding their shapes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum StateType {
    Scalar(StateScalar),
    Tuple(Vec<StateType>),
    Sequence(Box<StateType>),
    Nominal(NominalKey),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum StateScalar {
    Unit,
    Bool,
    Char,
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
    String,
}

/// Stable declaration components and closed arguments, never runtime IDs.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct NominalKey {
    pub package: String,
    pub module: Vec<String>,
    pub declaration: Vec<String>,
    pub arguments: Vec<StateType>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StateDefinition {
    pub key: NominalKey,
    pub shape: StateShape,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum StateShape {
    Struct(Vec<StateField>),
    Enum(Vec<StateVariant>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StateField {
    pub name: String,
    pub ty: StateType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StateVariant {
    pub name: String,
    pub fields: Vec<StateType>,
}

/// Owned tree data. Float variants preserve bits, including NaN payloads.
/// Record fields are strictly ordered by UTF-8 name; no alias/reference tag exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateValue {
    Unit,
    Bool(bool),
    Char(char),
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    F32Bits(u32),
    F64Bits(u64),
    String(String),
    Tuple(Vec<StateValue>),
    Sequence(Vec<StateValue>),
    Record {
        key: NominalKey,
        fields: Vec<(String, StateValue)>,
    },
    Enum {
        key: NominalKey,
        variant: String,
        fields: Vec<StateValue>,
    },
}

/// Validated owned state, without runtime roots, leases or host capabilities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateSnapshot {
    header: SnapshotHeader,
    schema: StateSchema,
    payload: StateValue,
}

impl StateSnapshot {
    pub(crate) fn new(
        header: SnapshotHeader,
        schema: StateSchema,
        payload: StateValue,
        budget: &mut Budget,
    ) -> SnapshotResult<Self> {
        validate_snapshot(&header, &schema, &payload, budget)?;
        Ok(Self::from_validated(header, schema, payload))
    }

    pub(crate) fn from_validated(
        header: SnapshotHeader,
        schema: StateSchema,
        payload: StateValue,
    ) -> Self {
        Self {
            header,
            schema,
            payload,
        }
    }

    pub(crate) fn into_parts(self) -> (SnapshotHeader, StateSchema, StateValue) {
        (self.header, self.schema, self.payload)
    }

    pub fn header(&self) -> &SnapshotHeader {
        &self.header
    }

    pub fn schema(&self) -> &StateSchema {
        &self.schema
    }

    pub fn payload(&self) -> &StateValue {
        &self.payload
    }

    /// Decode one bounded canonical envelope, rejecting trailing bytes.
    pub fn from_bytes(bytes: &[u8], options: &SnapshotOptions) -> SnapshotResult<Self> {
        wire::decode(bytes, options)
    }

    /// Validate against these operation limits, then encode one bounded envelope.
    pub fn to_bytes(&self, options: &SnapshotOptions) -> SnapshotResult<Vec<u8>> {
        wire::encode(self, options)
    }
}

/// Cooperative host migration helpers; no runtime capability is provided.
/// Arbitrary callback code and captures cannot be forcibly preempted or bounded.
pub struct MigrationContext<'a> {
    pub(crate) source: &'a SnapshotHeader,
    pub(crate) target: &'a StateSchema,
    pub(crate) budget: Budget,
}
