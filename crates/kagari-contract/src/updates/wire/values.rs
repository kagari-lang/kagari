use crate::updates::wire::shape::LeafShape;
use crate::{
    callable::{
        construction::evidence::ConstructionWitness,
        generic::GenericBody,
        interface::InterfaceCallContract,
        shared::SharedCall,
        wire::{
            GenericBodyRecord, InterfaceCallContractRecord, SharedCallRecord,
            evidence::EvidenceRecord,
        },
        witness::OperationWitness,
    },
    ids::FunctionRef,
    native_import::{NativeImport, wire::NativeImportRecord},
    source::{manifest::SourceManifest, wire::SourceManifestRecord},
    types::{ConcreteFunctionIdentity, wire::module::ConcreteFunctionIdentityRecord},
    updates::{
        UnknownReason,
        limits::{Budget, MAX_RECORDS, UpdateError},
    },
};
use bincode::{DefaultOptions, Options};
use kagari_common::identity::{
    mapping::DefinitionMappingError,
    table::{
        DefinitionId,
        wire::{DefinitionDecoding, DefinitionEncoding, PortableDefinitionRef},
    },
};
use kagari_types::{
    callable::Signature,
    declaration::wire::native::SignatureRecord,
    ty::{Ty, wire::WireType},
};
use serde::{Serialize, de::DeserializeOwned};

pub(super) trait WireValue: Sized {
    type Record: Serialize + DeserializeOwned;

    fn encode_value(
        &self,
        encoding: &DefinitionEncoding,
        budget: &mut Budget<'_>,
    ) -> Result<Self::Record, UpdateError>;

    fn decode_value(
        value: Self::Record,
        decoding: &DefinitionDecoding,
        budget: &mut Budget<'_>,
    ) -> Result<Self, UpdateError>;
}

fn size(value: &impl Serialize, budget: &mut Budget<'_>) -> Result<(), UpdateError> {
    let bytes = DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
        .serialized_size(value)
        .map_err(|_| UpdateError::InvalidEvidence)?;
    budget.bytes(usize::try_from(bytes).map_err(|_| UpdateError::LimitExceeded)?)
}

impl WireValue for SourceManifest {
    type Record = SourceManifestRecord;

    fn encode_value(
        &self,
        encoding: &DefinitionEncoding,
        budget: &mut Budget<'_>,
    ) -> Result<Self::Record, UpdateError> {
        self.validate_limits(budget.cancel)?;
        budget.records(self.documents.len() + self.declarations.len())?;
        let record = SourceManifestRecord::encode(self, encoding, budget.cancel)?;
        size(&record, budget)?;
        Ok(record)
    }

    fn decode_value(
        value: Self::Record,
        decoding: &DefinitionDecoding,
        budget: &mut Budget<'_>,
    ) -> Result<Self, UpdateError> {
        budget.check()?;
        size(&value, budget)?;
        let result = value.decode(decoding, budget.cancel)?;
        budget.records(result.documents.len() + result.declarations.len())?;
        Ok(result)
    }
}

macro_rules! plain {
    ($($ty:ty),*) => { $(
        impl WireValue for $ty {
            type Record = Self;

            fn encode_value(&self, _: &DefinitionEncoding, budget: &mut Budget<'_>) -> Result<Self, UpdateError> {
                size(self, budget)?;
                Ok(self.clone())
            }

            fn decode_value(value: Self, _: &DefinitionDecoding, budget: &mut Budget<'_>) -> Result<Self, UpdateError> {
                size(&value, budget)?;
                Ok(value)
            }
        }
    )* };
}

plain!(u8, u32, u64, i64, String, FunctionRef, UnknownReason);

impl WireValue for DefinitionId {
    type Record = PortableDefinitionRef;

    fn encode_value(
        &self,
        encoding: &DefinitionEncoding,
        budget: &mut Budget<'_>,
    ) -> Result<Self::Record, UpdateError> {
        budget.bytes(4)?;
        encoding
            .reference(*self)
            .map_err(DefinitionMappingError::from)
            .map_err(Into::into)
    }

    fn decode_value(
        value: Self::Record,
        decoding: &DefinitionDecoding,
        budget: &mut Budget<'_>,
    ) -> Result<Self, UpdateError> {
        budget.bytes(4)?;
        decoding
            .resolve(value)
            .map_err(DefinitionMappingError::from)
            .map_err(Into::into)
    }
}

