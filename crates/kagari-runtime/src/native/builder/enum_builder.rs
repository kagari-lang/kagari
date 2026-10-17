//! Ordinary nominal enum declarations authored through native registration.
use crate::{
    error::RuntimeError,
    native::{
        binding::NativeResult,
        builder::ModuleBuilder,
        types::{ParameterRef, Type, TypeRef, VariantRef},
    },
};
use kagari_common::identity::{DefinitionKind, table::DefinitionId};
use kagari_types::{
    declaration::{TypeDef, TypeDefKind, VariantDef, module::ModuleDecl},
    ty::{GenericParam, NominalTy, Ty},
};
use std::{collections::BTreeMap, sync::Arc};

pub struct EnumBuilder<'module> {
    module: &'module mut ModuleBuilder,
    id: DefinitionId,
    declaration: TypeDef,
    parameter_names: Vec<String>,
    documentation: BTreeMap<DefinitionId, String>,
}

impl<'module> EnumBuilder<'module> {
    pub(crate) fn new(module: &'module mut ModuleBuilder, name: String) -> NativeResult<Self> {
        Ok(Self {
            id: module.definition(DefinitionKind::Enum, &name)?,
            module,
            declaration: TypeDef {
                name,
                kind: TypeDefKind::Enum,
                generic_params: vec![],
                bounds: vec![],
                fields: vec![],
                variants: vec![],
            },
            parameter_names: vec![],
            documentation: BTreeMap::new(),
        })
    }

    /// Preserve complete Markdown for the enum's generated declaration.
    pub fn documentation(&mut self, text: impl Into<String>) {
        self.documentation.insert(self.id, text.into());
    }

    pub fn type_parameter(&mut self, name: impl Into<String>) -> NativeResult<ParameterRef> {
        let name = name.into();
        if self.parameter_names.contains(&name) {
            return Err(RuntimeError::metadata_conflict(
                "duplicate enum type parameter",
            ));
        }
        let parameter = GenericParam {
            owner: self.id,
            position: self.declaration.generic_params.len(),
        };
        self.parameter_names.push(name);
        self.declaration.generic_params.push(parameter.clone());
        Ok(ParameterRef {
            ty: Type::from_semantic(parameter.as_type(), &self.module.providers.definitions())?,
        })
    }

    /// Refer to the current enum with its own parameters, including in recursive payloads.
    pub fn self_type(&self) -> NativeResult<Type> {
        Type::from_semantic(
            Ty::Enum(NominalTy {
                declaration: self.id,
                arguments: self
                    .declaration
                    .generic_params
                    .iter()
                    .map(GenericParam::as_type)
                    .collect(),
                associated_types: BTreeMap::new(),
            }),
            &self.module.providers.definitions(),
        )
    }

    pub fn variant(
        &mut self,
        name: impl Into<String>,
        payload: impl IntoIterator<Item = Type>,
    ) -> NativeResult<VariantRef> {
        let name = name.into();
        if self
            .declaration
            .variants
            .iter()
            .any(|variant| variant.name == name)
        {
            return Err(RuntimeError::metadata_conflict("duplicate enum variant"));
        }
        let id = self
            .module
            .context()
            .intern_child(self.id, DefinitionKind::Variant, &name, 0)
            .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
        self.declaration.variants.push(VariantDef {
            reports_failure: false,
            name,
            payload: payload
                .into_iter()
                .map(|ty| ty.adopt(self.module.context()))
                .collect::<NativeResult<_>>()?,
        });
        Ok(VariantRef {
            id,
            definitions: self.module.providers.definitions(),
        })
    }

    pub fn variant_documentation(
        &mut self,
        variant: &VariantRef,
        text: impl Into<String>,
    ) -> NativeResult<()> {
        if !self.declaration.variants.iter().any(|member| {
            ModuleDecl::variant_id(self.id, &member.name, &self.module.providers.definitions()).ok()
                == Some(variant.id)
        }) {
            return Err(RuntimeError::metadata_conflict(
                "variant belongs to another enum",
            ));
        }
        self.documentation.insert(variant.id, text.into());
        Ok(())
    }

    /// Validate before publishing any declaration, documentation or provider entry.
    pub fn finish(self) -> NativeResult<TypeRef> {
        let mut declaration = ModuleDecl::new(self.module.declaration.identity.clone());
        declaration.types.push(self.declaration.clone());
        declaration
            .validate(&|_| None, &self.module.providers.definitions())
            .map_err(|cause| RuntimeError::metadata_conflict(cause.to_string()))?;
        if self
            .module
            .declaration
            .types
            .iter()
            .any(|ty| ty.name == self.declaration.name)
        {
            return Err(RuntimeError::metadata_conflict(
                "duplicate enum declaration",
            ));
        }
        self.module
            .providers
            .insert_type(self.id, self.declaration.clone())?;
        self.module
            .declaration
            .documentation
            .extend(self.documentation);
        self.module.declaration.types.push(self.declaration.clone());
        Ok(TypeRef {
            id: self.id,
            definitions: self.module.providers.definitions(),
            declaration: Arc::new(self.declaration),
            parameter_names: self.parameter_names,
        })
    }
}
