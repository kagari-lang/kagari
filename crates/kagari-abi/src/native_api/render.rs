//! Generated declaration text and coordinates; never an executable input.
use super::{NativeApiError, NativeModule};
use crate::{
    scalar::BuiltinType,
    types::{
        AbiType, ConstraintAbi, FunctionAbi, GenericBoundAbi, GenericParameterAbi, NominalAbiType,
        TypeAbiKind, native::NativeTypeConstructor,
    },
};
use kagari_common::{
    collection::CollectionAccess,
    identity::{DefinitionId, DefinitionKind, associated_type_id},
    span::Span,
};
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct NativeDeclarationSite {
    pub span: Span,
    pub name_span: Span,
    pub generics: Vec<Span>,
    pub parameters: Vec<Span>,
    pub bounds: Vec<NativeBoundSite>,
}

#[derive(Debug, Clone)]
pub struct NativeBoundSite {
    pub target: Span,
    pub constraints: Vec<Span>,
}

#[derive(Debug, Clone)]
pub struct NativeApiSource {
    pub uri: String,
    pub text: String,
    pub sites: BTreeMap<DefinitionId, NativeDeclarationSite>,
}

impl NativeModule {
    pub fn declaration_source(&self) -> Result<NativeApiSource, NativeApiError> {
        let mut output = Renderer {
            module: self,
            text: "// Generated from native registration definitions. Do not edit.\n\n".into(),
            sites: BTreeMap::new(),
        };
        for ty in &self.types {
            if ty.kind != TypeAbiKind::Native(NativeTypeConstructor::Array) {
                return Err(NativeApiError(
                    "initial native API importer supports array storage only".into(),
                ));
            }
            let id = self.definition(DefinitionKind::AssociatedType, &ty.name);
            output.doc(&id);
            let start = output.text.len();
            output.text.push_str("pub type ");
            let name_span = output.name(&ty.name);
            let generics = output.generics(&ty.generic_params);
            output.text.push_str(";\n\n");
            output.site(id, start, name_span, generics, vec![]);
        }
        for item in &self.traits {
            let id = self.definition(DefinitionKind::Trait, &item.name);
            output.doc(&id);
            let start = output.text.len();
            output.text.push_str("pub trait ");
            let name_span = output.name(&item.name);
            let generics = output.generics(&item.generic_params);
            if !item.supertraits.is_empty() {
                output.text.push_str(": ");
                let names = item
                    .supertraits
                    .iter()
                    .map(|ty| self.nominal_spelling(ty))
                    .collect::<Result<Vec<_>, _>>()?;
                output.text.push_str(&names.join(" + "));
            }
            output.text.push_str(" {\n");
            for member in &item.associated_types {
                let name = &member
                    .declaration
                    .path
                    .last()
                    .ok_or_else(|| NativeApiError("missing associated name".into()))?
                    .name;
                output.doc(&member.declaration);
                let start = output.text.len();
                output.text.push_str("    type ");
                let name_span = output.name(name);
                let constraints = output.constraints(&member.bounds)?;
                output.text.push_str(";\n");
                output.site(member.declaration.clone(), start, name_span, vec![], vec![]);
                output
                    .sites
                    .get_mut(&member.declaration)
                    .expect("associated declaration")
                    .bounds = vec![NativeBoundSite {
                    target: name_span,
                    constraints,
                }];
            }
            for method in &item.methods {
                output.function(Self::method_id(&id, &method.name), method, true)?;
            }
            output.text.push_str("}\n\n");
            output.site(id, start, name_span, generics, vec![]);
        }
        for (index, implementation) in self.implementations.iter().enumerate() {
            let id = self.implementation_id(index);
            let start = output.text.len();
            output.text.push_str("impl");
            let generics = output.generics(&implementation.generic_params);
            output.text.push(' ');
            if let Some(trait_type) = &implementation.trait_type {
                let mut header = trait_type.clone();
                header.associated_types.clear();
                output.text.push_str(&self.nominal_spelling(&header)?);
                output.text.push_str(" for ");
            }
            let name_span = output.name(&self.type_spelling(&implementation.for_type)?);
            let bounds = output.bounds(&implementation.bounds)?;
            output.text.push_str(" {\n");
            if let Some(trait_type) = &implementation.trait_type {
                for (member, ty) in &trait_type.associated_types {
                    let name = &member
                        .path
                        .last()
                        .ok_or_else(|| NativeApiError("missing associated name".into()))?
                        .name;
                    let member_id = associated_type_id(&id, name);
                    let start = output.text.len();
                    output.text.push_str("    type ");
                    let name_span = output.name(name);
                    output.text.push_str(" = ");
                    let value_span = output.name(&self.type_spelling(ty)?);
                    output.text.push_str(";\n");
                    output.site(member_id, start, name_span, vec![], vec![value_span]);
                }
            }
            for method in &implementation.methods {
                output.function(Self::method_id(&id, &method.name), method, true)?;
            }
            output.text.push_str("}\n\n");
            output.site(id.clone(), start, name_span, generics, vec![]);
            output
                .sites
                .get_mut(&id)
                .expect("implementation declaration")
                .bounds = bounds;
        }
        for function in &self.functions {
            output.function(
                self.definition(DefinitionKind::Function, &function.name),
                function,
                false,
            )?;
        }
        while output.text.ends_with("\n\n") {
            output.text.pop();
        }
        Ok(NativeApiSource {
            uri: format!(
                "kagari://native/{}/{}.kgr",
                self.identity.package.0,
                self.identity.path.join("/")
            ),
            text: output.text,
            sites: output.sites,
        })
    }

