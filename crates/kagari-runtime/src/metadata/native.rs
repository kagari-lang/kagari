//! Prepare external reflection from the same checked declarations as native calls.
//! Preparation owns no heap values, external objects or Rust payloads.
use crate::{
    error::RuntimeError,
    metadata::{
        AbiFingerprint, FieldInfo, FieldMetadataId, TypeId, TypeInfo, TypeKind, TypeRegistry,
        TypeRegistryInner, VariantInfo, VariantMetadataId,
    },
    native::catalog::DeclarationCatalog,
};
use kagari_bytecode::artifact::ArtifactFingerprint;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        scoped::DefinitionReferences,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_contract::host::external;
use kagari_types::{
    declaration::{
        TypeDeclaration, TypeDef, TypeDefKind, host::PathAccess,
        wire::declarations::TypeDeclarationRecord,
    },
    scalar::BuiltinType,
    ty::{
        NominalTy, Ty,
        substitution::{MAX_TYPE_DEPTH, TypeSubstitution},
        wire::WireType,
    },
    visibility::Visibility,
};
use std::{cell::RefCell, collections::BTreeSet, fmt::Debug};

#[cfg(test)]
mod tests;

mod members;

/// Immutable prepared metadata. Runtime construction clones its value-only rows;
/// runtime-local installation prepares against that runtime's current rows.
#[derive(Debug, Clone, Default)]
pub(crate) struct MetadataSnapshot {
    inner: TypeRegistryInner,
}

impl MetadataSnapshot {
    pub(crate) fn runtime_registry(&self) -> TypeRegistry {
        TypeRegistry {
            inner: RefCell::new(self.inner.clone()),
        }
    }

    pub(crate) fn prepare(&self, catalog: &DeclarationCatalog) -> Result<Self, RuntimeError> {
        let definitions = catalog.definitions();
        let mut records = catalog
            .types
            .entries()
            .filter(|(_, definition)| matches!(definition.kind, TypeDefKind::Host(_)))
            .map(|(declaration, definition)| TypeDeclaration {
                declaration,
                definition: definition.clone(),
            })
            .collect::<Vec<_>>();
        records.sort_by(|a, b| {
            definitions
                .compare(a.declaration, b.declaration)
                .expect("checked native catalog identities")
        });
        external::validate(&records, &Default::default(), &definitions).map_err(conflict)?;
        let mut staged = self.inner.clone();
        // Reserve every root before building fields, including mutually recursive
        // schemas. No partially prepared rows can escape this local candidate.
        for record in &records {
            if let Some((previous, _)) = staged.native_external.get(&record.declaration) {
                if previous != &record.definition {
                    return Err(conflict("installed external metadata schema changed"));
                }
                continue;
            }
            let view = definitions.resolve(record.declaration).map_err(conflict)?;
            let name = format!("{}::{}", view.module(), record.definition.name);
            if staged.by_name.contains_key(&name) {
                return Err(conflict("external metadata name is already registered"));
            }
            let fingerprint =
                type_fingerprint(record.declaration, &record.definition, &definitions)?;
            let id = reserve(
                &mut staged,
                Ty::Host(record.declaration),
                name.clone(),
                TypeKind::HostObject,
                fingerprint,
            );
            staged.by_name.insert(name, id);
            staged
                .native_external
                .insert(record.declaration, (record.definition.clone(), id));
        }
        let mut builder = Builder {
            inner: &mut staged,
            catalog,
            definitions,
        };
        for record in &records {
            let id = builder.inner.native_external[&record.declaration].1;
            let fields = builder.fields(
                &record.definition,
                record.declaration,
                &TypeSubstitution::default(),
                0,
            )?;
            builder.inner.by_id[id.index()].fields = fields;
            builder.members(record.declaration, id)?;
        }
        Ok(Self { inner: staged })
    }
}

impl TypeRegistry {
    pub(crate) fn snapshot(&self) -> MetadataSnapshot {
        MetadataSnapshot {
            inner: self.inner.borrow().clone(),
        }
    }
}

struct Builder<'registry> {
    inner: &'registry mut TypeRegistryInner,
    catalog: &'registry DeclarationCatalog,
    definitions: DefinitionTable,
}

