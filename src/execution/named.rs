use super::{ArgKind, PrimitiveSpec, execute_browser_primitive, registry};
use crate::{BrowserSession, Error, ErrorKind, Target, jelly_error};
use serde_json::{Map, Value, json};
use std::collections::HashSet;

pub fn validate_named_primitive_contract(spec: &PrimitiveSpec) -> Result<(), String> {
    let mut names = HashSet::with_capacity(spec.args.len());
    let mut optional_seen = false;

    for arg in spec.args {
        if arg.name.trim().is_empty() {
            return Err(format!("{} declares an empty argument name", spec.name));
        }
        if !names.insert(arg.name) {
            return Err(format!(
                "{} declares duplicate argument name {}",
                spec.name, arg.name
            ));
        }
        if optional_seen && arg.required {
            return Err(format!(
                "{} declares required argument {} after an optional positional argument",
                spec.name, arg.name
            ));
        }
        optional_seen |= !arg.required;
    }

    if let Some(max_args) = spec.max_args
        && max_args != spec.args.len()
    {
        return Err(format!(
            "{} declares {} named arguments but max_args is {}",
            spec.name,
            spec.args.len(),
            max_args
        ));
    }

    Ok(())
}

pub fn named_primitive_input_schema(spec: &PrimitiveSpec) -> Result<Value, String> {
    validate_named_primitive_contract(spec)?;

    let mut properties = Map::new();
    let mut required = Vec::new();
    let mut dependent_required = Map::new();

    for (index, arg) in spec.args.iter().enumerate() {
        let schema = match arg.kind {
            ArgKind::Integer => json!({"type":"integer","minimum":0}),
            ArgKind::Target => json!({
                "type":"string",
                "description":"Element target: document-scoped @e…/@img… ref, css:<selector>, text:<exact text>, or plain exact text."
            }),
            ArgKind::String => json!({"type":"string"}),
        };
        properties.insert(arg.name.to_owned(), schema);

        if arg.required {
            required.push(Value::String(arg.name.to_owned()));
        }

        let preceding_optional = spec.args[..index]
            .iter()
            .filter(|prior| !prior.required)
            .map(|prior| Value::String(prior.name.to_owned()))
            .collect::<Vec<_>>();
        if !preceding_optional.is_empty() {
            dependent_required.insert(arg.name.to_owned(), Value::Array(preceding_optional));
        }
    }

    let mut schema = Map::from_iter([
        ("type".to_owned(), Value::String("object".to_owned())),
        ("properties".to_owned(), Value::Object(properties)),
        ("required".to_owned(), Value::Array(required)),
        ("additionalProperties".to_owned(), Value::Bool(false)),
    ]);
    if !dependent_required.is_empty() {
        schema.insert(
            "dependentRequired".to_owned(),
            Value::Object(dependent_required),
        );
    }
    Ok(Value::Object(schema))
}

