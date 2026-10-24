//! File-local identity codecs. Decode with a byte-limited outer artifact codec;
//! these records additionally bound every vector and all conversion work. No
//! completeness, native trust or successful verification flag is serialized.
mod shape;
mod values;
use crate::{
    callable::{
        construction::evidence::ConstructionWitness, generic::GenericBody,
        interface::InterfaceCallContract, shared::SharedCall, witness::OperationWitness,
    },
    ids::FunctionRef,
    native_import::NativeImport,
    source::manifest::SourceManifest,
    types::ConcreteFunctionIdentity,
    updates::{
        BodyApplication, BodyClassification, BodyRecord, BodyRequirement, CallRequirement,
        CanonicalInstruction, CanonicalOperand, CurrentTarget, DemandEnvelope, Dependency,
        ProtectedRoot, RootKind, SourceRecord, SourceToken, UnknownDependency, UnknownReason,
        UpdateEvidence,
        limits::{Budget, MAX_APPLICATIONS, MAX_NESTED, UpdateError},
    },
};
use bincode::{DefaultOptions, Options};
use kagari_common::identity::table::{
    DefinitionId,
    wire::{DefinitionDecoding, DefinitionEncoding},
};
use kagari_types::{callable::Signature, ty::Ty};
use serde::{Deserialize, Serialize};

fn canonical_key<T: WireValue>(
    value: &T,
    encoding: &DefinitionEncoding,
    budget: &mut Budget<'_>,
) -> Result<Vec<u8>, UpdateError> {
    let record = value.encode_value(encoding, budget)?;
    DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
        .serialize(&record)
        .map_err(|_| UpdateError::InvalidEvidence)
}

pub(crate) fn body_key(
    value: &BodyRequirement,
    encoding: &DefinitionEncoding,
    budget: &mut Budget<'_>,
) -> Result<Vec<u8>, UpdateError> {
    canonical_key(value, encoding, budget)
}

pub(crate) fn call_key(
    value: &CallRequirement,
    encoding: &DefinitionEncoding,
    budget: &mut Budget<'_>,
) -> Result<Vec<u8>, UpdateError> {
    canonical_key(value, encoding, budget)
}

pub(crate) fn application_key(
    value: &BodyApplication,
    encoding: &DefinitionEncoding,
    budget: &mut Budget<'_>,
) -> Result<Vec<u8>, UpdateError> {
    canonical_key(value, encoding, budget)
}
use values::WireValue;

