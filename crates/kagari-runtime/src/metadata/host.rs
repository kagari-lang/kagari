//! Build reflection metadata from portable declarations, then commit once.
use super::*;
use crate::host::{HostRegistry, HostTypeInfo, HostTypeRegistration};
use kagari_common::{host_interface::HostValueType, identity::DefinitionId};

impl TypeRegistry {
    pub(crate) fn register_host_types(
        &self,
        registrations: &[HostTypeRegistration],
        host: &HostRegistry,
    ) -> Result<Vec<HostTypeInfo>, RuntimeError> {
        let mut candidate = self.inner.borrow().clone();
        let mut nominal = host
            .host_types()
            .map(|info| (info.declaration.id.clone(), info.type_id))
            .collect::<HashMap<_, _>>();
        let mut bindings = Vec::new();
        for registration in registrations {
            let declaration = &registration.declaration;
            declaration.validate().map_err(metadata_error)?;
            host.validate_type_identity(&declaration.id, &declaration.symbol)?;
            if nominal.contains_key(&declaration.id)
                || candidate.by_name.contains_key(&declaration.symbol)
            {
                return Err(RuntimeError::metadata_conflict(&declaration.symbol));
            }
            let id = TypeId::new(candidate.by_id.len());
            let fingerprint = AbiFingerprint(declaration.fingerprint().map_err(metadata_error)?);
            nominal.insert(declaration.id.clone(), id);
            candidate.by_name.insert(declaration.symbol.clone(), id);
            candidate.public_abi_fingerprints.insert(fingerprint);
            candidate.by_id.push(TypeInfo {
                id,
                name: declaration.symbol.clone(),
                kind: TypeKind::HostObject,
                epoch: None,
                fields: Vec::new(),
                variants: Vec::new(),
                methods: Vec::new(),
                traits: Vec::new(),
                abi_fingerprint: fingerprint,
            });
            bindings.push(HostTypeInfo {
                type_id: id,
                declaration: declaration.clone(),
                rust_type_name: registration.rust_type_name.clone(),
                abi_fingerprint: fingerprint,
            });
        }
        for binding in &bindings {
            let declaration = &binding.declaration;
            let fields = declaration
                .fields
                .iter()
                .enumerate()
                .map(|(slot, field)| {
                    Ok(FieldInfo {
                        id: FieldMetadataId::new(slot),
                        name: field.name.clone(),
                        ty: intern(&mut candidate, &nominal, &field.ty)?,
                        readable: field.readable,
                        writable: field.writable,
                        visibility: field.visibility,
                        path_access: field.path_access,
                        abi_fingerprint: AbiFingerprint(
                            field.fingerprint().map_err(metadata_error)?,
                        ),
                    })
                })
                .collect::<Result<Vec<_>, RuntimeError>>()?;
            let methods = declaration
                .methods
                .iter()
                .enumerate()
                .map(|(slot, method)| {
                    let params = method
                        .params
                        .iter()
                        .map(|param| {
                            Ok(ParameterInfo {
                                name: param.name.clone(),
                                ty: intern(&mut candidate, &nominal, &param.ty)?,
                            })
                        })
                        .collect::<Result<_, RuntimeError>>()?;
                    Ok(MethodInfo {
                        id: MethodMetadataId::new(slot),
                        name: method.name.clone(),
                        params,
                        return_type: intern(&mut candidate, &nominal, &method.return_type)?,
                        origin: MethodOrigin::Host,
                        capability_requirements: method.capability_requirements,
                        abi_fingerprint: AbiFingerprint(
                            method.fingerprint().map_err(metadata_error)?,
                        ),
                    })
                })
                .collect::<Result<Vec<_>, RuntimeError>>()?;
            let info = &mut candidate.by_id[binding.type_id.index()];
            info.fields = fields;
            info.methods = methods;
        }
        *self.inner.borrow_mut() = candidate;
        Ok(bindings)
    }
}

fn metadata_error(error: impl std::fmt::Display) -> RuntimeError {
    RuntimeError::metadata_conflict(error.to_string())
}

