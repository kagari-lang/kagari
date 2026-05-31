use std::{env, fs, process::ExitCode};

use kagari_common::{Diagnostic, SourceFile};
use kagari_hir::analyze_module;
use kagari_ir::{bytecode::lower_to_bytecode, lower_to_ir};
use kagari_runtime::{
    Runtime,
    host::{HostError, HostFunction, HostParameter, HostPassingStyle},
    value::Value,
};
use kagari_syntax::parse_module;
use kagari_vm::Vm;

fn main() -> ExitCode {
    let Some(path) = script_path() else {
        eprintln!("usage: kagari <script.kgr>");
        eprintln!("       kagari run <script.kgr>");
        return ExitCode::from(2);
    };

    let source_text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("failed to read `{path}`: {error}");
            return ExitCode::from(1);
        }
    };
    let source = SourceFile::new(path.clone(), source_text);

    let ast = match parse_module(&source) {
        Ok(ast) => ast,
        Err(diagnostics) => {
            print_diagnostics(&diagnostics);
            return ExitCode::from(1);
        }
    };

    let analyzed = match analyze_module(&ast) {
        Ok(analyzed) => analyzed,
        Err(diagnostics) => {
            print_diagnostics(&diagnostics);
            return ExitCode::from(1);
        }
    };

    let ir = match lower_to_ir(&analyzed) {
        Ok(ir) => ir,
        Err(error) => {
            eprintln!("{error:?}");
            return ExitCode::from(1);
        }
    };

    let bytecode = match lower_to_bytecode(&ir) {
        Ok(bytecode) => bytecode,
        Err(error) => {
            eprintln!("{error:?}");
            return ExitCode::from(1);
        }
    };

    let has_main = bytecode
        .functions
        .iter()
        .any(|function| function.name == "main");

    let mut runtime = Runtime::default();
    register_default_host_functions(&mut runtime);
    let loaded = match runtime.load_module(source.name(), bytecode) {
        Ok(loaded) => loaded,
        Err(error) => {
            eprintln!("{error:?}");
            return ExitCode::from(1);
        }
    };
    let mut vm = Vm::new(runtime);

    let result = if has_main {
        vm.execute(&loaded, "main").map(|_| ())
    } else {
        vm.execute_module(&loaded).map(|_| ())
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error:?}");
            ExitCode::from(1)
        }
    }
}

fn script_path() -> Option<String> {
    let mut args = env::args().skip(1);
    match args.next()?.as_str() {
        "run" => args.next(),
        path => Some(path.to_owned()),
    }
}

fn register_default_host_functions(runtime: &mut Runtime) {
    runtime.host_mut().register(HostFunction::new(
        "host.log",
        vec![HostParameter {
            name: "message",
            type_name: "String",
            passing: HostPassingStyle::SharedBorrow,
        }],
        "()",
        |args| {
            let Some(Value::Str(message)) = args.first() else {
                return Err(HostError::new("host.log expects one string argument"));
            };
            println!("{message}");
            Ok(Value::Unit)
        },
    ));
}

fn print_diagnostics(diagnostics: &[Diagnostic]) {
    for diagnostic in diagnostics {
        eprintln!("{diagnostic}");
    }
}
