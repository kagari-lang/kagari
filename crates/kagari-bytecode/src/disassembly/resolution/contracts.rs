//! Shared streaming spelling for instruction- and descriptor-owned call contracts.
use kagari_contract::callable::construction::evidence::{
    ConstructionSelection, ConstructionWitness,
};

use crate::disassembly::{
    presentation::{DisplayContext, effects},
    resolution::Resolution,
};
use kagari_contract::{
    callable::{
        adapter::SharedAdapterWitness,
        interface::{InterfaceCallContract, TypeNormalization},
        shared::SharedCall,
        witness::{OperationWitness, SharedMethodWitness},
    },
    native_import::callables::{NativeCallableApplication, NativeCallableOrigin},
    types::ConcreteFunctionIdentity,
};
use kagari_types::{
    callable::{CallableImplementation, NativeDefaultApplication, Signature},
    declaration::requirement::NativeCallableRequirement,
    reference::ReferenceAccess,
    ty::Ty,
};
use std::io::{self, Write};

pub(in crate::disassembly) fn types(
    writer: &mut impl Write,
    context: &DisplayContext<'_>,
    values: &[Ty],
) -> io::Result<()> {
    write!(writer, "[")?;
    for (index, value) in values.iter().enumerate() {
        if index != 0 {
            write!(writer, ", ")?;
        }
        write!(writer, "{}", context.ty(value))?;
    }
    write!(writer, "]")
}

pub(in crate::disassembly) fn access(value: Option<ReferenceAccess>) -> &'static str {
    match value {
        None => "none",
        Some(ReferenceAccess::Shared) => "shared",
        Some(ReferenceAccess::Mutable) => "mutable",
    }
}

pub(in crate::disassembly) fn instance(
    writer: &mut impl Write,
    context: &DisplayContext<'_>,
    instance: &ConcreteFunctionIdentity,
) -> io::Result<()> {
    let ConcreteFunctionIdentity {
        declaration,
        arguments,
    } = instance;
    write!(writer, "{} arguments=", context.definition(*declaration))?;
    types(writer, context, arguments)
}

fn implementation(
    writer: &mut impl Write,
    context: &DisplayContext<'_>,
    implementation: &CallableImplementation,
) -> io::Result<()> {
    match implementation {
        CallableImplementation::Required => write!(writer, "required"),
        CallableImplementation::Script => write!(writer, "script"),
        CallableImplementation::Native(binding) => {
            write!(writer, "native {}", context.definition(*binding))
        }
        CallableImplementation::NativeDefault(application) => {
            let NativeDefaultApplication {
                declaration,
                arguments,
            } = application;
            write!(
                writer,
                "native_default {} arguments=",
                context.definition(*declaration)
            )?;
            types(writer, context, arguments)
        }
    }
}

fn signature(
    writer: &mut impl Write,
    context: &DisplayContext<'_>,
    signature: &Signature,
) -> io::Result<()> {
    let Signature { params, result } = signature;
    write!(writer, "{{params=")?;
    types(writer, context, params)?;
    write!(writer, ", result={}}}", context.ty(result))
}

fn requirement(
    writer: &mut impl Write,
    context: &DisplayContext<'_>,
    requirement: &NativeCallableRequirement,
) -> io::Result<()> {
    let NativeCallableRequirement {
        receiver,
        interface,
        member,
        arguments,
    } = requirement;
    write!(
        writer,
        "{{receiver={}, interface={}, member={}, arguments=",
        context.ty(receiver),
        context.ty(&Ty::Trait(interface.clone())),
        context.definition(*member)
    )?;
    types(writer, context, arguments)?;
    write!(writer, "}}")
}

pub(in crate::disassembly) fn shared(
    writer: &mut impl Write,
    context: &DisplayContext<'_>,
    resolution: &Resolution<'_>,
    call: &SharedCall,
) -> io::Result<()> {
    let SharedCall {
        receiver_access,
        instance: callee,
        implementation: selected,
        arguments,
        signature: applied,
        operations: witnesses,
        constructions: capabilities,
    } = call;
    write!(
        writer,
        "{{receiver_access={}, instance=",
        access(*receiver_access)
    )?;
    instance(writer, context, callee)?;
    write!(writer, ", implementation=")?;
    implementation(writer, context, selected)?;
    write!(writer, ", arguments=")?;
    types(writer, context, arguments)?;
    write!(writer, ", signature=")?;
    signature(writer, context, applied)?;
    write!(writer, ", operations=")?;
    operations(writer, context, resolution, witnesses)?;
    constructions(writer, context, resolution, capabilities)?;
    write!(writer, "}}")
}

pub(in crate::disassembly) fn interface(
    writer: &mut impl Write,
    context: &DisplayContext<'_>,
    resolution: &Resolution<'_>,
    owner: usize,
    contract: &InterfaceCallContract,
) -> io::Result<()> {
    let InterfaceCallContract {
        normalizations,
        receiver,
        operations: witnesses,
        constructions: capabilities,
        interface,
        method_slot,
        arguments,
    } = contract;
    let declaration = resolution.trait_reference(owner, interface.declaration);
    let member = resolution.trait_member(declaration, *method_slot as usize);
    write!(
        writer,
        "{{interface={}, declaration={declaration}, method_slot={method_slot}, member={}, arguments=",
        context.ty(&Ty::Trait(interface.clone())),
        context.definition(member)
    )?;
    types(writer, context, arguments)?;
    write!(writer, ", receiver=")?;
    match receiver {
        Some(receiver) => write!(writer, "{}", context.ty(receiver))?,
        None => write!(writer, "none")?,
    }
    write!(writer, ", normalizations=[")?;
    for (index, normalization) in normalizations.iter().enumerate() {
        if index != 0 {
            write!(writer, ", ")?;
        }
        let TypeNormalization { source, result } = normalization;
        write!(
            writer,
            "{{source={}, result={}}}",
            context.ty(source),
            context.ty(result)
        )?;
    }
    write!(writer, "], operations=")?;
    operations(writer, context, resolution, witnesses)?;
    constructions(writer, context, resolution, capabilities)?;
    write!(writer, "}}")
}

