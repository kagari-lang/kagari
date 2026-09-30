pub(super) const TYPES: &str = r#"
struct Key<T>{val id:T,var visits:i32}
impl<T:PartialEq> PartialEq for Key<T>{fn eq(self,other:Self)->bool{print("eq");self.visits+=1;self.id==other.id}}
impl<T:Eq> Eq for Key<T>{}
impl<T:Eq+Hash> Hash for Key<T>{fn hash(self)->i64{print("hash");self.visits+=1;0i64}}
struct Proxy<T>{val items:ArrayList<T>}
impl<T> Index<usize> for Proxy<T>{type Output=T;fn index(self,i:usize)->T{self.items[i]}}
impl<T> Iterable for Proxy<T>{type Item=T;type Iter=Iter<T>;fn iter(self)->Iter<T>{print("iter");self.items.iter().inspect(|item|print("next"))}}
impl<T> List<T> for Proxy<T>{fn len(self)->usize{self.items.len()}fn is_empty(self)->bool{self.items.is_empty()}fn get(self,i:usize)->Option<T>{self.items.get(i)}}
struct Cursor<T>{val items:ArrayList<T>,var index:usize}
impl<T> Iterator for Cursor<T>{type Item=T;fn next(self)->Option<T>{print("next");val item=self.items.get(self.index);self.index+=1usize;item}}
struct Span<T>{val items:ArrayList<T>}
impl<T> Iterable for Span<T>{type Item=T;type Iter=Cursor<T>;fn iter(self)->Cursor<T>{print("iter");Cursor{items:self.items,index:0usize}}}
fn set_from<K:Eq+Hash>(source:List<K>)->LinkedHashSet<K>{LinkedHashSet::from(source)}
fn map_from<K:Eq+Hash>(source:List<(K,i32)>)->LinkedHashMap<K,i32>{LinkedHashMap::from(source)}
fn set_gather<K:Eq+Hash,I:Iterable<Item=K>>(source:I)->LinkedHashSet<K>{LinkedHashSet::from_iter(source)}
fn map_gather<K:Eq+Hash,I:Iterable<Item=(K,i32)>>(source:I)->LinkedHashMap<K,i32>{LinkedHashMap::from_iter(source)}
fn take<T>(value:Option<T>)->T{match value{Some(item)=>item,None=>std::debug::panic("entry")}}
fn input<T>(value:T)->T{print("input");value}
"#;
pub(super) fn cases() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for destination in ["map", "set"] {
        for shape in ["scalar", "nominal", "tuple", "option", "identity"] {
            let key_type = match shape {
                "scalar" => "i32",
                "nominal" => "Key<i32>",
                "tuple" => "(Key<i32>,i32)",
                "option" => "Option<Key<i32>>",
                _ => "ArrayList<i32>",
            };
            let key = |id| match shape {
                "scalar" => format!("{id}"),
                "nominal" => format!("Key{{id:{id},visits:0}}"),
                "tuple" => format!("(Key{{id:{id},visits:0}},0)"),
                "option" => format!("Some(Key{{id:{id},visits:0}})"),
                _ => format!("[{id}]"),
            };
            let map = destination == "map";
            let item = if map {
                format!("({key_type},i32)")
            } else {
                key_type.to_owned()
            };
            let storage = if map {
                format!("LinkedHashMap<{key_type},i32>")
            } else {
                format!("LinkedHashSet<{key_type}>")
            };
            let owner = if map {
                "LinkedHashMap"
            } else {
                "LinkedHashSet"
            };
            for empty in [true, false] {
                let elements = if empty {
                    String::new()
                } else {
                    [0, 1, 0, 2]
                        .iter()
                        .zip([20, 21, 42, 22])
                        .map(|(i, v)| {
                            if map {
                                format!("(keys[{i}usize],{v})")
                            } else {
                                format!("keys[{i}usize]")
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(",")
                };
                for method in ["from", "from_iter"] {
                    let routes = if method == "from" {
                        vec!["native", "proxy", "generic", "dynamic"]
                    } else {
                        vec![
                            "array",
                            "iter",
                            "proxy",
                            "custom_iter",
                            "dynamic",
                            "generic",
                        ]
                    };
                    for route in routes {
                        let source = match route {
                            "native" | "array" | "generic" => "val source=items;".to_owned(),
                            "iter" => "val source=items.iter();".to_owned(),
                            "custom_iter" => "val source=Span{items:items};".to_owned(),
                            "dynamic" if method == "from" => {
                                format!("val source:List<{item}> =Proxy{{items:items}};")
                            }
                            "dynamic" => format!(
                                "val source:Iterable<Item={item},Iter=Iter<{item}>> =Proxy{{items:items}};"
                            ),
                            _ => "val source=Proxy{items:items};".to_owned(),
                        };
                        let call = if route == "generic" {
                            format!(
                                "{destination}_{}(input(source))",
                                if method == "from" { "from" } else { "gather" }
                            )
                        } else {
                            format!("{owner}::{method}(input(source))")
                        };
                        let check = if empty {
                            String::new()
                        } else if map {
                            r#"std::debug::assert(output.get(keys[0usize])==Some(42),"last value");val entries=output.entries();std::debug::assert(take(entries.get(0usize))[0]==keys[0usize] && take(entries.get(1usize))[0]==keys[1usize] && take(entries.get(2usize))[0]==keys[2usize],"order");"#.to_owned()
                        } else {
                            r#"val entries=output.to_array();std::debug::assert(take(entries.get(0usize))==keys[0usize] && take(entries.get(1usize))==keys[1usize] && take(entries.get(2usize))==keys[2usize],"order");"#.to_owned()
                        };
                        out.push((format!("{destination}_{method}_{shape}_{empty}_{route}"),format!(r#"{TYPES}
fn main()->i32{{val keys:ArrayList<{key_type}> =[{}, {}, {}];val items:ArrayList<{item}> =[{elements}];{source}val output:{storage} ={call};print("constructed");std::debug::assert(output.len()=={}usize,"length");{check}items.clear();print("done");42}}
"#,key(20),key(21),key(22),if empty{0}else{3})));
                    }
                }
            }
        }
    }
    out
}
