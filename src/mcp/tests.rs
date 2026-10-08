use super::*;

fn web_test_state() -> AuthState {
    AuthState::new(
        "01234567890123456789012345678901".into(),
        "0123456789abcdef".into(),
        "abcdef0123456789abcdef0123456789".into(),
        ConsentMode::Browser,
        true,
        "https://jelly.example".into(),
    )
    .unwrap()
}

#[tokio::test]
async fn detailed_status_and_dashboard_require_admin_access() {
    let state = web_test_state();
    let unauthenticated = status_json(State(state.clone()), HeaderMap::new()).await;
    assert_eq!(unauthenticated.status(), StatusCode::FORBIDDEN);
    let dashboard_page = dashboard(State(state.clone()), HeaderMap::new()).await;
    assert_eq!(dashboard_page.status(), StatusCode::FORBIDDEN);

    let mut authorized = HeaderMap::new();
    authorized.insert(
        axum::http::header::AUTHORIZATION,
        "Bearer abcdef0123456789abcdef0123456789".parse().unwrap(),
    );
    let status = status_json(State(state), authorized).await;
    assert_eq!(status.status(), StatusCode::OK);
    let body = axum::body::to_bytes(status.into_body(), 128 * 1024)
        .await
        .unwrap();
    assert!(
        String::from_utf8(body.to_vec())
            .unwrap()
            .contains("runtime_root")
    );
}

#[tokio::test]
async fn connections_page_has_jelly_branding_without_credentials() {
    let page = connections_page(State(web_test_state()), HeaderMap::new()).await;
    assert_eq!(page.status(), StatusCode::OK);
    assert_eq!(
        page.headers()[axum::http::header::CACHE_CONTROL],
        "no-store, max-age=0"
    );
    let bytes = axum::body::to_bytes(page.into_body(), 128 * 1024)
        .await
        .unwrap();
    let html = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(html.contains("/brand/jelly.css"));
    assert!(html.contains("/brand/full-logo.png"));
    assert!(html.contains("Connections"));
    assert!(!html.contains("abcdef0123456789abcdef0123456789"));
    assert!(!html.contains("01234567890123456789012345678901"));
}

#[tokio::test]
async fn health_and_readiness_are_public_service_probes() {
    let Json(health) = health().await;
    let Json(readiness) = ready().await;
    assert_eq!(health["status"], "ok");
    assert_eq!(readiness["component"], "mcp-server");
    assert_eq!(readiness["ready"], true);
}

#[test]
fn initialize_includes_operating_instructions() {
    let response = initialize(&json!({"protocolVersion": DEFAULT_PROTOCOL_VERSION}));
    let instructions = response["instructions"].as_str().unwrap();
    assert!(instructions.contains("**Interface uncertainty**"));
    assert!(instructions.contains("**Browser-state uncertainty**"));
    assert!(instructions.contains("**User-intent uncertainty**"));
    assert!(
        instructions.contains("never use an environment-changing operation as an interface probe")
    );
    assert!(instructions.contains("current machine-readable information"));
    assert!(instructions.contains("do not redefine Jelly's tool surface"));
    assert!(instructions.contains("Inspect before mutation"));
    assert!(instructions.contains("Reject nonessential cookies by default"));
    assert!(instructions.contains("active ChatGPT conversation"));
    assert!(instructions.contains("through Telegram"));
    assert!(instructions.contains("exhaust legitimate automatable paths"));
    assert!(instructions.contains(".agent/tools/index.md"));
    assert!(instructions.contains("docs/reference/DISCOVERY.md"));
    assert!(instructions.contains("agent-discover schema <tool>"));
    assert!(instructions.contains("Detect the active surface from `tools/list`"));
    assert!(instructions.contains("Prefer a **semantic Jelly operation**"));
    assert!(instructions.contains("execute semantic operations through `browser-call`"));
    assert!(instructions.contains("Use **browser-events** only when"));
    assert!(instructions.contains("Use **raw CDP** only when it is published"));
    assert!(instructions.contains("Do not supply Chromium `targetId` or `sessionId` values"));
    assert!(instructions.contains("Use `call-routine`"));
    assert!(instructions.contains("published system artifact tools"));
    assert!(instructions.contains("inputSchema"));
    assert!(instructions.contains("agent-run <tool> [args...]"));
    assert!(instructions.contains("agent-run type-text -- --help"));
    assert!(instructions.contains("Do not automatically retry side-effecting operations"));
    assert!(instructions.contains("Treat browser lifecycle as owned state"));
    assert!(instructions.contains("close that Jelly-owned browser when the task is complete"));
    assert!(instructions.contains("Continuous mode streams captured renderer frames into FFmpeg"));
    assert!(
        instructions.contains("automatically derives a short action comment from trace metadata")
    );
    assert!(instructions.contains("stores it as the step `label`"));
    assert!(!instructions.contains("Use direct primitives for short, local interactions"));
    assert!(!instructions.contains("Use snapshot-interactive before clicking"));
}

