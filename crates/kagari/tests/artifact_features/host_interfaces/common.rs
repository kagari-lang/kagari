use crate::standard_library;
use kagari::{BytecodeArtifact, engine::KagariEngine};
use kagari_bytecode::program::{ModuleRef, verified::VerifiedBytecodeProgram};
use kagari_common::identity::{
    DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity,
    metadata::DefinitionMetadata, table::DefinitionId,
};
use kagari_runtime::{
    Runtime,
    error::RuntimeErrorKind,
    host::{HostCallContext, HostError, HostObjectId, HostRootHandle, HostSchemaEpoch},
    module::ExecutableModule,
    native::{
        builder::ModuleBuilder,
        declarations::FunctionDecl,
        host,
        module::NativeModule,
        types::{Type, TypeRef},
    },
    value::{kind::ObjectKind, view::ValueView},
};
use kagari_source::source_database::SourceLayer;
use kagari_types::declaration::host::{
    HostCallPolicy, HostFunctionEffects, HostPassingStyle, HostReflectionPolicy, HostTypeOwnership,
    HostTypePolicy, PathAccess,
};
use std::sync::{Arc, Mutex};

pub const URI: &str = "mem://host-interface";

pub struct Config {
    pub source: String,
    pub trait_source: String,
    pub output: fn(&TypeRef) -> Type,
    pub owner: Option<ModuleIdentity>,
    pub reader_arguments: Vec<Type>,
    pub child: bool,
    pub consumer: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            source: super::SOURCE.into(),
            trait_source: super::SOURCE
                .split("\nfn read<")
                .next()
                .unwrap()
                .split_inclusive('\n')
                .filter(|line| !line.trim_start().starts_with("use demo::"))
                .collect(),
            output: |_| Type::i32(),
            owner: None,
            reader_arguments: vec![],
            child: false,
            consumer: None,
        }
    }
}

pub struct Fixture {
    pub engine: KagariEngine,
    pub artifact: DefinitionMetadata<BytecodeArtifact>,
    pub module: NativeModule,
    pub trace: Arc<Mutex<Vec<i32>>>,
    method: DefinitionPath,
}

impl Fixture {
    pub fn new(
        config: Config,
        read: impl for<'call> Fn(
            &mut HostCallContext<'call>,
            &[ValueView<'call>],
        ) -> Result<ValueView<'call>, HostError>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        let (mut module, owner) = pinned_builder(&config.trait_source, config.owner);
        let path = trait_path(&owner, "Reader");
        let mut method = path.clone();
        method.path.push(DefinitionPathSegment {
            kind: DefinitionKind::Method,
            name: "read".into(),
            occurrence: 0,
        });
        let reader = module.reference_trait(&path).unwrap();
        let child = config.child.then(|| {
            module
                .reference_trait(&trait_path(&owner, "Child"))
                .unwrap()
        });
        let mut counter = module.define_type("Counter").unwrap();
        counter
            .external(HostTypePolicy {
                ownership: HostTypeOwnership::HostRoot,
                reflection: HostReflectionPolicy::Hidden,
                path_access: PathAccess::ReadOnly,
            })
            .unwrap();
        let counter = counter.finish().unwrap();
        let trace = Arc::new(Mutex::new(Vec::new()));
        let calls = trace.clone();
        module
            .implement(counter.clone(), |group| {
                let receiver = group.receiver();
                group.trait_impl(reader.apply(config.reader_arguments)?, |methods| {
                    let output = (config.output)(&counter);
                    methods.associated_type("Item", output.clone())?;
                    let read_method_policy = HostCallPolicy {
                        passing: vec![HostPassingStyle::SharedBorrow, HostPassingStyle::Owned],
                        effects: HostFunctionEffects {
                            may_allocate: true,
                            may_trap: true,
                            may_call_host_services: true,
                            may_mutate_host_state: true,
                            ..Default::default()
                        },
                    };
                    let read_method = FunctionDecl::new("read")
                        .parameter("self", receiver)
                        .parameter("amount", Type::i32())
                        .returns(output)
                        .host_policy(read_method_policy.clone());
                    methods.define_method(
                        read_method,
                        host::callback(read_method_policy, move |cx, args| {
                            let [receiver, amount] = args else {
                                panic!("checked host arguments")
                            };
                            assert_eq!(receiver.kind(), Some(ObjectKind::HostRoot));
                            let amount = amount.as_i32().expect("checked host scalar");
                            calls.lock().unwrap().push(amount);
                            let root = cx.runtime().host().root(HostObjectId(7)).unwrap();
                            assert_eq!(
                                cx.borrows()
                                    .borrow_unique(root.object_id(), root.type_id())
                                    .unwrap_err()
                                    .kind(),
                                RuntimeErrorKind::HostBorrowConflict
                            );
                            read(cx, args)
                        }),
                    )
                })
            })
            .unwrap();
        if let Some(child) = child {
            module
                .implement(counter.clone(), |group| {
                    group.trait_impl(child.apply([])?, |_| Ok(()))
                })
                .unwrap();
        }
        let make_policy = HostCallPolicy {
            passing: vec![],
            effects: HostFunctionEffects {
                may_allocate: true,
                ..Default::default()
            },
        };
        let make = module
            .define_function(
                FunctionDecl::new("make")
                    .returns(counter.apply([]).unwrap())
                    .host_policy(make_policy.clone()),
            )
            .unwrap();
        let calls = trace.clone();
        module
            .bind(
                make,
                host::callback(make_policy, move |cx, _| {
                    calls.lock().unwrap().push(0);
                    let root = cx.runtime().host().root(HostObjectId(7)).unwrap();
                    let root = cx.runtime_mut().host_root_value(root).unwrap();
                    cx.import(&root)
                        .map_err(|error| HostError::new(error.to_string()))
                }),
            )
            .unwrap();
        let module = module.finish().unwrap();
        let engine = KagariEngine::with_native_modules(
            Default::default(),
            standard_library()
                .modules()
                .unwrap()
                .into_iter()
                .chain(vec![module.clone()])
                .collect(),
        )
        .unwrap();
        let mut file = engine.bind_module(URI, path.module.clone()).unwrap();
        engine
            .set_source(URI, config.source, SourceLayer::Base)
            .unwrap();
        if let Some(consumer) = config.consumer {
            file = engine
                .bind_module(
                    "mem://consumer",
                    ModuleIdentity {
                        package: path.module.package,
                        path: vec!["consumer".into()],
                    },
                )
                .unwrap();
            engine
                .set_source("mem://consumer", consumer, SourceLayer::Base)
                .unwrap();
        }
        let checked = engine
            .compile_snapshot(engine.source_snapshot(), file, &Default::default())
            .unwrap();
        let artifact = engine.emit_artifact(&checked, Default::default()).unwrap();
        assert!(
            trace.lock().unwrap().is_empty(),
            "cold compilation called Rust"
        );
        Self {
            engine,
            artifact,
            module,
            trace,
            method,
        }
    }

