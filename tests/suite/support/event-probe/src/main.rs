use jelly::{BrowserSession, RawCdpAccess, execute_browser_call, execute_browser_events};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    env,
    error::Error,
    thread,
    time::{Duration, Instant},
};

fn main() -> Result<(), Box<dyn Error>> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.first().map(String::as_str) == Some("multitab") {
        if args.len() != 1 {
            return Err("usage: jelly-event-probe multitab".into());
        }
        return multitab_probe();
    }
    idle_probe(&args)
}

fn idle_probe(args: &[String]) -> Result<(), Box<dyn Error>> {
    if args.len() != 2 {
        return Err("usage: jelly-event-probe <count> <delay_ms>".into());
    }
    let count = args[0].parse::<usize>()?;
    let delay_ms = args[1].parse::<u64>()?;
    if count == 0 || delay_ms < 25 {
        return Err(
            "usage: jelly-event-probe <count> <delay_ms>; count > 0, delay_ms >= 25".into(),
        );
    }

    let mut browser = BrowserSession::connect()?;
    browser.call("Runtime.enable", json!({}))?;
    browser.reset_cdp_event_stream();

    let subscribed = execute_browser_events(
        &mut browser,
        &json!({"action":"subscribe","target":"main","methods":["Runtime.consoleAPICalled"]}),
    )?;
    let subscription_id = subscribed["subscription_id"]
        .as_str()
        .ok_or("missing subscription_id")?
        .to_owned();

    let prefix = "jelly-idle-event-";
    let expression = format!(
        "setTimeout(()=>{{for(let i=0;i<{count};i++)console.log('{prefix}'+i)}},{delay_ms});'scheduled'"
    );
    browser.call(
        "Runtime.evaluate",
        json!({"expression":expression,"returnByValue":true}),
    )?;

    let before_idle = browser.cdp_event_stats();
    thread::sleep(Duration::from_millis(delay_ms + 350));
    let after_idle_before_poll = browser.cdp_event_stats();

    let started = Instant::now();
    let mut events = Vec::<Value>::new();
    let mut dropped = 0_u64;
    let mut resets = 0_u64;
    let mut cursor_lost = false;
    let mut polls = 0_u64;

    loop {
        polls += 1;
        let polled = execute_browser_events(
            &mut browser,
            &json!({"action":"poll","subscription_id":subscription_id,"limit":500}),
        )?;
        dropped += polled["loss"]["dropped"].as_u64().unwrap_or(0);
        resets += polled["loss"]["stream_resets"].as_u64().unwrap_or(0);
        cursor_lost |= polled["loss"]["cursor_lost"].as_bool().unwrap_or(false);
        if let Some(batch) = polled["events"].as_array() {
            events.extend(batch.iter().cloned());
        }
        if polled["has_more"] != true {
            break;
        }
        if polls > 16 {
            return Err("pagination did not converge".into());
        }
    }

    let elapsed_ms = started.elapsed().as_millis();
    let final_stats = browser.cdp_event_stats();
    let mut indexes = BTreeSet::new();
    for event in &events {
        let value = event["params"]["args"][0]["value"]
            .as_str()
            .ok_or("console event missing first string argument")?;
        let suffix = value
            .strip_prefix(prefix)
            .ok_or("unexpected console event value")?;
        indexes.insert(suffix.parse::<usize>()?);
    }
    let first = indexes.first().copied();
    let last = indexes.last().copied();
    let contiguous = match (first, last) {
        (Some(a), Some(b)) => b.saturating_sub(a) + 1 == indexes.len(),
        (None, None) => true,
        _ => false,
    };

    execute_browser_events(
        &mut browser,
        &json!({"action":"unsubscribe","subscription_id":subscription_id}),
    )?;

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "scheduled":count,
            "delay_ms":delay_ms,
            "before_idle":{
                "retained_count":before_idle.retained_count,
                "dropped":before_idle.dropped,
                "next_sequence":before_idle.next_sequence
            },
            "after_idle_before_poll":{
                "retained_count":after_idle_before_poll.retained_count,
                "dropped":after_idle_before_poll.dropped,
                "next_sequence":after_idle_before_poll.next_sequence
            },
            "poll":{
                "elapsed_ms":elapsed_ms,
                "calls":polls,
                "events":events.len(),
                "unique_events":indexes.len(),
                "first_index":first,
                "last_index":last,
                "contiguous":contiguous,
                "cursor_lost":cursor_lost,
                "dropped":dropped,
                "stream_resets":resets
            },
            "final_ring":{
                "retained_count":final_stats.retained_count,
                "retained_bytes":final_stats.retained_bytes,
                "dropped_total":final_stats.dropped,
                "oldest_sequence":final_stats.oldest_sequence,
                "newest_sequence":final_stats.newest_sequence,
                "next_sequence":final_stats.next_sequence,
                "max_count":final_stats.max_count,
                "max_bytes":final_stats.max_bytes
            }
        }))?
    );

    Ok(())
}

