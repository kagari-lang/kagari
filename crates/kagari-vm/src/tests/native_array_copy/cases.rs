pub(super) const TYPES: &str = r#"
struct Cell {var value:i32}
struct Proxy<T> {val items:ArrayList<T>}
impl<T> Index<usize> for Proxy<T> {type Output=T;fn index(self,i:usize)->T{self.items[i]}}
impl<T> Iterable for Proxy<T> {type Item=T;type Iter=Iter<T>;fn iter(self)->Iter<T>{print("iter");self.items.iter().inspect(|item|print("next"))}}
impl<T> List<T> for Proxy<T> {fn len(self)->usize{self.items.len()}fn is_empty(self)->bool{self.items.is_empty()}fn get(self,i:usize)->Option<T>{self.items.get(i)}}
struct Cursor<T> {val items:ArrayList<T>,var index:usize}
impl<T> Iterator for Cursor<T> {type Item=T;fn next(self)->Option<T>{print("next");val item=self.items.get(self.index);self.index+=1usize;item}}
struct Span<T> {val items:ArrayList<T>}
impl<T> Iterable for Span<T> {type Item=T;type Iter=Cursor<T>;fn iter(self)->Cursor<T>{print("iter");Cursor{items:self.items,index:0usize}}}
fn make<T>(source:List<T>)->ArrayList<T>{ArrayList::from(source)}
fn append<T,M:MutableList<T>>(target:M,source:List<T>){target.extend(source);}
fn copy<T>(target:ArrayList<T>,source:List<T>){target.copy_from(source);}
fn gather<T,I:Iterable<Item=T>>(source:I)->ArrayList<T>{ArrayList::from_iter(source)}
"#;

pub(super) fn cases() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for count in [0, 1, 3] {
        for heap in [false, true] {
            let item = if heap { "Cell" } else { "i32" };
            let literal = |value| {
                if heap {
                    format!("Cell{{value:{value}}}")
                } else {
                    format!("{value}")
                }
            };
            let elements = (0..count)
                .map(|i| literal(20 + i))
                .collect::<Vec<_>>()
                .join(",");
            let zeroes = (0..count).map(|_| literal(0)).collect::<Vec<_>>().join(",");
            for method in ["from", "copy", "extend", "from_iter"] {
                let routes = if method == "from_iter" {
                    vec!["array", "iter", "proxy", "custom_iter", "dynamic"]
                } else {
                    vec!["native", "proxy", "generic", "dynamic", "self"]
                };
                for route in routes {
                    let setup = match (method, route) {
                        ("from_iter", "array") | (_, "native" | "generic" | "self") => {
                            "val source=items;".to_owned()
                        }
                        ("from_iter", "iter") => "val source=items.iter();".to_owned(),
                        ("from_iter", "custom_iter") => "val source=Span{items:items};".to_owned(),
                        ("from_iter", "dynamic") => format!(
                            "val source:Iterable<Item={item},Iter=Iter<{item}>> =Proxy{{items:items}};"
                        ),
                        (_, "dynamic") => format!("val source:List<{item}> =Proxy{{items:items}};"),
                        _ => "val source=Proxy{items:items};".to_owned(),
                    };
                    let constructor = matches!(method, "from" | "from_iter");
                    let (operation, prefix) = match method {
                        "from" => (
                            if route == "generic" {
                                format!("val output:ArrayList<{item}> =make(source);")
                            } else {
                                format!("val output:ArrayList<{item}> =ArrayList::from(source);")
                            }
                            .to_owned(),
                            0,
                        ),
                        "from_iter" => {
                            (format!("val output:ArrayList<{item}> =gather(source);"), 0)
                        }
                        "copy" => {
                            let target = if route == "self" {
                                "val output=items;".to_owned()
                            } else {
                                format!("val output:ArrayList<{item}> =[{zeroes}];")
                            };
                            (
                                format!(
                                    "{target} {}",
                                    if route == "generic" {
                                        "copy(output,source);"
                                    } else {
                                        "output.copy_from(source);"
                                    }
                                ),
                                0,
                            )
                        }
                        _ => {
                            let (target, prefix) = if route == "self" {
                                ("val output=items;".to_owned(), count)
                            } else {
                                (format!("val output=[{}];", literal(9)), 1)
                            };
                            let action = match route {
                                "generic" => "append(output,source);".to_owned(),
                                "dynamic" => format!(
                                    "val target:MutableList<{item}> =output;target.extend(source);"
                                ),
                                _ => "output.extend(source);".to_owned(),
                            };
                            (format!("{target}{action}"), prefix)
                        }
                    };
                    let actual = if heap { "output[i].value" } else { "output[i]" };
                    let identity = if count == 0 {
                        String::new()
                    } else if heap {
                        format!(
                            "items[0usize].value=42;std::debug::assert(output[{prefix}usize].value==42,\"shared payload\");"
                        )
                    } else if constructor {
                        "items[0usize]=99;std::debug::assert(output[0usize]==20,\"independent slots\");".to_owned()
                    } else {
                        String::new()
                    };
                    out.push((
                        format!("{method}_{route}_{heap}_{count}"),
                        format!(
                            r#"{TYPES}
fn main()->i32{{val items:ArrayList<{item}> =[{elements}];{setup}{operation}
print("snapshot");std::debug::assert(output.len()=={}usize,"length");
for i in {prefix}usize..{}usize{{std::debug::assert({actual}==20+(i-{prefix}usize) as i32,"item");}}
{identity}items.push({});print("done");42}}
"#,
                            prefix + count,
                            prefix + count,
                            literal(0)
                        ),
                    ));
                }
            }
        }
    }
    out
}