#[test]
fn instructions_expose_selection_gate_without_bloating_with_tool_catalog() {
    let instructions = initialize(&json!({}))["instructions"]
        .as_str()
        .unwrap()
        .to_owned();
    for rule in [
        "### Tool choice in one pass",
        "### Final task check",
        "**MUST** use the named MCP schema",
        "**NEVER** claim an outcome solely",
        "AGENT_PLAYBOOK.md",
    ] {
        assert!(
            instructions.contains(rule),
            "missing operating rule: {rule}"
        );
    }
    assert!(
        instructions.split_whitespace().count() <= 1700,
        "MCP initialization instructions must stay bounded; move details to the on-demand playbook"
    );
}

#[test]
fn agent_routing_fixture_uses_actual_published_tool_names_and_input_fields() {
    let cases: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/agent-guidance-cases.json"
    ))
    .expect("agent-routing fixture must be valid JSON");
    let cases = cases.as_array().unwrap();
    assert!(cases.len() >= 16);
    for case in cases {
        let surface = match case["surface"].as_str().unwrap() {
            "small" => crate::McpSurface::SmallSurface,
            "large" => crate::McpSurface::LargeSurface,
            unexpected => panic!("unknown agent-routing surface: {unexpected}"),
        };
        let catalog = crate::agent_catalog_for_surface(surface, crate::RawCdpAccess::Disabled);
        let name = case["expected_tool"].as_str().unwrap();
        let tool = catalog
            .get(name)
            .unwrap_or_else(|| panic!("{}: tool {name} is not published", case["id"]));
        let args = case["required_args"].as_object().unwrap();
        let schema = tool.input_schema();
        // A few builtins use oneOf action branches instead of a root properties map.
        if schema["properties"].is_object() {
            for key in args.keys() {
                assert!(
                    schema["properties"].get(key).is_some(),
                    "{}: unknown {name} input field {key}",
                    case["id"]
                );
            }
        } else if let Some(branches) = schema["oneOf"].as_array() {
            assert!(
                branches.iter().any(|branch| {
                    args.keys()
                        .all(|key| branch["properties"].get(key).is_some())
                        && args
                            .get("action")
                            .is_none_or(|action| branch["properties"]["action"]["const"] == *action)
                }),
                "{}: no matching inputSchema action branch for {name}",
                case["id"]
            );
        }
        if let crate::AgentToolBinding::Builtin(builtin) = tool.binding() {
            builtin
                .preflight(&Value::Object(args.clone()))
                .unwrap_or_else(|error| {
                    panic!(
                        "{}: invalid builtin {name} reference arguments: {error}",
                        case["id"]
                    )
                });
        }
    }
}