pub fn prepare_named_primitive_args(
    spec: &PrimitiveSpec,
    arguments: &Value,
) -> Result<Vec<String>, Error> {
    validate_named_primitive_contract(spec).map_err(|message| {
        jelly_error(
            ErrorKind::Internal,
            format!(
                "invalid named-argument contract for {}: {message}",
                spec.name
            ),
            false,
        )
    })?;

    let object = arguments.as_object().ok_or_else(|| {
        jelly_error(
            ErrorKind::InvalidArguments,
            format!("{} arguments must be a JSON object", spec.name),
            false,
        )
    })?;

    let mut unknown = object
        .keys()
        .filter(|name| !spec.args.iter().any(|arg| arg.name == name.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    unknown.sort();
    if !unknown.is_empty() {
        return Err(jelly_error(
            ErrorKind::InvalidArguments,
            format!(
                "unknown argument{} for {}: {}",
                if unknown.len() == 1 { "" } else { "s" },
                spec.name,
                unknown.join(", ")
            ),
            false,
        ));
    }

    let mut positional = Vec::with_capacity(spec.args.len());
    let mut first_omitted_optional: Option<&str> = None;

    for arg in spec.args {
        match object.get(arg.name) {
            Some(value) => {
                if let Some(omitted) = first_omitted_optional {
                    return Err(jelly_error(
                        ErrorKind::InvalidArguments,
                        format!(
                            "{} requires preceding optional argument {} when using named arguments for {}",
                            arg.name, omitted, spec.name
                        ),
                        false,
                    ));
                }
                positional.push(named_value_to_positional(spec, arg.name, arg.kind, value)?);
            }
            None if arg.required => {
                return Err(jelly_error(
                    ErrorKind::InvalidArguments,
                    format!("missing required argument for {}: {}", spec.name, arg.name),
                    false,
                ));
            }
            None => {
                if first_omitted_optional.is_none() {
                    first_omitted_optional = Some(arg.name);
                }
            }
        }
    }

    spec.validate(&positional)?;
    Ok(positional)
}

fn named_value_to_positional(
    spec: &PrimitiveSpec,
    name: &str,
    kind: ArgKind,
    value: &Value,
) -> Result<String, Error> {
    match kind {
        ArgKind::String => value.as_str().map(str::to_owned).ok_or_else(|| {
            jelly_error(
                ErrorKind::InvalidArguments,
                format!("{name} for {} must be a string", spec.name),
                false,
            )
        }),
        ArgKind::Target => {
            let value = value.as_str().ok_or_else(|| {
                jelly_error(
                    ErrorKind::InvalidArguments,
                    format!("{name} for {} must be a target string", spec.name),
                    false,
                )
            })?;
            Target::parse(value)?;
            Ok(value.to_owned())
        }
        ArgKind::Integer => {
            let value = value.as_u64().ok_or_else(|| {
                jelly_error(
                    ErrorKind::InvalidArguments,
                    format!("{name} for {} must be a non-negative integer", spec.name),
                    false,
                )
            })?;
            Ok(value.to_string())
        }
    }
}

pub fn execute_named_browser_primitive(
    browser: &mut BrowserSession,
    name: &str,
    arguments: &Value,
) -> Result<String, Error> {
    let spec = registry::lookup(name).ok_or_else(|| {
        jelly_error(
            ErrorKind::Unsupported,
            format!("unsupported browser primitive: {name}"),
            false,
        )
    })?;
    let positional = prepare_named_primitive_args(spec, arguments)?;
    execute_browser_primitive(browser, name, &positional)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ErrorKind, classify_error, primitive_specs};

    fn spec(name: &str) -> &'static PrimitiveSpec {
        primitive_specs
            .iter()
            .find(|spec| spec.name == name)
            .unwrap()
    }

    fn invalid(error: Error, expected: &str) {
        let (kind, retryable) = classify_error(error.as_ref());
        assert_eq!(kind, ErrorKind::InvalidArguments);
        assert!(!retryable);
        assert!(
            error.to_string().contains(expected),
            "expected {:?} to contain {:?}",
            error.to_string(),
            expected
        );
    }

    #[test]
    fn every_registered_primitive_has_a_valid_named_contract() {
        for primitive in primitive_specs {
            validate_named_primitive_contract(primitive)
                .unwrap_or_else(|error| panic!("{}: {error}", primitive.name));
        }
    }

    #[test]
    fn named_schema_and_optional_dependencies_share_the_positional_contract() {
        let snapshot = named_primitive_input_schema(spec("snapshot-interactive")).unwrap();
        assert_eq!(snapshot["properties"]["limit"]["type"], "integer");
        assert_eq!(snapshot["properties"]["offset"]["type"], "integer");
        assert_eq!(snapshot["dependentRequired"]["offset"], json!(["limit"]));

        let highlight = named_primitive_input_schema(spec("highlight")).unwrap();
        assert_eq!(highlight["required"], json!(["target"]));
        assert_eq!(highlight["dependentRequired"]["mode"], json!(["label"]));
        assert_eq!(highlight["additionalProperties"], false);
    }

    #[test]
    fn named_arguments_are_normalized_in_declared_order() {
        assert_eq!(
            prepare_named_primitive_args(
                spec("fill"),
                &json!({"target":"css:#email","text":"a b"})
            )
            .unwrap(),
            vec!["a b".to_owned(), "css:#email".to_owned()]
        );
        assert_eq!(
            prepare_named_primitive_args(spec("accessibility-tree"), &json!({"max":25})).unwrap(),
            vec!["25".to_owned()]
        );
    }

    #[test]
    fn omitted_trailing_optional_arguments_do_not_create_placeholders() {
        assert_eq!(
            prepare_named_primitive_args(spec("fill"), &json!({"text":"hello"})).unwrap(),
            vec!["hello".to_owned()]
        );
        assert!(
            prepare_named_primitive_args(spec("snapshot-interactive"), &json!({}))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn optional_holes_are_rejected_instead_of_changing_positional_meaning() {
        invalid(
            prepare_named_primitive_args(spec("snapshot-interactive"), &json!({"offset":10}))
                .unwrap_err(),
            "offset requires preceding optional argument limit",
        );
        invalid(
            prepare_named_primitive_args(
                spec("highlight"),
                &json!({
                    "target":"css:#save",
                    "mode":"box"
                }),
            )
            .unwrap_err(),
            "mode requires preceding optional argument label",
        );
    }

    #[test]
    fn unknown_missing_and_wrong_typed_arguments_are_rejected() {
        invalid(
            prepare_named_primitive_args(spec("read-page"), &json!({"surprise":true})).unwrap_err(),
            "unknown argument for read-page: surprise",
        );
        invalid(
            prepare_named_primitive_args(spec("click"), &json!({})).unwrap_err(),
            "missing required argument for click: target",
        );
        invalid(
            prepare_named_primitive_args(spec("accessibility-tree"), &json!({"max":"25"}))
                .unwrap_err(),
            "max for accessibility-tree must be a non-negative integer",
        );
        invalid(
            prepare_named_primitive_args(spec("accessibility-tree"), &json!({"max":-1}))
                .unwrap_err(),
            "max for accessibility-tree must be a non-negative integer",
        );
        invalid(
            prepare_named_primitive_args(spec("accessibility-tree"), &json!({"max":1.5}))
                .unwrap_err(),
            "max for accessibility-tree must be a non-negative integer",
        );
    }

    #[test]
    fn targets_are_parsed_before_any_handler_can_run() {
        invalid(
            prepare_named_primitive_args(spec("click"), &json!({"target":"css:"})).unwrap_err(),
            "css target cannot be empty",
        );
    }

    #[test]
    fn argument_container_must_be_an_object() {
        invalid(
            prepare_named_primitive_args(spec("click"), &json!(["css:#save"])).unwrap_err(),
            "click arguments must be a JSON object",
        );
    }

    #[test]
    fn variadic_cli_primitives_have_a_precise_named_surface() {
        let args = prepare_named_primitive_args(
            spec("evaluate-js"),
            &json!({"expression":"document.title + ' x'"}),
        )
        .unwrap();
        assert_eq!(args, vec!["document.title + ' x'".to_owned()]);
        invalid(
            prepare_named_primitive_args(
                spec("evaluate-js"),
                &json!({"expression":"document.title","extra":"x"}),
            )
            .unwrap_err(),
            "unknown argument for evaluate-js: extra",
        );
    }
}
