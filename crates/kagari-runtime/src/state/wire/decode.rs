use crate::state::{
    data::{
        NominalKey, SchemaVersion, SnapshotHeader, StateDefinition, StateField, StateScalar,
        StateSchema, StateShape, StateSnapshot, StateType, StateValue, StateVariant,
    },
    error::{PathComponent, SnapshotError, SnapshotErrorKind, SnapshotPath, SnapshotResult},
    limits::{BYTE_CHUNK, Budget},
    wire::options,
};
use bincode::Options;
use serde::{
    Deserialize, Deserializer,
    de::{DeserializeSeed, Error, SeqAccess, Visitor},
};
use std::{fmt, marker::PhantomData, mem::size_of, str};

struct BodySeed<'a> {
    budget: &'a mut Budget,
    failure: &'a mut Option<SnapshotError>,
}

impl<'de> DeserializeSeed<'de> for BodySeed<'_> {
    type Value = StateSnapshot;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        // A streaming tuple cursor lets this visitor admit every explicit bincode
        // u64 count before reserving; no derived recursive decoder is involved.
        deserializer.deserialize_tuple(usize::MAX, self)
    }
}

impl<'de> Visitor<'de> for BodySeed<'_> {
    type Value = StateSnapshot;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("bounded state body")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
        let mut reader = Reader {
            seq,
            budget: self.budget,
            path: SnapshotPath::root(),
            marker: PhantomData,
        };
        match reader.snapshot() {
            Ok(value) => Ok(value),
            Err(error) => {
                *self.failure = Some(error);
                Err(Error::custom("state body rejected"))
            }
        }
    }
}

struct Reader<'a, 'de, A> {
    seq: A,
    budget: &'a mut Budget,
    path: SnapshotPath,
    marker: PhantomData<&'de [u8]>,
}

