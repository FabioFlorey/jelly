use super::{
    CdpEvent, CdpEventFilter, CdpEventPoll, CdpEventRing, CdpEventRingStats, CdpEventSubscriptions,
    LogicalTarget, TargetManager, perf,
    transport::{CdpNotification, CdpTransport, browser_unavailable, check_error},
};
use crate::{DOWNLOAD_DIR, ENDPOINT, Error, ErrorKind, jelly_error};
use serde_json::{Value, json};
use std::time::Instant;

pub struct BrowserSession {
    transport: CdpTransport,
    targets: TargetManager,
    events: CdpEventRing,
    subscriptions: CdpEventSubscriptions,
}

impl BrowserSession {
    pub fn connect() -> Result<Self, Error> {
        let started = Instant::now();
        let mut transport = CdpTransport::connect(ENDPOINT)?;
        let mut events = CdpEventRing::default();
        let exchange = transport.request("Target.getTargets", None, json!({}))?;
        retain_initial_notifications(&mut events, exchange.notifications)?;
        check_error(&exchange.response, "Target.getTargets")?;
        let targets_response = exchange.response;
        let target_infos = targets_response["result"]["targetInfos"]
            .as_array()
            .ok_or_else(|| {
                jelly_error(
                    ErrorKind::Internal,
                    "Target.getTargets response is missing targetInfos",
                    false,
                )
            })?;

        let targets = TargetManager::from_initial_targets(target_infos)?;
        let target_id = targets.active_target_id().to_owned();
        let target_info = target_infos
            .iter()
            .find(|target| {
                target["type"] == "page" && target["targetId"].as_str() == Some(target_id.as_str())
            })
            .ok_or_else(|| browser_unavailable("selected page target disappeared"))?;

        let mut session = Self {
            transport,
            targets,
            events,
            subscriptions: CdpEventSubscriptions::default(),
        };

        let mut download_params = json!({
            "behavior":"allowAndName",
            "downloadPath":DOWNLOAD_DIR,
            "eventsEnabled":true
        });
        if let Some(context_id) = target_info["browserContextId"].as_str() {
            download_params["browserContextId"] = json!(context_id);
        }
        session.browser_request("Browser.setDownloadBehavior", download_params)?;
        session.browser_request("Target.setDiscoverTargets", json!({"discover":true}))?;
        session.browser_request(
            "Target.setAutoAttach",
            json!({
                "autoAttach":true,
                "waitForDebuggerOnStart":false,
                "flatten":true,
                "filter":[
                    {"type":"page","exclude":false},
                    {"exclude":true}
                ]
            }),
        )?;
        session.browser_request("Target.getTargets", json!({}))?;
        session.browser_request("Target.activateTarget", json!({"targetId":target_id}))?;

        let session_id = match session.targets.session_id_for_target(&target_id) {
            Some(session_id) => session_id.to_owned(),
            None => session.attach_target(&target_id)?,
        };
        session.targets.activate(&target_id, session_id)?;

        perf::record("browser.connect", started.elapsed().as_millis(), None);
        Ok(session)
    }

    pub fn target_id(&self) -> &str {
        self.targets.active_target_id()
    }

    pub fn current_target_label(&self) -> Option<&str> {
        self.targets.current_label()
    }

    pub fn cdp_events(&self) -> Vec<CdpEvent> {
        self.events.events()
    }

    pub fn cdp_event_stats(&self) -> CdpEventRingStats {
        self.events.stats()
    }

    pub fn reset_cdp_event_stream(&mut self) {
        self.events.reset_stream();
    }

    pub fn subscribe_cdp_events(
        &mut self,
        filter: CdpEventFilter,
    ) -> Result<(String, super::CdpEventCursor), Error> {
        if let Some(target) = filter.target() {
            self.validate_logical_targets(&[target])?;
        }
        self.subscriptions.subscribe(filter, self.events.stats())
    }

