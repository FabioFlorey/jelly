use jelly::{capabilities, lightweight_tools, search_tools, tool_schema, tools_in};
use std::env;
fn main() -> Result<(), jelly::Error> {
    let a: Vec<String> = env::args().skip(1).collect();
    let out=match a.first().map(String::as_str){
        None|Some("capabilities")=>capabilities(),
        Some("list")=>lightweight_tools(a.get(1).map(String::as_str)),
        Some("category")=>tools_in(a.get(1).ok_or("usage: discover category <name>")?),
        Some("schema")=>tool_schema(a.get(1).ok_or("usage: discover schema <tool>")?).ok_or("tool not found")?,
        Some("search")=>{let limit=a.iter().position(|x|x=="--limit").and_then(|i|a.get(i+1)).and_then(|x|x.parse().ok()).unwrap_or(5);let q=a[1..].iter().take_while(|x|x.as_str()!="--limit").cloned().collect::<Vec<_>>().join(" ");if q.is_empty(){return Err("usage: discover search <query> [--limit N]".into())}search_tools(&q,limit)},
        _=>return Err("usage: discover [capabilities|list [category]|category <name>|search <query> [--limit N]|schema <tool>]".into()),
    };
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}