impl<'de, A: SeqAccess<'de>> Reader<'_, 'de, A> {
    fn next<T: Deserialize<'de>>(&mut self) -> SnapshotResult<T> {
        self.budget.work(size_of::<T>().max(1) as u64)?;
        self.seq
            .next_element()
            .map_err(|error| {
                self.budget
                    .error(SnapshotErrorKind::InvalidWire, error.to_string())
                    .at(&self.path)
            })?
            .ok_or_else(|| {
                self.budget
                    .error(SnapshotErrorKind::InvalidWire, "truncated body")
                    .at(&self.path)
            })
    }

    fn count(
        &mut self,
        schema: bool,
        maximum: usize,
        limit: &'static str,
    ) -> SnapshotResult<usize> {
        let length: u64 = self.next()?;
        let length =
            usize::try_from(length).map_err(|_| SnapshotError::limit(self.budget.stage, limit))?;
        self.budget.count(length, maximum, limit)?;
        self.budget.admit_children(length, schema)?;
        Ok(length)
    }

    fn text(&mut self, identifier: bool) -> SnapshotResult<String> {
        // Bincode strings and byte slices share the u64 length + byte framing.
        // Borrow bytes so bincode cannot scan UTF-8 before our length/work checks.
        let mut bytes: &'de [u8] = self.next()?;
        self.budget.string_len(bytes.len(), identifier)?;
        let mut output = String::new();
        output.try_reserve_exact(bytes.len()).map_err(|_| {
            self.budget
                .error(SnapshotErrorKind::Allocation, "string allocation failed")
        })?;
        while !bytes.is_empty() {
            let chunk = text_chunk(bytes, self.budget)?;
            // string_len prepaid the copy; text_chunk charges UTF-8 inspection.
            // Poll between inspection and copying, each at most BYTE_CHUNK bytes.
            self.budget.cancel()?;
            output.push_str(chunk);
            bytes = &bytes[chunk.len()..];
        }
        Ok(output)
    }

    fn version(&mut self) -> SnapshotResult<SchemaVersion> {
        let id = self.text(true)?;
        let version = self.next()?;
        if id.is_empty() || version == 0 {
            return Err(self.budget.error(
                SnapshotErrorKind::InvalidSchema,
                "schema ID must be nonempty and version nonzero",
            ));
        }
        Ok(SchemaVersion { id, version })
    }

    fn names(&mut self, depth: usize) -> SnapshotResult<Vec<String>> {
        let length = self.count(true, self.budget.limits.max_sequence_len, "sequence_len")?;
        let mut values = Vec::new();
        self.budget.reserve(&mut values, length)?;
        for _ in 0..length {
            self.budget.schema_node(depth)?;
            values.push(self.text(true)?);
        }
        Ok(values)
    }

    fn key(&mut self, depth: usize) -> SnapshotResult<NominalKey> {
        self.budget.schema_node(depth)?;
        let package = self.text(true)?;
        let module = self.names(depth)?;
        let declaration = self.names(depth)?;
        let arguments = self.types(depth + 1, self.budget.limits.max_sequence_len)?;
        Ok(NominalKey {
            package,
            module,
            declaration,
            arguments,
        })
    }

    fn scalar(&mut self) -> SnapshotResult<StateScalar> {
        match self.next::<u32>()? {
            0 => Ok(StateScalar::Unit),
            1 => Ok(StateScalar::Bool),
            2 => Ok(StateScalar::Char),
            3 => Ok(StateScalar::I8),
            4 => Ok(StateScalar::I16),
            5 => Ok(StateScalar::I32),
            6 => Ok(StateScalar::I64),
            7 => Ok(StateScalar::U8),
            8 => Ok(StateScalar::U16),
            9 => Ok(StateScalar::U32),
            10 => Ok(StateScalar::U64),
            11 => Ok(StateScalar::F32),
            12 => Ok(StateScalar::F64),
            13 => Ok(StateScalar::String),
            _ => Err(self
                .budget
                .error(SnapshotErrorKind::InvalidSchema, "unknown scalar tag")),
        }
    }

    fn ty(&mut self, depth: usize) -> SnapshotResult<StateType> {
        self.budget.schema_node(depth)?;
        match self.next::<u32>()? {
            0 => Ok(StateType::Scalar(self.scalar()?)),
            1 => Ok(StateType::Tuple(
                self.types(depth + 1, self.budget.limits.max_sequence_len)?,
            )),
            2 => Ok(StateType::Sequence(Box::new(self.ty(depth + 1)?))),
            3 => Ok(StateType::Nominal(self.key(depth)?)),
            _ => Err(self
                .budget
                .error(SnapshotErrorKind::InvalidSchema, "unknown type tag")),
        }
    }

    fn types(&mut self, depth: usize, maximum: usize) -> SnapshotResult<Vec<StateType>> {
        let length = self.count(true, maximum, "schema_children")?;
        if length != 0 {
            self.budget.depth(depth)?;
        }
        let mut values = Vec::new();
        self.budget.reserve(&mut values, length)?;
        for _ in 0..length {
            values.push(self.ty(depth)?);
        }
        Ok(values)
    }

    fn definition(&mut self) -> SnapshotResult<StateDefinition> {
        self.budget.schema_node(1)?;
        let key = self.key(1)?;
        let shape = match self.next::<u32>()? {
            0 => {
                let length = self.count(true, self.budget.limits.max_fields, "fields")?;
                let mut fields = Vec::new();
                self.budget.reserve(&mut fields, length)?;
                for _ in 0..length {
                    self.budget.schema_node(1)?;
                    fields.push(StateField {
                        name: self.text(true)?,
                        ty: self.ty(1)?,
                    });
                }
                StateShape::Struct(fields)
            }
            1 => {
                let length = self.count(true, self.budget.limits.max_fields, "variants")?;
                let mut variants = Vec::new();
                self.budget.reserve(&mut variants, length)?;
                for _ in 0..length {
                    self.budget.schema_node(1)?;
                    variants.push(StateVariant {
                        name: self.text(true)?,
                        fields: self.types(1, self.budget.limits.max_fields)?,
                    });
                }
                StateShape::Enum(variants)
            }
            _ => {
                return Err(self
                    .budget
                    .error(SnapshotErrorKind::InvalidSchema, "unknown shape tag"));
            }
        };
        Ok(StateDefinition { key, shape })
    }

    fn values(&mut self, depth: usize, maximum: usize) -> SnapshotResult<Vec<StateValue>> {
        let length = self.count(false, maximum, "sequence_len")?;
        if length != 0 {
            self.budget.depth(depth)?;
        }
        let mut values = Vec::new();
        self.budget.reserve(&mut values, length)?;
        for index in 0..length {
            self.path.push(PathComponent::Index(index));
            let value = self.value(depth);
            self.path.pop();
            values.push(value?);
        }
        Ok(values)
    }

    fn value(&mut self, depth: usize) -> SnapshotResult<StateValue> {
        let result = self.value_inner(depth);
        result.map_err(|error| {
            if error.path.0.is_empty() {
                error.at(&self.path)
            } else {
                error
            }
        })
    }

    fn value_inner(&mut self, depth: usize) -> SnapshotResult<StateValue> {
        self.budget.node(depth)?;
        Ok(match self.next::<u32>()? {
            0 => StateValue::Unit,
            1 => StateValue::Bool(self.next()?),
            2 => StateValue::Char(char::from_u32(self.next()?).ok_or_else(|| {
                self.budget
                    .error(SnapshotErrorKind::InvalidWire, "invalid Unicode scalar")
            })?),
            3 => StateValue::I8(self.next()?),
            4 => StateValue::I16(self.next()?),
            5 => StateValue::I32(self.next()?),
            6 => StateValue::I64(self.next()?),
            7 => StateValue::U8(self.next()?),
            8 => StateValue::U16(self.next()?),
            9 => StateValue::U32(self.next()?),
            10 => StateValue::U64(self.next()?),
            11 => StateValue::F32Bits(self.next()?),
            12 => StateValue::F64Bits(self.next()?),
            13 => StateValue::String(self.text(false)?),
            14 => StateValue::Tuple(self.values(depth + 1, self.budget.limits.max_sequence_len)?),
            15 => {
                StateValue::Sequence(self.values(depth + 1, self.budget.limits.max_sequence_len)?)
            }
            16 => {
                let key = self.key(1)?;
                let length = self.count(false, self.budget.limits.max_fields, "fields")?;
                if length != 0 {
                    self.budget.depth(depth + 1)?;
                }
                let mut fields = Vec::new();
                self.budget.reserve(&mut fields, length)?;
                for _ in 0..length {
                    let name = self.text(true)?;
                    self.path.push(PathComponent::Field(name.clone()));
                    let value = self.value(depth + 1);
                    self.path.pop();
                    fields.push((name, value?));
                }
                StateValue::Record { key, fields }
            }
            17 => {
                let key = self.key(1)?;
                let variant = self.text(true)?;
                self.path.push(PathComponent::Variant(variant.clone()));
                let fields = self.values(depth + 1, self.budget.limits.max_fields);
                self.path.pop();
                StateValue::Enum {
                    key,
                    variant,
                    fields: fields?,
                }
            }
            _ => {
                return Err(self
                    .budget
                    .error(SnapshotErrorKind::InvalidWire, "unknown payload tag"));
            }
        })
    }

    fn snapshot(&mut self) -> SnapshotResult<StateSnapshot> {
        let header = SnapshotHeader {
            schema: self.version()?,
            source_deployment: self.text(true)?,
            state_owner: self.text(true)?,
            state_revision: self.next()?,
        };
        let version = self.version()?;
        let root = self.ty(1)?;
        let length = self.count(true, self.budget.limits.max_definitions, "definitions")?;
        let mut definitions = Vec::new();
        self.budget.reserve(&mut definitions, length)?;
        for index in 0..length {
            self.path.push(PathComponent::Definition(index));
            let definition = self.definition().map_err(|error| error.at(&self.path));
            self.path.pop();
            definitions.push(definition?);
        }
        let schema = StateSchema {
            version,
            root,
            definitions,
        };
        let payload = self.value(1)?;
        self.budget.reset_data_counts();
        StateSnapshot::new(header, schema, payload, self.budget)
    }
}