#[test]
fn descriptions_disambiguate_commonly_confused_tools() {
    let small = mcp_tools_from_catalog(crate::agent_catalog_for_surface(
        crate::McpSurface::SmallSurface,
        crate::RawCdpAccess::Disabled,
    ));
    let description = |name: &str| {
        small.iter().find(|tool| tool["name"] == name).unwrap()["description"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert!(description("screenshot").contains("record-browser"));
    assert!(description("record-browser").contains("screenshot"));
    assert!(description("call-routine").contains("browser-call"));
    assert!(description("browser-call").contains("browser-schema"));
    assert!(description("hitl").contains("Telegram"));

    let large = mcp_tools_from_catalog(crate::agent_catalog_for_surface(
        crate::McpSurface::LargeSurface,
        crate::RawCdpAccess::Disabled,
    ));
    let find = large
        .iter()
        .find(|tool| tool["name"] == "find-interactive")
        .unwrap();
    let snapshot = large
        .iter()
        .find(|tool| tool["name"] == "snapshot-interactive")
        .unwrap();
    assert!(
        find["description"]
            .as_str()
            .unwrap()
            .contains("snapshot-interactive")
    );
    assert!(
        snapshot["description"]
            .as_str()
            .unwrap()
            .contains("find-interactive")
    );
}

#[test]
fn large_surface_agent_catalog_preserves_the_current_mcp_surface() {
    let tools = mcp_tools_from_catalog(crate::agent_catalog_for_surface(
        crate::McpSurface::LargeSurface,
        crate::RawCdpAccess::Disabled,
    ));
    for spec in crate::primitive_specs {
        assert!(tools.iter().any(|tool| tool["name"] == spec.name));
    }
    for spec in crate::tool_specs {
        assert!(
            tools.iter().any(|tool| tool["name"] == spec.name),
            "system tool missing large-surface MCP mapping: {}",
            spec.name
        );
    }
}

#[test]
fn mcp_schema_generation_accepts_an_intentional_projection() {
    let click = crate::primitive_specs
        .iter()
        .find(|spec| spec.name == "click")
        .unwrap();
    let catalog =
        AgentToolCatalog::try_new(vec![crate::AgentToolSpec::browser_primitive(click)]).unwrap();
    let tools = mcp_tools_from_catalog(&catalog);

    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"], "click");
    assert_eq!(tools[0]["description"], click.description);
    assert_eq!(tools[0]["inputSchema"]["required"][0], "target");
    assert!(crate::primitive_specs.len() > tools.len());
}

#[test]
fn projected_catalog_is_also_the_execution_allowlist() {
    let read_page = crate::primitive_specs
        .iter()
        .find(|spec| spec.name == "read-page")
        .unwrap();
    let catalog =
        AgentToolCatalog::try_new(vec![crate::AgentToolSpec::browser_primitive(read_page)])
            .unwrap();

    let error =
        execute_tool_from_catalog(&catalog, "click", &json!({"target":"css:button"})).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unsupported);
    assert!(error.message.contains("unknown agent tool: click"));
}

#[test]
fn browser_schema_builtin_can_be_published_and_executed_without_browser_state() {
    let catalog = AgentToolCatalog::try_new(vec![crate::AgentToolSpec::builtin(
        crate::AgentBuiltin::BrowserSchema,
    )])
    .unwrap();

    let tools = mcp_tools_from_catalog(&catalog);
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"], "browser-schema");
    assert_eq!(
        tools[0]["inputSchema"]["oneOf"].as_array().unwrap().len(),
        3
    );

    let output = execute_tool_from_catalog(
        &catalog,
        "browser-schema",
        &json!({"action":"capabilities"}),
    )
    .unwrap();
    let value: Value = serde_json::from_str(&output).unwrap();
    assert_eq!(value["action"], "capabilities");
    assert_eq!(
        value["operation_count"],
        crate::primitive_specs.len() as u64
    );
}

#[test]
fn browser_call_builtin_is_publishable_and_preflights_before_browser_connection() {
    let catalog = AgentToolCatalog::try_new(vec![crate::AgentToolSpec::builtin(
        crate::AgentBuiltin::browser_call(crate::RawCdpAccess::Disabled),
    )])
    .unwrap();

    let tools = mcp_tools_from_catalog(&catalog);
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"], "browser-call");
    assert_eq!(
        tools[0]["inputSchema"]["properties"]["calls"]["minItems"],
        1
    );

    let error =
        execute_tool_from_catalog(&catalog, "browser-call", &json!({"calls":[]})).unwrap_err();
    assert_eq!(error.kind, ErrorKind::InvalidArguments);
    assert!(error.message.contains("at least one call"));
}

#[test]
fn large_surface_does_not_publish_small_surface_browser_builtins() {
    let tools = mcp_tools_from_catalog(crate::agent_catalog_for_surface(
        crate::McpSurface::LargeSurface,
        crate::RawCdpAccess::Disabled,
    ));
    for name in ["browser-schema", "browser-call", "browser-events"] {
        assert!(
            !tools.iter().any(|tool| tool["name"] == name),
            "large-surface unexpectedly publishes {name}"
        );
    }
}

#[test]
fn small_surface_publishes_facade_and_system_tools_but_not_browser_primitives() {
    let catalog = crate::agent_catalog_for_surface(
        crate::McpSurface::SmallSurface,
        crate::RawCdpAccess::Disabled,
    );
    let tools = mcp_tools_from_catalog(catalog);
    let names = tools
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect::<std::collections::HashSet<_>>();

    for name in ["browser-schema", "browser-call", "browser-events"] {
        assert!(names.contains(name), "small-surface missing {name}");
    }
    for spec in crate::tool_specs {
        assert!(
            names.contains(spec.name),
            "small-surface missing system tool {}",
            spec.name
        );
    }
    for spec in crate::primitive_specs {
        assert!(
            !names.contains(spec.name),
            "small-surface unexpectedly publishes primitive {}",
            spec.name
        );
    }
    assert_eq!(tools.len(), crate::tool_specs.len() + 3);

    let error =
        execute_tool_from_catalog(catalog, "click", &json!({"target":"css:button"})).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unsupported);
    assert!(error.message.contains("unknown agent tool: click"));

    let schema =
        execute_tool_from_catalog(catalog, "browser-schema", &json!({"action":"capabilities"}))
            .unwrap();
    let value: Value = serde_json::from_str(&schema).unwrap();
    assert_eq!(value["action"], "capabilities");
}