    fn type_spelling(&self, ty: &AbiType) -> Result<String, NativeApiError> {
        if !ty.within_wire_limits() {
            return Err(NativeApiError("type exceeds native API limits".into()));
        }
        self.spell(ty)
    }

    fn spell(&self, ty: &AbiType) -> Result<String, NativeApiError> {
        let join = |items: &[AbiType]| -> Result<String, NativeApiError> {
            Ok(items
                .iter()
                .map(|item| self.spell(item))
                .collect::<Result<Vec<_>, _>>()?
                .join(", "))
        };
        Ok(match ty {
            AbiType::Builtin(kind) => match kind {
                BuiltinType::Never => "!",
                BuiltinType::Unit => "()",
                BuiltinType::Bool => "bool",
                BuiltinType::I8 => "i8",
                BuiltinType::I16 => "i16",
                BuiltinType::I32 => "i32",
                BuiltinType::I64 => "i64",
                BuiltinType::ISize => "isize",
                BuiltinType::U8 => "u8",
                BuiltinType::U16 => "u16",
                BuiltinType::U32 => "u32",
                BuiltinType::U64 => "u64",
                BuiltinType::USize => "usize",
                BuiltinType::F32 => "f32",
                BuiltinType::F64 => "f64",
                BuiltinType::String => "String",
            }
            .into(),
            AbiType::Parameter { position, .. } => format!("T{position}"),
            AbiType::SelfType(_) => "Self".into(),
            AbiType::Array(item, CollectionAccess::ReadOnly) => format!("[{}]", self.spell(item)?),
            AbiType::Array(item, CollectionAccess::Mutable) => {
                let name = self
                    .types
                    .iter()
                    .find(|ty| ty.kind == TypeAbiKind::Native(NativeTypeConstructor::Array))
                    .map(|ty| ty.name.as_str())
                    .unwrap_or("ArrayList");
                format!("{name}<{}>", self.spell(item)?)
            }
            AbiType::Tuple(items) => format!(
                "({}{})",
                join(items)?,
                if items.len() == 1 { "," } else { "" }
            ),
            AbiType::Function { params, result } => {
                format!("fn({}) -> {}", join(params)?, self.spell(result)?)
            }
            AbiType::Trait(ty) => self.nominal_spelling(ty)?,
            AbiType::Projection {
                receiver,
                interface,
                member,
                arguments,
            } if arguments.is_empty() => {
                let name = &member
                    .path
                    .last()
                    .ok_or_else(|| NativeApiError("missing associated name".into()))?
                    .name;
                format!(
                    "<{} as {}>::{name}",
                    self.spell(receiver)?,
                    self.nominal_spelling(interface)?
                )
            }
            AbiType::StandardEnum { kind, args } if args.is_empty() => format!("{kind:?}"),
            AbiType::StandardEnum { kind, args } => format!("{kind:?}<{}>", join(args)?),
            _ => {
                return Err(NativeApiError(
                    "unsupported type in initial native API importer".into(),
                ));
            }
        })
    }

    fn nominal_spelling(&self, ty: &NominalAbiType) -> Result<String, NativeApiError> {
        let name = ty
            .declaration
            .path
            .last()
            .ok_or_else(|| NativeApiError("missing declaration name".into()))?;
        let mut text = if ty.declaration.module == self.identity {
            name.name.clone()
        } else {
            let module = &ty.declaration.module;
            let prefix = if module.package.0 == "kagari-std" {
                "std"
            } else {
                &module.package.0
            };
            format!("{}::{}::{}", prefix, module.path.join("::"), name.name)
        };
        let mut args = ty
            .arguments
            .iter()
            .map(|ty| self.type_spelling(ty))
            .collect::<Result<Vec<_>, _>>()?;
        for (id, value) in &ty.associated_types {
            args.push(format!(
                "{} = {}",
                id.path
                    .last()
                    .ok_or_else(|| NativeApiError("missing associated name".into()))?
                    .name,
                self.type_spelling(value)?
            ));
        }
        if !args.is_empty() {
            text.push_str(&format!("<{}>", args.join(", ")));
        }
        Ok(text)
    }
}

