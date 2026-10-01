use super::{
    CategorySpec, ToolSpec, category_specs, named_primitive_input_schema, primitive_specs,
    tool_category_specs, tool_specs,
};
use serde_json::{Value, json};
use std::cmp::Reverse;

fn tokens(s: &str) -> Vec<String> {
    s.to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|x| !x.is_empty())
        .map(str::to_owned)
        .collect()
}

pub fn browser_capabilities() -> Value {
    json!({
        "operation_count": primitive_specs.len(),
        "categories": category_specs.iter().map(|category| json!({
            "name": category.name,
            "description": category.description,
            "operation_count": primitive_specs
                .iter()
                .filter(|primitive| primitive.category == category.name)
                .count()
        })).collect::<Vec<_>>()
    })
}

pub fn search_browser_operations(query: &str, limit: usize) -> Value {
    let query_tokens = tokens(query);
    let mut scored = primitive_specs
        .iter()
        .filter_map(|primitive| {
            let score = score(
                primitive.name,
                primitive.category,
                primitive.description,
                primitive.usage,
                &query_tokens,
            );
            (score > 0).then_some((
                score,
                primitive.name,
                json!({
                    "operation": primitive.name,
                    "category": primitive.category,
                    "description": primitive.description
                }),
            ))
        })
        .collect::<Vec<_>>();

    scored.sort_by_key(|(score, name, _)| (Reverse(*score), *name));
    Value::Array(
        scored
            .into_iter()
            .take(limit)
            .map(|(_, _, value)| value)
            .collect(),
    )
}

pub fn browser_operation_schema(name: &str) -> Result<Option<Value>, String> {
    let Some(primitive) = primitive_specs
        .iter()
        .find(|primitive| primitive.name == name)
    else {
        return Ok(None);
    };

    let input_schema = named_primitive_input_schema(primitive)?;
    Ok(Some(json!({
        "name": primitive.name,
        "category": primitive.category,
        "description": primitive.description,
        "input_schema": input_schema
    })))
}

pub fn capabilities() -> Value {
    json!({
        "browser": category_specs.iter().map(|c| json!({
            "name": c.name,
            "description": c.description,
            "tool_count": primitive_specs.iter().filter(|p| p.category == c.name).count()
        })).collect::<Vec<_>>(),
        "system": tool_category_specs.iter().map(|c| json!({
            "name": c.name,
            "description": c.description,
            "tool_count": tool_specs.iter().filter(|p| p.category == c.name).count()
        })).collect::<Vec<_>>()
    })
}

pub fn lightweight_tools(category: Option<&str>) -> Value {
    let mut out = Vec::new();
    out.extend(
        primitive_specs
            .iter()
            .filter(|p| category.is_none_or(|c| p.category == c))
            .map(|p| {
                json!({
                    "name": p.name,
                    "kind": "browser",
                    "category": p.category,
                    "description": p.description,
                    "usage": p.usage
                })
            }),
    );
    out.extend(
        tool_specs
            .iter()
            .filter(|p| category.is_none_or(|c| p.category == c))
            .map(|p| {
                json!({
                    "name": p.name,
                    "kind": "system",
                    "category": p.category,
                    "description": p.description,
                    "usage": p.usage
                })
            }),
    );
    Value::Array(out)
}

pub fn tools_in(category: &str) -> Value {
    lightweight_tools(Some(category))
}

pub fn search_tools(query: &str, limit: usize) -> Value {
    let q = tokens(query);
    let mut scored = Vec::new();
    scored.extend(primitive_specs.iter().filter_map(|p| {
        let score = score(p.name, p.category, p.description, p.usage, &q);
        (score > 0).then_some((
            score,
            p.name,
            json!({
                "name": p.name,
                "kind": "browser",
                "category": p.category,
                "description": p.description,
                "usage": p.usage,
                "score": score
            }),
        ))
    }));
    scored.extend(tool_specs.iter().filter_map(|p| {
        let score = score(p.name, p.category, p.description, p.usage, &q);
        (score > 0).then_some((
            score,
            p.name,
            json!({
                "name": p.name,
                "kind": "system",
                "category": p.category,
                "description": p.description,
                "usage": p.usage,
                "score": score
            }),
        ))
    }));
    scored.sort_by_key(|(score, name, _)| (Reverse(*score), *name));
    Value::Array(
        scored
            .into_iter()
            .take(limit)
            .map(|(_, _, value)| value)
            .collect(),
    )
}

fn score(name: &str, category: &str, desc: &str, usage: &str, q: &[String]) -> u64 {
    if q.is_empty() {
        return 0;
    }
    let name = name.to_ascii_lowercase();
    let category = category.to_ascii_lowercase();
    let desc = desc.to_ascii_lowercase();
    let usage = usage.to_ascii_lowercase();
    let mut score = 0;
    for term in q {
        if name == *term {
            score += 120
        } else if name.contains(term) {
            score += 70
        }
        if category == *term {
            score += 55
        } else if category.contains(term) {
            score += 25
        }
        if usage.contains(term) {
            score += 30
        }
        let dt = tokens(&desc);
        score += dt.iter().filter(|t| *t == term).count() as u64 * 22;
        if desc.contains(term) {
            score += 8
        }
    }
    score
}

pub fn category(name: &str) -> Option<&'static CategorySpec> {
    category_specs.iter().find(|c| c.name == name)
}

pub fn tool_schema(name: &str) -> Option<Value> {
    primitive_specs
        .iter()
        .find(|p| p.name == name)
        .map(|p| {
            let mut value = p.schema();
            value["kind"] = json!("browser");
            value
        })
        .or_else(|| {
            tool_specs
                .iter()
                .find(|p| p.name == name)
                .map(ToolSpec::schema)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_counts_cover_both_registries() {
        let value = capabilities();
        let browser = value["browser"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x["tool_count"].as_u64().unwrap())
            .sum::<u64>();
        let system = value["system"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x["tool_count"].as_u64().unwrap())
            .sum::<u64>();
        assert_eq!(browser, primitive_specs.len() as u64);
        assert_eq!(system, tool_specs.len() as u64);
    }

    #[test]
    fn search_prefers_semantic_matches_across_registries() {
        let r = search_tools("upload local file", 5);
        let a = r.as_array().unwrap();
        assert_eq!(a[0]["name"], "upload");

        let r = search_tools("inspect visible links", 5);
        let a = r.as_array().unwrap();
        assert!(a.iter().take(3).any(|x| x["name"] == "inspect-links"));

        let r = search_tools("telegram", 5);
        let a = r.as_array().unwrap();
        assert!(a.iter().any(|x| x["name"] == "hitl"));
        assert!(!a.iter().any(|x| x["name"] == "send-telegram-message"));
    }

    #[test]
    fn category_filter_works_for_both_kinds() {
        let a = tools_in("tabs");
        assert!(
            a.as_array()
                .unwrap()
                .iter()
                .all(|x| x["category"] == "tabs" && x["kind"] == "browser")
        );

        let a = tools_in("hitl");
        assert!(
            a.as_array()
                .unwrap()
                .iter()
                .all(|x| x["category"] == "hitl" && x["kind"] == "system")
        );
    }

    #[test]
    fn schemas_identify_tool_kind() {
        assert_eq!(tool_schema("click").unwrap()["kind"], "browser");
        assert_eq!(tool_schema("hitl").unwrap()["kind"], "system");
    }
}
