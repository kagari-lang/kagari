//! Weak preparation caches never keep an otherwise dead program version alive.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    module::{ExecutableModule, Module, ModuleEpochRetention, StructLayoutRef},
    native::{
        binding::NativeResult,
        objects::{ObjectType, TypeRecord, fields::FieldRecord},
    },
};
use kagari_bytecode::instruction::StructId;
use kagari_common::identity::{DefinitionKind, DefinitionPath, DefinitionPathSegment};
use kagari_contract::types::PublicItem;
use kagari_types::declaration::TypeDefKind;
use std::sync::{Arc, Weak};

#[derive(Debug, Default)]
pub(crate) struct BindingCache {
    types: Vec<Weak<TypeRecord>>,
    pub(super) fields: Vec<Weak<FieldRecord>>,
}

impl Runtime {
    /// Resolve a public script struct in the current root publication once.
    /// Generic arguments retain their lexical scopes and require an executable
    /// layout. Module-relative public type paths are introduced by HM03.
    pub fn bind_type(
        &self,
        owner: &Module,
        name: &str,
        arguments: &[TypeArgument],
    ) -> NativeResult<ObjectType> {
        self.gc().ensure_no_native_borrow()?;
        let snapshot = self.capture_module(owner)?;
        let owner = &snapshot.root;
        let declaration = DefinitionPath {
            module: owner.bytecode.identity.clone(),
            path: vec![DefinitionPathSegment {
                kind: DefinitionKind::Struct,
                name: name.into(),
                occurrence: 0,
            }],
        };
        self.prepare_object_type(owner, &declaration, arguments)
    }

    /// Bind a public declaration/application within the Module's installation.
    /// This advanced route does not grant private namespace navigation.
    pub fn bind_type_declaration(
        &self,
        owner: &Module,
        declaration: &DefinitionPath,
        arguments: &[TypeArgument],
    ) -> NativeResult<ObjectType> {
        self.gc().ensure_no_native_borrow()?;
        let snapshot = self.capture_module(owner)?;
        self.prepare_object_type(&snapshot.root, declaration, arguments)
    }

    fn prepare_object_type(
        &self,
        owner: &ExecutableModule,
        declaration: &DefinitionPath,
        arguments: &[TypeArgument],
    ) -> NativeResult<ObjectType> {
        for argument in arguments {
            argument.validate(self)?;
        }
        let declaration = owner.definitions().lookup(declaration).ok_or_else(|| {
            RuntimeError::module_validation("missing executable struct application")
        })?;
        let types = arguments.iter().map(|a| a.ty().clone()).collect::<Vec<_>>();
        let (member, id) = owner
            .members()
            .find_map(|member| {
                let (index, _) = member
                    .bytecode
                    .structures
                    .iter()
                    .enumerate()
                    .filter(|(_, layout)| {
                        layout.declaration == declaration && layout.accepts(&types)
                    })
                    .max_by_key(|(_, layout)| {
                        !layout.arguments.iter().all(|ty| ty.is_concrete())
                    })?;
                Some((member, StructId::new(index)))
            })
            .ok_or_else(|| {
                RuntimeError::module_validation("missing executable struct application")
            })?;
        let layout = self.prepare_struct_application(&member, id, arguments)?;
        let result = self.retain_object_type(layout)?;
        if !result.0.public {
            return Err(RuntimeError::module_validation("object type is not public"));
        }
        Ok(result)
    }

    /// Share the same retained layout owner with nominal DTO conversion.
    pub(crate) fn retain_object_type(&self, layout: StructLayoutRef) -> NativeResult<ObjectType> {
        self.validate_executable_module(layout.module())?;
        let mut cache = self
            .object_bindings
            .try_borrow_mut()
            .map_err(|_| RuntimeError::module_validation("object binding cache is borrowed"))?;
        cache.types.retain(|record| record.strong_count() != 0);
        if let Some(record) = cache.types.iter().filter_map(Weak::upgrade).find(|record| {
            record.layout.module().program_root().key() == layout.module().program_root().key()
                && record.layout.matches(&layout)
        }) {
            return Ok(ObjectType(record));
        }
        let argument = self
            .type_arguments(
                layout.module(),
                layout.type_bindings().cloned(),
                &[layout.type_expression()],
            )?
            .pop()
            .ok_or_else(|| RuntimeError::module_validation("object type scope"))?;
        let definition = layout.module().definition(layout.layout().declaration)?;
        let public = layout.module().members().any(|member| {
            &member.bytecode.identity == definition.module()
                && member.bytecode.public_items.iter().any(|item| {
                    matches!(item,
                    PublicItem::Type(ty) if ty.kind == TypeDefKind::Struct
                        && definition.segments().last().is_some_and(|part| part.name == ty.name))
                })
        });
        let program = self
            .retain_program(layout.module(), ModuleEpochRetention::RuntimeValue)
            .ok_or_else(|| RuntimeError::module_validation("object type program retention"))?;
        let record = Arc::new(TypeRecord {
            layout,
            argument,
            public,
            _program: program,
        });
        cache
            .types
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("object binding cache"))?;
        cache.types.push(Arc::downgrade(&record));
        Ok(ObjectType(record))
    }
}