    pub fn poll_cdp_events(
        &mut self,
        subscription_id: &str,
        limit: usize,
    ) -> Result<CdpEventPoll, Error> {
        self.subscriptions.filter(subscription_id)?;
        self.pump_cdp_events()?;
        self.subscriptions
            .poll(subscription_id, &self.events, limit)
    }

    pub fn unsubscribe_cdp_events(&mut self, subscription_id: &str) -> Result<(), Error> {
        self.subscriptions.unsubscribe(subscription_id)
    }

    pub fn cdp_event_subscription_filter(
        &self,
        subscription_id: &str,
    ) -> Result<CdpEventFilter, Error> {
        self.subscriptions.filter(subscription_id).cloned()
    }

    pub fn pump_cdp_events(&mut self) -> Result<(), Error> {
        self.browser_request("Target.getTargets", json!({}))?;
        Ok(())
    }

    pub fn logical_targets(&mut self) -> Result<Vec<LogicalTarget>, Error> {
        self.refresh_target_registry()?;
        Ok(self.targets.targets().to_vec())
    }

    pub fn active_target_label(&mut self) -> Result<String, Error> {
        self.refresh_target_registry()?;
        self.current_target_label()
            .map(str::to_owned)
            .ok_or_else(|| {
                jelly_error(
                    ErrorKind::TargetNotFound,
                    format!(
                        "active browser target {} has no logical identity",
                        self.targets.active_target_id()
                    ),
                    true,
                )
            })
    }

    pub fn validate_logical_targets(&mut self, labels: &[&str]) -> Result<(), Error> {
        if labels.is_empty() {
            return Ok(());
        }
        self.refresh_target_registry()?;
        for label in labels {
            if self.targets.target_id_for_label(label).is_none() {
                return Err(jelly_error(
                    ErrorKind::TargetNotFound,
                    format!("logical browser target not found: {label}"),
                    true,
                ));
            }
        }
        Ok(())
    }

    pub fn resolve_target_query(&mut self, query: &str) -> Result<(String, String), Error> {
        self.refresh_target_registry()?;
        let target = self.targets.resolve_compat(query).ok_or_else(|| {
            jelly_error(
                ErrorKind::TargetNotFound,
                format!("browser tab not found: {query}"),
                true,
            )
        })?;
        Ok((target.label().to_owned(), target.target_id().to_owned()))
    }

    pub fn switch_logical_target(&mut self, label: &str) -> Result<(), Error> {
        self.refresh_target_registry()?;
        let target_id = self
            .targets
            .target_id_for_label(label)
            .ok_or_else(|| {
                jelly_error(
                    ErrorKind::TargetNotFound,
                    format!("logical browser target not found: {label}"),
                    true,
                )
            })?
            .to_owned();
        if target_id == self.targets.active_target_id() {
            return Ok(());
        }
        self.switch_target(&target_id)
    }

    pub fn sync_active_target(&mut self) -> Result<(), Error> {
        let Some(active_target) = self.targets.external_active_target() else {
            return Ok(());
        };
        self.switch_target(&active_target)
    }

    pub fn switch_target(&mut self, target_id: &str) -> Result<(), Error> {
        if target_id == self.targets.active_target_id() {
            self.targets.persist_current_target()?;
            return Ok(());
        }

        self.refresh_target_registry()?;
        if !self.targets.contains_target(target_id) {
            return Err(jelly_error(
                ErrorKind::TargetNotFound,
                format!("browser target not found: {target_id}"),
                true,
            ));
        }

        self.browser_request("Target.activateTarget", json!({"targetId":target_id}))?;
        let session_id = match self.targets.session_id_for_target(target_id) {
            Some(session_id) => session_id.to_owned(),
            None => self.attach_target(target_id)?,
        };
        self.targets.activate(target_id, session_id)
    }

