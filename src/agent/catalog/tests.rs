use super::*;
use crate::agent::catalog_cache::{large_surface_agent_catalog, small_surface_agent_catalog};
use crate::{McpSurface, agent_catalog_from_config};

fn click_spec() -> &'static PrimitiveSpec {
    primitive_specs
        .iter()
        .find(|spec| spec.name == "click")
        .unwrap()
}

#[test]
fn large_surface_catalog_preserves_registry_order_and_metadata() {
    let catalog = AgentToolCatalog::large_surface().unwrap();
    assert_eq!(catalog.len(), primitive_specs.len() + tool_specs.len());

    let mut expected = primitive_specs
        .iter()
        .map(|spec| (spec.name, spec.description))
        .chain(tool_specs.iter().map(|spec| (spec.name, spec.description)));

    for tool in catalog.iter() {
        let (name, description) = expected.next().unwrap();
        assert_eq!(tool.name(), name);
        assert_eq!(tool.description(), description);
    }
    assert!(expected.next().is_none());
}

#[test]
fn catalog_can_be_an_intentional_projection_with_agent_facing_metadata() {
    let click = click_spec();
    let catalog = AgentToolCatalog::try_new(vec![AgentToolSpec::projected(
        "browser-action",
        "Agent-facing projection of one semantic browser capability.",
        AgentToolBinding::BrowserPrimitive(click),
    )])
    .unwrap();

    assert_eq!(catalog.len(), 1);
    assert!(primitive_specs.len() > catalog.len());
    let tool = catalog.get("browser-action").unwrap();
    assert_eq!(tool.name(), "browser-action");
    assert_eq!(tool.binding().source_name(), "click");
    assert_eq!(tool.input_schema()["required"][0], "target");
}

