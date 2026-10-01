use super::{
    boundaries::{clean, route},
    runtime,
};
use crate::{executor::Executor, tests::common::compile_test_bytecode, vm::Vm};
use kagari_common::host_interface::standard_log;
use kagari_runtime::{host::HostFunction, value::Value};
use std::{cell::RefCell, rc::Rc, slice};

#[test]
fn list_join_failures_preserve_effects_and_release_the_selected_iteration_guard() {
    for (mode, labels, state) in [
        (1, vec!["iter", "item", "item"], 23),
        (2, vec!["iter", "item", "item"], 23),
        (3, vec!["iter"], 3),
    ] {
        for dynamic in [false, true] {
            let call = if dynamic {
                "val view:List<String> =source;view.join(\"/\")"
            } else {
                "source.join(\"/\")"
            };
            let program = compile_test_bytecode(&format!(
                r#"
struct Sequence {{val items:ArrayList<String>,val mode:i32,var seen:i32}}
impl Index<usize> for Sequence {{type Output=String;fn index(self,index:usize)->String{{self.items[index]}}}}
impl Iterable for Sequence {{type Item=String;type Iter=Iter<String>;fn iter(self)->Iter<String>{{
 print("iter");if self.mode==3{{val x=2147483647;val ignored=x+1;}}
 self.items.iter().map(|item|{{print("item");self.seen+=1;if item=="b"{{if self.mode==1{{self.items.push("x");}}else{{val x=2147483647;val ignored=x+1;}}}}item}})
}}}}
impl List<String> for Sequence {{fn len(self)->usize{{self.items.len()}}fn is_empty(self)->bool{{self.items.is_empty()}}fn get(self,index:usize)->Option<String>{{self.items.get(index)}}}}
fn setup()->Sequence{{Sequence{{items:["a","b","c"],mode:{mode},seen:0}}}}
fn main(source:Sequence)->String{{{call}}}
fn state(source:Sequence)->i32{{source.seen*10+source.items.len() as i32}}
fn recover(source:Sequence)->i32{{source.items.push("z");source.items.len() as i32}}
"#
            ));
            let owner = &program.modules[program.root.index()];
            let function = |name| owner.functions.iter().find(|f| f.name == name).unwrap().id;
            for encoded in [false, true] {
                let effects = Rc::new(RefCell::new(Vec::new()));
                let sink = effects.clone();
                let mut rt = runtime();
                rt.register_host_function(HostFunction::new(
                    standard_log(),
                    move |context, args| {
                        let Value::Str(label) = &args[0] else {
                            panic!()
                        };
                        sink.borrow_mut().push(label.clone());
                        context.runtime().collect_garbage().unwrap();
                        Ok(Value::Unit)
                    },
                ))
                .unwrap();
                let loaded = rt
                    .load_program("list-join-failures", route(&program, encoded))
                    .unwrap();
                let mut vm = Vm::new(rt);
                let source = vm.execute(&loaded, "setup").unwrap().return_value;
                let roots = vm.runtime().gc().root_value(source.clone()).unwrap();
                let error = Executor::new(
                    vm.runtime(),
                    &loaded,
                    function("main"),
                    slice::from_ref(&source),
                )
                .unwrap()
                .run()
                .unwrap_err();
                assert_eq!(
                    error.trace().unwrap().frames.last().unwrap().function_name,
                    "main"
                );
                assert_eq!(*effects.borrow(), labels);
                assert_eq!(
                    Executor::new(
                        vm.runtime(),
                        &loaded,
                        function("state"),
                        slice::from_ref(&source)
                    )
                    .unwrap()
                    .run()
                    .unwrap(),
                    Value::I32(state)
                );
                assert_eq!(
                    Executor::new(vm.runtime(), &loaded, function("recover"), &[source])
                        .unwrap()
                        .run()
                        .unwrap(),
                    Value::I32(4)
                );
                drop(roots);
                clean(&vm);
            }
        }
    }
}
