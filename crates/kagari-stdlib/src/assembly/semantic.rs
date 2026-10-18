//! Semantic projections own symbolic enum handles, never runtime objects or roots.
use kagari_common::identity::{DefinitionKind, DefinitionPath};
use kagari_runtime::{
    error::RuntimeError,
    native::{
        assembly::DeclarationSnapshot,
        binding::NativeResult,
        types::{TypeRef, VariantRef},
    },
};
use kagari_types::declaration::TypeDefKind;
use std::collections::BTreeMap;

/// Independently retained declarations and enum members from one sealed snapshot.
/// Future StandardDeclarations consumers keep this value, not the parent Bundle.
/// Handles contain only declaration identity/metadata; allocation still resolves
/// the invoking runtime's generation-pinned layout through its checked call API.
#[derive(Debug)]
pub struct Semantic {
    snapshot: DeclarationSnapshot,
    enumerations: BTreeMap<DefinitionPath, Enumeration>,
}

#[derive(Debug)]
struct Enumeration {
    declaration: TypeRef,
    variants: BTreeMap<String, VariantRef>,
}

impl Semantic {
    pub(super) fn new(snapshot: DeclarationSnapshot) -> NativeResult<Self> {
        let mut enumerations = BTreeMap::new();
        for module in snapshot.modules() {
            for ty in &module.records().types {
                if ty.kind != TypeDefKind::Enum {
                    continue;
                }
                let id = module
                    .records()
                    .definition(DefinitionKind::Enum, &ty.name, module.definitions())
                    .map_err(conflict)?;
                let path = module
                    .definitions()
                    .resolve(id)
                    .map_err(conflict)?
                    .to_path();
                let declaration = snapshot.catalog().type_reference(&path)?;
                let variants = ty
                    .variants
                    .iter()
                    .map(|variant| {
                        declaration
                            .variant(&variant.name)
                            .map(|handle| (variant.name.clone(), handle))
                    })
                    .collect::<NativeResult<_>>()?;
                enumerations.insert(
                    path,
                    Enumeration {
                        declaration,
                        variants,
                    },
                );
            }
        }
        Ok(Self {
            snapshot,
            enumerations,
        })
    }

    pub fn snapshot(&self) -> &DeclarationSnapshot {
        &self.snapshot
    }

    /// Resolve an owned enum by canonical identity, without entering any cache.
    pub fn enumeration(&self, path: &DefinitionPath) -> NativeResult<&TypeRef> {
        self.enumerations
            .get(path)
            .map(|enumeration| &enumeration.declaration)
            .ok_or_else(|| conflict("stdlib enum is not declared by this semantic snapshot"))
    }

    /// Borrow a symbolic variant; this does not select or retain a runtime layout.
    pub fn variant(&self, path: &DefinitionPath, member: &str) -> NativeResult<&VariantRef> {
        self.enumerations
            .get(path)
            .and_then(|enumeration| enumeration.variants.get(member))
            .ok_or_else(|| {
                conflict("stdlib enum variant is not declared by this semantic snapshot")
            })
    }
}

fn conflict(error: impl ToString) -> RuntimeError {
    RuntimeError::metadata_conflict(error.to_string())
}
