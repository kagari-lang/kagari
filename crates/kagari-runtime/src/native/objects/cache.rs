//! Weak preparation caches never keep an otherwise dead program version alive.
use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    frame::types::arguments::TypeArgument,
    module::{ExecutableModule, Module, ModuleEpochRetention, StructLayoutRef},
    native::{
        binding::NativeResult,
        objects::{ObjectType, TypeRecord, fields::FieldRecord},
    },
};
use kagari_bytecode::instruction::StructId;
use kagari_common::identity::{DefinitionKind, DefinitionPath, table::DefinitionId};
use kagari_contract::{
    exports::{
        TypeExport,
        path::{PublicPathKind, PublicPathTarget},
    },
    types::PublicItem,
};
use kagari_types::{declaration::TypeDefKind, ty::Ty};
use std::{
    slice,
    sync::{Arc, Weak},
};

#[derive(Debug, Default)]
pub(crate) struct BindingCache {
    types: Vec<Weak<TypeRecord>>,
    pub(super) fields: Vec<Weak<FieldRecord>>,
}

impl Runtime {
    /// Bind a public script object path relative to the Module's current namespace.
    /// Renamed declarations keep their explicit argument scopes; closed aliases
    /// use their verified normalized application and accept no host arguments.
    /// Non-object types, open aliases and wrong arity report `TypedPathValidation`.
    /// Generic declarations require existing executable layout evidence; binding
    /// never compiles or specializes a missing application.
    pub fn bind_type(
        &self,
        owner: &Module,
        path: &str,
        arguments: &[TypeArgument],
    ) -> NativeResult<ObjectType> {
        self.gc().ensure_no_native_borrow()?;
        let snapshot = self.capture_module(owner)?;
        let export = snapshot.resolve_public_path(path, PublicPathKind::Type)?;
        let PublicPathTarget::Type(export) = export else {
            return Err(invalid_export());
        };
        let owner = &snapshot.root;
        match export {
            TypeExport::Declaration(declaration) => {
                let arity = object_declaration_arity(owner, *declaration, path)?;
                if arguments.len() != arity {
                    return Err(object_path_error(path, "type argument count mismatch"));
                }
                self.prepare_object_application(owner, *declaration, arguments)
            }
            TypeExport::Closed(ty @ Ty::Struct(_)) => {
                if !arguments.is_empty() {
                    return Err(object_path_error(path, "type argument count mismatch"));
                }
                let applied = self
                    .resolve_type_arguments(owner, slice::from_ref(ty))?
                    .pop()
                    .ok_or_else(invalid_export)?;
                let layout = self.prepare_struct_layout(owner, &applied)?;
                self.retain_public_object_type(layout)
            }
            TypeExport::Closed(_) => Err(object_path_error(path, "type is not a script object")),
            TypeExport::OpenAlias { .. } => Err(object_path_error(
                path,
                "open generic alias is not supported for host binding",
            )),
        }
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
        let declaration = snapshot
            .root
            .definitions()
            .lookup(declaration)
            .ok_or_else(|| {
                RuntimeError::module_validation("missing executable struct application")
            })?;
        self.prepare_object_application(&snapshot.root, declaration, arguments)
    }

    fn prepare_object_application(
        &self,
        owner: &ExecutableModule,
        declaration: DefinitionId,
        arguments: &[TypeArgument],
    ) -> NativeResult<ObjectType> {
        for argument in arguments {
            argument.validate(self)?;
        }
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
        self.retain_public_object_type(layout)
    }

    fn retain_public_object_type(&self, layout: StructLayoutRef) -> NativeResult<ObjectType> {
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

fn object_declaration_arity(
    owner: &ExecutableModule,
    declaration: DefinitionId,
    path: &str,
) -> NativeResult<usize> {
    let definition = owner
        .definitions()
        .resolve(declaration)
        .map_err(|_| invalid_export())?;
    let mut segments = definition.segments();
    let segment = segments.next().ok_or_else(invalid_export)?;
    if segments.next().is_some() || segment.occurrence != 0 {
        return Err(invalid_export());
    }
    let member = owner
        .members()
        .find(|member| &member.bytecode.identity == definition.module())
        .ok_or_else(invalid_export)?;
    for item in &member.bytecode.public_items {
        match item {
            PublicItem::Type(record)
                if record.name == segment.name && record.kind.definition_kind() == segment.kind =>
            {
                return if record.kind == TypeDefKind::Struct {
                    Ok(record.generic_params.len())
                } else {
                    Err(object_path_error(path, "type is not a script object"))
                };
            }
            PublicItem::Trait(record)
                if record.name == segment.name && segment.kind == DefinitionKind::Trait =>
            {
                return Err(object_path_error(path, "type is not a script object"));
            }
            _ => {}
        }
    }
    Err(invalid_export())
}

fn object_path_error(path: &str, detail: &str) -> RuntimeError {
    RuntimeError::typed_path_validation(format!("{detail}: {path:?} (expected script object)"))
}

fn invalid_export() -> RuntimeError {
    RuntimeError::new(
        RuntimeErrorKind::EngineFault,
        "invalid verified public object export",
    )
}

#[cfg(test)]
mod tests {
    use crate::Runtime;
    use kagari_compiler::{
        bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir,
    };
    use kagari_hir::analysis::AnalysisDatabase;
    use kagari_source::source_database::{SourceDatabase, SourceLayer};
    use kagari_types::ty::Ty;

    #[test]
    fn closed_object_alias_preserves_private_fields_and_advanced_declaration_identity() {
        let mut sources = SourceDatabase::default();
        let root = sources
            .set(
                "object-exports.kgr",
                r#"
            mod hidden {
                pub struct User { pub var visible: i32, val secret: i32 }
            }
            pub type Player = hidden::User;
            fn main() {}
        "#
                .into(),
                SourceLayer::Base,
            )
            .unwrap();
        let mut analysis = AnalysisDatabase::default();
        let snapshot = analysis
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        let checked = snapshot.check_program(root, &Default::default()).unwrap();
        let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
        let bytecode = lower_program_to_bytecode(&mir, &Default::default())
            .unwrap()
            .into_unverified();
        let mut runtime = Runtime::default();
        let program = runtime.load_program("objects", bytecode).unwrap();
        let allocated = runtime.gc().allocated_objects();
        let player = runtime.bind_type(&program.root(), "Player", &[]).unwrap();
        assert!(player.field("visible").is_ok());
        assert!(player.field("secret").is_err());
        assert!(player.builder().is_err());
        let Ty::Struct(nominal) = player.type_argument().ty() else {
            panic!("object binding must retain its nominal struct type");
        };
        let declaration = player
            .owner()
            .definitions()
            .resolve(nominal.declaration)
            .unwrap()
            .to_path();
        let advanced = runtime
            .bind_type_declaration(&program.root(), &declaration, &[])
            .unwrap();
        assert_eq!(player.type_argument().ty(), advanced.type_argument().ty());

        assert_eq!(runtime.gc().allocated_objects(), allocated);
    }
}
