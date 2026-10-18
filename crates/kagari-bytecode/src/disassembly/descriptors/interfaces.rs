//! Interface tables retain dispatch, parent mappings and result adapters.

use crate::{
    disassembly::{
        descriptors::contracts::{
            access, arguments, bounds, instance, nominal, parameters, table_reference,
        },
        presentation::{DisplayContext, quoted},
        resolution::{Resolution, TraitReference},
    },
    module::{
        CallableTarget, InterfaceMethodSlot, InterfaceParentRecord, InterfaceResultAdapter,
        InterfaceTableRecord, InterfaceViewRecord,
    },
};
use kagari_types::declaration::FnDecl;
use std::io::{self, Write};

pub(super) fn write(
    writer: &mut impl Write,
    context: &DisplayContext<'_>,
    resolution: &Resolution<'_>,
    owner: usize,
    slot: usize,
) -> io::Result<()> {
    let InterfaceTableRecord {
        view,
        arguments: types,
        declaration,
        methods,
        parents,
    } = &resolution.program.modules[owner].interface_tables[slot];
    writeln!(
        writer,
        "  @{owner}:interface#{slot} {} arguments={}",
        context.definition(*declaration),
        arguments(context, types)
    )?;
    let template = resolution.interface_declaration(*declaration);
    writeln!(
        writer,
        "    template trait_type={} for_type={}",
        context.ty(&template.trait_type),
        context.ty(&template.for_type)
    )?;
    parameters(writer, context, "    template ", &template.generic_params)?;
    bounds(writer, context, "    template ", &template.bounds)?;
    for (index, method) in methods.iter().enumerate() {
        let InterfaceMethodSlot {
            method,
            target,
            arguments: types,
        } = method;
        write!(
            writer,
            "    member#{index} {} target=",
            context.definition(*method)
        )?;
        match target {
            CallableTarget::Script(function) => write!(writer, "@{owner}:{}", function.index())?,
            CallableTarget::Native(import) => write!(writer, "@{owner}:native#{}", import.index())?,
        }
        writeln!(writer, " arguments={}", arguments(context, types))?;
    }
    for (index, parent) in parents.iter().enumerate() {
        let InterfaceParentRecord {
            interface,
            implementation,
            view,
        } = parent;
        writeln!(
            writer,
            "    parent#{index} {} interface=({}) implementation=({}) view={view}",
            table_reference(resolution, implementation),
            nominal(context, interface),
            instance(context, implementation)
        )?;
    }
    match view {
        None => writeln!(writer, "    view=none")?,
        Some(view) => {
            let InterfaceViewRecord { interface, results } = view;
            writeln!(writer, "    view=({})", nominal(context, interface))?;
            for (index, adapter) in results.iter().enumerate() {
                let InterfaceResultAdapter {
                    method,
                    implementation,
                } = adapter;
                writeln!(
                    writer,
                    "      result_adapter#{index} method={} target={} implementation=({})",
                    context.definition(*method),
                    table_reference(resolution, implementation),
                    instance(context, implementation)
                )?;
            }
        }
    }
    Ok(())
}

/// Declaration order is the dispatch method-slot order, unlike executable impl slots.
pub(super) fn declaration(
    writer: &mut impl Write,
    context: &DisplayContext<'_>,
    resolution: &Resolution<'_>,
    reference: TraitReference,
) -> io::Result<()> {
    let (identity, declaration) = resolution.trait_declaration(reference);
    writeln!(
        writer,
        "  {reference} trait {}",
        context.definition(identity)
    )?;
    parameters(writer, context, "    ", &declaration.generic_params)?;
    bounds(writer, context, "    ", &declaration.bounds)?;
    for (index, parent) in declaration.supertraits.iter().enumerate() {
        writeln!(
            writer,
            "    supertrait#{index} {}",
            nominal(context, parent)
        )?;
    }
    for (slot, method) in declaration.methods.iter().enumerate() {
        writeln!(
            writer,
            "    method_slot={slot} member={}",
            context.definition(resolution.trait_member(reference, slot))
        )?;
        method_signature(writer, context, method)?;
    }
    Ok(())
}

fn method_signature(
    writer: &mut impl Write,
    context: &DisplayContext<'_>,
    method: &FnDecl,
) -> io::Result<()> {
    writeln!(
        writer,
        "      declared_signature receiver_access={}",
        access(method.method_policy.receiver_access)
    )?;
    parameters(writer, context, "      ", &method.generic_params)?;
    bounds(writer, context, "      ", &method.bounds)?;
    for (slot, parameter) in method.params.iter().enumerate() {
        writeln!(
            writer,
            "      param#{slot} name={} type={} mutable={}",
            quoted(&parameter.name),
            context.ty(&parameter.ty),
            parameter.mutable
        )?;
    }
    writeln!(writer, "      result={}", context.ty(&method.return_type))
}
