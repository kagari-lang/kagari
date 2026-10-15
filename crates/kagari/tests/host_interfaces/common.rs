use kagari::{BytecodeArtifact, engine::KagariEngine};
use kagari_bytecode::program::ModuleRef;
use kagari_common::identity::{
    DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity,
    metadata::DefinitionMetadata, table::DefinitionId,
};
use kagari_contract::types::PublicItem;
use kagari_runtime::{
    Runtime,
    host::{HostCallContext, HostError, HostObjectId, HostRootHandle, HostSchemaEpoch},
    module::LoadedModule,
    native::{
        builder::ModuleBuilder,
        declarations::FunctionDecl,
        module::NativeModule,
        types::{TraitRef, Type, TypeRef},
    },
    value::Value,
};
use kagari_source::source_database::SourceLayer;
use kagari_stdlib::declarations::StandardDeclarations;
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
                .replace("use demo::{Counter, make};", ""),
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
        read: impl Fn(&HostCallContext<'_>, &[Value]) -> Result<Value, HostError>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        let contract = source_contract(&config.trait_source, config.owner);
        let source = &contract.records().program.modules[contract.records().program.root.index()];
        let path = DefinitionPath {
            module: source.identity.clone(),
            path: vec![DefinitionPathSegment {
                kind: DefinitionKind::Trait,
                name: "Reader".into(),
                occurrence: 0,
            }],
        };
        let mut method = path.clone();
        method.path.push(DefinitionPathSegment {
            kind: DefinitionKind::Method,
            name: "read".into(),
            occurrence: 0,
        });
        let catalog = StandardDeclarations::default().catalog().unwrap();
        let mut module = ModuleBuilder::new("demo", &catalog).unwrap();
        let reader = reference_trait(&mut module, &contract, "Reader");
        let child = config
            .child
            .then(|| reference_trait(&mut module, &contract, "Child"));
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
                group.trait_impl(reader.apply(config.reader_arguments)?, |methods| {
                    methods.associated_type("Item", (config.output)(&counter))?;
                    methods.host_policy(
                        "read",
                        HostCallPolicy {
                            passing: vec![HostPassingStyle::SharedBorrow, HostPassingStyle::Owned],
                            effects: HostFunctionEffects {
                                may_allocate: true,
                                may_trap: true,
                                may_call_host_services: true,
                                may_mutate_host_state: true,
                                ..Default::default()
                            },
                        },
                    )?;
                    methods.bind("read", move |cx: &HostCallContext<'_>, args: &[Value]| {
                        let [Value::HostRoot(_), Value::I32(amount)] = args else {
                            panic!("checked host arguments")
                        };
                        calls.lock().unwrap().push(*amount);
                        let root = cx.runtime().host().root(HostObjectId(7)).unwrap();
                        assert!(
                            cx.borrows()
                                .borrow_unique(root.object_id(), root.type_id())
                                .is_err()
                        );
                        read(cx, args)
                    })
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
        let make = module
            .define_function(
                FunctionDecl::new("make")
                    .returns(counter.apply([]).unwrap())
                    .host_policy(HostCallPolicy {
                        passing: vec![],
                        effects: HostFunctionEffects {
                            may_allocate: true,
                            ..Default::default()
                        },
                    }),
            )
            .unwrap();
        let calls = trace.clone();
        module
            .bind(make, move |cx: &HostCallContext<'_>, _: &[Value]| {
                calls.lock().unwrap().push(0);
                let root = cx.runtime().host().root(HostObjectId(7)).unwrap();
                Ok(cx.runtime().gc().alloc_host_root(root).unwrap())
            })
            .unwrap();
        let module = module.finish().unwrap();
        let engine =
            KagariEngine::with_native_modules(Default::default(), vec![module.clone()]).unwrap();
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
        let artifact = engine.emit_bytecode(&checked, Default::default()).unwrap();
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
        let mut modules = kagari_stdlib::modules().unwrap();
        modules.push(self.module.clone());
        NativeModule::install_all(&modules, &mut runtime).unwrap();
        runtime
    }

    pub fn table_owner(&self, loaded: &LoadedModule) -> LoadedModule {
        let slot = loaded
            .verified_program()
            .modules()
            .iter()
            .position(|module| module.identity == self.module.declaration().identity)
            .unwrap();
        loaded.member(ModuleRef::new(slot)).unwrap()
    }

    pub fn method(&self, loaded: &LoadedModule) -> DefinitionId {
        loaded.definitions().lookup(&self.method).unwrap()
    }
}

pub fn register_root(runtime: &mut Runtime) -> HostRootHandle {
    let ty = runtime.types().get_by_name("demo::Counter").unwrap().id;
    runtime
        .register_host_root(HostObjectId(7), ty, HostSchemaEpoch::new(0))
        .unwrap()
}

pub fn source_contract(
    source: &str,
    owner: Option<ModuleIdentity>,
) -> DefinitionMetadata<BytecodeArtifact> {
    let author = KagariEngine::default();
    if let Some(owner) = owner {
        author.bind_module(URI, owner).unwrap();
    }
    let file = author
        .set_source(URI, source.into(), SourceLayer::Base)
        .unwrap();
    let checked = author
        .compile_snapshot(author.source_snapshot(), file, &Default::default())
        .unwrap();
    author.emit_bytecode(&checked, Default::default()).unwrap()
}

pub fn reference_trait(
    module: &mut ModuleBuilder,
    contract: &DefinitionMetadata<BytecodeArtifact>,
    name: &str,
) -> TraitRef {
    let source = &contract.records().program.modules[contract.records().program.root.index()];
    let abi = source
        .trait_contracts
        .iter()
        .find(|contract| contract.abi.name == name)
        .map(|contract| contract.abi.clone())
        .or_else(|| {
            source.public_items.iter().find_map(|item| {
                if let PublicItem::Trait(abi) = item {
                    (abi.name == name).then(|| abi.clone())
                } else {
                    None
                }
            })
        })
        .unwrap();
    let path = DefinitionPath {
        module: source.identity.clone(),
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Trait,
            name: name.into(),
            occurrence: 0,
        }],
    };
    module
        .reference_trait(
            &path,
            DefinitionMetadata::checked(contract.definitions().clone(), abi, &Default::default())
                .unwrap(),
        )
        .unwrap()
}
