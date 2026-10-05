//! Derive and validate selected parent tables and result representation adapters.
use crate::{
    module::{
        BytecodeModule, InterfaceParentRecord, InterfaceResultAdapter, InterfaceTableRecord,
        InterfaceViewRecord,
    },
    program::ModuleRef,
    trait_bounds::{contract, executable_interface},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, DefinitionPath, DefinitionPathSegment, reference::DefinitionReference,
    },
};
use kagari_contract::{
    callable::interface::InterfaceCallContract,
    native_import::{NativeImport, result::NativeResultAdapter},
    types::{ConcreteFunctionIdentity, InterfaceTable, PublicItem, matching::match_implementation},
};
use kagari_types::{
    language::{Protocol, role::LangRole},
    ty::{
        GenericParam, NominalTy, Ty,
        inheritance::{erased_iterator_view, interface_views},
        substitution::TypeTransformError,
    },
};

pub struct InterfaceLinks {
    pub parents: Vec<InterfaceParentRecord>,
    pub view: Option<InterfaceViewRecord>,
}

pub fn links(
    module: &BytecodeModule,
    linked: &InterfaceTableRecord,
    closure: &[&BytecodeModule],
    cancel: &CancellationToken,
) -> Result<InterfaceLinks, TypeTransformError> {
    let invalid = TypeTransformError::InvalidContract;
    let template = template(module, linked).ok_or(invalid)?;
    if linked.arguments.is_empty() && !template.generic_params.is_empty() {
        return Ok(InterfaceLinks {
            parents: vec![],
            view: None,
        });
    }
    let table = template
        .instantiate_in(&linked.arguments, &template.generic_params)
        .ok_or(invalid)?;
    let Ty::Trait(interface) = &table.trait_type else {
        return Err(invalid);
    };
    let lookup = |id: &DefinitionPath| contract(id, closure);
    let mut parents = vec![];
    for parent in interface_views(interface, &table.for_type, cancel, &lookup)?
        .into_iter()
        .skip(1)
    {
        // Static language protocols are proved by bounds and operation witnesses;
        // they have no boxed parent table (including implicit implementations).
        if Protocol::from_id(&parent.declaration).is_some_and(|protocol| {
            LangRole::from_protocol(protocol).is_some_and(LangRole::requires_static_dispatch)
        }) {
            continue;
        }
        parents.push(select(
            &parent,
            &table.for_type,
            &template.generic_params,
            closure,
            cancel,
        )?);
    }
    let view =
        if let Some(view) = erased_iterator_view(interface, &table.for_type, cancel, &lookup)? {
            let declared = lookup(&view.declaration).ok_or(invalid)?;
            let mut results = vec![];
            for (index, method) in declared.methods.iter().enumerate() {
                let raw_method = table
                    .methods
                    .iter()
                    .find(|raw| raw.name == method.name)
                    .ok_or(invalid)?;
                let call = InterfaceCallContract {
                    normalizations: vec![],
                    receiver: None,
                    operations: vec![],
                    interface: view.clone(),
                    method_slot: index as u32,
                    arguments: method.generic_params.iter().map(|p| p.as_type()).collect(),
                };
                let signature = call.signature(declared, cancel)?;
                if signature.params.len() != raw_method.params.len()
                    || signature
                        .params
                        .iter()
                        .skip(1)
                        .zip(raw_method.params.iter().skip(1))
                        .any(|(a, b)| a != &b.ty)
                {
                    return Err(invalid);
                }
                if signature.result == raw_method.return_type {
                    continue;
                }
                let Ty::Trait(result) = signature.result else {
                    return Err(invalid);
                };
                let selected = select(
                    &result,
                    &raw_method.return_type,
                    &template.generic_params,
                    closure,
                    cancel,
                )?;
                // Result adaptation boxes the raw value; a second erased view is not needed here.
                if selected.view {
                    return Err(invalid);
                }
                let mut member = view.declaration.clone();
                member.path.push(DefinitionPathSegment {
                    kind: DefinitionKind::Method,
                    name: method.name.clone(),
                    occurrence: 0,
                });
                results.push(InterfaceResultAdapter {
                    method: member,
                    implementation: selected.implementation,
                });
            }
            Some(InterfaceViewRecord {
                interface: view,
                results,
            })
        } else {
            None
        };
    Ok(InterfaceLinks { parents, view })
}

