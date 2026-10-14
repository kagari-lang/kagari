//! Transactional nominal schema registration uses ordinary portable declarations.
use crate::{
    error::RuntimeError,
    native::{
        binding::NativeResult, builder::ModuleBuilder, catalog::DeclarationCatalog,
        conversion::KagariType, payload::Opaque, storage::NativePayload, types::Type,
    },
};
use kagari_types::{
    declaration::{FieldDef, TypeDef, TypeDefKind, VariantDef},
    ty::{NominalTy, Ty},
    visibility::Visibility,
};
use std::{
    any::{TypeId, type_name},
    collections::{BTreeMap, HashSet},
    sync::Arc,
};

/// An exported DTO field. Writable script storage is independent of the source
/// Rust value. This descriptor supplies no Rust method or field projector.
pub struct ValueField {
    pub name: String,
    pub ty: Type,
}

impl ValueField {
    pub fn new(name: impl Into<String>, ty: Type) -> Self {
        Self {
            name: name.into(),
            ty,
        }
    }
}

/// A unit or tuple-payload DTO enum variant.
pub struct ValueVariant {
    pub name: String,
    pub payload: Vec<Type>,
}

impl ValueVariant {
    pub fn new(name: impl Into<String>, payload: impl IntoIterator<Item = Type>) -> Self {
        Self {
            name: name.into(),
            payload: payload.into_iter().collect(),
        }
    }
}

/// One staged graph. Headers are reserved before child dependencies are visited,
/// allowing cycles through supported composites without registration-order rules.
/// Failure discards the entire staged module; no usable partial schema is published.
pub struct TypeRegistration<'module> {
    module: &'module mut ModuleBuilder,
    depth: usize,
    unfinished: HashSet<usize>,
}

impl ModuleBuilder {
    /// Register a Rust mapping and its schema dependencies atomically. Explicit
    /// duplicate registrations reject; repeated graph edges reuse the same header.
    ///
    /// Dedicated derives reject open generic factories and borrowed data:
    /// ```compile_fail
    /// use kagari_derive::KagariValue;
    /// #[derive(KagariValue)]
    /// struct Open<T> { value: T }
    /// ```
    /// ```compile_fail
    /// use kagari_derive::KagariValue;
    /// #[derive(KagariValue)]
    /// struct Borrowed<'a> { text: &'a str }
    /// ```
    /// Structs must have named fields:
    /// ```compile_fail
    /// use kagari_derive::KagariValue;
    /// #[derive(KagariValue)]
    /// struct Tuple(i32);
    /// ```
    /// Packed structs and union/record-enum representations are outside the subset:
    /// ```compile_fail
    /// use kagari_derive::KagariValue;
    /// #[derive(KagariValue)]
    /// #[repr(packed)]
    /// struct Packed { value: i32 }
    /// ```
    /// ```compile_fail
    /// use kagari_derive::KagariValue;
    /// #[derive(KagariValue)]
    /// union Union { integer: i32, float: f32 }
    /// ```
    /// ```compile_fail
    /// use kagari_derive::KagariValue;
    /// #[derive(KagariValue)]
    /// enum Record { Entry { value: i32 } }
    /// ```
    /// Unsupported scalar conversions cannot silently change their script type:
    /// ```compile_fail
    /// use kagari_derive::KagariValue;
    /// #[derive(KagariValue)]
    /// struct Wide { value: i128 }
    /// ```
    /// ```compile_fail
    /// use kagari_derive::KagariValue;
    /// #[derive(KagariValue)]
    /// struct Character { value: char }
    /// ```
    pub fn register_type<T: KagariType + 'static>(&mut self) -> NativeResult<Type> {
        if self.providers.has_rust_type::<T>() {
            return Err(conflict("duplicate Rust type registration"));
        }
        let mut staged = self.clone();
        let ty = {
            let mut registration = TypeRegistration {
                module: &mut staged,
                depth: 0,
                unfinished: HashSet::new(),
            };
            let ty = registration.dependency::<T>()?;
            if !registration.unfinished.is_empty() {
                return Err(conflict("incomplete schema dependency"));
            }
            ty
        };
        let definitions = staged
            .declaration
            .collect_definitions(staged.context())
            .map_err(conflict)?;
        let owners = staged
            .providers
            .receiver_owners(Some(&staged.declaration))?;
        staged
            .declaration
            .validate(
                &|receiver| owners.owner(receiver, &definitions),
                &definitions,
            )
            .map_err(conflict)?;
        // Validate the complete graph, including missing nominal dependencies.
        staged
            .providers
            .dependency_closure([], [], [&staged.declaration])?;
        *self = staged;
        Ok(ty)
    }
}

