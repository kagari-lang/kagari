use crate::{
    RootedInterfaceMethod, Runtime,
    error::RuntimeError,
    module::LoadedModule,
    native::{
        binding::{Codec, NativeBinding},
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        declarations::MethodDecl,
        module::NativeModule,
        types::Type,
    },
    value::Value,
};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::declaration::requirement::NativeCallableRequirement;
use std::sync::Arc;

fn load_source(source: &str, module: Option<&NativeModule>) -> (Runtime, LoadedModule) {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("group.kgr", source.into(), SourceLayer::Base)
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    if let Some(module) = module {
        analysis.set_native_modules(vec![Arc::new(module.to_declaration().unwrap())]);
    }
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let code = lower_program_to_bytecode(&mir).unwrap();
    let mut runtime = Runtime::default();
    if let Some(module) = module {
        module.install(&mut runtime).unwrap();
    }
    let loaded = runtime.load_program("group", code).unwrap();
    (runtime, loaded)
}

#[test]
fn cached_native_application_reuses_its_receiver_environment_and_reclaims_the_cycle() {
    for teardown in [false, true] {
        let mut module = ModuleBuilder::new("example::defaults", &DeclarationCatalog::default());
        let mut declaration = module.define_trait("Read");
        let read = declaration
            .define_method(MethodDecl::instance("read").returns(Type::i32()))
            .unwrap();
        let twice = declaration
            .define_method(MethodDecl::instance("twice").returns(Type::i32()))
            .unwrap();
        let required = declaration.operation(&read).unwrap();
        let selected = declaration
            .method(&twice, |method| Ok(method.requires(required)))
            .unwrap();
        declaration
            .bind_default_with(
                twice,
                NativeBinding::new(
                    vec![Codec::Value],
                    Codec::Scalar(Type::i32().abi().clone()),
                    move |cx| {
                        let receiver = cx.argument(0)?;
                        let target = cx.selected(selected)?;
                        let Value::I32(value) = cx.call_values(target, &[receiver])? else {
                            return Err(RuntimeError::module_validation("checked Read result"));
                        };
                        Ok(Value::I32(value * 2))
                    },
                ),
            )
            .unwrap();
        declaration.finish().unwrap();
        let module = module.finish().unwrap();
        let (runtime, loaded) = load_source(
            r#"
        use example::defaults::Read;
        impl Read for i32 { fn read(self) -> i32 { self } }
        fn main() -> Read { 7 }
        "#,
            Some(&module),
        );
        let table = loaded
            .bytecode
            .interface_tables
            .iter()
            .position(|table| {
                table
                    .methods
                    .iter()
                    .any(|method| loaded.definition_name(method.method) == Some("twice"))
            })
            .unwrap();
        let Value::Interface(id) = runtime
            .make_interface(&loaded, table, Value::I32(7))
            .unwrap()
        else {
            panic!("interface fixture");
        };
        let snapshot = runtime.gc.interface_snapshot(id).unwrap();
        let group = runtime
            .bind_table_operations(&snapshot.receiver_table)
            .unwrap();
        let member = snapshot
            .methods
            .iter()
            .flatten()
            .find(|method| loaded.definition_name(method.method) == Some("twice"))
            .unwrap()
            .method;
        let requirement = NativeCallableRequirement {
            receiver: snapshot.concrete_type.clone(),
            interface: snapshot.interface_type.clone(),
            member,
            arguments: vec![],
        };
        let operation_id = runtime
            .gc
            .operation_group(group)
            .unwrap()
            .operation(&requirement)
            .unwrap();
        let operation = runtime.gc.bound_operation(operation_id).unwrap();
        let prepare = || {
            let method = RootedInterfaceMethod::from_operation(
                &runtime,
                runtime.root_value(Value::I32(7)).unwrap(),
                operation_id,
                Value::I32(7),
                snapshot.concrete_type.clone(),
                snapshot.interface_type.clone(),
            )
            .unwrap();
            runtime.apply_interface_method(method, &[]).unwrap()
        };
        let first = prepare();
        let second = prepare();
        let environment = first.environment.as_ref().unwrap();
        assert_eq!(environment.id, second.environment.as_ref().unwrap().id);
        assert_eq!(
            environment.id,
            runtime
                .gc
                .method_application(*operation.application.get().unwrap())
                .unwrap()
                .environment
                .as_ref()
                .unwrap()
                .id
        );
        assert!(environment.operation(&runtime.gc, &requirement).is_some());
        let environment_id = environment.id;
        let runtime_owner = runtime.resources().lifetime_probe();
        let application_id = *operation.application.get().unwrap();
        assert_eq!(first.application, Some(application_id));
        assert_eq!(second.application, Some(application_id));
        drop((operation, snapshot, first));
        runtime.collect_garbage().unwrap();
        assert_eq!(runtime.gc.stats().operation_groups, 1);
        assert!(runtime.gc.environment(environment_id).is_some());
        assert!(runtime.gc.method_application(application_id).is_some());
        assert_eq!(runtime.gc.stats().method_applications, 1);
        if teardown {
            // A prepared identity can survive runtime teardown without owning the
            // operation or its cached application. This handle has no applied view.
            let retained = RootedInterfaceMethod::from_operation(
                &runtime,
                runtime.root_value(Value::I32(7)).unwrap(),
                operation_id,
                Value::I32(7),
                requirement.receiver.clone(),
                requirement.interface.clone(),
            )
            .unwrap();
            retained.refresh_roots(&runtime).unwrap();
            drop(second);
            drop(runtime);
            assert!(runtime_owner.upgrade().is_none());
            let foreign = Runtime::default();
            assert!(retained.implementation(&foreign).is_err());
            assert!(retained.target(&foreign).is_err());
            assert!(retained.parameter_types(&foreign).is_err());
            assert!(retained.return_type(&foreign).is_err());
            drop(retained);
        } else {
            drop(second);
            assert_eq!(
                runtime
                    .collect_garbage()
                    .unwrap()
                    .reclaimed_operation_groups,
                1
            );
            assert!(runtime.gc.environment(environment_id).is_none());
            assert!(runtime.gc.method_application(application_id).is_none());
            assert_eq!(runtime.gc.stats().method_applications, 0);
        }
    }
}
