use crate::{engine::KagariEngine, error::EmbeddingError};
use kagari_source::source::SourceFile;

#[test]
fn invalid_ordering_contracts_are_diagnostics() {
    for source in [
        "use std::cmp::{Ordering};\nfn main(){val a=Ordering::Equal?;}",
        "use std::cmp::{Ordering};\nfn main()->Ordering {Ordering::Equal?}",
        "fn needs<T:Ord>(v:T){} fn main(){needs(1.0);}",
        "use std::cmp::{Ordering};\nstruct X{} impl Ord for X {fn cmp(self,other:Self)->Ordering {Ordering::Equal}} fn main(){}",
        "struct X{} fn main()->bool {X{}<X{}}",
        "fn main()->bool {(1,2)<(2,3)}",
    ] {
        assert!(
            matches!(
                KagariEngine::default().compile_to_artifact(
                    SourceFile::new("bad-order.kgr", source),
                    Default::default()
                ),
                Err(EmbeddingError::Diagnostics { .. })
            ),
            "{source}"
        );
    }
}

#[test]
fn index_does_not_grant_element_replacement_or_immutable_field_writes() {
    for tail in ["b[0]=Item{value:1};", "b[0].value=1;"] {
        let source = format!(
            "use std::ops::{{Index}};\nstruct Item {{val value:i32}} struct Bag {{val item:Item}} impl Index<i32> for Bag {{type Output=Item;fn index(self,rhs:i32)->Item {{self.item}}}} fn main() {{val b=Bag{{item:Item{{value:0}}}};{tail}}}"
        );
        assert!(
            matches!(
                KagariEngine::default().compile_to_artifact(
                    SourceFile::new("bad-index.kgr", source),
                    Default::default()
                ),
                Err(EmbeddingError::Diagnostics { .. })
            ),
            "{tail}"
        );
    }
}

#[test]
fn invalid_operator_signatures_bounds_and_writes_are_diagnostics() {
    for source in [
        "use std::ops::{Add};\nstruct X{} impl Add<X> for X {fn add(self,rhs:X)->X {self}} fn main(){}",
        "use std::ops::{Add};\nstruct X{} impl Add<X> for X {type Output=i32;fn add(self,rhs:X)->bool {true}} fn main(){}",
        "use std::ops::{Add};\nstruct X{} impl Add<X> for X {type Output=X;fn add(self,rhs:X)->X {self}} fn main(){var a=X{};a+=X{};}",
        "use std::ops::{Add};\nfn f<T:Add<i32,Output=bool>>(x:T){} fn main(){f(1);}",
        "fn f<T:Ord>(x:T){} enum E{A} fn main(){f(E::A);}",
        "fn f<T:Ord>(x:T){} fn main(){f((1,2));}",
        "use std::ops::{Add};\nimpl Add<i32> for i32 {type Output=i32;fn add(self,rhs:i32)->i32 {0}} fn main(){}",
        "use std::ops::{Neg};\nstruct X{} impl Neg for X {type Output=i32;fn neg(self)->i32 {42}} fn main(){val x=X{};val b=x && x;}",
    ] {
        let result = KagariEngine::default().compile_to_artifact(
            SourceFile::new("bad-operator.kgr", source),
            Default::default(),
        );
        assert!(
            matches!(result, Err(EmbeddingError::Diagnostics { .. })),
            "{source}: {result:?}"
        );
    }
}
