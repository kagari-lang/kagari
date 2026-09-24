//! Portable member contracts. No Rust type names, runtime IDs or callbacks.
use super::{
    HostFunctionEffects, HostInterfaceError, HostParameter, HostPassingStyle, HostValueType, codec,
    hash, host_type_identity, validate_host_type_identity,
};
use crate::{
    capability::CapabilitySet,
    identity::{DefinitionId, DefinitionKind, DefinitionPathSegment},
};
use bincode::Options;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HostTypeOwnership {
    Opaque,
    Owned,
    HostRoot,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HostReflectionPolicy {
    Hidden,
    TypeNameOnly,
    Metadata,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PathAccess {
    None,
    ReadOnly,
    ReadWrite,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Visibility {
    Private,
    Public,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostFieldDeclaration {
    pub id: DefinitionId,
    pub name: String,
    pub ty: HostValueType,
    pub readable: bool,
    pub writable: bool,
    pub visibility: Visibility,
    pub path_access: PathAccess,
    pub documentation: String,
}

impl HostFieldDeclaration {
    pub fn new(owner: &DefinitionId, name: impl Into<String>, ty: HostValueType) -> Self {
        let name = name.into();
        Self {
            id: member_id(owner, DefinitionKind::Field, &name),
            name,
            ty,
            readable: true,
            writable: false,
            visibility: Visibility::Public,
            path_access: PathAccess::None,
            documentation: String::new(),
        }
    }
    pub fn fingerprint(&self) -> Result<u64, HostInterfaceError> {
        self.ty.validate()?;
        let mut abi = self.clone();
        abi.documentation.clear();
        fingerprint(b"kagari-host-field-v1\0", &abi)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostMethodDeclaration {
    pub id: DefinitionId,
    pub name: String,
    pub receiver: HostPassingStyle,
    #[serde(deserialize_with = "super::decode_limits::members")]
    pub params: Vec<HostParameter>,
    pub return_type: HostValueType,
    pub capability_requirements: CapabilitySet,
    pub resource_cost_hint: Option<u64>,
    pub effects: HostFunctionEffects,
    pub documentation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostTraitMethodBinding {
    pub trait_method: DefinitionId,
    pub host_method: DefinitionId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostTraitImplementationDeclaration {
    pub trait_id: DefinitionId,
    #[serde(deserialize_with = "super::decode_limits::members")]
    pub trait_arguments: Vec<HostValueType>,
    #[serde(deserialize_with = "super::decode_limits::members")]
    pub methods: Vec<HostTraitMethodBinding>,
    pub documentation: String,
}

impl HostTraitImplementationDeclaration {
    pub fn new(
        trait_id: DefinitionId,
        trait_arguments: Vec<HostValueType>,
        methods: Vec<HostTraitMethodBinding>,
    ) -> Self {
        Self {
            trait_id,
            trait_arguments,
            methods,
            documentation: String::new(),
        }
    }
}

impl HostMethodDeclaration {
    pub fn new(
        owner: &DefinitionId,
        name: impl Into<String>,
        params: Vec<HostParameter>,
        return_type: HostValueType,
    ) -> Self {
        let name = name.into();
        Self {
            id: member_id(owner, DefinitionKind::Method, &name),
            name,
            receiver: HostPassingStyle::SharedBorrow,
            params,
            return_type,
            capability_requirements: Default::default(),
            resource_cost_hint: None,
            effects: Default::default(),
            documentation: String::new(),
        }
    }
    pub fn fingerprint(&self) -> Result<u64, HostInterfaceError> {
        super::validate_signature(&self.params, &self.return_type)?;
        let mut abi = self.clone();
        abi.documentation.clear();
        fingerprint(b"kagari-host-method-v1\0", &abi)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostTypeDeclaration {
    pub id: DefinitionId,
    pub symbol: String,
    pub ownership: HostTypeOwnership,
    #[serde(deserialize_with = "super::decode_limits::members")]
    pub fields: Vec<HostFieldDeclaration>,
    #[serde(deserialize_with = "super::decode_limits::members")]
    pub methods: Vec<HostMethodDeclaration>,
    #[serde(deserialize_with = "super::decode_limits::members")]
    pub trait_implementations: Vec<HostTraitImplementationDeclaration>,
    pub path_access: PathAccess,
    pub reflection: HostReflectionPolicy,
    pub documentation: String,
}

impl HostTypeDeclaration {
    /// Executable contract derived solely from this member declaration.
    pub fn method_contract(
        &self,
        id: &DefinitionId,
    ) -> Result<super::HostFunctionDeclaration, HostInterfaceError> {
        self.validate()?;
        let method = self
            .methods
            .iter()
            .find(|method| &method.id == id)
            .ok_or(HostInterfaceError::InvalidDeclaration)?;
        let mut params = vec![HostParameter {
            name: "self".into(),
            ty: HostValueType::Opaque(self.id.clone()),
            passing: method.receiver,
        }];
        params.extend(method.params.iter().cloned());
        let function = super::HostFunctionDeclaration {
            id: method.id.clone(),
            symbol: format!("{}.{}", self.symbol, method.name),
            params,
            return_type: method.return_type.clone(),
            capability_requirements: method.capability_requirements,
            resource_cost_hint: method.resource_cost_hint,
            effects: method.effects,
            documentation: method.documentation.clone(),
        };
        function.validate()?;
        Ok(function)
    }
    pub fn new(symbol: impl Into<String>) -> Self {
        let symbol = symbol.into();
        Self {
            id: host_type_identity(&symbol),
            symbol,
            ownership: HostTypeOwnership::Opaque,
            fields: Vec::new(),
            methods: Vec::new(),
            trait_implementations: Vec::new(),
            path_access: PathAccess::None,
            reflection: HostReflectionPolicy::Hidden,
            documentation: String::new(),
        }
    }
    pub fn validate(&self) -> Result<(), HostInterfaceError> {
        validate_host_type_identity(&self.id)?;
        if self.fields.len() > super::decode_limits::MAX_MEMBERS
            || self.methods.len() > super::decode_limits::MAX_MEMBERS
            || self.trait_implementations.len() > super::decode_limits::MAX_MEMBERS
            || self
                .methods
                .iter()
                .any(|method| method.params.len() > super::decode_limits::MAX_MEMBERS)
        {
            return Err(HostInterfaceError::TooLarge);
        }
        if self.symbol.is_empty()
            || self.symbol.split('.').any(str::is_empty)
            || self.fields.len() > u16::MAX as usize
            || self.methods.len() > u16::MAX as usize
        {
            return Err(HostInterfaceError::InvalidDeclaration);
        }
        let mut names = std::collections::HashSet::new();
        for field in &self.fields {
            validate_member(&self.id, &field.id, &field.name, DefinitionKind::Field)?;
            if !names.insert(&field.name) {
                return Err(HostInterfaceError::DuplicateDeclaration);
            }
            field.ty.validate()?;
            if (field.path_access != PathAccess::None && !field.readable)
                || (field.path_access == PathAccess::ReadWrite && !field.writable)
            {
                return Err(HostInterfaceError::InvalidDeclaration);
            }
        }
        for method in &self.methods {
            validate_member(&self.id, &method.id, &method.name, DefinitionKind::Method)?;
            if !names.insert(&method.name) {
                return Err(HostInterfaceError::DuplicateDeclaration);
            }
            // Parameter and result rules are the same as standalone functions.
            super::validate_signature(&method.params, &method.return_type)?;
            if method.params.len() >= u16::MAX as usize
                || method.params.iter().any(|param| param.name == "self")
            {
                return Err(HostInterfaceError::InvalidDeclaration);
            }
        }
        let mut implemented_traits = std::collections::HashSet::new();
        let mut mapped_methods = 0usize;
        let mut trait_arguments = 0usize;
        for implementation in &self.trait_implementations {
            let trait_id = &implementation.trait_id;
            if !trait_id.within_path_limit() {
                return Err(HostInterfaceError::TooLarge);
            }
            if trait_id.module.package.0.is_empty()
                || trait_id.module.path.iter().any(String::is_empty)
                || trait_id.path.iter().any(|segment| segment.name.is_empty())
                || trait_id
                    .path
                    .last()
                    .is_none_or(|segment| segment.kind != DefinitionKind::Trait)
            {
                return Err(HostInterfaceError::InvalidDeclaration);
            }
            trait_arguments = trait_arguments.saturating_add(implementation.trait_arguments.len());
            if trait_arguments > super::decode_limits::MAX_MEMBERS {
                return Err(HostInterfaceError::TooLarge);
            }
            for argument in &implementation.trait_arguments {
                argument.validate()?;
            }
            if !implemented_traits.insert((trait_id, &implementation.trait_arguments)) {
                return Err(HostInterfaceError::DuplicateDeclaration);
            }
            mapped_methods = mapped_methods.saturating_add(implementation.methods.len());
            if mapped_methods > super::decode_limits::MAX_MEMBERS {
                return Err(HostInterfaceError::TooLarge);
            }
            let mut bound_methods = std::collections::HashSet::new();
            for binding in &implementation.methods {
                if !binding.trait_method.within_path_limit() {
                    return Err(HostInterfaceError::TooLarge);
                }
                let mut trait_owner = binding.trait_method.clone();
                if trait_owner.path.pop().is_none_or(|segment| {
                    segment.kind != DefinitionKind::Method || segment.name.is_empty()
                }) || trait_owner != *trait_id
                    || !bound_methods.insert(&binding.trait_method)
                    || !self
                        .methods
                        .iter()
                        .any(|method| method.id == binding.host_method)
                {
                    return Err(HostInterfaceError::InvalidDeclaration);
                }
            }
        }
        Ok(())
    }
    pub fn value_types(&self) -> impl Iterator<Item = &HostValueType> {
        self.fields
            .iter()
            .map(|field| &field.ty)
            .chain(self.methods.iter().flat_map(|method| {
                method
                    .params
                    .iter()
                    .map(|param| &param.ty)
                    .chain(std::iter::once(&method.return_type))
            }))
            .chain(
                self.trait_implementations
                    .iter()
                    .flat_map(|implementation| &implementation.trait_arguments),
            )
    }
    pub fn clear_documentation(&mut self) {
        self.documentation.clear();
        for implementation in &mut self.trait_implementations {
            implementation.documentation.clear();
        }
        for field in &mut self.fields {
            field.documentation.clear();
        }
        for method in &mut self.methods {
            method.documentation.clear();
        }
    }
    pub fn fingerprint(&self) -> Result<u64, HostInterfaceError> {
        self.validate()?;
        let mut abi = self.clone();
        abi.clear_documentation();
        fingerprint(b"kagari-host-type-v1\0", &abi)
    }
    pub fn matches_binding(&self, actual: &Self) -> bool {
        let mut required = self.clone();
        let mut actual = actual.clone();
        required.clear_documentation();
        actual.clear_documentation();
        required == actual
    }
}

fn member_id(owner: &DefinitionId, kind: DefinitionKind, name: &str) -> DefinitionId {
    let mut id = owner.clone();
    id.path.push(DefinitionPathSegment {
        kind,
        name: name.into(),
        occurrence: 0,
    });
    id
}
fn validate_member(
    owner: &DefinitionId,
    id: &DefinitionId,
    name: &str,
    kind: DefinitionKind,
) -> Result<(), HostInterfaceError> {
    if !id.within_path_limit() {
        return Err(HostInterfaceError::TooLarge);
    }
    if name.is_empty() || *id != member_id(owner, kind, name) {
        Err(HostInterfaceError::InvalidDeclaration)
    } else {
        Ok(())
    }
}
fn fingerprint(domain: &[u8], value: &impl Serialize) -> Result<u64, HostInterfaceError> {
    let bytes = codec()
        .serialize(value)
        .map_err(|_| HostInterfaceError::Encoding)?;
    Ok(hash(domain.iter().copied().chain(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_interface::HostInterface;
    use crate::identity::{ModuleIdentity, PackageId};

    fn script_trait(name: &str) -> DefinitionId {
        DefinitionId {
            module: ModuleIdentity {
                package: PackageId("pkg".into()),
                path: vec!["api".into()],
            },
            path: vec![DefinitionPathSegment {
                kind: DefinitionKind::Trait,
                name: name.into(),
                occurrence: 0,
            }],
        }
    }

    #[test]
    fn host_trait_bindings_round_trip_and_validate_method_ownership() {
        let mut owner = HostTypeDeclaration::new("demo.Counter");
        let method = HostMethodDeclaration::new(&owner.id, "read", vec![], HostValueType::I32);
        owner.methods.push(method.clone());
        let trait_id = script_trait("Readable");
        let mut trait_method = trait_id.clone();
        trait_method.path.push(DefinitionPathSegment {
            kind: DefinitionKind::Method,
            name: "get".into(),
            occurrence: 0,
        });
        owner
            .trait_implementations
            .push(HostTraitImplementationDeclaration::new(
                trait_id.clone(),
                vec![],
                vec![HostTraitMethodBinding {
                    trait_method: trait_method.clone(),
                    host_method: method.id.clone(),
                }],
            ));
        owner.validate().unwrap();
        let interface = HostInterface {
            paths: vec![],
            types: vec![owner.clone()],
            functions: vec![],
        };
        assert_eq!(
            HostInterface::from_bytes(&interface.to_bytes().unwrap()).unwrap(),
            interface
        );
        let fingerprint = owner.fingerprint().unwrap();
        owner.trait_implementations[0].documentation = "For the editor".into();
        assert_eq!(owner.fingerprint().unwrap(), fingerprint);
        for corruption in 0..5 {
            let mut invalid = owner.clone();
            let implementation = &mut invalid.trait_implementations[0];
            match corruption {
                0 => implementation.trait_id.path[0].kind = DefinitionKind::Struct,
                1 => implementation.methods[0].trait_method.path[0].name = "Other".into(),
                2 => implementation.methods[0].host_method.path[1].name = "missing".into(),
                3 => implementation
                    .methods
                    .push(implementation.methods[0].clone()),
                _ => invalid
                    .trait_implementations
                    .push(invalid.trait_implementations[0].clone()),
            }
            assert!(invalid.validate().is_err(), "corruption {corruption}");
        }
        let mut changed = owner.clone();
        changed.trait_implementations.clear();
        assert_ne!(changed.fingerprint().unwrap(), fingerprint);

        let mut applied = owner.clone();
        applied.trait_implementations[0].trait_arguments = vec![HostValueType::I32];
        assert_ne!(applied.fingerprint().unwrap(), fingerprint);
        let mut disjoint = applied.trait_implementations[0].clone();
        disjoint.trait_arguments = vec![HostValueType::Bool];
        applied.trait_implementations.push(disjoint);
        applied.validate().unwrap();
        assert_eq!(
            HostInterface::from_bytes(
                &HostInterface {
                    paths: vec![],
                    types: vec![applied.clone()],
                    functions: vec![],
                }
                .to_bytes()
                .unwrap()
            )
            .unwrap()
            .types,
            vec![applied.clone()]
        );
        applied.trait_implementations[1].trait_arguments = vec![HostValueType::I32];
        assert_eq!(
            applied.validate(),
            Err(HostInterfaceError::DuplicateDeclaration)
        );
        applied.trait_implementations[1].trait_arguments =
            vec![HostValueType::Opaque(host_type_identity("demo.Missing"))];
        assert_eq!(
            HostInterface {
                paths: vec![],
                types: vec![applied],
                functions: vec![],
            }
            .validate(),
            Err(HostInterfaceError::InvalidDeclaration)
        );
    }

    #[test]
    fn callable_method_contracts_cannot_override_their_declaring_member() {
        let mut owner = HostTypeDeclaration::new("demo.Counter");
        let mut method = HostMethodDeclaration::new(&owner.id, "add", vec![], HostValueType::I32);
        method.receiver = HostPassingStyle::UniqueBorrow;
        owner.methods.push(method);
        let call = owner.method_contract(&owner.methods[0].id).unwrap();
        assert_eq!(call.method_owner(), Some(owner.id.clone()));
        let valid = HostInterface {
            paths: vec![],
            types: vec![owner.clone()],
            functions: vec![call],
        };
        assert_eq!(
            HostInterface::from_bytes(&valid.to_bytes().unwrap()).unwrap(),
            valid
        );
        for corruption in 0..5 {
            let mut invalid = valid.clone();
            match corruption {
                0 => invalid.functions[0].params[0].passing = HostPassingStyle::Owned,
                1 => invalid.functions[0].effects.may_mutate_host_state = true,
                2 => invalid.functions[0].symbol = "other.add".into(),
                3 => {
                    invalid.types.clear();
                }
                _ => invalid.functions[0].id.path.last_mut().unwrap().name = "missing".into(),
            }
            assert!(invalid.validate().is_err(), "corruption {corruption}");
        }
        owner.methods[0].params.push(HostParameter {
            name: "self".into(),
            ty: HostValueType::I32,
            passing: HostPassingStyle::Owned,
        });
        assert!(owner.validate().is_err());
    }

    #[test]
    fn interface_encoding_is_canonical_and_rejects_old_memberless_formats() {
        let mut a = HostTypeDeclaration::new("pkg.A");
        let b = HostTypeDeclaration::new("pkg.B");
        a.fields.push(HostFieldDeclaration::new(
            &a.id,
            "b",
            HostValueType::Opaque(b.id.clone()),
        ));
        let first = HostInterface {
            paths: vec![],
            types: vec![a.clone(), b.clone()],
            functions: vec![],
        };
        let second = HostInterface {
            paths: vec![],
            types: vec![b, a],
            functions: vec![],
        };
        let bytes = first.to_bytes().unwrap();
        assert_eq!(bytes, second.to_bytes().unwrap());
        assert_eq!(HostInterface::from_bytes(&bytes).unwrap(), first);
        for version in 1_u16..super::super::VERSION {
            let mut old = bytes.clone();
            old[4..6].copy_from_slice(&version.to_le_bytes());
            assert_eq!(
                HostInterface::from_bytes(&old),
                Err(HostInterfaceError::Version)
            );
        }
    }

    #[test]
    fn invalid_member_owners_access_signatures_and_reference_closure_are_rejected() {
        let mut a = HostTypeDeclaration::new("pkg.A");
        let b = HostTypeDeclaration::new("pkg.B");
        a.fields.push(HostFieldDeclaration::new(
            &b.id,
            "value",
            HostValueType::I32,
        ));
        assert!(a.validate().is_err());
        a.fields[0] = HostFieldDeclaration::new(&a.id, "value", HostValueType::I32);
        a.fields[0].path_access = PathAccess::ReadWrite;
        assert!(a.validate().is_err());
        a.fields[0].writable = true;
        a.validate().unwrap();
        a.fields.push(a.fields[0].clone());
        assert_eq!(a.validate(), Err(HostInterfaceError::DuplicateDeclaration));
        a.fields.pop();
        a.methods.push(HostMethodDeclaration::new(
            &a.id,
            "borrow",
            vec![HostParameter {
                name: "x".into(),
                ty: HostValueType::I32,
                passing: HostPassingStyle::SharedBorrow,
            }],
            HostValueType::Unit,
        ));
        assert!(a.validate().is_err());
        a.methods.clear();
        a.fields[0].ty = HostValueType::Opaque(b.id.clone());
        let mut interface = HostInterface {
            paths: vec![],
            types: vec![a],
            functions: vec![],
        };
        assert!(interface.validate().is_err());
        interface.types.push(b);
        interface.validate().unwrap();
        interface.types[0].fields[0].id.module.package.0 = "foreign".into();
        // Bypass to_bytes validation to model malformed untrusted wire input.
        let bytes = codec()
            .serialize(&(
                super::super::MAGIC,
                super::super::VERSION,
                &interface.types,
                &interface.functions,
            ))
            .unwrap();
        assert!(HostInterface::from_bytes(&bytes).is_err());
    }
}