#[test]
fn builtin_agent_tools_are_first_class_catalog_entries() {
    let catalog = AgentToolCatalog::try_new(vec![
        AgentToolSpec::builtin(AgentBuiltin::BrowserSchema),
        AgentToolSpec::builtin(AgentBuiltin::browser_call(crate::RawCdpAccess::Disabled)),
        AgentToolSpec::builtin(AgentBuiltin::BrowserEvents),
    ])
    .unwrap();

    let schema = catalog.get("browser-schema").unwrap();
    assert_eq!(schema.binding().source_name(), "browser-schema");
    assert_eq!(schema.input_schema()["oneOf"].as_array().unwrap().len(), 3);
    assert_eq!(schema.output_schema()["type"], "object");

    let call = catalog.get("browser-call").unwrap();
    assert_eq!(call.binding().source_name(), "browser-call");
    assert_eq!(call.input_schema()["properties"]["calls"]["minItems"], 1);
    assert!(
        call.input_schema()["properties"]["calls"]["items"]
            .get("oneOf")
            .is_none()
    );
    assert_eq!(call.output_schema()["type"], "object");

    let events = catalog.get("browser-events").unwrap();
    assert_eq!(events.binding().source_name(), "browser-events");
    assert_eq!(events.input_schema()["oneOf"].as_array().unwrap().len(), 3);
    assert_eq!(events.output_schema()["type"], "object");

    let raw_catalog = AgentToolCatalog::try_new(vec![AgentToolSpec::builtin(
        AgentBuiltin::browser_call(crate::RawCdpAccess::Enabled),
    )])
    .unwrap();
    let raw_call = raw_catalog.get("browser-call").unwrap();
    assert_eq!(
        raw_call.input_schema()["properties"]["calls"]["items"]["oneOf"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn duplicate_public_names_are_rejected() {
    let click = click_spec();
    let error = AgentToolCatalog::try_new(vec![
        AgentToolSpec::browser_primitive(click),
        AgentToolSpec::browser_primitive(click),
    ])
    .unwrap_err();
    assert_eq!(error, AgentCatalogError::DuplicateName { name: "click" });
}

#[test]
fn empty_catalog_is_rejected() {
    assert_eq!(
        AgentToolCatalog::try_new(Vec::new()).unwrap_err(),
        AgentCatalogError::EmptyCatalog
    );
}

#[test]
fn invalid_named_primitive_contract_is_rejected_before_publication() {
    fn no_op(
        _browser: &mut crate::BrowserSession,
        _args: &[String],
    ) -> Result<String, crate::Error> {
        Ok(String::new())
    }

    static BAD_ARGS: &[crate::ArgSpec] = &[
        crate::ArgSpec::opt("first", crate::ArgKind::String),
        crate::ArgSpec::req("second", crate::ArgKind::String),
    ];
    static BAD: PrimitiveSpec = PrimitiveSpec {
        name: "bad-named-shape",
        description: "Test-only invalid positional grammar.",
        usage: "bad-named-shape [first] <second>",
        category: "test",
        args: BAD_ARGS,
        max_args: Some(2),
        handler: no_op,
    };

    let error =
        AgentToolCatalog::try_new(vec![AgentToolSpec::browser_primitive(&BAD)]).unwrap_err();
    match error {
        AgentCatalogError::InvalidBrowserPrimitiveContract { name, reason } => {
            assert_eq!(name, "bad-named-shape");
            assert!(reason.contains("required argument second after an optional"));
        }
        other => panic!("unexpected error: {other}"),
    }
}

#[test]
fn missing_system_schema_is_rejected_instead_of_silently_dropping_the_tool() {
    static UNKNOWN: ToolSpec = ToolSpec {
        name: "unknown-agent-system-tool",
        description: "Test-only system tool with no agent schema mapping.",
        usage: "unknown-agent-system-tool",
        category: "test",
    };
    let error = AgentToolCatalog::try_new(vec![AgentToolSpec::system_tool(&UNKNOWN)]).unwrap_err();
    assert_eq!(
        error,
        AgentCatalogError::MissingInputSchema {
            name: "unknown-agent-system-tool"
        }
    );
}

#[test]
fn name_lookup_uses_the_validated_catalog_index() {
    let catalog =
        AgentToolCatalog::try_new(vec![AgentToolSpec::browser_primitive(click_spec())]).unwrap();
    assert_eq!(
        catalog.get("click").unwrap().binding().source_name(),
        "click"
    );
    assert!(catalog.get("fill").is_none());
}

#[test]
fn mcp_surface_parser_accepts_only_canonical_names() {
    assert_eq!(McpSurface::parse(None).unwrap(), McpSurface::SmallSurface);
    assert_eq!(McpSurface::SmallSurface.as_str(), "small-surface");
    assert_eq!(McpSurface::LargeSurface.as_str(), "large-surface");
    assert!(McpSurface::parse(Some("")).is_err());
    assert_eq!(
        McpSurface::parse(Some("  LARGE-SURFACE ")).unwrap(),
        McpSurface::LargeSurface
    );
    assert_eq!(
        McpSurface::parse(Some(" Small-Surface ")).unwrap(),
        McpSurface::SmallSurface
    );
    for unsupported in ["legacy", "compact", "both"] {
        let error = McpSurface::parse(Some(unsupported)).unwrap_err();
        assert!(error.contains("must be large-surface or small-surface"));
    }
}

#[test]
fn rollout_config_applies_raw_cdp_policy_to_both_surfaces() {
    let large_disabled = agent_catalog_from_config(Some("large-surface"), None).unwrap();
    assert!(large_disabled.get("cdp-call").is_none());

    let large_enabled = agent_catalog_from_config(Some("large-surface"), Some("true")).unwrap();
    assert!(large_enabled.get("click").is_some());
    assert!(large_enabled.get("cdp-call").is_some());
    assert!(large_enabled.get("browser-call").is_none());

    let small_disabled = agent_catalog_from_config(Some("small-surface"), None).unwrap();
    assert!(
        small_disabled.get("browser-call").unwrap().input_schema()["properties"]["calls"]["items"]
            .get("oneOf")
            .is_none()
    );

    let small_enabled = agent_catalog_from_config(Some("small-surface"), Some("true")).unwrap();
    assert_eq!(
        small_enabled.get("browser-call").unwrap().input_schema()["properties"]["calls"]
            ["items"]["oneOf"]
            .as_array()
            .unwrap()
            .len(),
        3
    );

    assert!(agent_catalog_from_config(Some("large-surface"), Some("invalid")).is_err());
    assert!(agent_catalog_from_config(Some("small-surface"), Some("invalid")).is_err());
    assert!(agent_catalog_from_config(Some("both"), None).is_err());
}

#[test]
fn small_surface_catalog_replaces_browser_primitives_but_preserves_system_tools() {
    let catalog = AgentToolCatalog::small_surface(crate::RawCdpAccess::Disabled).unwrap();
    assert_eq!(catalog.len(), tool_specs.len() + 3);

    for name in ["browser-schema", "browser-call", "browser-events"] {
        assert!(
            catalog.get(name).is_some(),
            "small-surface catalog missing {name}"
        );
    }
    for spec in tool_specs {
        assert!(
            catalog.get(spec.name).is_some(),
            "small-surface catalog missing system tool {}",
            spec.name
        );
    }
    for spec in primitive_specs {
        assert!(
            catalog.get(spec.name).is_none(),
            "small-surface catalog unexpectedly publishes primitive {}",
            spec.name
        );
    }
}

#[test]
fn system_tool_surface_is_identical_between_large_and_small_surfaces() {
    let large = large_surface_agent_catalog();
    let small = small_surface_agent_catalog(crate::RawCdpAccess::Disabled);

    let large_system = large
        .iter()
        .filter_map(|entry| match entry.binding() {
            AgentToolBinding::SystemTool(spec) => Some((entry.name(), spec.name)),
            _ => None,
        })
        .collect::<Vec<_>>();
    let small_system = small
        .iter()
        .filter_map(|entry| match entry.binding() {
            AgentToolBinding::SystemTool(spec) => Some((entry.name(), spec.name)),
            _ => None,
        })
        .collect::<Vec<_>>();
    let registry = tool_specs
        .iter()
        .map(|spec| (spec.name, spec.name))
        .collect::<Vec<_>>();

    assert_eq!(large_system, registry);
    assert_eq!(small_system, registry);
}

#[test]
fn hitl_and_profile_import_remain_explicit_system_tools_during_browser_rollout() {
    for catalog in [
        large_surface_agent_catalog(),
        small_surface_agent_catalog(crate::RawCdpAccess::Disabled),
    ] {
        let hitl = catalog.get("hitl").expect("hitl must remain published");
        assert!(
            matches!(hitl.binding(), AgentToolBinding::SystemTool(spec) if spec.name == "hitl")
        );

        let profile = catalog
            .get("profile-import")
            .expect("profile-import must remain published during step 14");
        assert!(
            matches!(profile.binding(), AgentToolBinding::SystemTool(spec) if spec.name == "profile-import")
        );
    }
}

#[test]
fn small_surface_raw_cdp_policy_is_reflected_in_browser_call_schema() {
    let disabled = small_surface_agent_catalog(crate::RawCdpAccess::Disabled);
    assert!(
        disabled.get("browser-call").unwrap().input_schema()["properties"]["calls"]["items"]
            .get("oneOf")
            .is_none()
    );

    let enabled = small_surface_agent_catalog(crate::RawCdpAccess::Enabled);
    assert_eq!(
        enabled.get("browser-call").unwrap().input_schema()["properties"]["calls"]["items"]
            ["oneOf"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}
