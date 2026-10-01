use jelly::{
    AgentToolBinding, BrowserSession, ErrorKind, McpSurface, RawCdpAccess,
    agent_catalog_for_surface, agent_catalog_from_config, classify_error, execute_browser_call,
    execute_browser_events, execute_browser_schema, execute_named_browser_primitive,
    primitive_specs, tool_specs, validate_browser_call,
};
use serde_json::{Value, json};
use std::{collections::HashSet, env, error::Error, process::Command, time::Instant};

fn main() -> Result<(), Box<dyn Error>> {
    let mode = env::args()
        .nth(1)
        .ok_or("usage: jelly-agent-api-probe <mode>")?;
    let report_mode = matches!(
        mode.as_str(),
        "surface-report" | "latency-report" | "active-tools-list"
    );
    match mode.as_str() {
        "small-surface-catalog" => small_surface_catalog()?,
        "large-surface-catalog" => large_surface_catalog()?,
        "system-parity" => system_parity()?,
        "schema-capabilities" => schema_capabilities()?,
        "schema-search" => schema_search()?,
        "schema-contract" => schema_contract()?,
        "raw-disabled" => raw_disabled()?,
        "large-surface-raw" => large_surface_raw()?,
        "default-small-surface" => default_small_surface()?,
        "explicit-large-surface" => explicit_large_surface()?,
        "semantic-read" => semantic_read()?,
        "semantic-mutate-verify" => semantic_mutate_verify()?,
        "preflight-atomic" => preflight_atomic()?,
        "failure-stop" => failure_stop()?,
        "failure-continue" => failure_continue()?,
        "stale-ref" => stale_ref()?,
        "logical-target" => logical_target()?,
        "raw-target" => raw_target()?,
        "raw-browser" => raw_browser()?,
        "mixed-batch" => mixed_batch()?,
        "events-lifecycle" => events_lifecycle()?,
        "subscription-stale" => subscription_stale()?,
        "surface-report" => surface_report()?,
        "latency-report" => latency_report()?,
        "active-tools-list" => active_tools_list()?,
        other => return Err(format!("unknown probe mode: {other}").into()),
    }
    if !report_mode {
        println!("{}", json!({"ok":true,"mode":mode}));
    }
    Ok(())
}

fn small_surface_catalog() -> Result<(), Box<dyn Error>> {
    let catalog = agent_catalog_for_surface(McpSurface::SmallSurface, RawCdpAccess::Disabled);
    let names = catalog
        .iter()
        .map(|entry| entry.name())
        .collect::<HashSet<_>>();
    for name in ["browser-schema", "browser-call", "browser-events"] {
        ensure(
            names.contains(name),
            format!("small-surface catalog missing {name}"),
        )?;
    }
    for primitive in primitive_specs {
        ensure(
            !names.contains(primitive.name),
            format!("small-surface catalog leaked primitive {}", primitive.name),
        )?;
    }
    ensure(
        catalog.len() == tool_specs.len() + 3,
        "small-surface catalog size mismatch",
    )
}

fn large_surface_catalog() -> Result<(), Box<dyn Error>> {
    let catalog = agent_catalog_for_surface(McpSurface::LargeSurface, RawCdpAccess::Disabled);
    for name in ["browser-schema", "browser-call", "browser-events"] {
        ensure(
            catalog.get(name).is_none(),
            format!("large-surface published {name}"),
        )?;
    }
    for primitive in primitive_specs {
        ensure(
            catalog.get(primitive.name).is_some(),
            format!("large-surface missing primitive {}", primitive.name),
        )?;
    }
    Ok(())
}

