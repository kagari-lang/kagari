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
/// StandardDeclarations queries this value, not pending work in the parent Bundle.
/// Handles contain only declaration identity/metadata; allocation still resolves
/// the invoking runtime's generation-pinned layout through its checked call API.
#[derive(Debug)]
pub struct Semantic {
    snapshot: DeclarationSnapshot,
    enumerations: BTreeMap<DefinitionPath, Enumeration>,
    // Preserve the existing short-name API without scanning modules on enum calls.
    // Ambiguous names remain valid declarations, but cannot use this convenience index.
    enum_names: BTreeMap<String, Option<DefinitionPath>>,
}

#[derive(Debug)]
struct Enumeration {
    declaration: TypeRef,
    variants: BTreeMap<String, VariantRef>,
}

impl Semantic {
    pub(super) fn new(snapshot: DeclarationSnapshot) -> NativeResult<Self> {
        let mut enumerations = BTreeMap::new();
        let mut enum_names = BTreeMap::new();
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
                enum_names
                    .entry(ty.name.clone())
                    .and_modify(|entry| *entry = None)
                    .or_insert_with(|| Some(path.clone()));
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
            enum_names,
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

    /// Resolve an unambiguous short enum name from this immutable snapshot.
    pub fn enumeration_named(&self, name: &str) -> NativeResult<&TypeRef> {
        let path = self
            .enum_names
            .get(name)
            .and_then(Option::as_ref)
            .ok_or_else(|| conflict("unknown or ambiguous standard enum"))?;
        self.enumeration(path)
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