struct Renderer<'a> {
    module: &'a NativeModule,
    text: String,
    sites: BTreeMap<DefinitionId, NativeDeclarationSite>,
}

impl Renderer<'_> {
    fn doc(&mut self, id: &DefinitionId) {
        if let Some(documentation) = self.module.documentation.get(id) {
            for line in documentation.lines() {
                if id.path.len() > 1 {
                    self.text.push_str("    ");
                }
                self.text.push_str("/// ");
                self.text.push_str(line);
                self.text.push('\n');
            }
        }
    }
    fn name(&mut self, name: &str) -> Span {
        let start = self.text.len();
        self.text.push_str(name);
        Span::new(start, self.text.len())
    }
    fn generics(&mut self, parameters: &[GenericParameterAbi]) -> Vec<Span> {
        if parameters.is_empty() {
            return vec![];
        }
        self.text.push('<');
        let mut spans = vec![];
        for (index, parameter) in parameters.iter().enumerate() {
            if index != 0 {
                self.text.push_str(", ");
            }
            spans.push(self.name(&format!("T{}", parameter.position)));
        }
        self.text.push('>');
        spans
    }
    fn site(
        &mut self,
        id: DefinitionId,
        start: usize,
        name_span: Span,
        generics: Vec<Span>,
        parameters: Vec<Span>,
    ) {
        self.sites.insert(
            id,
            NativeDeclarationSite {
                span: Span::new(start, self.text.trim_end_matches('\n').len()),
                name_span,
                generics,
                parameters,
                bounds: vec![],
            },
        );
    }
    fn function(
        &mut self,
        id: DefinitionId,
        function: &FunctionAbi,
        method: bool,
    ) -> Result<(), NativeApiError> {
        self.doc(&id);
        let start = self.text.len();
        self.text.push_str(if !method {
            "pub fn "
        } else if id.path[0].kind == DefinitionKind::Trait {
            "    fn "
        } else {
            "    pub fn "
        });
        let name_span = self.name(&function.name);
        let own = if method {
            &[][..]
        } else {
            &function.generic_params[..]
        };
        let generics = self.generics(own);
        self.text.push('(');
        let mut parameters = vec![];
        for (index, parameter) in function.params.iter().enumerate() {
            if index != 0 {
                self.text.push_str(", ");
            }
            parameters.push(self.name(&parameter.name));
            if parameter.name != "self" {
                self.text.push_str(": ");
                self.text
                    .push_str(&self.module.type_spelling(&parameter.ty)?);
            }
        }
        self.text.push(')');
        if function.return_type != AbiType::Builtin(BuiltinType::Unit) {
            self.text.push_str(" -> ");
            self.text
                .push_str(&self.module.type_spelling(&function.return_type)?);
        }
        let bounds = self.bounds(&function.bounds)?;
        self.text.push_str(";\n");
        self.site(id.clone(), start, name_span, generics, parameters);
        self.sites
            .get_mut(&id)
            .expect("rendered declaration")
            .bounds = bounds;
        Ok(())
    }

    fn bounds(
        &mut self,
        bounds: &[GenericBoundAbi],
    ) -> Result<Vec<NativeBoundSite>, NativeApiError> {
        let mut sites = vec![];
        if !bounds.is_empty() {
            self.text.push_str(" where ");
        }
        for (index, bound) in bounds.iter().enumerate() {
            if index != 0 {
                self.text.push_str(", ");
            }
            let target = self.name(&self.module.type_spelling(&bound.ty)?);
            let constraints = self.constraints(&bound.constraints)?;
            sites.push(NativeBoundSite {
                target,
                constraints,
            });
        }
        Ok(sites)
    }

    fn constraints(&mut self, constraints: &[ConstraintAbi]) -> Result<Vec<Span>, NativeApiError> {
        let mut spans = vec![];
        if !constraints.is_empty() {
            self.text.push_str(": ");
        }
        for (index, constraint) in constraints.iter().enumerate() {
            if index != 0 {
                self.text.push_str(" + ");
            }
            let ConstraintAbi::Trait(trait_type) = constraint else {
                return Err(NativeApiError("native bound requires a named trait".into()));
            };
            spans.push(self.name(&self.module.nominal_spelling(trait_type)?));
        }
        Ok(spans)
    }
}
