use super::{
    boundaries::{clean, lifecycle, route},
    cases::TYPES,
    runtime,
};
use crate::{error::VmError, tests::common::compile_test_bytecode, vm::Vm};
use kagari_common::host_interface::standard_log;
use kagari_runtime::{error::RuntimeErrorKind, host::HostFunction, value::Value};
use std::{cell::RefCell, rc::Rc};

#[test]
fn partition_destinations_reenter_cancel_and_exhaust_every_allocation_limit() {
    for (name, item, values, body) in [
        (
            "partition_array",
            "Key",
            "Key{value:20},Key{value:22},Key{value:20}",
            "val parts:(ArrayList<Key>,ArrayList<Key>) =fixture.iter().partition(|item|{print(\"predicate\");item.value==20});std::debug::assert(parts[0].len()==2usize && parts[1].len()==1usize,\"array\");",
        ),
        (
            "partition_bag",
            "Key",
            "Key{value:20},Key{value:22},Key{value:20}",
            "val parts:(Bag<Key>,Bag<Key>) =fixture.iter().partition(|item|{print(\"predicate\");item.value==20});std::debug::assert(parts[0].items.len()==2usize && parts[1].items.len()==1usize,\"bag\");",
        ),
        (
            "partition_total",
            "i32",
            "20,22,20",
            "val parts:(Total,Total) =fixture.iter().partition(|item|{print(\"predicate\");item==20});std::debug::assert(parts[0].value==40 && parts[1].value==22,\"total\");",
        ),
        (
            "partition_custom",
            "Key",
            "Key{value:20},Key{value:22},Key{value:20}",
            "val parts:(LinkedHashSet<Key>,LinkedHashSet<Key>) =Counter{items:fixture,index:0}.partition(|item|{print(\"predicate\");item.value==20});std::debug::assert(parts[0].len()==1usize && parts[1].len()==1usize,\"custom\");",
        ),
        (
            "partition_lazy",
            "Key",
            "Key{value:20},Key{value:22},Key{value:20}",
            "val parts:(Bag<Key>,Bag<Key>) =fixture.iter().map(|item|{print(\"item\");item}).partition(|item|{print(\"predicate\");item.value==20});std::debug::assert(parts[0].items.len()==2usize && parts[1].items.len()==1usize,\"lazy\");",
        ),
        (
            "partition_map",
            "(Key,i32)",
            "(Key{value:20},0),(Key{value:22},1),(Key{value:20},2)",
            "val parts:(LinkedHashMap<Key,i32>,LinkedHashMap<Key,i32>) =fixture.iter().partition(|item|{print(\"predicate\");item[0].value==20});std::debug::assert(parts[0].len()==1usize && parts[1].len()==1usize && parts[0].get(fixture[0][0]).unwrap_or(0)==2,\"map\");",
        ),
        (
            "partition_fallible",
            "Option<Key>",
            "Some(Key{value:20}),None,Some(Key{value:22})",
            "val parts:(Option<Bag<Key>>,Option<Bag<Key>>) =fixture.iter().partition(|item|{print(\"predicate\");item.is_some()});std::debug::assert(parts[0].map_or(false,|bag|bag.items.len()==2usize) && parts[1].is_none(),\"fallible\");",
        ),
        (
            "partition_nested",
            "Result<Option<Key>,String>",
            "Ok(Some(Key{value:20})),Ok(None),Err(\"error\")",
            "val parts:(Result<Option<LinkedHashSet<Key>>,String>,Result<Option<LinkedHashSet<Key>>,String>) =fixture.iter().partition(|item|{print(\"predicate\");item.is_ok()});std::debug::assert(parts[0].map_or(false,|value|value.is_none()) && parts[1].is_err(),\"nested\");",
        ),
        (
            "partition_empty",
            "Key",
            "",
            "val parts:(Bag<Key>,Bag<Key>) =fixture.iter().partition(|item|{print(\"unreachable\");true});std::debug::assert(parts[0].items.is_empty() && parts[1].items.is_empty(),\"empty\");",
        ),
    ] {
        let program = compile_test_bytecode(&format!(
            r#"
{TYPES}
fn setup()->ArrayList<{item}>{{[{values}]}}
fn main(fixture:ArrayList<{item}>)->i32{{print("start");{body}print("done");42}}
fn inner()->i32{{42}}fn fail()->i32{{val x=2147483647;x+1}}fn ready()->i32{{7}}
"#
        ));
        lifecycle(&program, name);
    }
}

