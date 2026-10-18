//! Call-target identity and instruction-owned call contracts; never callee bodies.

use crate::{
    disassembly::{
        presentation::{DisplayContext, physical, quoted},
        resolution::{Resolution, contracts},
    },
    instruction::{CallTarget, RuntimeHelper},
    module::CallableTarget,
    program::BytecodeProgram,
};
use kagari_contract::{ids::FunctionRef, standard::RuntimePrimitive};
use std::io::{self, Write};

pub(super) fn function(
    writer: &mut impl Write,
    context: &DisplayContext<'_>,
    program: &BytecodeProgram,
    module: usize,
    function: FunctionRef,
) -> io::Result<()> {
    write!(writer, "@{module}:{} ", function.index())?;
    let owner = &program.modules[module];
    let function = &owner.functions[function.index()];
    match &function.identity {
        Some(identity) => contracts::instance(writer, context, identity),
        None => {
            write!(
                writer,
                "{}::{}",
                context.module(&owner.identity),
                quoted(&function.name)
            )
        }
    }
}

pub(super) fn target(
    writer: &mut impl Write,
    context: &DisplayContext<'_>,
    resolution: &Resolution<'_>,
    module: usize,
    target: &CallTarget,
) -> io::Result<()> {
    match target {
        CallTarget::Shared {
            module: owner,
            target,
            contract,
        } => {
            write!(writer, "shared.")?;
            match target {
                CallableTarget::Script(callee) => {
                    write!(writer, "script ")?;
                    function(writer, context, resolution.program, owner.index(), *callee)?;
                }
                CallableTarget::Native(native) => write!(
                    writer,
                    "native @{}:native#{}",
                    owner.index(),
                    native.index()
                )?,
            }
            write!(writer, " contract=")?;
            contracts::shared(writer, context, resolution, contract)
        }
        CallTarget::ModuleFunction {
            module: owner,
            function: callee,
        } => {
            write!(writer, "module_function ")?;
            function(writer, context, resolution.program, owner.index(), *callee)
        }
        CallTarget::Function(callee) => {
            write!(writer, "function ")?;
            function(writer, context, resolution.program, module, *callee)
        }
        CallTarget::InterfaceMethod {
            module: owner,
            contract,
        } => {
            write!(
                writer,
                "interface_method module=@{}, contract=",
                owner.index()
            )?;
            contracts::interface(writer, context, resolution, owner.index(), contract)
        }
        CallTarget::Native(native) => write!(writer, "native @{module}:native#{}", native.index()),
        CallTarget::Register(register) => write!(writer, "register r{}", register.index()),
        CallTarget::ClosureRegister {
            register,
            params,
            return_type,
        } => {
            write!(
                writer,
                "closure_register r{}, physical_params=[",
                register.index()
            )?;
            for (index, param) in params.iter().enumerate() {
                if index != 0 {
                    write!(writer, ", ")?;
                }
                write!(writer, "{}", physical(*param))?;
            }
            write!(writer, "], physical_result={}", physical(*return_type))
        }
        CallTarget::RuntimePrimitive(primitive) => {
            let name = match primitive {
                RuntimePrimitive::ValuePartialCmp => "value_partial_cmp",
                RuntimePrimitive::ValueCmp => "value_cmp",
                RuntimePrimitive::ValueEq => "value_eq",
                RuntimePrimitive::ValueHash => "value_hash",
                RuntimePrimitive::ValueDebug => "value_debug",
                RuntimePrimitive::ValueDisplay => "value_display",
                RuntimePrimitive::StringPartsJoin => "string_parts_join",
                RuntimePrimitive::Assert => "assert",
            };
            write!(writer, "runtime_primitive.{name}")
        }
        CallTarget::RuntimeHelper(helper) => {
            write!(writer, "runtime_helper.")?;
            match helper {
                RuntimeHelper::ReflectTypeOf => write!(writer, "reflect_type_of"),
                RuntimeHelper::ReflectGetField(name) => {
                    write!(writer, "reflect_get_field {}", quoted(name))
                }
                RuntimeHelper::ReflectSetField(name) => {
                    write!(writer, "reflect_set_field {}", quoted(name))
                }
                RuntimeHelper::ReflectSetIndex => write!(writer, "reflect_set_index"),
                RuntimeHelper::DynamicCall => write!(writer, "dynamic_call"),
            }
        }
    }
}