fn intern(
    inner: &mut TypeRegistryInner,
    nominal: &HashMap<DefinitionId, TypeId>,
    ty: &HostValueType,
) -> Result<TypeId, RuntimeError> {
    use HostValueType as T;
    if let T::Opaque(id) = ty {
        return nominal.get(id).copied().ok_or_else(|| {
            RuntimeError::metadata_conflict(format!("missing host member type {id:?}"))
        });
    }
    if let Some(id) = inner.host_value_types.get(ty) {
        return Ok(*id);
    }
    let scalar = match ty {
        T::Unit => Some("()"),
        T::Bool => Some("bool"),
        T::I32 => Some("i32"),
        T::I64 => Some("i64"),
        T::F32 => Some("f32"),
        T::F64 => Some("f64"),
        T::String => Some("String"),
        _ => None,
    };
    if let Some(name) = scalar
        && let Some(id) = inner.by_name.get(name).copied()
    {
        if inner.by_id[id.index()].kind != TypeKind::Primitive {
            return Err(RuntimeError::metadata_conflict(
                "builtin metadata name has a non-primitive binding",
            ));
        }
        inner.host_value_types.insert(ty.clone(), id);
        return Ok(id);
    }
    let mut fields = Vec::new();
    let mut variants = Vec::new();
    let kind = match ty {
        T::Tuple(elements) => {
            for (slot, element) in elements.iter().enumerate() {
                fields.push(FieldInfo {
                    id: FieldMetadataId::new(slot),
                    name: slot.to_string(),
                    ty: intern(inner, nominal, element)?,
                    readable: true,
                    writable: false,
                    visibility: Visibility::Public,
                    path_access: PathAccess::None,
                    abi_fingerprint: AbiFingerprint(element.fingerprint().map_err(metadata_error)?),
                });
            }
            TypeKind::Tuple
        }
        T::Array(element) | T::Set(element) => {
            intern(inner, nominal, element)?;
            if matches!(ty, T::Array(_)) {
                TypeKind::Array
            } else {
                TypeKind::Set
            }
        }
        T::Map { key, value } => {
            intern(inner, nominal, key)?;
            intern(inner, nominal, value)?;
            TypeKind::Map
        }
        T::Option(element) => {
            variants.push(VariantInfo {
                id: VariantMetadataId::new(0),
                name: "None".into(),
                payload: vec![],
                abi_fingerprint: AbiFingerprint(ty.fingerprint().map_err(metadata_error)?),
            });
            variants.push(VariantInfo {
                id: VariantMetadataId::new(1),
                name: "Some".into(),
                payload: vec![intern(inner, nominal, element)?],
                abi_fingerprint: AbiFingerprint(ty.fingerprint().map_err(metadata_error)?),
            });
            TypeKind::Enum
        }
        T::Result { ok, error } => {
            for (slot, name, payload) in [(0, "Ok", ok), (1, "Err", error)] {
                variants.push(VariantInfo {
                    id: VariantMetadataId::new(slot),
                    name: name.into(),
                    payload: vec![intern(inner, nominal, payload)?],
                    abi_fingerprint: AbiFingerprint(ty.fingerprint().map_err(metadata_error)?),
                });
            }
            TypeKind::Enum
        }
        _ => TypeKind::Primitive,
    };
    let id = TypeId::new(inner.by_id.len());
    // Composite display names are not identity keys and never enter by_name.
    let name = scalar
        .map(str::to_owned)
        .unwrap_or_else(|| format!("host value {}", id.index()));
    inner.by_id.push(TypeInfo {
        id,
        name: name.clone(),
        kind,
        epoch: None,
        fields,
        variants,
        methods: Vec::new(),
        traits: Vec::new(),
        abi_fingerprint: AbiFingerprint(ty.fingerprint().map_err(metadata_error)?),
    });
    if scalar.is_some() {
        inner.by_name.insert(name, id);
    }
    inner.host_value_types.insert(ty.clone(), id);
    Ok(id)
}