macro_rules! record {
    ($model:ident => $wire:ident { $($(#[$attr:meta])* $field:ident : $ty:ty $(=> $max:expr)?),* $(,)? }) => {
        #[derive(Serialize, Deserialize)]
        pub struct $wire {
            $($(#[$attr])* $field: <$ty as WireValue>::Record,)*
        }

        impl WireValue for $model {
            type Record = $wire;

            fn encode_value(&self, encoding: &DefinitionEncoding, budget: &mut Budget<'_>) -> Result<Self::Record, UpdateError> {
                $($(if self.$field.len() > $max { return Err(UpdateError::LimitExceeded); })?)*
                budget.records(1)?;
                Ok($wire { $($field: self.$field.encode_value(encoding, budget)?,)* })
            }

            fn decode_value(value: Self::Record, decoding: &DefinitionDecoding, budget: &mut Budget<'_>) -> Result<Self, UpdateError> {
                $($(if value.$field.len() > $max { return Err(UpdateError::LimitExceeded); })?)*
                budget.records(1)?;
                Ok(Self { $($field: <$ty>::decode_value(value.$field, decoding, budget)?,)* })
            }
        }
    };
}

macro_rules! variants {
    ($model:ident => $wire:ident { $($(#[$attr:meta])* $variant:ident : $ty:ty $(=> $max:expr)?),* $(,)? }) => {
        #[derive(Serialize, Deserialize)]
        enum $wire {
            $($variant($(#[$attr])* <$ty as WireValue>::Record),)*
        }

        impl WireValue for $model {
            type Record = $wire;

            fn encode_value(&self, encoding: &DefinitionEncoding, budget: &mut Budget<'_>) -> Result<Self::Record, UpdateError> {
                budget.records(1)?;
                budget.bytes(4)?;
                Ok(match self { $(Self::$variant(value) => { $(if value.len() > $max { return Err(UpdateError::LimitExceeded); })? $wire::$variant(value.encode_value(encoding, budget)?) },)* })
            }

            fn decode_value(value: Self::Record, decoding: &DefinitionDecoding, budget: &mut Budget<'_>) -> Result<Self, UpdateError> {
                budget.records(1)?;
                budget.bytes(4)?;
                Ok(match value { $($wire::$variant(value) => { $(if value.len() > $max { return Err(UpdateError::LimitExceeded); })? Self::$variant(<$ty>::decode_value(value, decoding, budget)?) },)* })
            }
        }
    };
}

record!(UpdateEvidence => UpdateEvidenceRecord {
    source_manifest: Option<SourceManifest>,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    sources: Vec<SourceRecord>,
    #[serde(deserialize_with = "crate::decode_limits::functions")]
    bodies: Vec<BodyRecord> => MAX_APPLICATIONS,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    roots: Vec<ProtectedRoot>,
    #[serde(deserialize_with = "crate::decode_limits::functions")]
    targets: Vec<CurrentTarget> => MAX_APPLICATIONS,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    selections: Vec<CallRequirement>,
});
record!(DemandEnvelope => DemandEnvelopeRecord {
    #[serde(deserialize_with = "crate::decode_limits::functions")]
    applications: Vec<BodyApplication> => MAX_APPLICATIONS,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    bodies: Vec<BodyRequirement>,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    requirements: Vec<CallRequirement>,
});
record!(SourceRecord => SourceRecordWire {
    declaration: DefinitionId,
    generic: Option<GenericBody>,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    tokens: Vec<SourceToken>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    inputs: Vec<CanonicalOperand> => MAX_NESTED,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    calls: Vec<CallRequirement> => MAX_NESTED,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    dependencies: Vec<Dependency> => MAX_NESTED,
});
record!(SourceToken => SourceTokenWire { kind: u32, text: String });
record!(BodyApplication => BodyApplicationWire {
    body: ConcreteFunctionIdentity,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    arguments: Vec<Ty> => MAX_NESTED,
});
record!(BodyRecord => BodyRecordWire {
    requirement: BodyRequirement,
    classification: BodyClassification,
    signature: Signature,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    captures: Vec<Ty> => MAX_NESTED,
    format: u32,
    #[serde(deserialize_with = "crate::decode_limits::instructions")]
    instructions: Vec<CanonicalInstruction>,
    actual_format: u32,
    #[serde(deserialize_with = "crate::decode_limits::instructions")]
    actual_instructions: Vec<CanonicalInstruction>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    dependencies: Vec<Dependency> => MAX_NESTED,
});
record!(BodyRequirement => BodyRequirementWire {
    origin: ConcreteFunctionIdentity,
    generic: Option<GenericBody>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    calls: Vec<CallRequirement> => MAX_NESTED,
});
record!(CanonicalInstruction => CanonicalInstructionWire {
    opcode: u32,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    operands: Vec<CanonicalOperand> => MAX_NESTED,
});
record!(ProtectedRoot => ProtectedRootWire {
    declaration: ConcreteFunctionIdentity,
    kind: RootKind,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    dependencies: Vec<Dependency> => MAX_NESTED,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    selections: Vec<CallRequirement> => MAX_NESTED,
});
record!(CurrentTarget => CurrentTargetWire {
    application: ConcreteFunctionIdentity, body: ConcreteFunctionIdentity,
    module: u32, function: FunctionRef,
});
variants!(BodyClassification => BodyClassificationWire {
    Named: DefinitionId, Closure: ConcreteFunctionIdentity, Async: ConcreteFunctionIdentity,
    Resume: ConcreteFunctionIdentity, Adapter: ConcreteFunctionIdentity, StaticInitializer: DefinitionId,
    IntrinsicAdapter: ConcreteFunctionIdentity,
});
variants!(CanonicalOperand => CanonicalOperandWire {
    Local: u32, Block: u32, Unsigned: u64, Signed: i64,
        #[serde(deserialize_with = "crate::decode_limits::nested")]
        Bytes: Vec<u8> => MAX_NESTED,
    Declaration: DefinitionId, Type: Ty, Application: ConcreteFunctionIdentity, Call: u32,
});
variants!(CallRequirement => CallRequirementWire {
    Direct: ConcreteFunctionIdentity, Shared: Box<SharedCall>, Interface: Box<InterfaceCallContract>,
    RetainedBody: BodyApplication, Native: Box<NativeImport>, Operation: Box<OperationWitness>,
    Construction: Box<ConstructionWitness>, Layout: Ty, Unknown: UnknownReason,
});
variants!(Dependency => DependencyWire {
    Source: DefinitionId, Body: ConcreteFunctionIdentity, Unknown: UnknownDependency,
});

impl UpdateEvidenceRecord {
    pub fn encode(
        value: &UpdateEvidence,
        encoding: &DefinitionEncoding,
        budget: &mut Budget<'_>,
    ) -> Result<Self, UpdateError> {
        value.encode_value(encoding, budget)
    }

    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        budget: &mut Budget<'_>,
    ) -> Result<UpdateEvidence, UpdateError> {
        UpdateEvidence::decode_value(self, decoding, budget)
    }
}

impl DemandEnvelopeRecord {
    pub fn encode(
        value: &DemandEnvelope,
        encoding: &DefinitionEncoding,
        budget: &mut Budget<'_>,
    ) -> Result<Self, UpdateError> {
        value.encode_value(encoding, budget)
    }

    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        budget: &mut Budget<'_>,
    ) -> Result<DemandEnvelope, UpdateError> {
        DemandEnvelope::decode_value(self, decoding, budget)
    }
}
