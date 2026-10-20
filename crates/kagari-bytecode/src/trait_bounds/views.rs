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
        DefinitionKind, ModuleIdentity,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_contract::{
    callable::interface::InterfaceCallContract,
    native_import::{NativeImport, result::NativeResultAdapter},
    types::{
        ConcreteFunctionIdentity, InterfaceBridge, InterfaceTable, PublicItem,
        matching::match_implementation,
    },
};
use kagari_types::ty::{
    GenericParam, NominalTy, Ty,
    inheritance::{erased_iterator_view, interface_views},
    substitution::{TypeSubstitution, TypeTransformError},
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
    definitions: &DefinitionTable,
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
        .instantiate(&linked.arguments, &template.generic_params, definitions)
        .ok_or(invalid)?;
    let Ty::Trait(interface) = &table.trait_type else {
        return Err(invalid);
    };
    let lookup = |id: &DefinitionId| contract(id, closure, definitions);
    let mut parents = vec![];
    for parent in interface_views(interface, &table.for_type, cancel, definitions, &lookup)?
        .into_iter()
        .skip(1)
    {
        // Static implementations also have tables, but only structurally dynamic
        // parents have boxed views. Admission checks the entire dynamic closure.
        if !executable_interface(
            &template.generic_params,
            &parent,
            closure,
            definitions,
            cancel,
        ) {
            continue;
        }
        parents.push(select(
            &module.identity,
            &parent,
            &table.for_type,
            &template.generic_params,
            closure,
            cancel,
            definitions,
        )?);
    }
    let view = if let Some(view) =
        erased_iterator_view(interface, &table.for_type, cancel, definitions, &lookup)?
    {
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
            let signature = call.signature(declared, cancel, definitions)?;
            if raw_method.generic_params.len() != call.arguments.len() {
                return Err(invalid);
            }
            // The view and concrete implementation have distinct method
            // binders. Compare their applied signatures in the view's scope.
            let mut substitution = TypeSubstitution::default();
            for (parameter, argument) in raw_method.generic_params.iter().zip(&call.arguments) {
                substitution.bind(&parameter.owner, parameter.position, argument);
            }
            let params = raw_method
                .params
                .iter()
                .map(|parameter| substitution.apply(&parameter.ty, cancel))
                .collect::<Result<Vec<_>, _>>()?;
            let raw_result = substitution.apply(&raw_method.return_type, cancel)?;
            if signature.params.len() != raw_method.params.len()
                || signature
                    .params
                    .iter()
                    .skip(1)
                    .zip(params.iter().skip(1))
                    .any(|(a, b)| a != b)
            {
                return Err(invalid);
            }
            if signature.result == raw_result {
                continue;
            }
            let Ty::Trait(result) = signature.result else {
                return Err(invalid);
            };
            let selected = select(
                &module.identity,
                &result,
                &raw_result,
                &template.generic_params,
                closure,
                cancel,
                definitions,
            )?;
            // Result adaptation boxes the raw value; a second erased view is not needed here.
            if selected.view {
                return Err(invalid);
            }
            let member = definitions
                .lookup_child(view.declaration, DefinitionKind::Method, &method.name, 0)
                .ok_or(invalid)?;
            results.push(InterfaceResultAdapter {
                method: member,
                implementation: selected.implementation,
            });
        }
        parents.push(InterfaceParentRecord {
            interface: view.clone(),
            view: true,
            implementation: ConcreteFunctionIdentity {
                declaration: linked.declaration,
                arguments: linked.arguments.clone(),
            },
        });
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
    local: &ModuleIdentity,
    interface: &NominalTy,
    receiver: &Ty,
    scope: &[GenericParam],
    closure: &[&BytecodeModule],
    cancel: &CancellationToken,
    definitions: &DefinitionTable,
) -> Result<InterfaceParentRecord, TypeTransformError> {
    let invalid = TypeTransformError::InvalidContract;
    let mut selected: Option<InterfaceParentRecord> = None;
    let mut selected_bridge = None;
    let mut selected_local = false;
    // Match the receiver and trait arguments first. An explicit erased view may
    // hide an associated output, and must then match that whole surface exactly.
    let mut header = interface.clone();
    header.associated_types.clear();
    for owner in closure {
        for linked in &owner.interface_tables {
            let Some(template) = template(owner, linked) else {
                return Err(invalid);
            };
            let Some(substitution) = match_implementation(template, &header, receiver, cancel)?
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
            let applied = template
                .instantiate(&arguments, scope, definitions)
                .ok_or(invalid)?;
            let Ty::Trait(actual) = &applied.trait_type else {
                return Err(invalid);
            };
            let view = if actual == interface {
                false
            } else if erased_iterator_view(actual, receiver, cancel, definitions, &|id| {
                contract(id, closure, definitions)
            })?
            .as_ref()
                == Some(interface)
            {
                true
            } else {
                continue;
            };
            if let Some(previous) = &selected {
                // Separate modules may materialize the same intrinsic operation.
                // These verified forwarding tables add no implementation evidence;
                // Prefer the local bridge so an imported module never gains a
                // dependency on its consumer; break remaining ties by identity.
                if selected_bridge != Some(InterfaceBridge::Intrinsic)
                    || template.bridge != Some(InterfaceBridge::Intrinsic)
                {
                    return Err(invalid);
                }
                let candidate_local = &owner.identity == local;
                if selected_local && !candidate_local
                    || selected_local == candidate_local
                        && !definitions
                            .compare(previous.implementation.declaration, linked.declaration)
                            .map_err(|_| invalid)?
                            .is_gt()
                {
                    continue;
                }
            }
            selected_bridge = template.bridge;
            selected_local = &owner.identity == local;
            selected = Some(InterfaceParentRecord {
                interface: interface.clone(),
                view,
                implementation: ConcreteFunctionIdentity {
                    declaration: linked.declaration,
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
    definitions: &DefinitionTable,
) -> Result<bool, TypeTransformError> {
    for linked in &module.interface_tables {
        let expected = links(module, linked, closure, cancel, definitions)?;
        if linked.view != expected.view || linked.parents != expected.parents {
            return Ok(false);
        }
    }
    Ok(true)
}

fn template<'a>(
    module: &'a BytecodeModule,
    linked: &InterfaceTableRecord,
) -> Option<&'a InterfaceTable> {
    module.public_items.iter().find_map(|item| match item {
        PublicItem::InterfaceTable(table) if table.declaration == linked.declaration => {
            Some(table.as_ref())
        }
        _ => None,
    })
}

/// Locate the exact preselected table; symbolic applications use its canonical body.
pub fn native_result_target(
    adapter: &NativeResultAdapter,
    closure: &[&BytecodeModule],
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

pub(super) fn native_result_valid(
    import: &NativeImport,
    closure: &[&BytecodeModule],
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> bool {
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
        && executable_interface(scope, interface, closure, definitions, cancel)
}