impl TypeRegistration<'_> {
    /// Register an Opaque dependency through the ordinary native type builder.
    /// Rust's short named type supplies only a default name, never methods or
    /// traits. Explicit nominal applications continue using their checked binding.
    pub fn opaque<T: NativePayload>(&mut self) -> NativeResult<Type> {
        let rust_name = type_name::<T>();
        let name = rust_name.rsplit_once("::").map(|(_, name)| name)
            .ok_or_else(|| conflict("automatic Opaque registration requires a named Rust payload; use explicit native declaration for primitive or generic storage"))?;
        if !name
            .chars()
            .all(|character| character.is_alphanumeric() || character == '_')
        {
            return Err(conflict(
                "automatic Opaque registration requires a finite named payload",
            ));
        }
        let storage = T::registration_storage();
        if !storage.accepts_payload::<T>() {
            return Err(conflict(
                "automatic Opaque storage policy differs from its Rust payload",
            ));
        }
        let mut declaration = self.module.define_type(name)?;
        declaration.native_storage(storage)?;
        let ty = declaration.finish()?.apply([])?;
        self.module
            .providers
            .insert_rust_type::<Opaque<T>>(ty.clone())?;
        Ok(ty)
    }

    pub fn catalog(&self) -> &DeclarationCatalog {
        &self.module.providers
    }

    /// Resolve an already registered dependency or visit its registration hook.
    pub fn dependency<T: KagariType + 'static>(&mut self) -> NativeResult<Type> {
        if self.module.providers.has_rust_type::<T>() {
            self.module.providers.rust_type::<T>()
        } else {
            if self.depth >= 64 {
                return Err(conflict("schema dependency depth limit"));
            }
            self.depth += 1;
            let result = T::register_type(self);
            self.depth -= 1;
            result
        }
    }

    /// Declare an ordinary named struct, reserving its identity before fields.
    pub fn structure<T: KagariType + 'static>(
        &mut self,
        name: &str,
        fields: impl FnOnce(&mut Self) -> NativeResult<Vec<ValueField>>,
    ) -> NativeResult<Type> {
        let (slot, ty) = self.header::<T>(name, TypeDefKind::Struct)?;
        let fields = fields(self)?;
        let fields = fields
            .into_iter()
            .map(|field| {
                Ok(FieldDef {
                    visibility: Visibility::Public,
                    name: field.name,
                    ty: field.ty.adopt(self.module.context())?,
                    mutable: true,
                })
            })
            .collect::<NativeResult<Vec<_>>>()?;
        self.module.declaration.types[slot].fields = fields;
        self.publish(slot, &ty)?;
        if let Some(projectors) = T::generated_fields() {
            let fields = &self.module.declaration.types[slot].fields;
            if fields.len() != projectors.len() {
                return Err(conflict(
                    "generated Rust fields differ from the struct schema",
                ));
            }
            for (field, projector) in fields.iter().zip(&projectors) {
                let target = (projector.target_type)(&self.module.providers)?;
                if projector.root != TypeId::of::<T>()
                    || projector.name != field.name
                    || target.adopt(self.module.context())? != field.ty
                {
                    return Err(conflict(
                        "generated Rust field identity or type differs from its schema",
                    ));
                }
            }
            Arc::make_mut(&mut self.module.providers.rust_types)
                .get_mut(&TypeId::of::<T>())
                .expect("reserved Rust schema header")
                .fields = Some(projectors.into());
        }
        Ok(ty)
    }

    /// Declare unit/tuple variants; no sample value is needed to discover shape.
    pub fn enumeration<T: 'static>(
        &mut self,
        name: &str,
        variants: impl FnOnce(&mut Self) -> NativeResult<Vec<ValueVariant>>,
    ) -> NativeResult<Type> {
        let (slot, ty) = self.header::<T>(name, TypeDefKind::Enum)?;
        let variants = variants(self)?;
        self.module.declaration.types[slot].variants = variants
            .into_iter()
            .map(|variant| {
                Ok(VariantDef {
                    reports_failure: false,
                    name: variant.name,
                    payload: variant
                        .payload
                        .into_iter()
                        .map(|ty| ty.adopt(self.module.context()))
                        .collect::<NativeResult<_>>()?,
                })
            })
            .collect::<NativeResult<_>>()?;
        self.publish(slot, &ty)?;
        Ok(ty)
    }

    fn header<T: 'static>(&mut self, name: &str, kind: TypeDefKind) -> NativeResult<(usize, Type)> {
        if name.is_empty() || name.contains("::") || name.contains('.') {
            return Err(conflict("schema requires a short type name"));
        }
        if self
            .module
            .declaration
            .types
            .iter()
            .any(|ty| ty.name == name)
        {
            return Err(conflict("duplicate schema declaration"));
        }
        let id = self.module.definition(kind.definition_kind(), name)?;
        let nominal = NominalTy {
            declaration: id,
            arguments: vec![],
            associated_types: BTreeMap::new(),
        };
        let expression = match kind {
            TypeDefKind::Struct => Ty::Struct(nominal),
            TypeDefKind::Enum => Ty::Enum(nominal),
            _ => return Err(conflict("unsupported value schema kind")),
        };
        let ty = Type::from_semantic(expression, &self.module.providers.definitions())?;
        self.module.providers.insert_rust_type::<T>(ty.clone())?;
        let slot = self.module.declaration.types.len();
        self.unfinished.insert(slot);
        self.module.declaration.types.push(TypeDef {
            name: name.into(),
            kind,
            generic_params: vec![],
            bounds: vec![],
            fields: vec![],
            variants: vec![],
        });
        Ok((slot, ty))
    }

    fn publish(&mut self, slot: usize, ty: &Type) -> NativeResult<()> {
        let id = match ty.abi() {
            Ty::Struct(nominal) | Ty::Enum(nominal) => nominal.declaration,
            _ => return Err(conflict("invalid schema header")),
        };
        self.module
            .providers
            .insert_type(id, self.module.declaration.types[slot].clone())?;
        self.unfinished.remove(&slot);
        Ok(())
    }
}

fn conflict(error: impl ToString) -> RuntimeError {
    RuntimeError::metadata_conflict(error.to_string())
}
