//! Parse authoring syntax using Rust tokens; only bounded supported declarations are accepted.
use syn::{
    Attribute, Generics, Ident, ItemTrait, ItemType, Path, Result as SyntaxResult, Signature,
    Token, Type, braced,
    parse::{Parse, ParseStream},
};

mod keyword {
    syn::custom_keyword!(module);
    syn::custom_keyword!(runtime);
}

pub struct Module {
    pub runtime: Path,
    pub path: Path,
    pub items: Vec<Item>,
}

pub enum Item {
    Storage(ItemType),
    Trait(ItemTrait),
    Implementation {
        generics: Generics,
        contract: Option<Box<Type>>,
        receiver: Box<Type>,
        methods: Vec<Method>,
        bindings: Vec<(Ident, Path)>,
    },
    Function(Method),
}

pub struct Method {
    pub attrs: Vec<Attribute>,
    pub signature: Signature,
    pub factory: Path,
}

impl Parse for Module {
    fn parse(input: ParseStream<'_>) -> SyntaxResult<Self> {
        let runtime = if input.peek(keyword::runtime) {
            input.parse::<keyword::runtime>()?;
            input.parse::<Token![=]>()?;
            let path = input.parse()?;
            input.parse::<Token![;]>()?;
            path
        } else {
            syn::parse_quote!(::kagari_runtime)
        };
        input.parse::<keyword::module>()?;
        let path = input.parse()?;
        input.parse::<Token![;]>()?;
        let mut items = vec![];
        while !input.is_empty() {
            let attrs = input.call(Attribute::parse_outer)?;
            items.push(if input.peek(Token![type]) {
                let mut item: ItemType = input.parse()?;
                item.attrs = attrs;
                Item::Storage(item)
            } else if input.peek(Token![trait]) {
                let mut item: ItemTrait = input.parse()?;
                item.attrs = attrs;
                Item::Trait(item)
            } else if input.peek(Token![impl]) {
                if !attrs.is_empty() {
                    return Err(
                        input.error("document native trait methods, not implementation bindings")
                    );
                }
                implementation(input)?
            } else if input.peek(Token![fn]) {
                Item::Function(method(input, attrs)?)
            } else {
                return Err(input.error("expected native type, trait, impl or function"));
            });
            if items.len() > 4096 {
                return Err(input.error("native module exceeds declaration limit"));
            }
        }
        Ok(Self {
            runtime,
            path,
            items,
        })
    }
}

fn method(input: ParseStream<'_>, attrs: Vec<Attribute>) -> SyntaxResult<Method> {
    let signature = input.parse()?;
    input.parse::<Token![=>]>()?;
    let factory = input.parse()?;
    input.parse::<Token![;]>()?;
    Ok(Method {
        attrs,
        signature,
        factory,
    })
}

fn implementation(input: ParseStream<'_>) -> SyntaxResult<Item> {
    input.parse::<Token![impl]>()?;
    let generics = input.parse()?;
    let first: Type = input.parse()?;
    let (contract, receiver) = if input.peek(Token![for]) {
        input.parse::<Token![for]>()?;
        (Some(Box::new(first)), Box::new(input.parse()?))
    } else {
        (None, Box::new(first))
    };
    let content;
    braced!(content in input);
    let mut methods = vec![];
    let mut bindings = vec![];
    while !content.is_empty() {
        if contract.is_some() {
            let name = content.parse()?;
            content.parse::<Token![=>]>()?;
            let factory = content.parse()?;
            content.parse::<Token![;]>()?;
            bindings.push((name, factory));
        } else {
            let attrs = content.call(Attribute::parse_outer)?;
            methods.push(method(&content, attrs)?);
        }
        if methods.len() + bindings.len() > 4096 {
            return Err(content.error("native impl exceeds method limit"));
        }
    }
    Ok(Item::Implementation {
        generics,
        contract,
        receiver,
        methods,
        bindings,
    })
}
