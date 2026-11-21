//! Declaration requirements of source-owned operations. Callers establish public
//! membership and reachability; IDs here always belong to the declaration's table.
use super::{Protocol, binding, iteration};
use crate::{
    callable::{CallableImplementation, MethodPolicy},
    declaration::{FnDecl, TraitDef},
    scalar::BuiltinType,
    ty::{GenericParam, NominalTy, Ty},
};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity,
    table::{DefinitionId, DefinitionTable},
};

pub(super) fn required_method(
    owner: DefinitionId,
    table: &DefinitionTable,
    method: &FnDecl,
    name: &str,
    params: &[(&str, Ty)],
    result: &Ty,
) -> bool {
    method.name == name
        && table
            .lookup_child(owner, DefinitionKind::Method, name, 0)
            .is_some()
        && method.implementation == CallableImplementation::Required
        && method.method_policy == MethodPolicy::default()
        && method.host_policy.is_none()
        && method.generic_params.is_empty()
        && method.bounds.is_empty()
        && method.params.len() == params.len()
        && method
            .params
            .iter()
            .zip(params)
            .all(|(actual, (name, ty))| !actual.mutable && actual.name == *name && actual.ty == *ty)
        && method.return_type == *result
}

/// Ordinary library traits whose contracts are required alongside the language roles.
pub fn companions(module: &ModuleIdentity) -> &'static [&'static str] {
    if *module == Protocol::From.module() {
        &["Into", "TryFrom", "TryInto"]
    } else if *module == Protocol::Iterator.module() {
        &["FromIterator", "Sum", "Product"]
    } else if *module == Protocol::Try.module() {
        &["RangeBounds"]
    } else if module.package.0 == "kagari-core" && module.path == ["str"] {
        &["FromStr"]
    } else if module.package.0 == "kagari-core" && module.path == ["default"] {
        &["Default"]
    } else {
        &[]
    }
}

/// Exact portable header for arithmetic, propagation, and ordinary core conversions.
pub fn trait_shape_valid(owner: DefinitionId, actual: &TraitDef, table: &DefinitionTable) -> bool {
    check(owner, actual, table) == Some(true)
}