impl<T: WireValue> WireValue for Vec<T> {
    type Record = Vec<T::Record>;

    fn encode_value(
        &self,
        encoding: &DefinitionEncoding,
        budget: &mut Budget<'_>,
    ) -> Result<Self::Record, UpdateError> {
        if self.len() > MAX_RECORDS {
            return Err(UpdateError::LimitExceeded);
        }
        budget.records(self.len())?;
        budget.bytes(8)?;
        self.iter()
            .map(|value| value.encode_value(encoding, budget))
            .collect()
    }

    fn decode_value(
        value: Self::Record,
        decoding: &DefinitionDecoding,
        budget: &mut Budget<'_>,
    ) -> Result<Self, UpdateError> {
        if value.len() > MAX_RECORDS {
            return Err(UpdateError::LimitExceeded);
        }
        budget.records(value.len())?;
        budget.bytes(8)?;
        value
            .into_iter()
            .map(|value| T::decode_value(value, decoding, budget))
            .collect()
    }
}

impl<T: WireValue> WireValue for Option<T> {
    type Record = Option<T::Record>;

    fn encode_value(
        &self,
        encoding: &DefinitionEncoding,
        budget: &mut Budget<'_>,
    ) -> Result<Self::Record, UpdateError> {
        budget.bytes(1)?;
        self.as_ref()
            .map(|value| value.encode_value(encoding, budget))
            .transpose()
    }

    fn decode_value(
        value: Self::Record,
        decoding: &DefinitionDecoding,
        budget: &mut Budget<'_>,
    ) -> Result<Self, UpdateError> {
        budget.bytes(1)?;
        value
            .map(|value| T::decode_value(value, decoding, budget))
            .transpose()
    }
}

impl<T: WireValue> WireValue for Box<T> {
    type Record = Box<T::Record>;

    fn encode_value(
        &self,
        encoding: &DefinitionEncoding,
        budget: &mut Budget<'_>,
    ) -> Result<Self::Record, UpdateError> {
        Ok(Box::new(self.as_ref().encode_value(encoding, budget)?))
    }

    fn decode_value(
        value: Self::Record,
        decoding: &DefinitionDecoding,
        budget: &mut Budget<'_>,
    ) -> Result<Self, UpdateError> {
        Ok(Box::new(T::decode_value(*value, decoding, budget)?))
    }
}

macro_rules! leaf {
    ($model:ty, $wire:ty, $encode:ident, $decode:ident) => {
        impl WireValue for $model {
            type Record = $wire;

            fn encode_value(
                &self,
                encoding: &DefinitionEncoding,
                budget: &mut Budget<'_>,
            ) -> Result<Self::Record, UpdateError> {
                self.check_leaf(budget)?;
                budget.records(1)?;
                let value = <$wire>::$encode(self, encoding, budget.cancel)
                    .map_err(DefinitionMappingError::from)?;
                size(&value, budget)?;
                Ok(value)
            }

            fn decode_value(
                value: Self::Record,
                decoding: &DefinitionDecoding,
                budget: &mut Budget<'_>,
            ) -> Result<Self, UpdateError> {
                budget.records(1)?;
                size(&value, budget)?;
                let decoded = value
                    .$decode(decoding, budget.cancel)
                    .map_err(DefinitionMappingError::from)?;
                decoded.check_leaf(budget)?;
                Ok(decoded)
            }
        }
    };
}

leaf!(Ty, WireType, encode, decode);
leaf!(Signature, SignatureRecord, encode, decode);
leaf!(
    ConcreteFunctionIdentity,
    ConcreteFunctionIdentityRecord,
    encode,
    decode
);
leaf!(GenericBody, GenericBodyRecord, encode, decode);
leaf!(SharedCall, SharedCallRecord, encode, decode);
leaf!(
    InterfaceCallContract,
    InterfaceCallContractRecord,
    encode,
    decode
);
leaf!(NativeImport, NativeImportRecord, encode, decode);
leaf!(
    OperationWitness,
    EvidenceRecord,
    encode_operation,
    decode_operation
);
leaf!(
    ConstructionWitness,
    EvidenceRecord,
    encode_construction,
    decode_construction
);
