//! Shared nominal fixtures author external identities through the common catalog.
use kagari::engine::KagariEngine;
use kagari_common::identity::{
    DefinitionKind, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_runtime::{
    host::{HostCallContext, HostObjectId},
    native::{
        builder::ModuleBuilder, declarations::FunctionDecl, module::NativeModule, types::TypeRef,
    },
    value::Value,
};
use kagari_types::{
    declaration::{
        FieldDef, TypeDeclaration,
        host::{
            HostCallPolicy, HostFieldPolicy, HostFunctionEffects, HostReflectionPolicy,
            HostTypeOwnership, HostTypePolicy, PathAccess,
        },
    },
    ty::Ty,
    visibility::Visibility,
};
use std::sync::{Arc, Mutex};

pub(super) struct NominalSchema {
    pub builder: ModuleBuilder,
    pub providers: [NativeModule; 2],
    pub item: TypeRef,
}

impl NominalSchema {
    pub fn new() -> Self {
        let catalog = kagari_stdlib::declarations::StandardDeclarations::default()
            .catalog()
            .unwrap();
        let policy = HostTypePolicy {
            ownership: HostTypeOwnership::HostRoot,
            reflection: HostReflectionPolicy::Hidden,
            path_access: PathAccess::ReadOnly,
        };
        let mut right = ModuleBuilder::new("right", &catalog).unwrap();
        let mut related = right.define_type("Item").unwrap();
        related
            .external(HostTypePolicy {
                ownership: HostTypeOwnership::Opaque,
                path_access: PathAccess::None,
                ..policy
            })
            .unwrap();
        let related = related.finish().unwrap();
        let related_path = related
            .definitions()
            .resolve(related.id())
            .unwrap()
            .to_path();
        let right = right.finish().unwrap();
        let mut unused = ModuleBuilder::new("unused", &catalog).unwrap();
        let mut other = unused.define_type("Other").unwrap();
        other
            .external(HostTypePolicy {
                ownership: HostTypeOwnership::Opaque,
                path_access: PathAccess::None,
                ..policy
            })
            .unwrap();
        other.finish().unwrap();
        let unused = unused.finish().unwrap();
        let mut builder = ModuleBuilder::new("left", &catalog)
            .unwrap()
            .with_modules(&[&right, &unused])
            .unwrap();
        let related = builder
            .declarations()
            .type_reference(&related_path)
            .unwrap();
        let mut item = builder.define_type("Item").unwrap();
        item.external(policy).unwrap();
        item.field(
            DefinitionMetadata::checked(
                related.definitions().clone(),
                FieldDef {
                    host_policy: Some(HostFieldPolicy {
                        readable: true,
                        path_access: PathAccess::None,
                    }),
                    visibility: Visibility::Public,
                    name: "related".into(),
                    ty: Ty::Host(related.id()),
                    mutable: false,
                },
                &Default::default(),
            )
            .unwrap(),
        )
        .unwrap();
        let item = item.finish().unwrap();
        Self {
            builder,
            providers: [right, unused],
            item,
        }
    }

    pub fn make(&mut self, trace: Arc<Mutex<Vec<&'static str>>>, label: &'static str) {
        let make = self
            .builder
            .define_function(
                FunctionDecl::new("make")
                    .returns(self.item.apply([]).unwrap())
                    .host_policy(HostCallPolicy {
                        passing: vec![],
                        effects: HostFunctionEffects {
                            may_allocate: true,
                            ..Default::default()
                        },
                    }),
            )
            .unwrap();
        self.builder
            .bind(make, move |cx: &HostCallContext<'_>, _: &[Value]| {
                trace.lock().unwrap().push(label);
                let root = cx.runtime().host().root(HostObjectId(7)).unwrap();
                Ok(cx.runtime().gc().alloc_host_root(root).unwrap())
            })
            .unwrap();
    }

    pub fn engine(self) -> KagariEngine {
        let [right, unused] = self.providers;
        KagariEngine::with_native_modules(
            Default::default(),
            vec![right, unused, self.builder.finish().unwrap()],
        )
        .unwrap()
    }

    /// Expected transitive schemas omit the installed, unrelated Other type.
    pub fn required_types(&self, context: &DefinitionContext) -> Vec<TypeDeclaration> {
        let left = self.builder.clone().finish().unwrap();
        [&left, &self.providers[0]]
            .into_iter()
            .map(|module| {
                let definition = module.declaration().types[0].clone();
                let root = module
                    .definitions()
                    .lookup_root(&module.declaration().identity)
                    .unwrap();
                let declaration = module
                    .definitions()
                    .lookup_child(root, DefinitionKind::Struct, &definition.name, 0)
                    .unwrap();
                DefinitionMetadata::checked(
                    module.definitions().clone(),
                    TypeDeclaration {
                        declaration,
                        definition,
                    },
                    &Default::default(),
                )
                .unwrap()
                .import_into(context, &Default::default())
                .unwrap()
                .into_records()
            })
            .collect()
    }
}