/// Inspect one bounded prefix, leaving a split Unicode scalar for the next chunk.
/// Continuation-byte inspection only selects a boundary; UTF-8 validity is still
/// checked by str::from_utf8, including overlong encodings and malformed tails.
pub(super) fn text_chunk<'a>(bytes: &'a [u8], budget: &mut Budget) -> SnapshotResult<&'a str> {
    budget.cancel()?;
    let mut end = bytes.len().min(BYTE_CHUNK);
    if end < bytes.len() {
        // A valid UTF-8 scalar has at most three continuation bytes. Never scan
        // backward through an unbounded run of malicious continuation bytes.
        for _ in 0..3 {
            budget.work(1)?;
            if bytes[end] & 0xc0 != 0x80 {
                break;
            }
            end -= 1;
        }
    }
    budget.bytes(end)?;
    str::from_utf8(&bytes[..end])
        .map_err(|_| budget.error(SnapshotErrorKind::InvalidWire, "invalid UTF-8 string"))
}

pub(super) fn body(bytes: &[u8], budget: &mut Budget) -> SnapshotResult<StateSnapshot> {
    let mut failure = None;
    let decoded = options(bytes.len() as u64).deserialize_seed(
        BodySeed {
            budget,
            failure: &mut failure,
        },
        bytes,
    );
    if let Some(error) = failure {
        return Err(error);
    }
    decoded.map_err(|error| budget.error(SnapshotErrorKind::InvalidWire, error.to_string()))
}
