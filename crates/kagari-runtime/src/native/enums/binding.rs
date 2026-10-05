use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::{arguments::TypeArgument, bindings::TypeBindings, compatibility::TypeView},
    module::{LoadedModule, ModuleEpochRetention},
    native::{
        binding::NativeResult,
        conversion::{arguments::KagariArguments, context::ConversionContext},
        enums::{EnumMember, EnumType, EnumVariant, TypeRecord, VariantRecord},
    },
};
use kagari_common::identity::{DefinitionKind, DefinitionPath, DefinitionPathSegment};
use kagari_contract::types::PublicItem;
use kagari_types::{
    declaration::TypeDefKind,
    ty::{NominalTy, Ty},
};
use std::{marker::PhantomData, slice, sync::Arc};

impl Runtime {
    pub fn bind_enum_type(
        &self,
        owner: &LoadedModule,
        name: &str,
        arguments: &[TypeArgument],
    ) -> NativeResult<EnumType> {
        let declaration = DefinitionPath {
            module: owner.bytecode.identity.clone(),
            path: vec![DefinitionPathSegment {
                kind: DefinitionKind::Enum,
                name: name.into(),
                occurrence: 0,
            }],
        };
        self.bind_enum_type_declaration(owner, &declaration, arguments)
    }

    pub fn bind_enum_type_declaration(
        &self,
        owner: &LoadedModule,
        declaration: &DefinitionPath,
        arguments: &[TypeArgument],
    ) -> NativeResult<EnumType> {
        self.gc().ensure_no_native_borrow()?;
        self.resources().ensure_execution_allowed()?;
        self.validate_loaded_module(owner)?;
        for argument in arguments {
            argument.validate(self)?;
        }
        let [segment] = declaration.path.as_slice() else {
            return Err(invalid());
        };
        if segment.kind != DefinitionKind::Enum || segment.occurrence != 0 {
            return Err(invalid());
        }
        let member = owner
            .members()
            .find(|member| member.bytecode.identity == declaration.module)
            .ok_or_else(invalid)?;
        let declared = member
            .bytecode
            .public_items
            .iter()
            .find_map(|item| match item {
                PublicItem::Type(ty) if ty.kind == TypeDefKind::Enum && ty.name == segment.name => {
                    Some(ty)
                }
                _ => None,
            })
            .ok_or_else(invalid)?;
        if declared.generic_params.len() != arguments.len() {
            return Err(invalid());
        }
        let id = member
            .definitions()
            .lookup(declaration)
            .ok_or_else(invalid)?;
        let types: Vec<_> = arguments
            .iter()
            .map(|argument| argument.ty().clone())
            .collect();
        let layouts: Vec<_> = member
            .bytecode
            .enumerations
            .iter()
            .filter(|layout| layout.declaration == id && layout.accepts(&types))
            .filter(|layout| {
                layout
                    .arguments
                    .iter()
                    .zip(arguments)
                    .all(|(compiled, supplied)| {
                        !compiled.is_concrete()
                            || supplied
                                .view(owner)
                                .compatible(TypeView::new(compiled, &member, None))
                    })
            })
            .collect();
        if layouts.is_empty() {
            return Err(invalid());
        }
        // An open layout establishes representation, not new bounded applications.
        // Only a concrete checked layout supplies evidence for declared bounds.
        if !declared.bounds.is_empty() && !layouts.iter().any(|layout| layout.arguments == types) {
            return Err(invalid());
        }
        let environment = Arc::new(TypeBindings::new(
            self.definition_context(),
            declared.generic_params.clone(),
            arguments.to_vec(),
        )?);
        let expression = Ty::Enum(NominalTy {
            declaration: id,
            arguments: declared
                .generic_params
                .iter()
                .map(|parameter| parameter.as_type())
                .collect(),
            associated_types: Default::default(),
        });
        let argument = self
            .type_arguments(&member, Some(environment), &[expression])?
            .pop()
            .ok_or_else(invalid)?;
        let program = self
            .retain_program(&member, ModuleEpochRetention::RuntimeValue)
            .ok_or_else(invalid)?;
        Ok(EnumType(Arc::new(TypeRecord {
            owner: member,
            argument,
            _program: program,
        })))
    }

    pub fn bind_enum_variant<A: KagariArguments>(
        &self,
        ty: &EnumType,
        name: &str,
    ) -> NativeResult<EnumVariant<A>> {
        self.bind_enum_variant_declaration(&ty.variant(self, name)?)
    }

    pub fn bind_enum_variant_declaration<A: KagariArguments>(
        &self,
        member: &EnumMember,
    ) -> NativeResult<EnumVariant<A>> {
        self.gc().ensure_no_native_borrow()?;
        self.resources().ensure_execution_allowed()?;
        self.validate_loaded_module(member.enum_type.owner())?;
        let mut payload = Vec::new();
        for index in 0..member.layout.variant().payload.len() {
            let (ty, _) = member.layout.payload_type(index).ok_or_else(invalid)?;
            payload.push(
                self.type_arguments(
                    member.layout.module(),
                    member.layout.environment.clone(),
                    slice::from_ref(ty),
                )?
                .pop()
                .ok_or_else(invalid)?,
            );
        }
        let cx = ConversionContext::new(self, member.enum_type.owner())?;
        A::check_types(&cx, &payload)?;
        Ok(EnumVariant {
            record: Arc::new(VariantRecord {
                member: member.clone(),
                payload,
            }),
            mapping: PhantomData,
        })
    }
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("enum constructor requires a public checked application")
}
