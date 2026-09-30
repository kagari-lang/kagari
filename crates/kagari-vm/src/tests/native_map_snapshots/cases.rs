pub(super) const TYPES: &str = r#"
struct Association<K,V> {val items:ArrayList<(K,V)>}
impl<K,V> Iterable for Association<K,V> {type Item=(K,V);type Iter=Iter<(K,V)>;
fn iter(self)->Iter<(K,V)> {print("iter");self.items.iter().inspect(|item|print("next"))}}
impl<K,V> Map<K,V> for Association<K,V> {
fn len(self)->usize{self.items.len()}fn is_empty(self)->bool{self.items.is_empty()}
fn contains_key(self,key:K)->bool{!self.items.is_empty()}
fn get(self,key:K)->Option<V>{self.items.get(0usize).map(|pair|pair[1])}}
struct Key {val value:i32}
impl PartialEq for Key {fn eq(self,other:Self)->bool{print("eq");self.value==other.value}}
impl Eq for Key {}
impl Hash for Key {fn hash(self)->i64{print("hash");self.value.hash()}}
"#;
pub(super) fn cases() -> Vec<(String, String)> {
    let mut cases = Vec::new();
    for kind in ["scalar", "heap", "float"] {
        let (key, value) = match kind {
            "scalar" => ("i32", "i32"),
            "heap" => ("Key", "ArrayList<i32>"),
            _ => ("f64", "i32"),
        };
        for source in ["direct", "custom", "dynamic", "native", "static"] {
            if kind == "float" && !matches!(source, "custom" | "dynamic") {
                continue;
            }
            for method in ["keys", "values", "entries"] {
                for length in [0, 1, 3] {
                    let pairs = (0..length)
                        .map(|index| match kind {
                            "scalar" => format!("({index},{})", 20 + index),
                            "heap" => format!("(Key{{value:{index}}},[{}])", 20 + index),
                            _ => format!("({index}.0,{})", 20 + index),
                        })
                        .collect::<Vec<_>>()
                        .join(",");
                    let setup = match source {
                        "direct" | "native" | "static" => format!(
                            "val storage=LinkedHashMap::from(items);val source{} =storage;",
                            if source == "direct" {
                                String::new()
                            } else if source == "static" {
                                format!(":LinkedHashMap<{key},{value}>")
                            } else {
                                format!(":Map<{key},{value}> ")
                            }
                        ),
                        "custom" => "val source=Association{items:items};".into(),
                        _ => format!("val source:Map<{key},{value}> =Association{{items:items}};"),
                    };
                    let item = match method {
                        "keys" => key.to_string(),
                        "values" => value.to_string(),
                        _ => format!("({key},{value})"),
                    };
                    let value_read = match (kind, method) {
                        ("heap", "keys") => "snapshot[index].value",
                        ("heap", "values") => "snapshot[index][0]",
                        ("heap", _) => "snapshot[index][0].value+snapshot[index][1][0]",
                        ("float", "keys") => "(snapshot[index] as i32)",
                        ("float", "entries") => "(snapshot[index][0] as i32)+snapshot[index][1]",
                        (_, "keys") | (_, "values") => "snapshot[index]",
                        _ => "snapshot[index][0]+snapshot[index][1]",
                    };
                    let expected = match method {
                        "keys" => "(index as i32)",
                        "values" => "20+(index as i32)",
                        _ => "20+2*(index as i32)",
                    };
                    let operation = if matches!(source, "custom" | "static") {
                        "consume(source)".into()
                    } else {
                        format!("source.{method}()")
                    };
                    let generic = if matches!(source, "custom" | "static") {
                        format!(
                            "fn consume<M:Map<{key},{value}>>(source:M)->List<{item}> {{source.{method}()}}"
                        )
                    } else {
                        String::new()
                    };
                    cases.push((format!("{kind}_{source}_{method}_{length}"),format!(r#"{TYPES}{generic}
fn main()->i32 {{val items:ArrayList<({key},{value})> =[{pairs}];{setup}
val snapshot:List<{item}> ={operation};print("snapshot");
std::debug::assert(snapshot.len()=={length}usize,"length");
var index=0usize;while index<{length}usize {{std::debug::assert({value_read}=={expected},"order");index+=1usize;}}
print("done");42}}"#)));
                }
            }
        }
    }
    cases
}