    pub fn runtime(&self) -> Runtime {
        let mut runtime = Runtime::default();
        let mut modules = standard_library().modules().unwrap();
        modules.push(self.module.clone());
        NativeModule::install_all(&modules, &mut runtime).unwrap();
        runtime
    }

    pub fn table_owner(&self, loaded: &ExecutableModule) -> ExecutableModule {
        let slot = loaded
            .verified_program()
            .modules()
            .iter()
            .position(|module| module.identity == self.module.declaration().identity)
            .unwrap();
        loaded.member(ModuleRef::new(slot)).unwrap()
    }

    pub fn method(&self, loaded: &ExecutableModule) -> DefinitionId {
        loaded.definitions().lookup(&self.method).unwrap()
    }
}

pub fn register_root(runtime: &mut Runtime) -> HostRootHandle {
    let ty = runtime.types().get_by_name("demo::Counter").unwrap().id;
    runtime
        .register_host_root(HostObjectId(7), ty, HostSchemaEpoch::new(0))
        .unwrap()
}

pub fn pinned_builder(
    source: &str,
    owner: Option<ModuleIdentity>,
) -> (ModuleBuilder, ModuleIdentity) {
    let author = KagariEngine::with_native_modules(
        Default::default(),
        standard_library().modules().unwrap(),
    )
    .unwrap();
    if let Some(owner) = owner {
        author.bind_module(URI, owner).unwrap();
    }
    let file = author
        .set_source(URI, source.into(), SourceLayer::Base)
        .unwrap();
    let checked = author
        .compile_snapshot(author.source_snapshot(), file, &Default::default())
        .unwrap();
    let artifact = author.emit_artifact(&checked, Default::default()).unwrap();
    let baseline = VerifiedBytecodeProgram::from_metadata(
        super::artifact_program(artifact),
        &Default::default(),
    )
    .unwrap();
    let owner = &baseline.program().modules[baseline.program().root.index()].identity;
    let catalog = standard_library()
        .declarations()
        .catalog()
        .with_verified_modules(&baseline, [owner])
        .unwrap();
    (ModuleBuilder::new("demo", &catalog).unwrap(), owner.clone())
}

pub fn trait_path(owner: &ModuleIdentity, name: &str) -> DefinitionPath {
    DefinitionPath {
        module: owner.clone(),
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Trait,
            name: name.into(),
            occurrence: 0,
        }],
    }
}
