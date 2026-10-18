//! Native declarations are offline requirements, not installed Rust bindings.

use crate::disassembly::{
    descriptors::contracts,
    presentation::{DisplayContext, quoted},
    resolution::Resolution,
};
use kagari_contract::{
    callable::generic::GenericBody,
    native_import::{NativeImport, result::NativeResultAdapter},
};
use kagari_types::{
    callable::MethodPolicy,
    declaration::{
        FnDecl, NativeDeclaration, Param,
        host::{HostCallPolicy, HostFunctionEffects, HostPassingStyle},
    },
};
use std::io::{self, Write};

pub(super) fn write(
    writer: &mut impl Write,
    context: &DisplayContext<'_>,
    resolution: &Resolution<'_>,
    owner: usize,
    slot: usize,
) -> io::Result<()> {
    let NativeImport {
        receiver_access,
        result_adapter,
        generic,
        instance,
        binding,
        signature,
        requirements,
        callables,
    } = &resolution.program.modules[owner].native_imports[slot];
    writeln!(
        writer,
        "  @{owner}:native#{slot} {} binding={} receiver_access={} installation=not_checked",
        contracts::instance(context, instance),
        context.definition(*binding),
        contracts::access(*receiver_access)
    )?;
    writeln!(
        writer,
        "    signature {}",
        contracts::signature(context, signature)
    )?;
    match generic {
        None => writeln!(writer, "    generic=none")?,
        Some(body) => {
            let GenericBody { parameters, bounds } = body;
            contracts::parameters(writer, context, "    generic ", parameters)?;
            contracts::bounds(writer, context, "    generic ", bounds)?;
        }
    }
    contracts::bounds(writer, context, "    ", requirements)?;
    match result_adapter {
        None => writeln!(writer, "    result_adapter=none")?,
        Some(adapter) => {
            let NativeResultAdapter {
                receiver,
                implementation,
            } = adapter;
            writeln!(
                writer,
                "    result_adapter receiver={} target={} implementation=({})",
                context.ty(receiver),
                contracts::table_reference(resolution, implementation),
                contracts::instance(context, implementation)
            )?;
        }
    }
    contracts::operations(writer, context, resolution, "    ", callables)?;
    // The retained declaration may belong to a different module than the import.
    let declaration = resolution.native_declaration(instance.declaration);
    write_declaration(writer, context, declaration)
}

fn write_declaration(
    writer: &mut impl Write,
    context: &DisplayContext<'_>,
    declaration: &NativeDeclaration,
) -> io::Result<()> {
    let NativeDeclaration {
        concrete_result,
        declaration,
        function,
        callable_requirements,
    } = declaration;
    writeln!(
        writer,
        "    declaration {}",
        context.definition(*declaration)
    )?;
    match concrete_result {
        None => writeln!(writer, "      concrete_result=none")?,
        Some(ty) => writeln!(writer, "      concrete_result={}", context.ty(ty))?,
    }
    let FnDecl {
        host_policy,
        method_policy,
        name,
        implementation,
        generic_params,
        bounds,
        params,
        return_type,
    } = function;
    writeln!(
        writer,
        "      name={} implementation=({})",
        quoted(name),
        contracts::implementation(context, implementation)
    )?;
    let MethodPolicy {
        override_allowed,
        receiver_access,
    } = method_policy;
    writeln!(
        writer,
        "      override_allowed={override_allowed} receiver_access={}",
        contracts::access(*receiver_access)
    )?;
    contracts::parameters(writer, context, "      ", generic_params)?;
    contracts::bounds(writer, context, "      ", bounds)?;
    for (index, parameter) in params.iter().enumerate() {
        let Param { name, ty, mutable } = parameter;
        writeln!(
            writer,
            "      param#{index} name={} type={} mutable={mutable}",
            quoted(name),
            context.ty(ty)
        )?;
    }
    writeln!(writer, "      result={}", context.ty(return_type))?;
    for (index, required) in callable_requirements.iter().enumerate() {
        writeln!(
            writer,
            "      callable_requirement#{index} {}",
            contracts::requirement(context, required)
        )?;
    }
    match host_policy {
        None => writeln!(writer, "      host_policy=none"),
        Some(policy) => host_policy_write(writer, policy),
    }
}

fn host_policy_write(writer: &mut impl Write, policy: &HostCallPolicy) -> io::Result<()> {
    let HostCallPolicy { passing, effects } = policy;
    for (index, style) in passing.iter().enumerate() {
        let style = match style {
            HostPassingStyle::Owned => "owned",
            HostPassingStyle::SharedBorrow => "shared_borrow",
            HostPassingStyle::UniqueBorrow => "unique_borrow",
        };
        writeln!(writer, "      host_passing#{index} {style}")?;
    }
    let HostFunctionEffects {
        may_allocate,
        may_trap,
        may_read_immutable_configuration,
        may_call_host_services,
        may_mutate_host_state,
        may_suspend,
    } = effects;
    writeln!(
        writer,
        "      declared_host_effects may_allocate={may_allocate} may_trap={may_trap} may_read_immutable_configuration={may_read_immutable_configuration} may_call_host_services={may_call_host_services} may_mutate_host_state={may_mutate_host_state} may_suspend={may_suspend}"
    )
}
