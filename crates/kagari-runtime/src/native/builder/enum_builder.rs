//! Ordinary nominal enum declarations authored through native registration.
use crate::{
    error::RuntimeError,
    native::{
        binding::NativeResult,
        builder::ModuleBuilder,
        types::{ParameterRef, Type, TypeRef, VariantRef},
    },
};
use kagari_common::identity::{DefinitionKind, DefinitionPath};
use kagari_types::{
    declaration::{TypeDef, TypeDefKind, VariantDef, module::ModuleDecl},
    ty::{GenericParam, NominalTy, Ty},
};
use std::{collections::BTreeMap, sync::Arc};

pub struct EnumBuilder<'module> {
    module: &'module mut ModuleBuilder,
    id: DefinitionPath,
    declaration: TypeDef,
    parameter_names: Vec<String>,
    documentation: BTreeMap<DefinitionPath, String>,
}

impl<'module> EnumBuilder<'module> {
    pub(crate) fn new(module: &'module mut ModuleBuilder, name: String) -> Self {
        Self {
            id: module.declaration.definition(DefinitionKind::Enum, &name),
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
        }
    }

    /// Preserve complete Markdown for the enum's generated declaration.
    pub fn documentation(&mut self, text: impl Into<String>) {
        self.documentation.insert(self.id.clone(), text.into());
    }

    pub fn type_parameter(&mut self, name: impl Into<String>) -> NativeResult<ParameterRef> {
        let name = name.into();
        if self.parameter_names.contains(&name) {
            return Err(RuntimeError::metadata_conflict(
                "duplicate enum type parameter",
            ));
        }
        let parameter = GenericParam {
            owner: self.id.clone(),
            position: self.declaration.generic_params.len(),
        };
        self.parameter_names.push(name);
        self.declaration.generic_params.push(parameter.clone());
        Ok(ParameterRef {
            ty: Type(parameter.as_type()),
        })
    }

    /// Refer to the current enum with its own parameters, including in recursive payloads.
    pub fn self_type(&self) -> Type {
        Type(Ty::Enum(NominalTy {
            declaration: self.id.clone(),
            arguments: self
                .declaration
                .generic_params
                .iter()
                .map(GenericParam::as_type)
                .collect(),
            associated_types: BTreeMap::new(),
        }))
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
        let id = ModuleDecl::variant_id(&self.id, &name);
        self.declaration.variants.push(VariantDef {
            reports_failure: false,
            name,
            payload: payload.into_iter().map(|ty| ty.0).collect(),
        });
        Ok(VariantRef { id })
    }

    pub fn variant_documentation(
        &mut self,
        variant: &VariantRef,
        text: impl Into<String>,
    ) -> NativeResult<()> {
        if !self
            .declaration
            .variants
            .iter()
            .any(|member| ModuleDecl::variant_id(&self.id, &member.name) == variant.id)
        {
            return Err(RuntimeError::metadata_conflict(
                "variant belongs to another enum",
            ));
        }
        self.documentation.insert(variant.id.clone(), text.into());
        Ok(())
    }

    /// Validate before publishing any declaration, documentation or provider entry.
    pub fn finish(self) -> NativeResult<TypeRef> {
        let mut declaration = ModuleDecl::new(self.id.module.clone());
        declaration.types.push(self.declaration.clone());
        declaration
            .validate(&|_| None)
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
            .insert_type(self.id.clone(), self.declaration.clone())?;
        self.module
            .declaration
            .documentation
            .extend(self.documentation);
        self.module.declaration.types.push(self.declaration.clone());
        Ok(TypeRef {
            id: self.id,
            declaration: Arc::new(self.declaration),
            parameter_names: self.parameter_names,
        })
    }
}