#[test]
fn small_surface_raw_cdp_disabled_rejects_method_form_before_browser_acquisition() {
    let catalog = crate::agent_catalog_for_surface(
        crate::McpSurface::SmallSurface,
        crate::RawCdpAccess::Disabled,
    );
    let tool = catalog.get("browser-call").unwrap();
    assert!(
        !tool.input_schema().to_string().contains("\"method\""),
        "raw CDP method form must not be published when disabled"
    );

    let error = execute_tool_from_catalog(
        catalog,
        "browser-call",
        &json!({
            "calls":[{
                "scope":"browser",
                "call":{"method":"Target.getTargets","params":{}}
            }]
        }),
    )
    .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unsupported);
    assert!(!error.retryable);
    assert!(error.message.contains("raw CDP is disabled"));
}

#[test]
fn browser_events_builtin_is_publishable_and_requires_persistent_mcp_state() {
    let catalog = AgentToolCatalog::try_new(vec![crate::AgentToolSpec::builtin(
        crate::AgentBuiltin::BrowserEvents,
    )])
    .unwrap();
    let tools = mcp_tools_from_catalog(&catalog);
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"], "browser-events");
    assert_eq!(
        tools[0]["inputSchema"]["oneOf"].as_array().unwrap().len(),
        3
    );
    assert!(builtin_requires_persistent_mcp_session(
        crate::AgentBuiltin::BrowserEvents
    ));
    assert!(!builtin_requires_persistent_mcp_session(
        crate::AgentBuiltin::BrowserSchema
    ));

    let error = execute_tool_from_catalog(
        &catalog,
        "browser-events",
        &json!({"action":"poll","subscription_id":"","limit":1}),
    )
    .unwrap_err();
    assert_eq!(error.kind, ErrorKind::InvalidArguments);
}

#[test]
fn system_args_are_mapped_to_existing_cli_shape() {
    let open = json!({"url":"https://example.com"});
    assert_eq!(
        system_cli_args("open-browser", open.as_object().unwrap()).unwrap(),
        vec!["https://example.com"]
    );
    let hitl = json!({"message":"approve"});
    assert_eq!(
        system_cli_args("hitl", hitl.as_object().unwrap()).unwrap(),
        vec!["approve"]
    );
}

#[test]
fn tool_results_are_always_top_level_objects() {
    let array = success_envelope("inspect-images", "[1,2,3]");
    assert!(array.is_object());
    assert_eq!(array["ok"], true);
    assert!(array["data"].is_array());

    let text = success_envelope("click", "performed");
    assert!(text.is_object());
    assert_eq!(text["data"], "performed");

    let failure = ToolFailure::new(ErrorKind::TargetNotFound, "missing", true);
    let error = failure_envelope("assert-visible", &failure);
    assert!(error.is_object());
    assert_eq!(error["ok"], false);
    assert_eq!(error["error"]["kind"], "target_not_found");
    assert!(error["error"].get("details").is_none());

    let cdp = ToolFailure::from_error(crate::cdp_error(
        "Runtime.missing",
        -32601,
        "Method not found",
        None,
    ));
    let error = failure_envelope("browser-call", &cdp);
    assert_eq!(error["error"]["kind"], "cdp_failed");
    assert_eq!(error["error"]["retryable"], false);
    assert_eq!(
        error["error"]["details"],
        json!({
            "protocol":"cdp",
            "method":"Runtime.missing",
            "code":-32601,
            "message":"Method not found"
        })
    );
}

#[test]
fn every_tool_declares_object_output_schema() {
    for catalog in [
        crate::agent_catalog_for_surface(
            crate::McpSurface::LargeSurface,
            crate::RawCdpAccess::Disabled,
        ),
        crate::agent_catalog_for_surface(
            crate::McpSurface::SmallSurface,
            crate::RawCdpAccess::Disabled,
        ),
    ] {
        for tool in mcp_tools_from_catalog(catalog) {
            assert_eq!(tool["outputSchema"]["type"], "object");
        }
    }
}
