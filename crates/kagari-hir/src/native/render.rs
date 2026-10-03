//! Generated declaration text and coordinates; never an executable input.
use kagari_common::{
    collection::CollectionAccess,
    identity::{DefinitionKind, DefinitionPath, DefinitionPathSegment, associated_type_id},
    span::Span,
};
use kagari_contract::{
    declaration::{DeclarationError, ModuleDecl},
    language,
    scalar::BuiltinType,
    types::{
        Constraint, FnDecl, GenericBound, GenericParam, NominalTy, Ty, TypeDefKind,
        native::NativeTypeConstructor,
    },
};
use std::{collections::BTreeMap, ops::Deref};

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
pub struct DeclarationSource {
    pub uri: String,
    pub text: String,
    pub sites: BTreeMap<DefinitionPath, NativeDeclarationSite>,
}

pub fn declaration_source(module: &ModuleDecl) -> Result<DeclarationSource, DeclarationError> {
    DeclarationView(module).render()
}

struct DeclarationView<'a>(&'a ModuleDecl);

impl Deref for DeclarationView<'_> {
    type Target = ModuleDecl;
    fn deref(&self) -> &ModuleDecl {
        self.0
    }
}

impl DeclarationView<'_> {
    fn render(&self) -> Result<DeclarationSource, DeclarationError> {
        let mut output = Renderer {
            module: self,
            text: "// Generated from native registration definitions. Do not edit.\n\n".into(),
            sites: BTreeMap::new(),
        };
        for ty in &self.types {
            let constructor = match ty.kind {
                TypeDefKind::Native(constructor) => Some(constructor),
                TypeDefKind::NativeStorage(_) => None,
                _ => {
                    return Err(DeclarationError(
                        "native declaration requires a registered representation".into(),
                    ));
                }
            };
            let id = self.definition(
                constructor.map_or(
                    DefinitionKind::AssociatedType,
                    NativeTypeConstructor::declaration_kind,
                ),
                &ty.name,
            );
            output.doc(&id);
            let start = output.text.len();
            output.text.push_str(
                if matches!(constructor, Some(NativeTypeConstructor::Enum(_))) {
                    "pub enum "
                } else {
                    "pub type "
                },
            );
            let name_span = output.name(&ty.name);
            let generics = output.generics(&ty.generic_params);
            let bounds = output.bounds(&ty.bounds)?;
            if matches!(constructor, Some(NativeTypeConstructor::Enum(_))) {
                output.text.push_str(" {\n");
                for variant in &ty.variants {
                    let start = output.text.len();
                    output.text.push_str("    ");
                    let name_span = output.name(&variant.name);
                    let mut payload = vec![];
                    if !variant.payload.is_empty() {
                        output.text.push('(');
                        for (index, ty) in variant.payload.iter().enumerate() {
                            if index > 0 {
                                output.text.push_str(", ");
                            }
                            payload.push(output.name(&self.type_spelling(ty)?));
                        }
                        output.text.push(')');
                    }
                    output.text.push_str(",\n");
                    let mut owner = id.clone();
                    owner.path.push(DefinitionPathSegment {
                        kind: DefinitionKind::Variant,
                        name: variant.name.clone(),
                        occurrence: 0,
                    });
                    output.site(owner, start, name_span, vec![], payload);
                }
                output.text.push_str("}\n\n");
            } else {
                output.text.push_str(";\n\n");
            }
            output.site(id.clone(), start, name_span, generics, vec![]);
            output
                .sites
                .get_mut(&id)
                .expect("type declaration site")
                .bounds = bounds;
            if self.variant_exports.contains(&ty.name) {
                output.text.push_str(&format!(
                    "pub use self::{}::{{{}}};\n\n",
                    ty.name,
                    ty.variants
                        .iter()
                        .map(|variant| variant.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
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
                    .ok_or_else(|| DeclarationError("missing associated name".into()))?
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
                output.function(ModuleDecl::method_id(&id, &method.name), method, true)?;
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
                        .ok_or_else(|| DeclarationError("missing associated name".into()))?
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
                output.function(ModuleDecl::method_id(&id, &method.name), method, true)?;
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
        Ok(DeclarationSource {
            uri: format!(
                "kagari://native/{}/{}.kgr",
                self.identity.package.0,
                self.identity.path.join("/")
            ),
            text: output.text,
            sites: output.sites,
        })
    }

    fn type_spelling(&self, ty: &Ty) -> Result<String, DeclarationError> {
        if !ty.within_wire_limits() {
            return Err(DeclarationError("type exceeds native API limits".into()));
        }
        self.spell(ty)
    }

    fn spell(&self, ty: &Ty) -> Result<String, DeclarationError> {
        let join = |items: &[Ty]| -> Result<String, DeclarationError> {
            Ok(items
                .iter()
                .map(|item| self.spell(item))
                .collect::<Result<Vec<_>, _>>()?
                .join(", "))
        };
        Ok(match ty {
            Ty::Builtin(kind) => match kind {
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
            Ty::Parameter { owner, position } => parameter_spelling(owner, *position),
            Ty::SelfType(_) => "Self".into(),
            Ty::Array(item, CollectionAccess::ReadOnly) => format!("[{}]", self.spell(item)?),
            Ty::Array(item, CollectionAccess::Mutable) => {
                let name = self
                    .types
                    .iter()
                    .find(|ty| ty.kind == TypeDefKind::Native(NativeTypeConstructor::Array))
                    .map(|ty| ty.name.as_str())
                    .unwrap_or("ArrayList");
                format!("{name}<{}>", self.spell(item)?)
            }
            Ty::Map {
                key,
                value,
                access: CollectionAccess::Mutable,
            } => {
                let name = self.representation_name(NativeTypeConstructor::Map, "HashMap");
                format!("{name}<{}, {}>", self.spell(key)?, self.spell(value)?)
            }
            Ty::Set(item, CollectionAccess::Mutable) => {
                let name = self.representation_name(NativeTypeConstructor::Set, "HashSet");
                format!("{name}<{}>", self.spell(item)?)
            }
            Ty::Tuple(items) => format!(
                "({}{})",
                join(items)?,
                if items.len() == 1 { "," } else { "" }
            ),
            Ty::Function { params, result } => {
                format!("fn({}) -> {}", join(params)?, self.spell(result)?)
            }
            Ty::Trait(ty) | Ty::NativeObject(ty) => self.nominal_spelling(ty)?,
            Ty::Projection {
                receiver,
                interface,
                member,
                arguments,
            } if arguments.is_empty() => {
                let name = &member
                    .path
                    .last()
                    .ok_or_else(|| DeclarationError("missing associated name".into()))?
                    .name;
                format!(
                    "<{} as {}>::{name}",
                    self.spell(receiver)?,
                    self.nominal_spelling(interface)?
                )
            }
            Ty::Range(item, kind) => {
                let name =
                    self.representation_name(NativeTypeConstructor::Range(*kind), kind.name());
                if NativeTypeConstructor::Range(*kind).arity() == 0 {
                    name.into()
                } else {
                    format!("{name}<{}>", self.spell(item)?)
                }
            }
            Ty::Iter(item) => {
                let name = self.representation_name(NativeTypeConstructor::Iter, "Iter");
                format!("{name}<{}>", self.spell(item)?)
            }
            Ty::StandardEnum { kind, args } => {
                let fallback = format!("{kind:?}");
                let name = self.representation_name(NativeTypeConstructor::Enum(*kind), &fallback);
                if args.is_empty() {
                    name.into()
                } else {
                    format!("{name}<{}>", join(args)?)
                }
            }
            _ => {
                return Err(DeclarationError(
                    "unsupported type in initial native API importer".into(),
                ));
            }
        })
    }

    fn representation_name<'a>(
        &'a self,
        constructor: NativeTypeConstructor,
        fallback: &'a str,
    ) -> &'a str {
        self.types
            .iter()
            .find(|ty| ty.kind == TypeDefKind::Native(constructor))
            .map(|ty| ty.name.as_str())
            .unwrap_or(fallback)
    }

    fn nominal_spelling(&self, ty: &NominalTy) -> Result<String, DeclarationError> {
        let name = ty
            .declaration
            .path
            .last()
            .ok_or_else(|| DeclarationError("missing declaration name".into()))?;
        let mut text = if ty.declaration.module == self.identity {
            name.name.clone()
        } else if ty.declaration.module == language::module_identity() {
            format!("{}::language::{}", language::SOURCE_PACKAGE, name.name)
        } else {
            let module = &ty.declaration.module;
            format!("{}::{}", module, name.name)
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
                    .ok_or_else(|| DeclarationError("missing associated name".into()))?
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
    module: &'a DeclarationView<'a>,
    text: String,
    sites: BTreeMap<DefinitionPath, NativeDeclarationSite>,
}

impl Renderer<'_> {
    fn doc(&mut self, id: &DefinitionPath) {
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

    fn generics(&mut self, parameters: &[GenericParam]) -> Vec<Span> {
        if parameters.is_empty() {
            return vec![];
        }
        self.text.push('<');
        let mut spans = vec![];
        for (index, parameter) in parameters.iter().enumerate() {
            if index != 0 {
                self.text.push_str(", ");
            }
            spans.push(self.name(&parameter_spelling(&parameter.owner, parameter.position)));
        }
        self.text.push('>');
        spans
    }

    fn site(
        &mut self,
        id: DefinitionPath,
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
        id: DefinitionPath,
        function: &FnDecl,
        method: bool,
    ) -> Result<(), DeclarationError> {
        self.doc(&id);
        let start = self.text.len();
        self.text
            .push_str(if self.module.private_functions.contains(&id) {
                "fn "
            } else if !method {
                "pub fn "
            } else if id.path[0].kind == DefinitionKind::Trait {
                "    fn "
            } else {
                "    pub fn "
            });
        let name_span = self.name(&function.name);
        let own = function
            .generic_params
            .iter()
            .filter(|parameter| !method || parameter.owner == id)
            .cloned()
            .collect::<Vec<_>>();
        let generics = self.generics(&own);
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
        if function.return_type != Ty::Builtin(BuiltinType::Unit) {
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
        bounds: &[GenericBound],
    ) -> Result<Vec<NativeBoundSite>, DeclarationError> {
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

    fn constraints(&mut self, constraints: &[Constraint]) -> Result<Vec<Span>, DeclarationError> {
        let mut spans = vec![];
        if !constraints.is_empty() {
            self.text.push_str(": ");
        }
        for (index, constraint) in constraints.iter().enumerate() {
            if index != 0 {
                self.text.push_str(" + ");
            }
            let name = match constraint {
                Constraint::Trait(trait_type) => self.module.nominal_spelling(trait_type)?,
                Constraint::Standard(kind) => kind
                    .source_bound_name()
                    .ok_or_else(|| DeclarationError("native bound has no source name".into()))?
                    .into(),
            };
            spans.push(self.name(&name));
        }
        Ok(spans)
    }
}

/// Separate lexical names for method binders and their enclosing declaration.
pub fn parameter_spelling(owner: &DefinitionPath, position: usize) -> String {
    let prefix = if owner
        .path
        .last()
        .is_some_and(|part| part.kind == DefinitionKind::Method)
    {
        "M"
    } else {
        "T"
    };
    format!("{prefix}{position}")
}
