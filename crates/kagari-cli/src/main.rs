use kagari_common::SourceFile;
use kagari_ir::lower_to_ir;
use kagari_runtime::Runtime;
use kagari_sema::analyze;
use kagari_syntax::parse_module;
use kagari_vm::Vm;

fn main() {
    let source = SourceFile::new(
        "bootstrap.kgr",
        r#"
fn update(delta: f32) -> unit {
    delta;
}

fn main() -> i32 {
    0;
}
"#,
    );

    let ast = match parse_module(&source) {
        Ok(ast) => ast,
        Err(diagnostics) => {
            print_diagnostics(&diagnostics);
            return;
        }
    };

    let typed = match analyze(&ast) {
        Ok(typed) => typed,
        Err(diagnostics) => {
            print_diagnostics(&diagnostics);
            return;
        }
    };

    let ir = lower_to_ir(&typed);

    let mut runtime = Runtime::default();
    runtime
        .host_mut()
        .register(kagari_runtime::host::HostFunction {
            symbol: "host.log",
            params: vec![kagari_runtime::host::HostParameter {
                name: "message",
                type_name: "str",
                passing: kagari_runtime::host::HostPassingStyle::SharedBorrow,
            }],
            return_type: "unit",
        });

    let loaded = runtime.load_module(source.name(), ir);
    let mut vm = Vm::new(runtime);
    let report = vm
        .execute(&loaded, "main")
        .expect("entry function must exist");

    println!("Kagari workspace skeleton is ready.");
    println!("source: {}", source.name());
    println!("package manager: kg");
    println!("parsed functions: {}", ast.items.len());
    println!("typed functions: {}", typed.functions.len());
    println!("bytecode extension: .kbc");
    println!("loaded epoch: {}", loaded.epoch.0);
    println!("vm entry: {}", report.entry);
    println!("host functions: {}", vm.runtime().host().functions().len());
    println!("next step: flesh out expressions, statements, and a real type checker.");
}

fn print_diagnostics(diagnostics: &[kagari_common::Diagnostic]) {
    for diagnostic in diagnostics {
        eprintln!("{diagnostic}");
    }
}
