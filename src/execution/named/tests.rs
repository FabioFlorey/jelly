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

    let set_cookie = named_primitive_input_schema(spec("set-cookie")).unwrap();
    assert_eq!(
        set_cookie["properties"]["cookie"]["required"],
        json!(["name", "value"])
    );
    assert_eq!(
        set_cookie["properties"]["cookie"]["properties"]["sameSite"]["enum"],
        json!(["Strict", "Lax", "None"])
    );
    assert_eq!(
        set_cookie["properties"]["cookie"]["additionalProperties"],
        false
    );

    let delete_cookie = named_primitive_input_schema(spec("delete-cookie")).unwrap();
    assert_eq!(
        delete_cookie["properties"]["selector"]["required"],
        json!(["name"])
    );
    assert_eq!(
        delete_cookie["properties"]["selector"]["additionalProperties"],
        false
    );

    let storage_set = named_primitive_input_schema(spec("storage-set")).unwrap();
    assert_eq!(
        storage_set["properties"]["area"]["enum"],
        json!(["local", "session"])
    );
}

#[test]
fn named_arguments_are_normalized_in_declared_order() {
    assert_eq!(
        prepare_named_primitive_args(spec("fill"), &json!({"target":"css:#email","text":"a b"}))
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
        prepare_named_primitive_args(spec("accessibility-tree"), &json!({"max":"25"})).unwrap_err(),
        "max for accessibility-tree must be a non-negative integer",
    );
    invalid(
        prepare_named_primitive_args(spec("accessibility-tree"), &json!({"max":-1})).unwrap_err(),
        "max for accessibility-tree must be a non-negative integer",
    );
    invalid(
        prepare_named_primitive_args(spec("accessibility-tree"), &json!({"max":1.5})).unwrap_err(),
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