    pub fn refresh_target_registry(&mut self) -> Result<(), Error> {
        let response = self.browser_call("Target.getTargets", json!({}))?;
        let target_infos = response["result"]["targetInfos"]
            .as_array()
            .ok_or_else(|| {
                jelly_error(
                    ErrorKind::Internal,
                    "Target.getTargets response is missing targetInfos",
                    false,
                )
            })?;
        self.targets.reconcile(target_infos)
    }

    pub fn call_on_logical_target(
        &mut self,
        label: &str,
        method: &str,
        params: Value,
    ) -> Result<Value, Error> {
        self.refresh_target_registry()?;
        let target_id = self
            .targets
            .target_id_for_label(label)
            .ok_or_else(|| {
                jelly_error(
                    ErrorKind::TargetNotFound,
                    format!("logical browser target not found: {label}"),
                    true,
                )
            })?
            .to_owned();
        let session_id = match self.targets.session_id_for_target(&target_id) {
            Some(session_id) => session_id.to_owned(),
            None => self.attach_target(&target_id)?,
        };
        self.call_with_session(&session_id, method, params)
    }

    pub fn browser_call(&mut self, method: &str, params: Value) -> Result<Value, Error> {
        let started = Instant::now();
        let value = self.browser_request(method, params)?;
        perf::record(
            "cdp.browser_call",
            started.elapsed().as_millis(),
            Some(method),
        );
        Ok(value)
    }

    pub fn call(&mut self, method: &str, params: Value) -> Result<Value, Error> {
        if self.targets.active_session_id().is_empty() {
            return Err(browser_unavailable(
                "cannot issue target-scoped CDP call before target attachment",
            ));
        }
        let session_id = self.targets.active_session_id().to_owned();
        self.call_with_session(&session_id, method, params)
    }

    pub fn eval(&mut self, expr: &str) -> Result<Value, Error> {
        let value = self.call(
            "Runtime.evaluate",
            json!({"expression":expr,"returnByValue":true,"awaitPromise":true}),
        )?;
        if let Some(exception) = value["result"]["exceptionDetails"].as_object() {
            let message = exception
                .get("exception")
                .and_then(|error| error["description"].as_str())
                .or_else(|| exception.get("text").and_then(Value::as_str))
                .unwrap_or("JavaScript evaluation failed");
            return Err(jelly_error(ErrorKind::JavascriptFailed, message, false));
        }
        Ok(value["result"]["result"]
            .get("value")
            .cloned()
            .unwrap_or(Value::Null))
    }

    fn attach_target(&mut self, target_id: &str) -> Result<String, Error> {
        let attached = self.browser_request(
            "Target.attachToTarget",
            json!({"targetId":target_id,"flatten":true}),
        )?;
        let session_id = attached["result"]["sessionId"]
            .as_str()
            .ok_or_else(|| {
                jelly_error(
                    ErrorKind::Internal,
                    "Target.attachToTarget response is missing sessionId",
                    false,
                )
            })?
            .to_owned();
        self.targets.set_session(target_id, session_id.clone())?;
        Ok(session_id)
    }

    fn call_with_session(
        &mut self,
        session_id: &str,
        method: &str,
        params: Value,
    ) -> Result<Value, Error> {
        let started = Instant::now();
        let value = self.transport_request(method, Some(session_id), params)?;
        perf::record("cdp.call", started.elapsed().as_millis(), Some(method));
        Ok(value)
    }

    fn browser_request(&mut self, method: &str, params: Value) -> Result<Value, Error> {
        self.transport_request(method, None, params)
    }

    fn transport_request(
        &mut self,
        method: &str,
        session_id: Option<&str>,
        params: Value,
    ) -> Result<Value, Error> {
        let exchange = self.transport.request(method, session_id, params)?;
        self.retain_notifications(exchange.notifications)?;
        check_error(&exchange.response, method)?;
        Ok(exchange.response)
    }

    fn retain_notifications(&mut self, notifications: Vec<CdpNotification>) -> Result<(), Error> {
        for notification in notifications {
            self.retain_notification(notification.value, notification.wire_bytes)?;
        }
        Ok(())
    }