#[test]
fn collect_delegates_the_original_iterator_and_partition_orders_shallow_factories() {
    let program = compile_test_bytecode(
        r#"
struct Cell{var value:i32}
struct Cursor<T>{val items:ArrayList<T>,var index:usize,var stopped:bool}
impl<T> Iterator for Cursor<T>{type Item=T;fn next(self)->Option<T>{print("next");if self.stopped{std::debug::panic("polled after termination")}val result=self.items.get(self.index);self.index+=1usize;if result.is_none(){self.stopped=true;}result}}
struct First<T>{val item:Option<T>}
impl<T> FromIterator<T> for First<T>{fn from_iter<I:Iterable<Item=T>>(source:I)->Self{print("first");First{item:source.iter().next()}}}
struct Bag<T>{val items:ArrayList<T>}
impl<T> FromIterator<T> for Bag<T>{fn from_iter<I:Iterable<Item=T>>(source:I)->Self{print("construct");Bag{items:source.iter().collect::<ArrayList<T>>()}}}
fn main()->i32{
 val a=Cell{value:20};val b=Cell{value:22};val c=Cell{value:99};
 val cursor=Cursor{items:[a,b,c],index:0,stopped:false};
 val first:First<Cell> =cursor.collect();
 std::debug::assert(cursor.index==1usize && first.item.map_or(false,|item|item===a),"original cursor");
 std::debug::assert(cursor.next().map_or(false,|item|item===b),"remaining source");
 val source=[a,b,c];val iterator=source.iter();
 val one:First<Cell> =iterator.collect();
 std::debug::assert(one.item.map_or(false,|item|item===a) && iterator.next().map_or(false,|item|item===b),"native progress");
 std::debug::assert(iterator.next().is_some() && iterator.next().is_none(),"exhaust");source.push(a);
 val nonfused=Cursor{items:[a,b,a],index:0,stopped:false};
 val parts:(Bag<Cell>,Bag<Cell>) =nonfused.partition(|item|{print("predicate");item.value+=1;item===a});
 std::debug::assert(nonfused.index==4usize && parts[0].items.len()==2usize && parts[1].items.len()==1usize,"once");
 std::debug::assert(parts[0].items[0]===a && parts[0].items[1]===a && parts[1].items[0]===b && a.value==22 && b.value==23,"shallow");
 val empty:(Bag<Cell>,Bag<Cell>) =[].iter().partition(|item|{print("unreachable");true});
 std::debug::assert(empty[0].items.is_empty() && empty[1].items.is_empty(),"both empty factories");
 val released=[a,b];val view:List<Cell> =released;
 val output:(ArrayList<Cell>,ArrayList<Cell>) =view.iter().partition(|item|{item.value+=1;item===a});
 released.push(c);std::debug::assert(output[0][0]===a && output[1][0]===b && released.len()==3usize,"view and release");
 42
}
"#,
    );
    for encoded in [false, true] {
        let effects = Rc::new(RefCell::new(Vec::new()));
        let sink = effects.clone();
        let mut rt = runtime();
        rt.register_host_function(HostFunction::new(standard_log(), move |context, args| {
            let Value::Str(label) = &args[0] else {
                panic!()
            };
            sink.borrow_mut().push(label.clone());
            context.runtime().collect_garbage().unwrap();
            Ok(Value::Unit)
        }))
        .unwrap();
        let loaded = rt
            .load_program("terminal-progress", route(&program, encoded))
            .unwrap();
        let mut vm = Vm::new(rt);
        assert_eq!(
            vm.execute(&loaded, "main").unwrap().return_value,
            Value::I32(42)
        );
        assert_eq!(
            effects.borrow().as_slice(),
            [
                "first",
                "next",
                "next",
                "first",
                "next",
                "predicate",
                "next",
                "predicate",
                "next",
                "predicate",
                "next",
                "construct",
                "construct",
                "construct",
                "construct"
            ]
        );
        clean(&vm);
    }
}

