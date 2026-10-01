use super::{AgentBuiltin, AgentToolSpec, RawCdpAccess};
use crate::{primitive_specs, tool_specs};

pub(crate) fn large_surface_specs(raw_cdp: RawCdpAccess) -> Vec<AgentToolSpec> {
    let mut specs = primitive_specs
        .iter()
        .map(AgentToolSpec::browser_primitive)
        .collect::<Vec<_>>();
    if raw_cdp.enabled() {
        specs.push(AgentToolSpec::builtin(AgentBuiltin::CdpCall));
    }
    specs.extend(tool_specs.iter().map(AgentToolSpec::system_tool));
    specs
}

pub(crate) fn small_surface_specs(raw_cdp: RawCdpAccess) -> Vec<AgentToolSpec> {
    let mut specs = vec![
        AgentToolSpec::builtin(AgentBuiltin::BrowserSchema),
        AgentToolSpec::builtin(AgentBuiltin::browser_call(raw_cdp)),
        AgentToolSpec::builtin(AgentBuiltin::BrowserEvents),
    ];
    specs.extend(tool_specs.iter().map(AgentToolSpec::system_tool));
    specs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builders_project_expected_browser_surface_shapes() {
        let large = large_surface_specs(RawCdpAccess::Disabled);
        let small = small_surface_specs(RawCdpAccess::Disabled);
        let large_raw = large_surface_specs(RawCdpAccess::Enabled);
        let small_raw = small_surface_specs(RawCdpAccess::Enabled);

        assert_eq!(large.len(), primitive_specs.len() + tool_specs.len());
        assert_eq!(small.len(), tool_specs.len() + 3);
        assert_eq!(large_raw.len(), large.len() + 1);
        assert_eq!(small_raw.len(), small.len());

        assert!(large.iter().any(|spec| spec.name() == "click"));
        assert!(large.iter().all(|spec| spec.name() != "browser-call"));
        assert!(small.iter().any(|spec| spec.name() == "browser-call"));
        assert!(small.iter().all(|spec| spec.name() != "click"));
        assert!(large_raw.iter().any(|spec| spec.name() == "cdp-call"));
    }
}
