//! Catalog-local Rust mappings; never part of portable declarations or artifacts.
use crate::{
    error::RuntimeError,
    native::{binding::NativeResult, catalog::DeclarationCatalog, types::Type},
};
use kagari_types::ty::Ty;
use std::{any::TypeId, collections::HashMap, sync::Arc};

pub(crate) type RustTypes = Arc<HashMap<TypeId, Type>>;

impl DeclarationCatalog {
    /// Resolve a registered Rust representation in this catalog. Independent
    /// catalogs may register the same Rust type under independent module names.
    pub fn rust_type<T: 'static>(&self) -> NativeResult<Type> {
        self.rust_types
            .get(&TypeId::of::<T>())
            .cloned()
            .ok_or_else(|| RuntimeError::metadata_conflict("Rust type is not registered"))
    }

    pub(crate) fn has_rust_type<T: 'static>(&self) -> bool {
        self.rust_types.contains_key(&TypeId::of::<T>())
    }

    pub(crate) fn insert_rust_type<T: 'static>(&mut self, ty: Type) -> NativeResult<()> {
        let ty = Type::from_semantic(ty.adopt(self.types.context())?, &self.definitions())?;
        if self.has_rust_type::<T>() {
            return Err(RuntimeError::metadata_conflict(
                "duplicate Rust type registration",
            ));
        }
        Arc::make_mut(&mut self.rust_types).insert(TypeId::of::<T>(), ty);
        Ok(())
    }

    pub(crate) fn import_rust_types(&self, other: &Self) -> NativeResult<RustTypes> {
        let mut result = HashMap::new();
        for (rust, ty) in other.rust_types.iter() {
            let ty = Type::from_semantic(ty.adopt(self.types.context())?, &self.definitions())?;
            result.insert(*rust, ty);
        }
        Ok(Arc::new(result))
    }

    pub(crate) fn merge_rust_types(&mut self, other: &Self) -> NativeResult<()> {
        let imported = self.import_rust_types(other)?;
        for (rust, ty) in imported.iter() {
            if let Some(previous) = self.rust_types.get(rust)
                && previous.abi() != ty.abi()
            {
                return Err(RuntimeError::metadata_conflict(
                    "Rust type is already registered under another declaration",
                ));
            }
        }
        Arc::make_mut(&mut self.rust_types)
            .extend(imported.iter().map(|(rust, ty)| (*rust, ty.clone())));
        Ok(())
    }

    /// Retain mappings only for declaration records owned by this catalog view.
    pub(crate) fn copy_rust_types(&mut self, other: &Self) -> NativeResult<()> {
        let imported = self.import_rust_types(other)?;
        self.rust_types = Arc::new(
            imported
                .iter()
                .filter(|(_, ty)| match ty.abi() {
                    Ty::Struct(nominal) | Ty::Enum(nominal) | Ty::NativeObject(nominal) => {
                        self.types.get_id(nominal.declaration).is_some()
                    }
                    _ => false,
                })
                .map(|(rust, ty)| (*rust, ty.clone()))
                .collect(),
        );
        Ok(())
    }
}