#[test]
fn terminal_source_predicate_and_factory_traps_preserve_completed_effects() {
    for (body, expected) in [
        (
            "val source=Fault{index:0};val result:(Broken,Broken) =source.partition(|item|{print(\"predicate\");true});",
            vec!["next"],
        ),
        (
            "val result:(Broken,Broken) =[20].iter().partition(|item|{print(\"predicate\");val x=2147483647;x+1==item});",
            vec!["predicate"],
        ),
        (
            "val result:(Broken,Broken) =[1,2].iter().partition(|item|{print(\"predicate\");item==1});",
            vec!["predicate", "predicate", "construct", "left"],
        ),
        (
            "val result:(Broken,Broken) =[3,2].iter().partition(|item|{print(\"predicate\");item==3});",
            vec!["predicate", "predicate", "construct", "construct", "right"],
        ),
        (
            "val result:Broken =Fault{index:0}.collect();",
            vec!["construct", "next"],
        ),
        (
            "val result:Broken =[1,2].iter().collect();",
            vec!["construct", "left"],
        ),
        (
            "val values=[20];val alias:List<i32> =values;val result:(ArrayList<i32>,ArrayList<i32>) =alias.iter().partition(|item|{print(\"mutation\");values.push(item);true});",
            vec!["mutation"],
        ),
    ] {
        let program = compile_test_bytecode(&format!(
            r#"
struct Fault{{var index:i32}}
impl Iterator for Fault{{type Item=i32;fn next(self)->Option<i32>{{print("next");val x=2147483647;Some(x+1)}}}}
struct Broken{{val item:Option<i32>}}
impl FromIterator<i32> for Broken{{fn from_iter<I:Iterable<Item=i32>>(source:I)->Self{{print("construct");val first=source.iter().next();if first==Some(1){{print("left");val x=2147483647;val fail=x+1;}}if first==Some(2){{print("right");val x=2147483647;val fail=x+1;}}Broken{{item:first}}}}}}
fn main()->i32{{{body}print("unreachable");0}}fn ready()->i32{{7}}
"#
        ));
        for encoded in [false, true] {
            let effects = Rc::new(RefCell::new(Vec::new()));
            let sink = effects.clone();
            let mut rt = runtime();
            rt.register_host_function(HostFunction::new(standard_log(), move |context, args| {
                let Value::Str(label) = &args[0] else {
                    panic!()
                };
                sink.borrow_mut().push(label.clone());
                context.runtime().collect_garbage().unwrap();
                Ok(Value::Unit)
            }))
            .unwrap();
            let loaded = rt
                .load_program("terminal-trap", route(&program, encoded))
                .unwrap();
            let mut vm = Vm::new(rt);
            let error = vm.execute(&loaded, "main").expect_err(body);
            assert!(error.trace().is_some(), "{body}");
            match (error.cause(), expected.as_slice() == ["mutation"]) {
                (VmError::RuntimeError(error), false) => {
                    assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
                }
                (VmError::BuiltinError(error), true) => {
                    assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
                    assert!(
                        error
                            .message()
                            .contains("structural modification during iteration")
                    );
                }
                _ => panic!("unexpected trap: {body}: {error:?}"),
            }
            assert_eq!(effects.borrow().as_slice(), expected);
            assert_eq!(
                vm.execute(&loaded, "ready").unwrap().return_value,
                Value::I32(7)
            );
            clean(&vm);
        }
    }
}