fn multitab_probe() -> Result<(), Box<dyn Error>> {
    let mut browser = BrowserSession::connect()?;
    browser.reset_cdp_event_stream();

    let active_before = browser.active_target_label()?;
    let initial = browser.logical_targets()?;
    let main = initial
        .iter()
        .find(|target| target.label() == "main")
        .ok_or("missing main logical target")?;
    if main.session_id().is_none() {
        return Err("main logical target has no attached CDP session".into());
    }

    let created = browser.browser_call(
        "Target.createTarget",
        json!({"url":"file:///data/jelly/tests/fixtures/browser-perf.html"}),
    )?;
    let created_id = created["result"]["targetId"]
        .as_str()
        .ok_or("Target.createTarget missing targetId")?
        .to_owned();

    let mut tab2_ready = false;
    for _ in 0..20 {
        browser.pump_cdp_events()?;
        let targets = browser.logical_targets()?;
        if targets.iter().any(|target| {
            target.label() == "tab-2"
                && target.target_id() == created_id
                && target.session_id().is_some()
        }) {
            tab2_ready = true;
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    if !tab2_ready {
        return Err("tab-2 did not become auto-attached".into());
    }

    let targets = browser.logical_targets()?;
    let main_session = targets
        .iter()
        .find(|target| target.label() == "main")
        .and_then(|target| target.session_id())
        .ok_or("main session disappeared")?
        .to_owned();
    let tab2_session = targets
        .iter()
        .find(|target| target.label() == "tab-2")
        .and_then(|target| target.session_id())
        .ok_or("tab-2 session missing")?
        .to_owned();
    if main_session == tab2_session {
        return Err("main and tab-2 unexpectedly share one CDP session".into());
    }

    browser.call_on_logical_target("main", "Runtime.enable", json!({}))?;
    browser.call_on_logical_target("tab-2", "Runtime.enable", json!({}))?;

    let subscribed = execute_browser_events(
        &mut browser,
        &json!({"action":"subscribe","methods":["Runtime.consoleAPICalled"]}),
    )?;
    let subscription_id = subscribed["subscription_id"]
        .as_str()
        .ok_or("missing multitab subscription_id")?
        .to_owned();

    let batch = execute_browser_call(
        &mut browser,
        &json!({
            "calls":[
                {
                    "scope":"target",
                    "target":"main",
                    "call":{
                        "method":"Runtime.evaluate",
                        "params":{"expression":"console.log('from-main')"}
                    }
                },
                {
                    "scope":"target",
                    "target":"tab-2",
                    "call":{
                        "method":"Runtime.evaluate",
                        "params":{"expression":"console.log('from-tab-2')"}
                    }
                }
            ]
        }),
        RawCdpAccess::Enabled,
    )?;
    if batch["status"] != "completed" || batch["calls_succeeded"] != 2 {
        return Err(format!("targeted browser-call batch failed: {batch}").into());
    }

    let polled = execute_browser_events(
        &mut browser,
        &json!({"action":"poll","subscription_id":subscription_id,"limit":20}),
    )?;
    let mut routed = polled["events"]
        .as_array()
        .ok_or("multitab poll missing events")?
        .iter()
        .map(|event| {
            Ok((
                event["target"]
                    .as_str()
                    .ok_or("routed event missing logical target")?
                    .to_owned(),
                event["params"]["args"][0]["value"]
                    .as_str()
                    .ok_or("routed event missing console value")?
                    .to_owned(),
            ))
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    routed.sort();

    let expected = vec![
        ("main".to_owned(), "from-main".to_owned()),
        ("tab-2".to_owned(), "from-tab-2".to_owned()),
    ];
    if routed != expected {
        return Err(format!("unexpected multi-target routing: {routed:?}").into());
    }

    let active_after_calls = browser.active_target_label()?;
    if active_after_calls != active_before {
        return Err(format!(
            "target-scoped dispatch changed active target: before={active_before} after={active_after_calls}"
        )
        .into());
    }

    browser.browser_call("Target.closeTarget", json!({"targetId":created_id}))?;
    let mut removed = false;
    for _ in 0..20 {
        browser.pump_cdp_events()?;
        if !browser
            .logical_targets()?
            .iter()
            .any(|target| target.label() == "tab-2")
        {
            removed = true;
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    if !removed {
        return Err("destroyed tab-2 remained in logical target registry".into());
    }

    let destroyed_routed = browser.cdp_events().iter().any(|event| {
        event.method() == "Target.targetDestroyed"
            && event.target() == Some("tab-2")
            && event.target_id() == Some(created_id.as_str())
    });
    if !destroyed_routed {
        return Err("Target.targetDestroyed was not retained with tab-2 attribution".into());
    }

    execute_browser_events(
        &mut browser,
        &json!({"action":"unsubscribe","subscription_id":subscription_id}),
    )?;

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "targets_before_destroy":[
                {"target":"main","session_attached":true},
                {"target":"tab-2","session_attached":true}
            ],
            "sessions_distinct":main_session != tab2_session,
            "routed":routed,
            "active_before":active_before,
            "active_after_target_calls":active_after_calls,
            "destroyed_target":"tab-2",
            "destroyed_routed":destroyed_routed,
            "tab2_removed":removed
        }))?
    );

    Ok(())
}
