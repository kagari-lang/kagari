//! Native type declarations retain nominal identity and their own binder.
use crate::{
    error::RuntimeError,
    native::{
        binding::NativeResult,
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        payload::Opaque,
        storage::{NativePayload, NativeStorage},
        types::{ParameterRef, Type, TypeRef},
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, metadata::DefinitionMetadata, table::DefinitionId},
};
use kagari_types::{
    declaration::{
        FieldDef, TypeDef, TypeDefKind, host::HostTypePolicy, verify::type_definition_valid,
    },
    ty::{GenericBound, GenericParam, Ty},
};
use std::sync::Arc;

pub struct TypeBuilder<'module> {
    module: &'module mut ModuleBuilder,
    id: DefinitionId,
    name: String,
    parameter_names: Vec<String>,
    parameters: Vec<GenericParam>,
    documentation: Option<String>,
    storage: Option<Storage>,
    fields: Vec<FieldDef>,
    bounds: Vec<GenericBound>,
    register_mapping: Option<fn(&mut DeclarationCatalog, Type) -> NativeResult<()>>,
}

enum Storage {
    Native(NativeStorage),
    External(HostTypePolicy),
}

impl ModuleBuilder {
    /// Declare an ordinary opaque payload with its single Rust storage policy.
    /// Generic applications retain contextual Rust mapping checks at use sites.
    pub fn define_opaque<P: NativePayload>(
        &mut self,
        name: impl Into<String>,
    ) -> NativeResult<TypeBuilder<'_>> {
        let storage = P::registration_storage();
        if !storage.accepts_payload::<P>() {
            return Err(RuntimeError::metadata_conflict(
                "opaque storage policy differs from its Rust payload",
            ));
        }
        let mut builder = TypeBuilder::new(self, name.into())?;
        builder.native_storage(storage)?;
        builder.register_mapping = Some(|catalog, ty| catalog.insert_rust_type::<Opaque<P>>(ty));
        Ok(builder)
    }

    /// Install storage for a local native type authored in a portable declaration.
    /// The complete declaration and layout must match; provider types cannot be rebound.
    pub fn bind_storage(&mut self, ty: &TypeRef, storage: NativeStorage) -> NativeResult<()> {
        let ty = ty.adopt(self.context())?;
        if self
            .providers
            .definitions()
            .resolve(ty.id)
            .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?
            .module()
            != &self.declaration.identity
            || self
                .declaration
                .types
                .iter()
                .find(|item| item.name == ty.declaration.name)
                != Some(ty.declaration.as_ref())
            || ty.declaration.kind != TypeDefKind::NativeStorage(storage.layout())
            || !storage
                .layout()
                .valid_parameters(ty.declaration.generic_params.len())
            || self.storage.contains_key(&ty.id)
        {
            return Err(RuntimeError::metadata_conflict(
                "native storage binding differs from its local declaration",
            ));
        }
        self.storage.insert(ty.id, storage);
        Ok(())
    }
}

impl<'module> TypeBuilder<'module> {
    pub(crate) fn new(module: &'module mut ModuleBuilder, name: String) -> NativeResult<Self> {
        let id = module.definition(DefinitionKind::AssociatedType, &name)?;
        Ok(Self {
            module,
            id,
            name,
            parameter_names: vec![],
            parameters: vec![],
            documentation: None,
            storage: None,
            fields: vec![],
            bounds: vec![],
            register_mapping: None,
        })
    }

    /// Set full Markdown for the registered native type.
    pub fn documentation(&mut self, text: impl Into<String>) {
        self.documentation = Some(text.into());
    }

    pub fn type_parameter(&mut self, name: impl Into<String>) -> NativeResult<ParameterRef> {
        if matches!(self.storage, Some(Storage::External(_))) {
            return Err(RuntimeError::metadata_conflict(
                "external identity types cannot have generic parameters",
            ));
        }
        let name = name.into();
        if self.parameter_names.contains(&name) {
            return Err(RuntimeError::metadata_conflict(
                "duplicate native type parameter",
            ));
        }
        let parameter = GenericParam {
            owner: self.id,
            position: self.parameters.len(),
        };
        self.parameter_names.push(name);
        self.parameters.push(parameter.clone());
        Ok(ParameterRef {
            ty: Type::from_semantic(parameter.as_type(), &self.module.providers.definitions())?,
        })
    }

    /// Add a declaration-owned trait obligation; finish checks its binder scope.
    pub fn bound(&mut self, ty: Type, interface: Type) -> NativeResult<()> {
        let ty = ty.adopt(self.module.context())?;
        let Ty::Trait(interface) = interface.adopt(self.module.context())? else {
            return Err(RuntimeError::metadata_conflict(
                "native type bound requires a trait application",
            ));
        };
        self.bounds.push(GenericBound {
            ty,
            constraints: vec![interface],
        });
        Ok(())
    }

