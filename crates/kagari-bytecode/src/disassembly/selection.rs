//! Exact source-free selection and the shared compact function header.

use crate::{
    disassembly::{
        DisassemblyError,
        presentation::{DisplayContext, effects, physical},
    },
    module::{BytecodeFunction, BytecodeModule},
    program::verified::VerifiedBytecodeProgram,
};
use kagari_common::identity::ModuleIdentity;
use std::fmt::Write;

/// Returns selected artifact-local buffer slots, preserving stored order.
pub(crate) fn selected_slots(
    program: &VerifiedBytecodeProgram,
    selector: Option<&str>,
) -> Result<Vec<(usize, usize)>, DisassemblyError> {
    let modules = &program.program().modules;
    if let Some(selector) = selector
        && let Some(numeric) = selector.strip_prefix('@')
    {
        let invalid = || DisassemblyError::InvalidSelector(selector.to_owned());
        let (m, f) = numeric.split_once(':').ok_or_else(invalid)?;
        let parse = |part: &str| {
            if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(invalid());
            }
            part.parse::<usize>().map_err(|_| invalid())
        };
        let (m, f) = (parse(m)?, parse(f)?);
        if modules
            .get(m)
            .and_then(|module| module.functions.get(f))
            .is_none()
        {
            return Err(invalid());
        }
        return Ok(vec![(m, f)]);
    }
    let context = DisplayContext {
        definitions: program.definitions(),
    };
    let mut slots = Vec::new();
    for (m, module) in modules.iter().enumerate() {
        for (f, function) in module.functions.iter().enumerate() {
            if selector.is_none_or(|name| qualified_name(&context, module, function) == name) {
                slots.push((m, f));
            }
        }
    }
    if let Some(selector) = selector {
        match slots.len() {
            0 => return Err(DisassemblyError::FunctionNotFound(selector.to_owned())),
            1 => {}
            _ => {
                let candidates = slots
                    .iter()
                    .map(|&(m, f)| {
                        function_header(&context, &modules[m], &modules[m].functions[f], m, f)
                    })
                    .collect();
                return Err(DisassemblyError::AmbiguousFunction {
                    selector: selector.to_owned(),
                    candidates,
                });
            }
        }
    }
    Ok(slots)
}

fn module_name(module: &ModuleIdentity) -> String {
    let mut name = module.package.0.clone();
    for component in &module.path {
        name.push_str("::");
        name.push_str(component);
    }
    name
}

fn qualified_name(
    context: &DisplayContext<'_>,
    module: &BytecodeModule,
    function: &BytecodeFunction,
) -> String {
    if let Some(identity) = &function.identity {
        let view = context
            .definitions
            .resolve(identity.declaration)
            .expect("verified function identity belongs to the retained scope");
        let mut name = module_name(view.module());
        for segment in view.segments() {
            name.push_str("::");
            name.push_str(segment.name);
            if segment.occurrence != 0 {
                write!(name, "#{}", segment.occurrence).expect("writing to a String");
            }
        }
        name
    } else {
        format!("{}::{}", module_name(&module.identity), function.name)
    }
}

/// Formats the header shared by the compact directory and full body renderer.
///
/// Semantic signature facts take precedence; missing facts are explicitly physical.
/// Concrete identity arguments are not inferred from generated function names.
pub(crate) fn function_header(
    context: &DisplayContext<'_>,
    module: &BytecodeModule,
    function: &BytecodeFunction,
    m: usize,
    f: usize,
) -> String {
    let name = if let Some(identity) = &function.identity {
        context.definition(identity.declaration)
    } else {
        // Reuse component escaping even for generated names without a definition ID.
        let mut path = module.identity.clone();
        path.path.push(function.name.clone());
        context.module(&path)
    };
    let mut output = format!("fn @{m}:{f} {name}");
    if let Some(identity) = &function.identity
        && !identity.arguments.is_empty()
    {
        output.push('<');
        for (index, ty) in identity.arguments.iter().enumerate() {
            if index != 0 {
                output.push_str(", ");
            }
            output.push_str(&context.ty(ty));
        }
        output.push('>');
    }
    output.push('(');
    let metadata = &function.metadata;
    for (index, ty) in metadata.params.iter().enumerate() {
        if index != 0 {
            output.push_str(", ");
        }
        write!(output, "arg{index}: ").expect("writing to a String");
        match metadata.semantic.params.get(&index) {
            Some(semantic) => output.push_str(&context.ty(semantic)),
            None => write!(output, "physical({})", physical(*ty)).expect("writing to a String"),
        }
    }
    output.push_str(") -> ");
    match &metadata.semantic.result {
        Some(semantic) => output.push_str(&context.ty(semantic)),
        None => write!(output, "physical({})", physical(metadata.return_type))
            .expect("writing to a String"),
    }
    write!(
        output,
        " ; instructions={} may_suspend={} recorded_effects={}",
        function.instructions.len(),
        metadata.effects.may_suspend,
        effects(metadata.effects)
    )
    .expect("writing to a String");
    output
}
