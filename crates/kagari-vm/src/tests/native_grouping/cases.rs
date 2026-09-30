pub(super) const TYPES: &str = r#"
struct Key{val id:i32,var visits:i32}
impl PartialEq for Key{fn eq(self,other:Self)->bool{print("eq");self.visits+=1;self.id==other.id}}
impl Eq for Key{}
impl Hash for Key{fn hash(self)->i64{print("hash");self.visits+=1;0i64}}
struct Item<K>{val key:K,val id:i32,var visits:i32}
struct Cursor<T>{val items:ArrayList<T>,var index:usize}
impl<T> Iterator for Cursor<T>{type Item=T;fn next(self)->Option<T>{print("next");val item=self.items.get(self.index);self.index+=1usize;item}}
fn group<K:Eq+Hash,I:Iterator<Item=Item<K>>>(source:I, key:fn(Item<K>)->K)->LinkedHashMap<K,ArrayList<Item<K>>>{source.group_by(key)}
fn input<T>(value:T)->T{print("input");value}
fn function<T>(value:T)->T{print("function");value}
fn take<T>(value:Option<T>)->T{match value{Some(item)=>item,None=>std::debug::panic("entry")}}
"#;
pub(super) fn cases() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for shape in [
        "scalar",
        "nominal",
        "tuple",
        "option",
        "identity",
        "interface",
    ] {
        let ty = match shape {
            "scalar" => "i32",
            "nominal" => "Key",
            "tuple" => "(Key,i32)",
            "option" => "Option<Key>",
            "identity" => "ArrayList<i32>",
            _ => "List<i32>",
        };
        for scenario in ["empty", "unique", "repeat", "all"] {
            let ids: Vec<usize> = match scenario {
                "empty" => vec![],
                "unique" => vec![0, 1, 2],
                "all" => vec![0, 0, 0],
                _ => vec![0, 1, 0, 2],
            };
            let item = ids
                .iter()
                .enumerate()
                .map(|(index, id)| {
                    let key = match shape {
                        "scalar" => format!("{id}"),
                        "nominal" => format!("Key{{id:{id},visits:0}}"),
                        "tuple" => format!("(Key{{id:{id},visits:0}},0)"),
                        "option" => format!("Some(Key{{id:{id},visits:0}})"),
                        _ => format!("keys[{id}usize]"),
                    };
                    format!("Item{{key:{key},id:{index},visits:0}}")
                })
                .collect::<Vec<_>>()
                .join(",");
            let mut check = String::new();
            let unique: Vec<_> = ids.iter().copied().fold(Vec::new(), |mut out, id| {
                if !out.contains(&id) {
                    out.push(id);
                }
                out
            });
            check.push_str(&format!(
                "std::debug::assert(output.len()=={}usize,\"groups\");",
                unique.len()
            ));
            for (group, id) in unique.iter().enumerate() {
                let indices: Vec<_> = ids
                    .iter()
                    .enumerate()
                    .filter_map(|(index, key)| (*key == *id).then_some(index))
                    .collect();
                check.push_str(&format!("val entry{group}=take(entries.get({group}usize));val values{group}=entry{group}[1];std::debug::assert(values{group}.len()=={}usize,\"group length\");",indices.len()));
                for (offset, index) in indices.iter().enumerate() {
                    check.push_str(&format!("val value{index}=take(values{group}.get({offset}usize));std::debug::assert(value{index}.id=={index} && value{index}.visits==1,\"item order\");value{index}.visits=42;std::debug::assert(items[{index}usize].visits==42,\"shared item\");"));
                }
                if matches!(shape, "nominal" | "tuple" | "option") {
                    let access = |root: String| match shape {
                        "tuple" => format!("{root}[0]"),
                        "option" => format!("take({root})"),
                        _ => root,
                    };
                    let first = indices[0];
                    let returned = access(format!("entry{group}[0]"));
                    let original = access(format!("items[{first}usize].key"));
                    check.push_str(&format!("{returned}.visits=99;std::debug::assert({original}.visits==99,\"first key identity\");"));
                    for duplicate in &indices[1..] {
                        let duplicate = access(format!("items[{duplicate}usize].key"));
                        check.push_str(&format!(
                            "std::debug::assert({duplicate}.visits!=99,\"duplicate identity\");"
                        ));
                    }
                }
            }
            for route in [
                "native",
                "inspect",
                "custom",
                "generic_native",
                "generic_custom",
                "generic_inspect",
            ] {
                let source = match route {
                    "native" | "generic_native" => "val source=items.iter();".to_owned(),
                    "inspect" | "generic_inspect" => {
                        "val source=items.iter().inspect(|item|print(\"next\"));".to_owned()
                    }
                    _ => "val source=Cursor{items:items,index:0usize};".to_owned(),
                };
                let call = if route.starts_with("generic") {
                    "group(input(source),function(|item|{print(\"key\");item.visits+=1;item.key}))"
                } else {
                    "input(source).group_by(function(|item|{print(\"key\");item.visits+=1;item.key}))"
                };
                out.push((format!("{shape}_{scenario}_{route}"),format!(r#"{TYPES}
fn main()->i32{{val keys:ArrayList<List<i32>> =[[0],[1],[2]];val items:ArrayList<Item<{ty}>> =[{item}];{source}val output:LinkedHashMap<{ty},ArrayList<Item<{ty}>>> ={call};print("grouped");val entries=output.entries();{check}items.clear();print("done");42}}
"#).replace("val keys:ArrayList<List<i32>>",if shape=="identity"{"val keys:ArrayList<ArrayList<i32>>"}else{"val keys:ArrayList<List<i32>>"})));
            }
        }
    }
    out
}
