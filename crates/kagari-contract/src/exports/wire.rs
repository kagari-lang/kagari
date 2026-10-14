//! Bounded export records with explicit portable definition references.

use crate::exports::{ExportNamespace, MAX_EXPORT_RECORDS, ProgramExports};
use kagari_common::{
    cancellation::CancellationToken,
    decode_limits::bounded_vec,
    identity::{
        ModuleIdentity,
        mapping::{DefinitionMappingError, check_cancel},
        table::wire::{DefinitionDecoding, DefinitionEncoding, PortableDefinitionRef},
    },
};
use serde::{Deserialize, Deserializer, Serialize};

#[cfg(test)]
mod tests;

/// File-local facts only; decoding does not grant executable or public authority.
#[derive(Serialize, Deserialize)]
pub struct ProgramExportsRecord {
    #[serde(deserialize_with = "records")]
    namespaces: Vec<NamespaceRecord>,
    entry: Option<PortableDefinitionRef>,
}

#[derive(Serialize, Deserialize)]
struct NamespaceRecord {
    identity: ModuleIdentity,
    #[serde(deserialize_with = "records")]
    modules: Vec<(String, ModuleIdentity)>,
    #[serde(deserialize_with = "records")]
    functions: Vec<(String, PortableDefinitionRef)>,
}

fn records<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Vec<T>, D::Error> {
    bounded_vec(deserializer, MAX_EXPORT_RECORDS, "export record")
}

impl ProgramExportsRecord {
    /// Encode a bounded graph using the enclosing artifact's definition table.
    pub fn encode(
        value: &ProgramExports,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        value.check_shape(cancel)?;
        Ok(Self {
            namespaces: value
                .namespaces
                .iter()
                .map(|(identity, namespace)| {
                    check_cancel(cancel)?;
                    Ok(NamespaceRecord {
                        identity: identity.clone(),
                        modules: namespace
                            .modules
                            .iter()
                            .map(|(name, identity)| (name.clone(), identity.clone()))
                            .collect(),
                        functions: namespace
                            .functions
                            .iter()
                            .map(|(name, id)| {
                                check_cancel(cancel)?;
                                Ok((name.clone(), encoding.reference(*id)?))
                            })
                            .collect::<Result<_, DefinitionMappingError>>()?,
                    })
                })
                .collect::<Result<_, DefinitionMappingError>>()?,
            entry: value.entry.map(|id| encoding.reference(id)).transpose()?,
        })
    }

    /// Resolve references without silently overwriting duplicate namespace/name records.
    /// The Program verifier must subsequently check reachability and callable contracts.
    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<ProgramExports, DefinitionMappingError> {
        check_cancel(cancel)?;
        let mut result = ProgramExports {
            entry: self.entry.map(|id| decoding.resolve(id)).transpose()?,
            ..Default::default()
        };
        let mut count = self.namespaces.len() + usize::from(result.entry.is_some());
        for record in self.namespaces {
            check_cancel(cancel)?;
            count = count
                .checked_add(record.modules.len())
                .and_then(|count| count.checked_add(record.functions.len()))
                .filter(|count| *count <= MAX_EXPORT_RECORDS)
                .ok_or(DefinitionMappingError::LimitExceeded)?;
            let mut namespace = ExportNamespace::default();
            for (name, identity) in record.modules {
                check_cancel(cancel)?;
                if namespace.modules.insert(name, identity).is_some() {
                    return Err(DefinitionMappingError::InvalidContract);
                }
            }
            for (name, id) in record.functions {
                check_cancel(cancel)?;
                if namespace
                    .functions
                    .insert(name, decoding.resolve(id)?)
                    .is_some()
                {
                    return Err(DefinitionMappingError::InvalidContract);
                }
            }
            if result
                .namespaces
                .insert(record.identity, namespace)
                .is_some()
            {
                return Err(DefinitionMappingError::InvalidContract);
            }
        }
        result.check_shape(cancel)?;
        Ok(result)
    }
}
