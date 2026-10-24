//! Bounded file-local source records. The enclosing artifact supplies the byte limit.
use crate::{
    source::{
        AuthoredDeclaration, AuthoredKind, ExecutableOrigin, ExecutableSource, MAX_SOURCE_BYTES,
        SourceDocument, SourcePackage,
        manifest::{SourceDocumentAnchor, SourceManifest},
    },
    types::wire::module::ConcreteFunctionIdentityRecord,
};
use bincode::{DefaultOptions, Options};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMappingError, check_cancel},
        table::wire::{DefinitionDecoding, DefinitionEncoding, PortableDefinitionRef},
    },
    span::Span,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct SourcePackageRecord {
    executable_fingerprint: u64,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    documents: Vec<SourceDocument>,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    declarations: Vec<DeclarationRecord>,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    executables: Vec<ExecutableRecord>,
}

/// File-local anchors embedded in the executable evidence, not the optional package.
#[derive(Serialize, Deserialize)]
pub struct SourceManifestRecord {
    #[serde(deserialize_with = "crate::decode_limits::table")]
    documents: Vec<SourceDocumentAnchor>,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    declarations: Vec<DeclarationRecord>,
}

impl SourceManifestRecord {
    pub fn encode(
        value: &SourceManifest,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        value.validate_limits(cancel)?;
        let mut result = Self {
            documents: value.documents.clone(),
            declarations: Vec::new(),
        };
        let mut remaining = MAX_SOURCE_BYTES as u64;
        charge(&result, &mut remaining)?;
        for declaration in &value.declarations {
            check_cancel(cancel)?;
            let record = DeclarationRecord::encode(declaration, encoding)?;
            charge(&record, &mut remaining)?;
            result.declarations.push(record);
        }
        Ok(result)
    }

    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<SourceManifest, DefinitionMappingError> {
        check_cancel(cancel)?;
        charge(&self, &mut (MAX_SOURCE_BYTES as u64))?;
        let declarations = self
            .declarations
            .into_iter()
            .map(|record| {
                check_cancel(cancel)?;
                record.decode(decoding)
            })
            .collect::<Result<_, _>>()?;
        let result = SourceManifest {
            documents: self.documents,
            declarations,
        };
        result.validate_limits(cancel)?;
        Ok(result)
    }
}

#[derive(Serialize, Deserialize)]
struct DeclarationRecord {
    declaration: PortableDefinitionRef,
    kind: AuthoredKind,
    document: u32,
    range: Span,
    fingerprint: u64,
}

impl DeclarationRecord {
    fn encode(
        value: &AuthoredDeclaration,
        encoding: &DefinitionEncoding,
    ) -> Result<Self, DefinitionMappingError> {
        Ok(Self {
            declaration: encoding.reference(value.declaration)?,
            kind: value.kind,
            document: value.document,
            range: value.range,
            fingerprint: value.fingerprint,
        })
    }

    fn decode(
        self,
        decoding: &DefinitionDecoding,
    ) -> Result<AuthoredDeclaration, DefinitionMappingError> {
        Ok(AuthoredDeclaration {
            declaration: decoding.resolve(self.declaration)?,
            kind: self.kind,
            document: self.document,
            range: self.range,
            fingerprint: self.fingerprint,
        })
    }
}

#[derive(Serialize, Deserialize)]
struct ExecutableRecord {
    target: ConcreteFunctionIdentityRecord,
    authored: Option<PortableDefinitionRef>,
    native_binding: Option<PortableDefinitionRef>,
    origin: ExecutableOrigin,
}

impl SourcePackageRecord {
    pub fn encode(
        value: &SourcePackage,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        value.validate_limits(cancel)?;
        let mut result = Self {
            executable_fingerprint: value.executable_fingerprint,
            documents: value.documents.clone(),
            declarations: Vec::new(),
            executables: Vec::new(),
        };
        let mut remaining = MAX_SOURCE_BYTES as u64;
        charge(&result, &mut remaining)?;
        for declaration in &value.declarations {
            check_cancel(cancel)?;
            let record = DeclarationRecord::encode(declaration, encoding)?;
            charge(&record, &mut remaining)?;
            result.declarations.push(record);
        }
        for executable in &value.executables {
            check_cancel(cancel)?;
            let record = ExecutableRecord {
                target: ConcreteFunctionIdentityRecord::encode(
                    &executable.target,
                    encoding,
                    cancel,
                )?,
                authored: executable
                    .authored
                    .map(|id| encoding.reference(id))
                    .transpose()?,
                native_binding: executable
                    .native_binding
                    .map(|id| encoding.reference(id))
                    .transpose()?,
                origin: executable.origin,
            };
            charge(&record, &mut remaining)?;
            result.executables.push(record);
        }
        Ok(result)
    }

    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<SourcePackage, DefinitionMappingError> {
        check_cancel(cancel)?;
        charge(&self, &mut (MAX_SOURCE_BYTES as u64))?;
        let declarations = self
            .declarations
            .into_iter()
            .map(|record| {
                check_cancel(cancel)?;
                record.decode(decoding)
            })
            .collect::<Result<_, DefinitionMappingError>>()?;
        let executables = self
            .executables
            .into_iter()
            .map(|record| {
                check_cancel(cancel)?;
                Ok(ExecutableSource {
                    target: record.target.decode(decoding, cancel)?,
                    authored: record.authored.map(|id| decoding.resolve(id)).transpose()?,
                    native_binding: record
                        .native_binding
                        .map(|id| decoding.resolve(id))
                        .transpose()?,
                    origin: record.origin,
                })
            })
            .collect::<Result<_, DefinitionMappingError>>()?;
        let result = SourcePackage {
            executable_fingerprint: self.executable_fingerprint,
            documents: self.documents,
            declarations,
            executables,
        };
        result.validate_limits(cancel)?;
        Ok(result)
    }
}

fn charge(value: &impl Serialize, remaining: &mut u64) -> Result<(), DefinitionMappingError> {
    let bytes = DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
        .with_limit(*remaining)
        .serialized_size(value)
        .map_err(|_| DefinitionMappingError::LimitExceeded)?;
    *remaining = remaining
        .checked_sub(bytes)
        .ok_or(DefinitionMappingError::LimitExceeded)?;
    Ok(())
}