fn select(
    interface: &NominalTy,
    receiver: &Ty,
    scope: &[GenericParam],
    closure: &[&BytecodeModule],
    cancel: &CancellationToken,
) -> Result<InterfaceParentRecord, TypeTransformError> {
    let invalid = TypeTransformError::InvalidContract;
    let mut selected = None;
    // Match the receiver and trait arguments first. An explicit erased view may
    // hide an associated output, and must then match that whole surface exactly.
    let mut header = interface.clone();
    header.associated_types.clear();
    for owner in closure {
        for linked in &owner.interface_tables {
            let Some(template) = template(owner, linked) else {
                return Err(invalid);
            };
            let Some(substitution) = match_implementation(
                template,
                contract(&header.declaration, closure),
                &header,
                receiver,
                cancel,
            )?
            else {
                continue;
            };
            let arguments = template
                .generic_params
                .iter()
                .map(|p| substitution.parameter(&p.owner, p.position).cloned())
                .collect::<Option<Vec<_>>>()
                .ok_or(invalid)?;
            let key = if arguments.iter().all(Ty::is_concrete) {
                arguments.clone()
            } else {
                template
                    .generic_params
                    .iter()
                    .map(|p| p.as_type())
                    .collect()
            };
            if linked.arguments != key {
                continue;
            }
            let applied = template.instantiate_in(&arguments, scope).ok_or(invalid)?;
            let Ty::Trait(actual) = &applied.trait_type else {
                return Err(invalid);
            };
            let view = if actual == interface {
                false
            } else if erased_iterator_view(actual, receiver, cancel, &|id| contract(id, closure))?
                .as_ref()
                == Some(interface)
            {
                true
            } else {
                continue;
            };
            if selected.is_some() {
                return Err(invalid);
            }
            selected = Some(InterfaceParentRecord {
                interface: interface.clone(),
                view,
                implementation: ConcreteFunctionIdentity {
                    declaration: linked.declaration.clone(),
                    arguments,
                },
            });
        }
    }
    selected.ok_or(invalid)
}

pub(super) fn valid(
    module: &BytecodeModule,
    closure: &[&BytecodeModule],
    cancel: &CancellationToken,
) -> Result<bool, TypeTransformError> {
    for linked in &module.interface_tables {
        let expected = links(module, linked, closure, cancel)?;
        if linked.view != expected.view || linked.parents != expected.parents {
            return Ok(false);
        }
    }
    Ok(true)
}

fn template<'a, I: DefinitionReference>(
    module: &'a BytecodeModule<I>,
    linked: &InterfaceTableRecord<I>,
) -> Option<&'a InterfaceTable<I>> {
    module.public_items.iter().find_map(|item| match item {
        PublicItem::InterfaceTable(table) if table.declaration == linked.declaration => {
            Some(table.as_ref())
        }
        _ => None,
    })
}

/// Locate the exact preselected table; symbolic applications use its canonical body.
pub fn native_result_target<I: DefinitionReference>(
    adapter: &NativeResultAdapter<I>,
    closure: &[&BytecodeModule<I>],
) -> Option<(ModuleRef, usize)> {
    let (owner_index, owner) = closure.iter().enumerate().find(|(_, owner)| {
        owner
            .interface_tables
            .iter()
            .any(|linked| linked.declaration == adapter.implementation.declaration)
    })?;
    let table_index = owner.interface_tables.iter().position(|linked| {
        if linked.declaration != adapter.implementation.declaration {
            return false;
        }
        let Some(template) = template(owner, linked) else {
            return false;
        };
        let arguments = if adapter.implementation.arguments.iter().all(Ty::is_concrete) {
            adapter.implementation.arguments.clone()
        } else {
            template
                .generic_params
                .iter()
                .map(GenericParam::as_type)
                .collect()
        };
        linked.arguments == arguments
    })?;
    Some((ModuleRef::new(owner_index), table_index))
}

pub(super) fn native_result_valid(import: &NativeImport, closure: &[&BytecodeModule]) -> bool {
    let Some(adapter) = &import.result_adapter else {
        return true;
    };
    let Ty::Trait(interface) = &import.signature.result else {
        return false;
    };
    let scope = import
        .generic
        .as_ref()
        .map_or(&[][..], |body| body.parameters.as_slice());
    native_result_target(adapter, closure).is_some()
        && executable_interface(scope, interface, &adapter.receiver, closure)
}