    pub fn sequence_storage(&mut self, element: &ParameterRef) -> NativeResult<()> {
        let Ty::Parameter { owner, position } = element.ty.abi() else {
            return Err(RuntimeError::metadata_conflict(
                "sequence element must be a declared type parameter",
            ));
        };
        if *owner != self.id || *position >= self.parameters.len() {
            return Err(RuntimeError::metadata_conflict(
                "sequence element belongs to another declaration",
            ));
        }
        self.native_storage(NativeStorage::sequence(*position))
    }

    pub fn native_storage(&mut self, storage: NativeStorage) -> NativeResult<()> {
        if self.storage.is_some() {
            return Err(RuntimeError::metadata_conflict(
                "duplicate native storage contract",
            ));
        }
        self.storage = Some(Storage::Native(storage));
        Ok(())
    }

    /// Declare an address-free external identity. It installs no Rust object or
    /// reference, and methods/traits still require explicit declarations/bindings.
    pub fn external(&mut self, policy: HostTypePolicy) -> NativeResult<()> {
        if self.storage.is_some() || !self.parameters.is_empty() {
            return Err(RuntimeError::metadata_conflict(
                "external identity conflicts with storage or type parameters",
            ));
        }
        self.id = self.module.definition(DefinitionKind::Struct, &self.name)?;
        self.storage = Some(Storage::External(policy));
        Ok(())
    }

    /// Adopt a common field record into this external declaration's scope. The
    /// field supplies declared access only, without a Rust layout/projection.
    pub fn field(&mut self, field: DefinitionMetadata<FieldDef>) -> NativeResult<()> {
        if !matches!(self.storage, Some(Storage::External(_))) {
            return Err(RuntimeError::metadata_conflict(
                "manual fields require an external identity declaration",
            ));
        }
        let field = field
            .import_into(self.module.context(), &CancellationToken::default())
            .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?
            .into_records();
        if field.host_policy.is_none()
            || self
                .fields
                .iter()
                .any(|existing| existing.name == field.name)
        {
            return Err(RuntimeError::metadata_conflict(
                "external field requires a unique name and access policy",
            ));
        }
        self.module
            .context()
            .intern_child(self.id, DefinitionKind::Field, &field.name, 0)
            .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
        self.fields.push(field);
        Ok(())
    }

    /// Finish the common nominal declaration. Native storage owns its payload;
    /// external identities describe host-managed state without transferring it.
    pub fn finish(self) -> NativeResult<TypeRef> {
        let storage = self
            .storage
            .ok_or_else(|| RuntimeError::metadata_conflict("native type requires storage"))?;
        if matches!(&storage, Storage::Native(storage) if !storage.layout().valid_parameters(self.parameters.len()))
        {
            return Err(RuntimeError::metadata_conflict(
                "native storage parameter positions",
            ));
        }
        if self
            .module
            .declaration
            .types
            .iter()
            .any(|ty| ty.name == self.name)
        {
            return Err(RuntimeError::metadata_conflict(
                "duplicate native type declaration",
            ));
        }
        let declaration = TypeDef {
            name: self.name,
            kind: match &storage {
                Storage::Native(storage) => TypeDefKind::NativeStorage(storage.layout()),
                Storage::External(policy) => TypeDefKind::Host(*policy),
            },
            generic_params: self.parameters,
            bounds: GenericBound::normalize(
                self.bounds,
                &self.module.providers.definitions(),
                &CancellationToken::default(),
            )
            .map_err(|error| {
                RuntimeError::metadata_conflict(format!("native type bounds: {error:?}"))
            })?,
            fields: self.fields,
            variants: vec![],
        };
        if !type_definition_valid(
            &declaration,
            self.id,
            &CancellationToken::default(),
            &self.module.providers.definitions(),
        ) {
            return Err(RuntimeError::metadata_conflict(
                "invalid native type declaration",
            ));
        }
        if let Some(text) = self.documentation {
            self.module.declaration.documentation.insert(self.id, text);
        }
        if let Storage::Native(storage) = storage {
            self.module.storage.insert(self.id, storage);
        }
        self.module.declaration.types.push(declaration.clone());
        self.module
            .providers
            .insert_type(self.id, declaration.clone())?;
        let result = TypeRef {
            id: self.id,
            definitions: self.module.providers.definitions(),
            declaration: Arc::new(declaration),
            parameter_names: self.parameter_names,
        };
        if result.declaration.generic_params.is_empty()
            && let Some(register) = self.register_mapping
        {
            register(&mut self.module.providers, result.apply([])?)?;
        }
        Ok(result)
    }
}
