//! Inspect verified struct layouts and interface identities without a runtime.

use kagari_bytecode::{instruction::BytecodeInstruction, module::CallableTarget};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext,
        metadata::DefinitionMetadata,
    },
};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_contract::types::PublicItem;
use kagari_hir::analysis::AnalysisDatabase;
use kagari_mir::verify::{MirVerificationErrorKind, verify_mir};
use kagari_source::{
    source::SourceFile,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_stdlib::{catalog as foundation_catalog, namespaces};
use kagari_types::{
    declaration::{TypeDef, TypeDefKind, module::ModuleDecl, native::NativeStorageLayout},
    scalar::BuiltinType,
    ty::{GenericParam, Ty},
};
use std::sync::Arc;

fn main() {
    let source = SourceFile::new(
        "layouts.kgr",
        "use demo::storage::Samples; pub struct External { val storage: Samples<i32> } pub struct Deferred<T> { val payload: T } pub struct Pair { var number: i32, val enabled: bool, val samples: [i32] } pub trait Number { fn get(self) -> i32; } impl Number for Pair { fn get(self) -> i32 { self.number } } impl<T> Number for Deferred<T> { fn get(self) -> i32 { 7 } } fn read<T: Number>(value: T) -> i32 { value.get() } fn main() -> i32 { read(Deferred { payload: 1 }); val generic: Number = Deferred { payload: 1 }; generic.get(); val p = Pair { enabled: true, number: 41, samples: [1, 2] }; if p.enabled { p.number += 1; }; p.number }",
    );
    let mut sources = SourceDatabase::default();
    let root = sources
        .set(source.name(), source.text().into(), SourceLayer::Base)
        .unwrap();
    // This compiler-only consumer receives an application storage declaration.
    // Nominal native storage remains distinct from the builtin array field.
    let mut storage = ModuleDecl::new(ModuleIdentity {
        package: PackageId("demo".into()),
        path: vec!["storage".into()],
    });
    let context = DefinitionContext::new().unwrap();
    let storage_root = context.intern_root(&storage.identity).unwrap();
    let samples = context
        .intern_child(storage_root, DefinitionKind::AssociatedType, "Samples", 0)
        .unwrap();
    storage.types.push(TypeDef {
        name: "Samples".into(),
        kind: TypeDefKind::NativeStorage(NativeStorageLayout::Opaque),
        generic_params: vec![GenericParam {
            owner: samples,
            position: 0,
        }],
        bounds: vec![],
        fields: vec![],
        variants: vec![],
    });
    let definitions = storage.collect_definitions(&context).unwrap();
    storage
        .validate(
            &|ty| namespaces::receiver_owner(ty, &definitions),
            &definitions,
        )
        .unwrap();
    let storage = DefinitionMetadata::checked(definitions, storage, &Default::default()).unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(
        foundation_catalog::shared()
            .expect("foundation declarations")
            .into_iter()
            .chain([Arc::new(storage)])
            .collect(),
    );
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir_program = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let ir = mir_program
        .modules()
        .iter()
        .find(|module| &module.identity == mir_program.root())
        .unwrap();
    for layout in &ir.structures {
        let identity = ir.definitions().resolve(layout.declaration).unwrap();
        let segment = identity.segments().last().unwrap();
        assert_eq!(identity.segments().count(), 1);
        assert_eq!(segment.occurrence, 0);
        println!("{}::{}", identity.module(), segment.name);
        for (slot, field) in layout.fields.iter().enumerate() {
            let identity = ir.definitions().resolve(field.declaration).unwrap();
            assert_eq!(identity.segments().count(), 2);
            assert_eq!(identity.segments().last().unwrap().occurrence, 0);
            println!(
                "  slot {slot}: {} {:?}, mutable={}",
                field.name, field.ty, field.mutable
            );
        }
    }
    let cancelled = CancellationToken::default();
    cancelled.cancel();
    assert_eq!(
        verify_mir(ir.clone().into_unverified(), &cancelled)
            .unwrap_err()
            .kind,
        MirVerificationErrorKind::Cancelled,
    );
    let program = lower_program_to_bytecode(&mir_program, &Default::default()).unwrap();
    let definitions = program.definitions();
    let bytecode = &program.program().modules[program.program().root.index()];
    assert!(bytecode.functions.iter().all(|function| {
        function.identity.as_ref().is_some_and(|identity| {
            definitions.resolve(identity.declaration).unwrap().module() == &bytecode.identity
                && bytecode.function_table[function.id.index()].identity == function.identity
        })
    }));
    let interface = bytecode
        .public_items
        .iter()
        .find_map(|item| match item {
            PublicItem::InterfaceTable(table) => Some(table),
            _ => None,
        })
        .expect("checked interface declaration");
    let identity = definitions.resolve(interface.declaration).unwrap();
    assert_eq!(identity.module(), &bytecode.identity);
    assert_eq!(
        identity.segments().next().unwrap().kind,
        DefinitionKind::Impl
    );
    let executable_table = bytecode
        .interface_tables
        .iter()
        .find(|table| table.declaration == interface.declaration)
        .expect("executable interface table");
    assert_eq!(executable_table.methods.len(), 1);
    let method = &executable_table.methods[0];
    assert_eq!(
        definitions
            .resolve(method.method)
            .unwrap()
            .last()
            .unwrap()
            .name,
        "get"
    );
    let CallableTarget::Script(function) = method.target else {
        panic!("script implementation")
    };
    assert_eq!(
        definitions
            .resolve(
                bytecode.functions[function.index()]
                    .identity
                    .as_ref()
                    .unwrap()
                    .declaration
            )
            .unwrap()
            .last()
            .unwrap()
            .name,
        "get"
    );
    // Static generic calls need no boxed table; the explicit Number value above
    // requests the concrete dynamic instance inspected here.
    let generic_table = bytecode
        .interface_tables
        .iter()
        .find(|table| {
            table.declaration != interface.declaration
                && table.arguments == [Ty::Builtin(BuiltinType::I32)]
        })
        .expect("specialized generic interface table");
    assert_eq!(generic_table.methods.len(), 1);
    let CallableTarget::Script(function) = generic_table.methods[0].target else {
        panic!("script implementation")
    };
    assert_eq!(
        bytecode.functions[function.index()]
            .identity
            .as_ref()
            .unwrap()
            .arguments,
        [Ty::Builtin(BuiltinType::I32)]
    );
    println!(
        "interface {} has a stable declaration identity and executable method slot",
        interface.name
    );
    println!(
        "{} executable functions retain their declaration identities",
        bytecode.functions.len()
    );
    let (structure, fields) = bytecode
        .functions
        .iter()
        .flat_map(|f| &f.instructions)
        .find_map(|instruction| {
            if let BytecodeInstruction::MakeStruct {
                structure, fields, ..
            } = instruction
                && bytecode.structures[structure.index()].name(definitions) == Some("Pair")
            {
                Some((structure, fields))
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(
        bytecode.structures[structure.index()]
            .fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>(),
        ["number", "enabled", "samples"]
    );
    assert_eq!(
        fields.len(),
        bytecode.structures[structure.index()].fields.len()
    );
}