fn check(owner: DefinitionId, actual: &TraitDef, table: &DefinitionTable) -> Option<bool> {
    let view = table.resolve(owner).ok()?;
    let name = actual.name.as_str();
    let role = Protocol::from_definition(&owner, table);
    if table.lookup_child(
        table.lookup_root(view.module())?,
        DefinitionKind::Trait,
        name,
        0,
    ) != Some(owner)
        || !(role.is_some_and(Protocol::source_operation)
            || companions(view.module()).contains(&name))
    {
        return None;
    }
    if view.module() == &Protocol::Iterator.module() {
        return iteration::valid(owner, actual, table);
    }
    let generic = !matches!(
        name,
        "Try" | "Neg" | "Not" | "Default" | "Debug" | "Display" | "Hash" | "FromStr"
    );
    let param = GenericParam { owner, position: 0 };
    let argument = param.as_type();
    if actual
        .generic_params
        .iter()
        .cloned()
        .ne(generic.then_some(param))
        || !actual.bounds.is_empty()
        || !actual.associated_consts.is_empty()
        || actual.conversion_adapter.is_some() != (name == "TryFrom")
    {
        return None;
    }
    let interface = NominalTy {
        declaration: owner,
        arguments: generic.then(|| argument.clone()).into_iter().collect(),
        associated_types: Default::default(),
    };
    let this = Ty::SelfType(owner);
    let projection = |name| {
        Some(Ty::Projection {
            receiver: Box::new(this.clone()),
            interface: Box::new(interface.clone()),
            member: table.lookup_child(owner, DefinitionKind::AssociatedType, name, 0)?,
            arguments: vec![],
        })
    };
    let associated: &[&str] = match name {
        "Try" => &["Output", "Residual"],
        "TryFrom" | "TryInto" => &["Error"],
        "FromStr" => &["Err"],
        _ if role.is_some_and(Protocol::operator) => &["Output"],
        _ => &[],
    };
    if actual.associated_types.len() != associated.len()
        || !associated.iter().all(|name| {
            actual.associated_types.iter().any(|a| {
                table.lookup_child(owner, DefinitionKind::AssociatedType, name, 0)
                    == Some(a.declaration)
                    && a.generic_params.is_empty()
                    && a.parameter_bounds.is_empty()
                    && a.bounds.is_empty()
            })
        })
    {
        return None;
    }
    let method = |slot, name, params: &[(&str, Ty)], result: &Ty| {
        actual
            .methods
            .get(slot)
            .is_some_and(|m| required_method(owner, table, m, name, params, result))
    };
    if name == "Try" {
        let residual = projection("Residual")?;
        let output = projection("Output")?;
        let parent = NominalTy {
            declaration: Protocol::FromResidual.definition(table)?,
            arguments: vec![residual.clone()],
            associated_types: Default::default(),
        };
        let flow = Ty::Enum(NominalTy {
            declaration: table.lookup(&binding::control_flow_declaration())?,
            arguments: vec![residual, output.clone()],
            associated_types: Default::default(),
        });
        return Some(
            actual.supertraits == [parent]
                && actual.methods.len() == 2
                && method(0, "from_output", &[("output", output)], &this)
                && method(1, "branch", &[("self", this)], &flow),
        );
    }
    if !actual.supertraits.is_empty() {
        return None;
    }
    if name == "RangeBounds" {
        let bound = Ty::Enum(NominalTy {
            declaration: table.lookup(&binding::bound_declaration())?,
            arguments: vec![argument],
            associated_types: Default::default(),
        });
        let receiver = [("self", this)];
        return Some(
            actual.methods.len() == 2
                && method(0, "start_bound", &receiver, &bound)
                && method(1, "end_bound", &receiver, &bound),
        );
    }
    let (selector, params, result) = match name {
        "Hash" => ("hash", vec![("self", this)], Ty::Builtin(BuiltinType::I64)),
        "Debug" => (
            "debug",
            vec![("self", this)],
            Ty::Builtin(BuiltinType::String),
        ),
        "Display" => (
            "display",
            vec![("self", this)],
            Ty::Builtin(BuiltinType::String),
        ),
        "FromStr" => (
            "from_str",
            vec![("text", Ty::Builtin(BuiltinType::String))],
            binding::result(table, this.clone(), projection("Err")?)?,
        ),
        "Default" => ("default", vec![], this),
        "From" => ("from", vec![("value", argument)], this),
        "FromResidual" => ("from_residual", vec![("residual", argument)], this),
        "Into" => ("into", vec![("self", this)], argument),
        "TryFrom" => {
            let adapter = actual.conversion_adapter.as_ref()?;
            if !adapter.valid_in(&owner, actual, table)
                || table.lookup(&binding::result_declaration()) != Some(adapter.result)
            {
                return None;
            }
            (
                "try_from",
                vec![("value", argument)],
                actual.methods.first()?.return_type.clone(),
            )
        }
        "TryInto" => (
            "try_into",
            vec![("self", this.clone())],
            binding::result(table, argument, projection("Error")?)?,
        ),
        _ if role.is_some_and(Protocol::operator) => {
            let mut params = vec![("self", this.clone())];
            if generic {
                params.push(("arg1", argument));
            }
            return Some(
                actual.methods.len() == 1
                    && method(
                        0,
                        match role? {
                            Protocol::Add => "add",
                            Protocol::Sub => "sub",
                            Protocol::Mul => "mul",
                            Protocol::Div => "div",
                            Protocol::Rem => "rem",
                            Protocol::BitAnd => "bitand",
                            Protocol::BitOr => "bitor",
                            Protocol::BitXor => "bitxor",
                            Protocol::Shl => "shl",
                            Protocol::Shr => "shr",
                            Protocol::Neg => "neg",
                            Protocol::Not => "not",
                            Protocol::Index => "index",
                            Protocol::Fn => "call",
                            _ => return None,
                        },
                        &params,
                        &projection("Output")?,
                    ),
            );
        }
        _ => return None,
    };
    Some(actual.methods.len() == 1 && method(0, selector, &params, &result))
}
