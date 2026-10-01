use super::*;

#[test]
fn initialize_includes_operating_instructions() {
    let response = initialize(&json!({"protocolVersion": DEFAULT_PROTOCOL_VERSION}));
    let instructions = response["instructions"].as_str().unwrap();
    assert!(instructions.contains("Inspect before mutation"));
    assert!(instructions.contains("Reject nonessential cookies by default"));
    assert!(instructions.contains("active ChatGPT conversation"));
    assert!(instructions.contains("through Telegram"));
    assert!(instructions.contains("exhaust legitimate automatable paths"));
    assert!(instructions.contains(".agent/tools/index.md"));
    assert!(instructions.contains("docs/DISCOVERY.md"));
    assert!(instructions.contains("agent-discover schema <tool>"));
    assert!(instructions.contains("Detect the active surface from `tools/list`"));
    assert!(instructions.contains("Prefer a **semantic Jelly operation**"));
    assert!(instructions.contains("use **browser-schema** to discover it"));
    assert!(instructions.contains("Execute semantic operations through **browser-call**"));
    assert!(instructions.contains("Use **browser-events** only when"));
    assert!(instructions.contains("Use **raw CDP** only when it is published"));
    assert!(instructions.contains("Do not supply Chromium `targetId` or `sessionId` values"));
    assert!(instructions.contains("Use `call-routine`"));
    assert!(instructions.contains("published system artifact tools"));
    assert!(instructions.contains("inputSchema"));
    assert!(instructions.contains("agent-run <tool> [args...]"));
    assert!(instructions.contains("Do not automatically retry side-effecting operations"));
    assert!(!instructions.contains("Use direct primitives for short, local interactions"));
    assert!(!instructions.contains("Use snapshot-interactive before clicking"));
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
