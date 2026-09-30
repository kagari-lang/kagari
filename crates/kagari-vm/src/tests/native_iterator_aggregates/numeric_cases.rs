pub(super) fn cases() -> Vec<(String, String)> {
    let mut cases = Vec::new();
    for scalar in [
        "i8", "i16", "i32", "i64", "isize", "u8", "u16", "u32", "u64", "usize", "f32", "f64",
    ] {
        for length in [0, 1, 3] {
            for method in ["sum", "product"] {
                let fields: &[i32] = match length {
                    0 => &[],
                    1 => &[2],
                    _ => &[2, 3, 7],
                };
                let items = fields
                    .iter()
                    .map(|n| {
                        if scalar.starts_with('f') {
                            format!("{n}.0{scalar}")
                        } else {
                            format!("{n}{scalar}")
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                let value = expected(method, length);
                let value = if scalar.starts_with('f') {
                    format!("{value}.0{scalar}")
                } else {
                    value.to_string()
                };
                cases.push((format!("direct_{method}_{scalar}_{length}"), format!(r#"
fn consume<I:Iterable<Item={scalar}>>(source:I)->{scalar} {{{scalar}::{method}(source)}}
fn main()->i32 {{val source:ArrayList<{scalar}> = [{items}];val result=consume(source);std::debug::assert_eq(result,{value},"numeric");print("done");42}}
"#)));
            }
        }
    }
    for kind in [
        "iterator",
        "custom_iterator",
        "lazy",
        "custom_iterable",
        "native_iterable",
        "dynamic_iterable",
        "dynamic_list",
        "readonly",
    ] {
        for length in [0, 1, 3] {
            for method in ["sum", "product"] {
                let items = match length {
                    0 => "",
                    1 => "2",
                    _ => "2,3,7",
                };
                let source = match kind {
                    "iterator" => "val source=values.iter();",
                    "custom_iterator" => "val source=Counter{items:values,index:0};",
                    "lazy" => "val source=values.iter().map(|n|{print(\"item\");n});",
                    "custom_iterable" => "val source=Wrap{values};",
                    "native_iterable" => "val source=NativeWrap{values};",
                    "dynamic_iterable" => {
                        "val source:Iterable<Item=i32,Iter=Counter<i32>> = Wrap{values};"
                    }
                    "dynamic_list" => "val source:List<i32> = values;",
                    _ => "val source:[i32] = values;",
                };
                let value = expected(method, length);
                cases.push((format!("direct_{method}_{kind}_{length}"),format!(r#"
struct Counter<T> {{val items:ArrayList<T>,var index:usize}}
impl<T> Iterator for Counter<T> {{type Item=T;fn next(self)->Option<T>{{print("next");if self.index>=self.items.len(){{None}}else{{val item=self.items[self.index];self.index+=1;Some(item)}}}}}}
struct Wrap<T> {{val values:ArrayList<T>}}
impl<T> Iterable for Wrap<T> {{type Item=T;type Iter=Counter<T>;fn iter(self)->Counter<T>{{print("iter");Counter{{items:self.values,index:0}}}}}}
struct NativeWrap<T> {{val values:ArrayList<T>}}
impl<T> Iterable for NativeWrap<T> {{type Item=T;type Iter=Iter<T>;fn iter(self)->Iter<T>{{print("iter");self.values.iter()}}}}
fn consume<I:Iterable<Item=i32>>(source:I)->i32 {{i32::{method}(source)}}
fn main()->i32 {{val values:ArrayList<i32> = [{items}];{source}val result=consume(source);std::debug::assert_eq(result,{value},"numeric");print("done");42}}
"#)));
            }
        }
    }
    cases
}

fn expected(method: &str, length: usize) -> i32 {
    match (method, length) {
        ("sum", 0) => 0,
        (_, 0) => 1,
        (_, 1) => 2,
        ("sum", _) => 12,
        _ => 42,
    }
}