impl Builder<'_> {
    fn intern(&mut self, ty: &Ty, depth: usize) -> Result<TypeId, RuntimeError> {
        if depth > MAX_TYPE_DEPTH || !ty.within_wire_limits() || ty.contains_reference() {
            return Err(conflict(
                "external metadata requires a bounded owned value type",
            ));
        }
        if let Some(id) = self.inner.native_values.get(ty) {
            return Ok(*id);
        }
        let fingerprint = value_fingerprint(ty, &self.definitions)?;
        match ty {
            Ty::Builtin(kind) => {
                let name = scalar_name(*kind);
                if let Some(id) = self.inner.by_name.get(name).copied() {
                    if self.inner.by_id[id.index()].kind != TypeKind::Primitive {
                        return Err(conflict(
                            "builtin metadata name has a non-primitive binding",
                        ));
                    }
                    self.inner.native_values.insert(ty.clone(), id);
                    return Ok(id);
                }
                let id = reserve(
                    self.inner,
                    ty.clone(),
                    name.into(),
                    TypeKind::Primitive,
                    fingerprint,
                );
                self.inner.by_name.insert(name.into(), id);
                Ok(id)
            }
            Ty::Host(_) => Err(conflict("external metadata target is not installed")),
            Ty::Struct(nominal) | Ty::Enum(nominal) | Ty::NativeObject(nominal) => {
                self.nominal(ty, nominal, fingerprint, depth)
            }
            Ty::Tuple(elements) => {
                let id = self.composite(ty, TypeKind::Tuple, fingerprint);
                let mut fields = Vec::new();
                for (slot, element) in elements.iter().enumerate() {
                    fields.push(FieldInfo {
                        id: FieldMetadataId::new(slot),
                        name: slot.to_string(),
                        ty: self.intern(element, depth + 1)?,
                        readable: true,
                        writable: false,
                        visibility: Visibility::Public,
                        path_access: PathAccess::None,
                        abi_fingerprint: value_fingerprint(element, &self.definitions)?,
                    });
                }
                self.inner.by_id[id.index()].fields = fields;
                Ok(id)
            }
            Ty::Array(element)
            | Ty::Set(element, _)
            | Ty::Iter(element)
            | Ty::Range(element, _) => {
                let kind = match ty {
                    Ty::Array(_) => TypeKind::Array,
                    Ty::Set(..) => TypeKind::Set,
                    _ => TypeKind::Struct,
                };
                let id = self.composite(ty, kind, fingerprint);
                self.intern(element, depth + 1)?;
                Ok(id)
            }
            Ty::Map { key, value, .. } => {
                let id = self.composite(ty, TypeKind::Map, fingerprint);
                self.intern(key, depth + 1)?;
                self.intern(value, depth + 1)?;
                Ok(id)
            }
            Ty::Function { params, result } => {
                let id = self.composite(ty, TypeKind::Function, fingerprint);
                for param in params {
                    self.intern(param, depth + 1)?;
                }
                self.intern(result, depth + 1)?;
                Ok(id)
            }
            Ty::Trait(nominal) => {
                let contract = self
                    .catalog
                    .traits
                    .get_id(nominal.declaration)
                    .ok_or_else(|| conflict("interface metadata target is not installed"))?;
                if contract.generic_params.len() != nominal.arguments.len() {
                    return Err(conflict("interface metadata argument count"));
                }
                let id = self.composite(ty, TypeKind::Interface, fingerprint);
                for argument in nominal
                    .arguments
                    .iter()
                    .chain(nominal.associated_types.values())
                {
                    self.intern(argument, depth + 1)?;
                }
                Ok(id)
            }
            _ => Err(conflict(
                "external metadata contains an unresolved type template",
            )),
        }
    }

    fn composite(&mut self, ty: &Ty, kind: TypeKind, fingerprint: AbiFingerprint) -> TypeId {
        let name = format!("native value {}", self.inner.by_id.len());
        reserve(self.inner, ty.clone(), name, kind, fingerprint)
    }

    fn nominal(
        &mut self,
        ty: &Ty,
        nominal: &NominalTy,
        fingerprint: AbiFingerprint,
        depth: usize,
    ) -> Result<TypeId, RuntimeError> {
        let record = self
            .catalog
            .types
            .get_id(nominal.declaration)
            .ok_or_else(|| conflict("nominal metadata target is not installed"))?;
        let kind = match (ty, record.kind) {
            (Ty::Struct(_), TypeDefKind::Struct) => TypeKind::Struct,
            (Ty::Enum(_), TypeDefKind::Enum) => TypeKind::Enum,
            (Ty::NativeObject(_), TypeDefKind::NativeStorage(_)) => TypeKind::Struct,
            _ => {
                return Err(conflict(
                    "nominal metadata storage kind differs from its declaration",
                ));
            }
        };
        if record.generic_params.len() != nominal.arguments.len()
            || !nominal.associated_types.is_empty()
        {
            return Err(conflict("nominal metadata argument count"));
        }
        let record = record.clone();
        let id = self.composite(ty, kind, fingerprint);
        let mut substitution = TypeSubstitution::default();
        for (parameter, argument) in record.generic_params.iter().zip(&nominal.arguments) {
            substitution.bind(&parameter.owner, parameter.position, argument);
        }
        let fields = self.fields(&record, nominal.declaration, &substitution, depth + 1)?;
        let mut variants = Vec::new();
        for (slot, variant) in record.variants.iter().enumerate() {
            let mut payload = Vec::new();
            for ty in &variant.payload {
                let ty = substitution
                    .apply(ty, &Default::default())
                    .map_err(conflict)?;
                payload.push(self.intern(&ty, depth + 1)?);
            }
            variants.push(VariantInfo {
                id: VariantMetadataId::new(slot),
                name: variant.name.clone(),
                payload,
                abi_fingerprint: fingerprint,
            });
        }
        self.inner.by_id[id.index()].fields = fields;
        self.inner.by_id[id.index()].variants = variants;
        Ok(id)
    }

    fn fields(
        &mut self,
        record: &TypeDef,
        owner: DefinitionId,
        substitution: &TypeSubstitution<'_>,
        depth: usize,
    ) -> Result<Vec<FieldInfo>, RuntimeError> {
        let mut fields = Vec::new();
        for (slot, field) in record.fields.iter().enumerate() {
            let ty = substitution
                .apply(&field.ty, &Default::default())
                .map_err(conflict)?;
            let policy = field.host_policy;
            fields.push(FieldInfo {
                id: FieldMetadataId::new(slot),
                name: field.name.clone(),
                ty: self.intern(&ty, depth + 1)?,
                readable: policy.is_none_or(|policy| policy.readable),
                writable: field.mutable,
                visibility: field.visibility,
                path_access: policy.map_or(PathAccess::None, |policy| policy.path_access),
                abi_fingerprint: field_fingerprint(owner, record, slot, &ty, &self.definitions)?,
            });
        }
        Ok(fields)
    }
}