fn system_parity() -> Result<(), Box<dyn Error>> {
    let large = agent_catalog_for_surface(McpSurface::LargeSurface, RawCdpAccess::Disabled);
    let small = agent_catalog_for_surface(McpSurface::SmallSurface, RawCdpAccess::Disabled);
    let system_names = |catalog: &jelly::AgentToolCatalog| {
        catalog
            .iter()
            .filter_map(|entry| match entry.binding() {
                AgentToolBinding::SystemTool(spec) => Some(spec.name),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let expected = tool_specs.iter().map(|spec| spec.name).collect::<Vec<_>>();
    ensure(
        system_names(large) == expected,
        "large-surface system projection mismatch",
    )?;
    ensure(
        system_names(small) == expected,
        "small-surface system projection mismatch",
    )
}

fn schema_capabilities() -> Result<(), Box<dyn Error>> {
    let value = execute_browser_schema(&json!({"action":"capabilities"}))?;
    ensure(value["schema_version"] == 1, "schema version mismatch")?;
    ensure(
        value["operation_count"].as_u64() == Some(primitive_specs.len() as u64),
        "capability count mismatch",
    )
}

fn schema_search() -> Result<(), Box<dyn Error>> {
    let first =
        execute_browser_schema(&json!({"action":"search","query":"upload local file","limit":3}))?;
    let second =
        execute_browser_schema(&json!({"action":"search","query":"upload local file","limit":3}))?;
    ensure(first == second, "browser-schema search is nondeterministic")?;
    ensure(
        first["results"][0]["operation"] == "upload",
        "unexpected search winner",
    )
}

fn schema_contract() -> Result<(), Box<dyn Error>> {
    let value =
        execute_browser_schema(&json!({"action":"schema","operation":"snapshot-interactive"}))?;
    let operation = &value["operation"];
    ensure(
        operation["name"] == "snapshot-interactive",
        "wrong operation",
    )?;
    ensure(
        operation["input_schema"]["additionalProperties"] == false,
        "semantic schema must be strict",
    )?;
    ensure(
        operation.get("usage").is_none(),
        "remote schema leaked CLI usage",
    )
}

fn raw_disabled() -> Result<(), Box<dyn Error>> {
    let schema = jelly::browser_call_input_schema(RawCdpAccess::Disabled);
    ensure(
        !schema.to_string().contains("\"method\""),
        "raw method form published while disabled",
    )?;
    let request = json!({
        "calls":[{"scope":"browser","call":{"method":"Target.getTargets","params":{}}}]
    });
    let error = validate_browser_call(&request, RawCdpAccess::Disabled).unwrap_err();
    ensure_kind(error.as_ref(), ErrorKind::Unsupported, false)
}

fn default_small_surface() -> Result<(), Box<dyn Error>> {
    let exe = env::current_exe()?;
    let output = Command::new(exe)
        .arg("active-tools-list")
        .env_remove("JELLY_MCP_SURFACE")
        .env_remove("JELLY_MCP_RAW_CDP")
        .output()?;
    ensure(
        output.status.success(),
        format!(
            "default tools/list subprocess failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ),
    )?;
    let payload: Value = serde_json::from_slice(&output.stdout)?;
    let names = payload["tools"]
        .as_array()
        .ok_or("default tools/list payload missing tools")?
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect::<HashSet<_>>();
    for name in ["browser-schema", "browser-call", "browser-events"] {
        ensure(
            names.contains(name),
            format!("default small-surface missing {name}"),
        )?;
    }
    ensure(
        !names.contains("click"),
        "default small-surface leaked large-surface click primitive",
    )
}

fn explicit_large_surface() -> Result<(), Box<dyn Error>> {
    let exe = env::current_exe()?;
    let output = Command::new(exe)
        .arg("active-tools-list")
        .env("JELLY_MCP_SURFACE", "large-surface")
        .env("JELLY_MCP_RAW_CDP", "1")
        .output()?;
    ensure(
        output.status.success(),
        format!(
            "large-surface tools/list subprocess failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ),
    )?;
    let payload: Value = serde_json::from_slice(&output.stdout)?;
    let names = payload["tools"]
        .as_array()
        .ok_or("large-surface tools/list payload missing tools")?
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect::<HashSet<_>>();
    ensure(names.contains("click"), "large-surface missing click")?;
    ensure(
        names.contains("cdp-call"),
        "large-surface raw mode missing cdp-call",
    )?;
    for name in ["browser-schema", "browser-call", "browser-events"] {
        ensure(
            !names.contains(name),
            format!("large-surface published {name}"),
        )?;
    }
    Ok(())
}

fn large_surface_raw() -> Result<(), Box<dyn Error>> {
    let catalog = agent_catalog_from_config(Some("large-surface"), Some("1"))?;
    ensure(
        catalog.get("click").is_some(),
        "large-surface missing click",
    )?;
    ensure(
        catalog.get("cdp-call").is_some(),
        "large-surface raw mode missing cdp-call",
    )?;
    ensure(
        catalog.get("browser-call").is_none(),
        "large-surface leaked small-surface builtin",
    )?;
    ensure(
        agent_catalog_from_config(Some("large-surface"), Some("invalid-raw-value")).is_err(),
        "large-surface accepted invalid raw policy",
    )?;
    ensure(
        agent_catalog_from_config(Some("small-surface"), Some("invalid-raw-value")).is_err(),
        "small-surface accepted invalid raw policy",
    )
}

fn semantic_read() -> Result<(), Box<dyn Error>> {
    let mut browser = BrowserSession::connect()?;
    let value = execute_browser_call(
        &mut browser,
        &json!({"calls":[{"call":{"jelly":"read-page","params":{}}}]}),
        RawCdpAccess::Disabled,
    )?;
    ensure(
        value["status"] == "completed",
        "semantic read did not complete",
    )?;
    ensure(value["results"][0]["ok"] == true, "semantic read failed")?;
    ensure(
        value["results"][0]["data"]["title"] == "Jelly browser performance fixture",
        "read-page returned wrong title",
    )
}

fn semantic_mutate_verify() -> Result<(), Box<dyn Error>> {
    let mut browser = BrowserSession::connect()?;
    let value = execute_browser_call(
        &mut browser,
        &json!({"calls":[
            {"call":{"jelly":"evaluate-js","params":{"expression":"document.title='Agent API verified'; document.title"}}},
            {"call":{"jelly":"assert-title","params":{"expected":"Agent API verified"}}}
        ]}),
        RawCdpAccess::Disabled,
    )?;
    ensure(
        value["calls_succeeded"] == 2,
        "semantic mutate/verify batch failed",
    )
}

fn preflight_atomic() -> Result<(), Box<dyn Error>> {
    let mut browser = BrowserSession::connect()?;
    let original = title(&mut browser)?;
    let request = json!({"calls":[
        {"call":{"jelly":"evaluate-js","params":{"expression":"document.title='SHOULD-NOT-RUN'"}}},
        {"target":"tab-999","call":{"jelly":"read-page","params":{}}}
    ]});
    let error = execute_browser_call(&mut browser, &request, RawCdpAccess::Disabled).unwrap_err();
    ensure_kind(error.as_ref(), ErrorKind::TargetNotFound, true)?;
    ensure(
        title(&mut browser)? == original,
        "preflight allowed partial side effect",
    )
}

fn failure_stop() -> Result<(), Box<dyn Error>> {
    let mut browser = BrowserSession::connect()?;
    let original = title(&mut browser)?;
    let value = execute_browser_call(
        &mut browser,
        &json!({"on_error":"stop","calls":[
            {"call":{"jelly":"assert-title","params":{"expected":"definitely-wrong"}}},
            {"call":{"jelly":"evaluate-js","params":{"expression":"document.title='STOP-SHOULD-NOT-RUN'"}}}
        ]}),
        RawCdpAccess::Disabled,
    )?;
    ensure(value["status"] == "stopped", "stop policy did not stop")?;
    ensure(
        value["calls_attempted"] == 1,
        "stop policy attempted later call",
    )?;
    ensure(
        title(&mut browser)? == original,
        "stop policy executed later side effect",
    )
}

fn failure_continue() -> Result<(), Box<dyn Error>> {
    let mut browser = BrowserSession::connect()?;
    let value = execute_browser_call(
        &mut browser,
        &json!({"on_error":"continue","calls":[
            {"call":{"jelly":"assert-title","params":{"expected":"definitely-wrong"}}},
            {"call":{"jelly":"evaluate-js","params":{"expression":"document.title='CONTINUE-RAN'"}}}
        ]}),
        RawCdpAccess::Disabled,
    )?;
    ensure(
        value["status"] == "completed_with_errors",
        "continue policy returned wrong status",
    )?;
    ensure(
        value["calls_attempted"] == 2,
        "continue policy skipped later call",
    )?;
    ensure(
        title(&mut browser)? == "CONTINUE-RAN",
        "continue side effect missing",
    )
}

fn stale_ref() -> Result<(), Box<dyn Error>> {
    let mut browser = BrowserSession::connect()?;
    let snapshot = execute_browser_call(
        &mut browser,
        &json!({"calls":[{"call":{"jelly":"snapshot-interactive","params":{"limit":20}}}]}),
        RawCdpAccess::Disabled,
    )?;
    let items = snapshot["results"][0]["data"]
        .as_array()
        .ok_or("snapshot data is not an array")?;
    let target = items
        .iter()
        .find(|item| {
            item["name"]
                .as_str()
                .is_some_and(|name| name.contains("small action 3"))
        })
        .and_then(|item| item["ref"].as_str())
        .ok_or("fixture target ref not found")?
        .to_owned();
    execute_browser_call(
        &mut browser,
        &json!({"calls":[{"call":{"jelly":"evaluate-js","params":{"expression":"document.querySelector('button:not(:disabled)').remove(); true"}}}]}),
        RawCdpAccess::Disabled,
    )?;
    let value = execute_browser_call(
        &mut browser,
        &json!({"calls":[{"call":{"jelly":"click","params":{"target":target}}}]}),
        RawCdpAccess::Disabled,
    )?;
    ensure(
        value["results"][0]["ok"] == false,
        "stale click unexpectedly succeeded",
    )?;
    ensure(
        value["results"][0]["error"]["kind"] == "target_stale",
        "stale click did not return target_stale",
    )
}

fn logical_target() -> Result<(), Box<dyn Error>> {
    let mut browser = BrowserSession::connect()?;
    let active_before = browser.active_target_label()?;
    let created = execute_browser_call(
        &mut browser,
        &json!({"calls":[
            {"scope":"browser","call":{"method":"Target.createTarget","params":{"url":"about:blank"}}}
        ]}),
        RawCdpAccess::Enabled,
    )?;
    let target_id = created["results"][0]["data"]["targetId"]
        .as_str()
        .ok_or("missing target id")?;
    let label = wait_for_new_label(&mut browser, target_id)?;
    let value = execute_browser_call(
        &mut browser,
        &json!({"calls":[
            {"target":label,"scope":"target","call":{"method":"Runtime.evaluate","params":{"expression":"document.title='secondary'; document.title","returnByValue":true}}}
        ]}),
        RawCdpAccess::Enabled,
    )?;
    ensure(
        value["results"][0]["ok"] == true,
        "logical-target raw call failed",
    )?;
    ensure(
        browser.active_target_label()? == active_before,
        "target-scoped raw call changed active tab",
    )?;
    let closed = execute_browser_call(
        &mut browser,
        &json!({"calls":[
            {"scope":"browser","call":{"method":"Target.closeTarget","params":{"targetId":target_id}}}
        ]}),
        RawCdpAccess::Enabled,
    )?;
    ensure(
        closed["results"][0]["ok"] == true,
        "raw target close failed",
    )?;

    for _ in 0..20 {
        browser.pump_cdp_events()?;
        if !browser
            .logical_targets()?
            .iter()
            .any(|target| target.label() == label)
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    ensure(
        !browser
            .logical_targets()?
            .iter()
            .any(|target| target.label() == label),
        "closed logical target remained registered",
    )?;

    let stale = execute_browser_call(
        &mut browser,
        &json!({"calls":[{"target":label,"call":{"jelly":"read-page","params":{}}}]}),
        RawCdpAccess::Disabled,
    )
    .unwrap_err();
    ensure_kind(stale.as_ref(), ErrorKind::TargetNotFound, true)?;

    let main = execute_browser_call(
        &mut browser,
        &json!({"calls":[{"target":"main","call":{"jelly":"read-page","params":{}}}]}),
        RawCdpAccess::Disabled,
    )?;
    ensure(
        main["results"][0]["ok"] == true,
        "main did not recover after raw target close",
    )
}

fn raw_target() -> Result<(), Box<dyn Error>> {
    let mut browser = BrowserSession::connect()?;
    let value = execute_browser_call(
        &mut browser,
        &json!({"calls":[
            {"scope":"target","call":{"method":"Runtime.evaluate","params":{"expression":"21*2","returnByValue":true}}}
        ]}),
        RawCdpAccess::Enabled,
    )?;
    ensure(
        value["results"][0]["data"]["result"]["value"] == 42,
        "raw target result mismatch",
    )
}

fn raw_browser() -> Result<(), Box<dyn Error>> {
    let mut browser = BrowserSession::connect()?;
    let value = execute_browser_call(
        &mut browser,
        &json!({"calls":[
            {"scope":"browser","call":{"method":"Target.getTargets","params":{}}}
        ]}),
        RawCdpAccess::Enabled,
    )?;
    ensure(
        value["results"][0]["data"]["targetInfos"].is_array(),
        "raw browser result missing targetInfos",
    )
}

fn mixed_batch() -> Result<(), Box<dyn Error>> {
    let mut browser = BrowserSession::connect()?;
    let value = execute_browser_call(
        &mut browser,
        &json!({"calls":[
            {"call":{"jelly":"evaluate-js","params":{"expression":"window.__apiMixed=10"}}},
            {"scope":"target","call":{"method":"Runtime.evaluate","params":{"expression":"window.__apiMixed+=5","returnByValue":true}}},
            {"call":{"jelly":"evaluate-js","params":{"expression":"window.__apiMixed"}}}
        ]}),
        RawCdpAccess::Enabled,
    )?;
    ensure(value["kind"] == "mixed", "mixed batch kind mismatch")?;
    ensure(value["calls_succeeded"] == 3, "mixed batch failed")?;
    ensure(
        value["results"][2]["data"] == 15,
        "mixed batch ordering/result mismatch",
    )
}

fn events_lifecycle() -> Result<(), Box<dyn Error>> {
    let mut browser = BrowserSession::connect()?;
    let enabled = execute_browser_call(
        &mut browser,
        &json!({"calls":[
            {"scope":"target","call":{"method":"Runtime.enable","params":{}}}
        ]}),
        RawCdpAccess::Enabled,
    )?;
    ensure(
        enabled["results"][0]["ok"] == true,
        "Runtime.enable through browser-call failed",
    )?;
    let subscribed = execute_browser_events(
        &mut browser,
        &json!({"action":"subscribe","target":"main","methods":["Runtime.consoleAPICalled"]}),
    )?;
    let id = subscribed["subscription_id"]
        .as_str()
        .ok_or("missing subscription id")?;
    execute_browser_call(
        &mut browser,
        &json!({"calls":[
            {"call":{"jelly":"evaluate-js","params":{"expression":"console.log('agent-api-event'); true"}}}
        ]}),
        RawCdpAccess::Disabled,
    )?;
    let polled = execute_browser_events(
        &mut browser,
        &json!({"action":"poll","subscription_id":id,"limit":20}),
    )?;
    let events = polled["events"].as_array().ok_or("events not array")?;
    ensure(
        events.iter().any(|event| {
            event["target"] == "main"
                && event["method"] == "Runtime.consoleAPICalled"
                && event["params"]["args"][0]["value"] == "agent-api-event"
        }),
        "expected routed console event not found",
    )?;
    let unsubscribed = execute_browser_events(
        &mut browser,
        &json!({"action":"unsubscribe","subscription_id":id}),
    )?;
    ensure(unsubscribed["unsubscribed"] == true, "unsubscribe failed")
}

fn subscription_stale() -> Result<(), Box<dyn Error>> {
    let mut browser = BrowserSession::connect()?;
    let subscribed = execute_browser_events(&mut browser, &json!({"action":"subscribe"}))?;
    let id = subscribed["subscription_id"]
        .as_str()
        .ok_or("missing subscription id")?
        .to_owned();
    execute_browser_events(
        &mut browser,
        &json!({"action":"unsubscribe","subscription_id":id}),
    )?;
    let error = execute_browser_events(
        &mut browser,
        &json!({"action":"poll","subscription_id":id,"limit":1}),
    )
    .unwrap_err();
    ensure_kind(error.as_ref(), ErrorKind::SubscriptionNotFound, false)
}

fn surface_report() -> Result<(), Box<dyn Error>> {
    let variants = [
        (
            "large_surface",
            McpSurface::LargeSurface,
            RawCdpAccess::Disabled,
        ),
        (
            "small_surface_raw_off",
            McpSurface::SmallSurface,
            RawCdpAccess::Disabled,
        ),
        (
            "small_surface_raw_on",
            McpSurface::SmallSurface,
            RawCdpAccess::Enabled,
        ),
    ];

    let mut report = serde_json::Map::new();
    for (name, surface, raw) in variants {
        let catalog = agent_catalog_for_surface(surface, raw);
        let tools_list = subprocess_tools_list(surface, raw)?;
        let tools = tools_list["tools"]
            .as_array()
            .ok_or("tools/list payload missing tools")?;
        let input_schema_bytes = tools
            .iter()
            .map(|tool| serde_json::to_vec(&tool["inputSchema"]).unwrap().len())
            .sum::<usize>();
        let output_schema_bytes = tools
            .iter()
            .map(|tool| serde_json::to_vec(&tool["outputSchema"]).unwrap().len())
            .sum::<usize>();
        let description_bytes = tools
            .iter()
            .map(|tool| tool["description"].as_str().unwrap_or("").len())
            .sum::<usize>();
        let browser_primitive_entries = catalog
            .iter()
            .filter(|entry| matches!(entry.binding(), AgentToolBinding::BrowserPrimitive(_)))
            .count();
        let builtin_entries = catalog
            .iter()
            .filter(|entry| matches!(entry.binding(), AgentToolBinding::Builtin(_)))
            .count();
        let system_entries = catalog
            .iter()
            .filter(|entry| matches!(entry.binding(), AgentToolBinding::SystemTool(_)))
            .count();
        let browser_entries = browser_primitive_entries + builtin_entries;
        let tools_list_bytes = serde_json::to_vec(&tools_list)?.len();

        report.insert(
            name.to_owned(),
            json!({
                "tool_count":tools.len(),
                "browser_entry_count":browser_entries,
                "system_tool_count":system_entries,
                "binding_distribution":{
                    "browser_primitive":browser_primitive_entries,
                    "builtin_facade":builtin_entries,
                    "system_tool":system_entries
                },
                "tools_list_bytes":tools_list_bytes,
                "aggregate_input_schema_bytes":input_schema_bytes,
                "aggregate_output_schema_bytes":output_schema_bytes,
                "description_bytes":description_bytes,
                "approx_context_tokens_4b":tools_list_bytes.div_ceil(4)
            }),
        );
    }

    let large = report["large_surface"]["tools_list_bytes"]
        .as_u64()
        .unwrap();
    let small = report["small_surface_raw_off"]["tools_list_bytes"]
        .as_u64()
        .unwrap();
    let small_raw = report["small_surface_raw_on"]["tools_list_bytes"]
        .as_u64()
        .unwrap();
    report.insert(
        "comparison".into(),
        json!({
            "small_vs_large_tools_list_ratio": small as f64 / large as f64,
            "small_vs_large_bytes_saved": large - small,
            "small_raw_on_vs_large_ratio": small_raw as f64 / large as f64
        }),
    );

    println!("{}", Value::Object(report));
    Ok(())
}

fn latency_report() -> Result<(), Box<dyn Error>> {
    const WARMUP: usize = 5;
    const SAMPLES: usize = 40;

    let mut browser = BrowserSession::connect()?;

    for _ in 0..WARMUP {
        execute_named_browser_primitive(&mut browser, "read-page", &json!({}))?;
        execute_browser_call(
            &mut browser,
            &json!({"calls":[{"call":{"jelly":"read-page","params":{}}}]}),
            RawCdpAccess::Disabled,
        )?;
        execute_browser_call(
            &mut browser,
            &json!({"calls":[
                {"scope":"target","call":{"method":"Runtime.evaluate","params":{"expression":"1+1","returnByValue":true}}}
            ]}),
            RawCdpAccess::Enabled,
        )?;
    }

    let (direct_read, small_surface_read) = measure_read_pair(&mut browser, SAMPLES)?;
    let raw_target = measure(SAMPLES, || {
        execute_browser_call(
            &mut browser,
            &json!({"calls":[
                {"scope":"target","call":{"method":"Runtime.evaluate","params":{"expression":"1+1","returnByValue":true}}}
            ]}),
            RawCdpAccess::Enabled,
        )
        .map(|_| ())
    })?;
    let raw_browser = measure(SAMPLES, || {
        execute_browser_call(
            &mut browser,
            &json!({"calls":[
                {"scope":"browser","call":{"method":"Target.getTargets","params":{}}}
            ]}),
            RawCdpAccess::Enabled,
        )
        .map(|_| ())
    })?;

    let (direct_workflow, small_surface_workflow) = measure_workflow_pair(&mut browser, SAMPLES)?;

    println!(
        "{}",
        json!({
            "environment":{
                "warmup_iterations":WARMUP,
                "samples":SAMPLES,
                "session":"single warm persistent BrowserSession",
                "fixture":"tests/fixtures/browser-perf.html",
                "paired_comparisons":"large-surface direct and small-surface samples alternate order each iteration"
            },
            "latency_us":{
                "large_surface_direct_read_page":direct_read,
                "small_surface_semantic_read_page":small_surface_read,
                "small_surface_raw_target":raw_target,
                "small_surface_raw_browser":raw_browser,
                "large_surface_direct_three_step_workflow":direct_workflow,
                "small_surface_batched_three_step_workflow":small_surface_workflow
            },
            "agent_round_trips":{
                "large_surface_three_step_workflow":3,
                "small_surface_batched_three_step_workflow":1
            }
        })
    );
    Ok(())
}

fn active_tools_list() -> Result<(), Box<dyn Error>> {
    println!("{}", json!({"tools":jelly::mcp::mcp_tools()?}));
    Ok(())
}

fn subprocess_tools_list(surface: McpSurface, raw: RawCdpAccess) -> Result<Value, Box<dyn Error>> {
    let exe = env::current_exe()?;
    let mut command = Command::new(exe);
    command.arg("active-tools-list");
    command.env("JELLY_MCP_SURFACE", surface.as_str());
    if surface == McpSurface::SmallSurface {
        command.env("JELLY_MCP_RAW_CDP", if raw.enabled() { "1" } else { "0" });
    } else {
        command.env_remove("JELLY_MCP_RAW_CDP");
    }
    let output = command.output()?;
    ensure(
        output.status.success(),
        format!(
            "tools/list subprocess failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ),
    )?;
    Ok(serde_json::from_slice(&output.stdout)?)
}

fn measure_read_pair(
    browser: &mut BrowserSession,
    samples: usize,
) -> Result<(Value, Value), Box<dyn Error>> {
    let mut direct = Vec::with_capacity(samples);
    let mut small_surface = Vec::with_capacity(samples);
    for index in 0..samples {
        if index % 2 == 0 {
            direct.push(time_once(|| {
                execute_named_browser_primitive(browser, "read-page", &json!({})).map(|_| ())
            })?);
            small_surface.push(time_once(|| {
                execute_browser_call(
                    browser,
                    &json!({"calls":[{"call":{"jelly":"read-page","params":{}}}]}),
                    RawCdpAccess::Disabled,
                )
                .map(|_| ())
            })?);
        } else {
            small_surface.push(time_once(|| {
                execute_browser_call(
                    browser,
                    &json!({"calls":[{"call":{"jelly":"read-page","params":{}}}]}),
                    RawCdpAccess::Disabled,
                )
                .map(|_| ())
            })?);
            direct.push(time_once(|| {
                execute_named_browser_primitive(browser, "read-page", &json!({})).map(|_| ())
            })?);
        }
    }
    Ok((summarize_samples(direct), summarize_samples(small_surface)))
}

fn measure_workflow_pair(
    browser: &mut BrowserSession,
    samples: usize,
) -> Result<(Value, Value), Box<dyn Error>> {
    let mut direct = Vec::with_capacity(samples);
    let mut small_surface = Vec::with_capacity(samples);
    for index in 0..samples {
        if index % 2 == 0 {
            direct.push(time_once(|| direct_workflow_once(browser))?);
            small_surface.push(time_once(|| small_surface_workflow_once(browser))?);
        } else {
            small_surface.push(time_once(|| small_surface_workflow_once(browser))?);
            direct.push(time_once(|| direct_workflow_once(browser))?);
        }
    }
    Ok((summarize_samples(direct), summarize_samples(small_surface)))
}

fn direct_workflow_once(browser: &mut BrowserSession) -> Result<(), Box<dyn Error>> {
    execute_named_browser_primitive(
        browser,
        "evaluate-js",
        &json!({"expression":"document.title='Bench workflow'"}),
    )?;
    execute_named_browser_primitive(
        browser,
        "assert-title",
        &json!({"expected":"Bench workflow"}),
    )?;
    execute_named_browser_primitive(browser, "read-page", &json!({}))?;
    Ok(())
}

fn small_surface_workflow_once(browser: &mut BrowserSession) -> Result<(), Box<dyn Error>> {
    execute_browser_call(
        browser,
        &json!({"calls":[
            {"call":{"jelly":"evaluate-js","params":{"expression":"document.title='Bench workflow'"}}},
            {"call":{"jelly":"assert-title","params":{"expected":"Bench workflow"}}},
            {"call":{"jelly":"read-page","params":{}}}
        ]}),
        RawCdpAccess::Disabled,
    )
    .map(|_| ())
}

fn time_once<F>(mut f: F) -> Result<u64, Box<dyn Error>>
where
    F: FnMut() -> Result<(), Box<dyn Error>>,
{
    let started = Instant::now();
    f()?;
    Ok(started.elapsed().as_micros() as u64)
}

fn summarize_samples(mut values: Vec<u64>) -> Value {
    values.sort_unstable();
    let p50 = percentile(&values, 50);
    let p95 = percentile(&values, 95);
    let min = values[0];
    let max = *values.last().unwrap();
    let mean = values.iter().sum::<u64>() as f64 / values.len() as f64;
    json!({"min":min,"p50":p50,"p95":p95,"max":max,"mean":mean})
}

fn measure<F>(samples: usize, mut f: F) -> Result<Value, Box<dyn Error>>
where
    F: FnMut() -> Result<(), Box<dyn Error>>,
{
    let mut values = Vec::with_capacity(samples);
    for _ in 0..samples {
        let started = Instant::now();
        f()?;
        values.push(started.elapsed().as_micros() as u64);
    }
    Ok(summarize_samples(values))
}

fn percentile(values: &[u64], percentile: usize) -> u64 {
    let index = ((values.len() - 1) * percentile).div_ceil(100);
    values[index.min(values.len() - 1)]
}

fn title(browser: &mut BrowserSession) -> Result<String, Box<dyn Error>> {
    let response = execute_browser_call(
        browser,
        &json!({"calls":[
            {"call":{"jelly":"evaluate-js","params":{"expression":"document.title"}}}
        ]}),
        RawCdpAccess::Disabled,
    )?;
    Ok(response["results"][0]["data"]
        .as_str()
        .ok_or("title semantic result missing")?
        .to_owned())
}

fn wait_for_new_label(
    browser: &mut BrowserSession,
    target_id: &str,
) -> Result<String, Box<dyn Error>> {
    for _ in 0..20 {
        browser.pump_cdp_events()?;
        let targets = browser.logical_targets()?;
        if let Some(target) = targets
            .iter()
            .find(|target| target.target_id() == target_id)
            && target.session_id().is_some()
        {
            return Ok(target.label().to_owned());
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    Err(format!("auto-attached logical target not found for {target_id}").into())
}

fn ensure_kind(
    error: &(dyn std::error::Error + 'static),
    kind: ErrorKind,
    retryable: bool,
) -> Result<(), Box<dyn Error>> {
    let actual = classify_error(error);
    ensure(
        actual == (kind, retryable),
        format!("error mismatch: expected ({kind:?}, {retryable}), got {actual:?}: {error}"),
    )
}

fn ensure(condition: bool, message: impl Into<String>) -> Result<(), Box<dyn Error>> {
    if condition {
        Ok(())
    } else {
        Err(message.into().into())
    }
}