fn operations(
    writer: &mut impl Write,
    context: &DisplayContext<'_>,
    resolution: &Resolution<'_>,
    operations: &[OperationWitness],
) -> io::Result<()> {
    // Verified witness trees have bounded depth. This follows only stored facts,
    // not executable references or recursive function/interface declarations.
    write!(writer, "[")?;
    for (index, operation) in operations.iter().enumerate() {
        if index != 0 {
            write!(writer, ", ")?;
        }
        witness(writer, context, resolution, operation)?;
    }
    write!(writer, "]")
}

pub(in crate::disassembly) fn witness(
    writer: &mut impl Write,
    context: &DisplayContext<'_>,
    resolution: &Resolution<'_>,
    witness: &OperationWitness,
) -> io::Result<()> {
    match witness {
        OperationWitness::Selected(application) => {
            let NativeCallableApplication {
                origin,
                requirement: required,
                instance: callee,
                implementation: selected,
                signature: applied,
                effects: recorded,
            } = application.as_ref();
            let origin = match origin {
                NativeCallableOrigin::Implementation => "implementation",
                NativeCallableOrigin::ProtocolAdapter => "protocol_adapter",
            };
            write!(writer, "selected{{target=")?;
            match selected {
                CallableImplementation::Script => {
                    let (owner, slot) = resolution.script(callee);
                    write!(writer, "@{owner}:{slot}")?;
                }
                CallableImplementation::Native(_) => {
                    let (owner, slot) = resolution.native(application).expect("native witness");
                    write!(writer, "@{owner}:native#{slot}")?;
                }
                CallableImplementation::Required | CallableImplementation::NativeDefault(_) => {
                    unreachable!("verified witnesses have executable targets")
                }
            }
            write!(writer, ", origin={origin}, requirement=")?;
            requirement(writer, context, required)?;
            write!(writer, ", instance=")?;
            instance(writer, context, callee)?;
            write!(writer, ", implementation=")?;
            implementation(writer, context, selected)?;
            write!(writer, ", signature=")?;
            signature(writer, context, applied)?;
            write!(writer, ", recorded_effects={}}}", effects(*recorded))
        }
        OperationWitness::SharedMethod(method) => {
            let SharedMethodWitness {
                requirement: required,
                implementation: selected,
                operations: witnesses,
                constructions: capabilities,
            } = method.as_ref();
            let (owner, slot) = resolution.interface(selected);
            write!(
                writer,
                "shared_method{{target=@{owner}:interface#{slot}, requirement="
            )?;
            requirement(writer, context, required)?;
            write!(writer, ", implementation=")?;
            instance(writer, context, selected)?;
            write!(writer, ", operations=")?;
            operations(writer, context, resolution, witnesses)?;
            constructions(writer, context, resolution, capabilities)?;
            write!(writer, "}}")
        }
        OperationWitness::SharedAdapter(adapter) => {
            let SharedAdapterWitness {
                requirement: required,
                call,
            } = adapter.as_ref();
            let (owner, slot) = resolution.script(&call.instance);
            write!(
                writer,
                "shared_adapter{{target=@{owner}:{slot}, requirement="
            )?;
            requirement(writer, context, required)?;
            write!(writer, ", call=")?;
            shared(writer, context, resolution, call)?;
            write!(writer, "}}")
        }
        OperationWitness::Forward(required) => {
            write!(writer, "forward")?;
            requirement(writer, context, required)
        }
    }
}

fn constructions(
    writer: &mut impl Write,
    context: &DisplayContext<'_>,
    resolution: &Resolution<'_>,
    witnesses: &[ConstructionWitness],
) -> io::Result<()> {
    if witnesses.is_empty() {
        return Ok(());
    }
    write!(writer, ", constructions=[")?;
    for (index, witness) in witnesses.iter().enumerate() {
        if index != 0 {
            write!(writer, ", ")?;
        }
        write!(
            writer,
            "{{receiver={}, interface={}, selection=",
            context.ty(&witness.requirement.receiver),
            context.ty(&Ty::Trait(witness.requirement.interface.clone()))
        )?;
        match &witness.selection {
            ConstructionSelection::Declared {
                implementation: selected,
                operations: dependencies,
                constructions: capabilities,
            } => {
                instance(writer, context, selected)?;
                write!(writer, ", operations=")?;
                operations(writer, context, resolution, dependencies)?;
                constructions(writer, context, resolution, capabilities)?;
            }
            ConstructionSelection::Interface => write!(writer, "interface")?,
            ConstructionSelection::Forward => write!(writer, "forward")?,
        }
        write!(writer, "}}")?;
    }
    write!(writer, "]")
}