fn reserve(
    inner: &mut TypeRegistryInner,
    ty: Ty,
    name: String,
    kind: TypeKind,
    fingerprint: AbiFingerprint,
) -> TypeId {
    let id = TypeId::new(inner.by_id.len());
    inner.native_values.insert(ty, id);
    inner.public_abi_fingerprints.insert(fingerprint);
    inner.by_id.push(TypeInfo {
        id,
        name,
        kind,
        epoch: None,
        fields: vec![],
        variants: vec![],
        methods: vec![],
        traits: vec![],
        abi_fingerprint: fingerprint,
    });
    id
}

fn type_fingerprint(
    owner: DefinitionId,
    record: &TypeDef,
    definitions: &DefinitionTable,
) -> Result<AbiFingerprint, RuntimeError> {
    let record = TypeDeclaration {
        declaration: owner,
        definition: record.clone(),
    };
    let cancel = CancellationToken::default();
    let mut references = BTreeSet::new();
    record
        .visit_definition_ids(
            &mut |id| {
                references.insert(*id);
                Ok(())
            },
            &cancel,
        )
        .map_err(conflict)?;
    let encoding = definitions.encode(references).map_err(conflict)?;
    let record = TypeDeclarationRecord::encode(&record, &encoding, &cancel).map_err(conflict)?;
    Ok(AbiFingerprint(
        ArtifactFingerprint::of_serialized(&(record, encoding.table)).0,
    ))
}

fn field_fingerprint(
    owner: DefinitionId,
    record: &TypeDef,
    slot: usize,
    ty: &Ty,
    definitions: &DefinitionTable,
) -> Result<AbiFingerprint, RuntimeError> {
    let mut field_record = record.clone();
    let mut field = record.fields[slot].clone();
    field.ty = ty.clone();
    field_record.fields = vec![field];
    type_fingerprint(owner, &field_record, definitions)
}

fn value_fingerprint(
    ty: &Ty,
    definitions: &DefinitionTable,
) -> Result<AbiFingerprint, RuntimeError> {
    let cancel = CancellationToken::default();
    let mut references = BTreeSet::new();
    ty.visit_definition_ids(
        &mut |id| {
            references.insert(*id);
            Ok(())
        },
        &cancel,
    )
    .map_err(conflict)?;
    let encoding = definitions.encode(references).map_err(conflict)?;
    let record = WireType::encode(ty, &encoding, &cancel).map_err(conflict)?;
    Ok(AbiFingerprint(
        ArtifactFingerprint::of_serialized(&(record, encoding.table)).0,
    ))
}

fn conflict(error: impl Debug) -> RuntimeError {
    RuntimeError::metadata_conflict(format!("{error:?}"))
}

fn scalar_name(kind: BuiltinType) -> &'static str {
    match kind {
        BuiltinType::Never => "never",
        BuiltinType::Unit => "()",
        BuiltinType::Bool => "bool",
        BuiltinType::I8 => "i8",
        BuiltinType::I16 => "i16",
        BuiltinType::I32 => "i32",
        BuiltinType::I64 => "i64",
        BuiltinType::ISize => "isize",
        BuiltinType::U8 => "u8",
        BuiltinType::U16 => "u16",
        BuiltinType::U32 => "u32",
        BuiltinType::U64 => "u64",
        BuiltinType::USize => "usize",
        BuiltinType::F32 => "f32",
        BuiltinType::F64 => "f64",
        BuiltinType::String => "String",
    }
}