    fn retain_notification(&mut self, value: Value, wire_bytes: usize) -> Result<(), Error> {
        let method = value
            .get("method")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                jelly_error(
                    ErrorKind::Internal,
                    "CDP notification is missing string method",
                    false,
                )
            })?
            .to_owned();
        let params = value.get("params").cloned().unwrap_or_else(|| json!({}));
        let session_id = notification_session_id(&method, &value, &params).map(str::to_owned);

        let direct_target_id = notification_target_id(&method, &params).map(str::to_owned);
        let session_target_id = session_id
            .as_deref()
            .and_then(|session| self.targets.target_id_for_session(session))
            .map(str::to_owned)
            .or_else(|| {
                session_id
                    .as_deref()
                    .filter(|session| {
                        !self.targets.active_session_id().is_empty()
                            && *session == self.targets.active_session_id()
                    })
                    .map(|_| self.targets.active_target_id().to_owned())
            });
        let target_id = direct_target_id.or(session_target_id);
        let target_before = target_id
            .as_deref()
            .and_then(|target_id| self.targets.label_for_target_id(target_id))
            .map(str::to_owned);

        let target_update = if method.starts_with("Target.") {
            self.targets.apply_notification(&method, &params)
        } else {
            Ok(())
        };

        let target = target_before.or_else(|| {
            target_id
                .as_deref()
                .and_then(|target_id| self.targets.label_for_target_id(target_id))
                .map(str::to_owned)
        });

        self.events
            .push(method, params, session_id, target_id, target, wire_bytes);
        target_update
    }
}

fn retain_initial_notifications(
    events: &mut CdpEventRing,
    notifications: Vec<CdpNotification>,
) -> Result<(), Error> {
    for notification in notifications {
        let value = notification.value;
        let method = value
            .get("method")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                jelly_error(
                    ErrorKind::Internal,
                    "CDP notification is missing string method",
                    false,
                )
            })?
            .to_owned();
        let params = value.get("params").cloned().unwrap_or_else(|| json!({}));
        let session_id = notification_session_id(&method, &value, &params).map(str::to_owned);
        let target_id = notification_target_id(&method, &params).map(str::to_owned);
        events.push(
            method,
            params,
            session_id,
            target_id,
            None,
            notification.wire_bytes,
        );
    }
    Ok(())
}

fn notification_session_id<'a>(
    method: &str,
    value: &'a Value,
    params: &'a Value,
) -> Option<&'a str> {
    value
        .get("sessionId")
        .and_then(Value::as_str)
        .or_else(|| match method {
            "Target.attachedToTarget" | "Target.detachedFromTarget" => {
                params.get("sessionId").and_then(Value::as_str)
            }
            _ => None,
        })
}

fn notification_target_id<'a>(method: &str, params: &'a Value) -> Option<&'a str> {
    match method {
        "Target.targetCreated" | "Target.targetInfoChanged" | "Target.attachedToTarget" => params
            .get("targetInfo")
            .and_then(|target| target.get("targetId"))
            .and_then(Value::as_str),
        "Target.targetDestroyed" | "Target.detachedFromTarget" | "Target.targetCrashed" => {
            params.get("targetId").and_then(Value::as_str)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_notification_id_extraction_covers_lifecycle_shapes() {
        assert_eq!(
            notification_target_id(
                "Target.targetCreated",
                &json!({"targetInfo":{"targetId":"created"}})
            ),
            Some("created")
        );
        assert_eq!(
            notification_target_id(
                "Target.attachedToTarget",
                &json!({"targetInfo":{"targetId":"attached"},"sessionId":"s"})
            ),
            Some("attached")
        );
        assert_eq!(
            notification_target_id("Target.targetDestroyed", &json!({"targetId":"gone"})),
            Some("gone")
        );
        assert_eq!(
            notification_target_id("Page.loadEventFired", &json!({})),
            None
        );
    }
}
