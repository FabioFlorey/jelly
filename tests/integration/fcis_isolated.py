#!/usr/bin/env python3
"""Isolated FC/IS integration: never starts/stops installed Jelly systemd units.

Usage: python3 tests/integration/fcis_isolated.py --sandbox /data/jelly-fcis-isolated-<nonce>
The sandbox is a copy of the repo with config/jelly.toml runtime_root under
<SANDBOX>/runtime, with .env absent, and separately compiled binaries at <SANDBOX>/build.
The script operates *only* on its own Chromium and MCP subprocess groups.
"""
import argparse
import base64
import hashlib
import json
import os
import signal
import shutil
import tempfile
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path


def require(condition, reason):
    if not condition:
        raise AssertionError(reason)


def available_port():
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, msg, headers, newurl):
        return None


HTTP = urllib.request.build_opener(NoRedirect())


def http(url, method="GET", json_body=None, form=None, headers=None):
    data = None
    request_headers = dict(headers or {})
    if json_body is not None:
        data = json.dumps(json_body).encode()
        request_headers["Content-Type"] = "application/json"
    if form is not None:
        data = urllib.parse.urlencode(form).encode()
        request_headers["Content-Type"] = "application/x-www-form-urlencoded"
    request = urllib.request.Request(url, data=data, headers=request_headers, method=method)
    try:
        result = HTTP.open(request, timeout=8)
    except urllib.error.HTTPError as error:
        result = error
    with result:
        body = result.read()
        try:
            payload = json.loads(body)
        except (ValueError, UnicodeDecodeError):
            payload = body.decode(errors="replace")
        return result.status, {key.lower(): value for key, value in result.headers.items()}, payload


def shutdown(process):
    if process and process.poll() is None:
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        try:
            process.wait(timeout=8)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait(timeout=8)


def wait_for(predicate, label, seconds=18):
    until = time.monotonic() + seconds
    while time.monotonic() < until:
        try:
            value = predicate()
        except (OSError, ValueError, urllib.error.URLError):
            value = None
        if value:
            return value
        time.sleep(0.15)
    raise AssertionError(f"timed out waiting for {label}")


def run_binary(binary, *args, env, cwd, expect_success=True):
    result = subprocess.run([str(binary), *args], cwd=cwd, env=env, text=True,
                            capture_output=True, timeout=25)
    if expect_success and result.returncode:
        raise AssertionError(f"{binary.name} {args}: {result.returncode}\n{result.stderr[-1200:]}\n{result.stdout[-1200:]}")
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--sandbox", required=True)
    args = parser.parse_args()
    sandbox = Path(args.sandbox).resolve(strict=True)
    require(sandbox.parent == Path("/data") and sandbox.name.startswith("jelly-fcis-isolated-"),
            "refusing unsafe sandbox path")
    repo = sandbox / "repo"
    runtime = sandbox / "runtime"
    binaries = sandbox / "build/debug"
    config = (repo / "config/jelly.toml").read_text()
    require(f'runtime_root = "{runtime}"' in config, "runtime root is not exclusively the sandbox")
    require(not (repo / ".env").exists(), "sandbox contains .env; refusing to use real credentials")
    require(repo.resolve() != Path("/data/github/jelly").resolve(), "sandbox is production repo")
    for program in ["jelly-agent-api-probe", "jelly-mcp", "agent-find-interactive", "agent-evaluate-js", "agent-tabs"]:
        require((binaries / program).is_file(), f"isolated binary missing: {program}")
    (runtime / "state").mkdir(parents=True, exist_ok=True)
    (runtime / "artifacts/downloads").mkdir(parents=True, exist_ok=True)
    trace = sandbox / "integration.log"
    trace.write_text("")
    env = os.environ.copy()
    for key in list(env):
        if key.startswith("JELLY_"):
            env.pop(key, None)
    env.update({"HOME": str(sandbox), "XDG_CONFIG_HOME": str(sandbox / "xdg"),
                "CARGO_TARGET_DIR": str(sandbox / "build")})
    fixture = (repo / "tests/fixtures/browser-perf.html").as_uri()
    chromium_log = open(sandbox / "isolated-chromium.log", "a")
    mcp_log = open(sandbox / "isolated-mcp.log", "a")
    chrome = None
    mcp = None
    browser_profile = Path(tempfile.mkdtemp(prefix="chrome-profile-", dir=sandbox))
    checks = []

    def log(message):
        print(message, flush=True)
        with trace.open("a") as output:
            output.write(message + "\n")

    def check(name, predicate=True):
        require(predicate, name)
        checks.append(name)
        log("PASS " + name)

    try:
        chrome = subprocess.Popen([
            "/usr/bin/chromium", "--headless=new", "--no-sandbox", "--disable-gpu",
            "--disable-background-networking", "--disable-sync", "--no-first-run",
            "--disable-dev-shm-usage", "--remote-allow-origins=*",
            "--remote-debugging-port=0", f"--user-data-dir={browser_profile}",
            fixture,
        ], stdout=chromium_log, stderr=subprocess.STDOUT, env=env, start_new_session=True)
        devtools = browser_profile / "DevToolsActivePort"
        def cdp_ready():
            if chrome.poll() is not None:
                raise AssertionError(f"isolated Chromium exited with {chrome.returncode}")
            if not devtools.is_file():
                return None
            lines = devtools.read_text().strip().splitlines()
            return lines if len(lines) >= 2 else None
        lines = wait_for(cdp_ready, "isolated CDP endpoint")
        port = int(lines[0])
        ws = f"ws://127.0.0.1:{port}{lines[1]}"
        require(f"127.0.0.1:{port}" in ws, "CDP endpoint not loopback")
        (runtime / "state/cdp_endpoint").write_text(ws)
        (runtime / "state/browser.pid").write_text(str(chrome.pid))
        pages = http(f"http://127.0.0.1:{port}/json/list")[2]
        page = next(p for p in pages if p.get("type") == "page" and p.get("url") == fixture)
        (runtime / "state/page_target_id").write_text(page["id"])
        check("isolated Chromium CDP/profile and page target")

        # Run existing production Agent API probes, not a mocked CDP adapter.
        modes = [
            "semantic-read", "semantic-mutate-verify", "preflight-atomic", "failure-stop",
            "failure-continue", "stale-ref", "logical-target", "raw-target", "raw-browser",
            "mixed-batch", "events-lifecycle", "subscription-stale",
        ]
        for mode in modes:
            r = run_binary(binaries / "jelly-agent-api-probe", mode, env=env, cwd=repo)
            check("Agent API/BrowserSession " + mode, json.loads(r.stdout)["ok"] is True)

        # Verify the optimized/rollback ranking through actual Jelly CLI binaries.
        config_path = repo / "config/jelly.toml"
        initial_config = config_path.read_text()
        try:
            from_fixture = (repo / "tests/fixtures/semantic-targets.html").as_uri()
            r = run_binary(binaries / "agent-evaluate-js",
                           f"location.href={json.dumps(from_fixture)}", env=env, cwd=repo)
            # A navigation may detach the running target; the next calls reattach.
            def find(query):
                r = run_binary(binaries / "agent-find-interactive", query, "10", env=env, cwd=repo)
                return json.loads(r.stdout)
            optimized = find("Duplicate action")
            check("Jelly CLI enabled-first ranking", optimized and optimized[0]["disabled"] is False)
            config_path.write_text(initial_config.replace("runtime = true", "runtime = false"))
            legacy = find("Duplicate action")
            check("Jelly CLI legacy rank priority", legacy and legacy[0]["disabled"] is False)
            config_path.write_text(initial_config)
        finally:
            config_path.write_text(initial_config)

        # E15: real routine runner with core planning and browser-side tool
        # execution; HITL delivery is deliberately stubbed only in this
        # separate sandbox (no Telegram or other external service contacted).
        routine_bin = binaries / "agent-call-routine"
        require(routine_bin.is_file(), "isolated agent-call-routine binary missing")
        routine_dir = repo / ".agent/routines"
        graphs = [routine_dir / f"fcis-integration-{name}.json" for name in ("tool", "hitl", "loop")]
        require(all(not path.exists() for path in graphs), "refusing to overwrite routine fixtures")
        graph_tool = {"entry":"tool", "nodes": {
            "tool":{"tool":"evaluate-js","args":["21*2"],"save":"answer","next":"decide"},
            "decide":{"guard":{"path":"answer","op":"equals","value":42},
                      "then":"done","else":"bad"},
            "done":{"terminal":"success","message":"browser tool passed"},
            "bad":{"terminal":"failure","message":"wrong browser output"},
        }}
        graph_hitl = {"entry":"review","nodes":{
            "review":{"hitl":"Approve {{name}}","resume":"check"},
            "check":{"guard":{"path":"verdict","op":"equals","value":"approved"},
                     "then":"done","else":"bad"},
            "done":{"terminal":"success","message":"approved {{name}}"},
            "bad":{"terminal":"failure","message":"rejected {{name}}"},
        }}
        graph_loop = {"entry":"loop","max_steps":10,"nodes":{
            "loop":{"goto":"loop","max_visits":2},
        }}
        for path, body in zip(graphs, [graph_tool, graph_hitl, graph_loop]):
            path.write_text(json.dumps(body))
        try:
            performed = run_binary(routine_bin, "fcis-integration-tool", env=env, cwd=repo)
            result = json.loads(performed.stdout)
            check("routine actual browser tool and core transition",
                  result["status"] == "completed" and result["context"]["answer"] == 42)
            loop = run_binary(routine_bin, "fcis-integration-loop", env=env, cwd=repo,
                              expect_success=False)
            check("routine max_visits protection and failure cleanup",
                  loop.returncode != 0 and "exceeded max_visits=2" in loop.stderr)
            fake_agent_run, fake_agent_hitl = binaries / "agent-run", binaries / "agent-hitl"
            require(not fake_agent_run.exists() and not fake_agent_hitl.exists(),
                    "refusing to override real system-tool binaries")
            fake_agent_run.write_text("#!/bin/sh\n[ \"$1\" = hitl ] || exit 91\nprintf '{\"delivered\":true}\n'\n")
            fake_agent_hitl.write_text('#!/bin/sh\nexit 0\n')
            fake_agent_run.chmod(0o700)
            fake_agent_hitl.chmod(0o700)
            try:
                suspended = run_binary(routine_bin, "fcis-integration-hitl", "name=Alice", env=env, cwd=repo)
                result = json.loads(suspended.stdout)
                require(result["status"] == "suspended", f"unexpected HITL state {result}")
                state_file = runtime / "routines" / (str(result["resume_id"]) + ".state")
                state = json.loads(state_file.read_text())
                check("routine HITL suspend persists next step and delivery",
                      state["current"] == "check" and state["context"]["_hitl_delivery"] == {"delivered": True})
                resumed = run_binary(routine_bin, "resume", str(result["resume_id"]),
                                     "verdict=approved", env=env, cwd=repo)
                resume = json.loads(resumed.stdout)
                check("routine HITL resume and persisted state cleanup",
                      resume["status"] == "completed" and
                      resume["message"] == "approved Alice" and not state_file.exists())
            finally:
                fake_agent_run.unlink(missing_ok=True)
                fake_agent_hitl.unlink(missing_ok=True)
        finally:
            for path in graphs:
                path.unlink(missing_ok=True)

        # E14: actual persisted record/artifact/collision transitions. The Rust
        # test is #[ignore] and refuses to write unless the config root and the
        # explicit sandbox path match. It never touches the active Jelly runtime.
        disk_env = dict(env, JELLY_FCIS_ISOLATION_ROOT=str(sandbox))
        disk_test = subprocess.run([
            "cargo", "test", "--lib", "fcis_real_download_persistence_and_artifact_finalize",
            "--quiet", "--locked", "--", "--ignored"
        ], cwd=repo, env=disk_env, capture_output=True, text=True, timeout=140)
        require(disk_test.returncode == 0,
                f"isolated download persistence test failed: {disk_test.stderr[-1000:]}\n{disk_test.stdout[-1200:]}")
        check("downloads real persisted lifecycle, artifact registration and collisions",
              "1 passed" in disk_test.stdout)

        # Separate loopback HTTP server, built from the isolated source/config.
        port = available_port()
        mcp_address = f"http://127.0.0.1:{port}"
        resource = "https://jelly-fcis-test.invalid/mcp"
        static_token = "fcis-test-token-0123456789012345678901234567"
        bootstrap = "fcis-bootstrap-012345678901234567890123456789"
        password = "fcis-test-password-012345678901234567890123"
        mcp_env = dict(env, JELLY_MCP_ADDR=f"127.0.0.1:{port}",
                       JELLY_PUBLIC_URL="https://jelly-fcis-test.invalid",
                       JELLY_OAUTH_CONSENT_MODE="browser", JELLY_OAUTH_PASSWORD=password,
                       JELLY_OAUTH_PUBLIC_CHATGPT_DCR="true", JELLY_MCP_TOKEN=static_token,
                       JELLY_BOOTSTRAP_SECRET=bootstrap)
        mcp = subprocess.Popen([str(binaries / "jelly-mcp")], cwd=repo,
                               env=mcp_env, stdout=mcp_log, stderr=subprocess.STDOUT,
                               start_new_session=True)
        def server_ready():
            if mcp.poll() is not None:
                raise AssertionError(f"isolated MCP exited with {mcp.returncode}")
            status, _, _ = http(mcp_address + "/health")
            return status == 200
        wait_for(server_ready, "isolated MCP server")
        check("isolated MCP loopback server")

        def rpc(name, params, bearer=static_token):
            status, _, body = http(mcp_address + "/mcp", "POST",
                json_body={"jsonrpc":"2.0", "id":1, "method":name, "params":params},
                headers={"Authorization":"Bearer " + bearer})
            require(status == 200, f"MCP {name} returned HTTP {status}: {body}")
            require("result" in body, f"MCP {name} JSON-RPC error: {body}")
            return body["result"]
        names = [t["name"] for t in rpc("tools/list", {})["tools"]]
        check("MCP tools/list small-surface", "browser-call" in names and "browser-schema" in names)
        call = rpc("tools/call", {"name":"browser-call","arguments": {
            "calls":[{"call":{"jelly":"evaluate-js","params":{"expression":"document.title"}}}]}})
        check("MCP browser-call over HTTP", not call["isError"] and call["structuredContent"]["ok"])

        # Full real HTTP OAuth grants and persistence in the sandbox runtime only.
        redirect_uri = "http://127.0.0.1/fcis/callback"
        status, _, client = http(mcp_address + "/register", "POST",
            json_body={"client_name":"FCIS integration", "redirect_uris":[redirect_uri]},
            headers={"Authorization":"Bearer " + bootstrap})
        check("OAuth DCR registration isolated", status in (200, 201) and bool(client.get("client_id")))
        client_id = client["client_id"]
        verifier = "isolated-code-verifier-0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ"
        challenge = base64.urlsafe_b64encode(hashlib.sha256(verifier.encode()).digest()).decode().rstrip("=")
        def authorize():
            status, headers, _ = http(mcp_address + "/authorize", "POST", form={
                "response_type":"code", "client_id":client_id, "redirect_uri":redirect_uri,
                "code_challenge_method":"S256", "code_challenge":challenge,
                "resource":resource, "scope":"jelly", "action":"approve", "password":password,
            })
            require(status in (302, 303, 307, 308), f"OAuth consent did not redirect: {status}")
            query = urllib.parse.parse_qs(urllib.parse.urlsplit(headers["location"]).query)
            require("code" in query, f"OAuth redirect missing code: {query}")
            return query["code"][0]
        def exchange(code, verify=verifier):
            return http(mcp_address + "/token", "POST", form={
                "grant_type":"authorization_code", "client_id":client_id,
                "redirect_uri":redirect_uri, "resource":resource,
                "code":code, "code_verifier":verify,
            })
        def rotate(token, extra=None):
            return http(mcp_address + "/token", "POST", form=dict({
                "grant_type":"refresh_token", "client_id":client_id, "refresh_token":token,
            }, **(extra or {})))
        bad_code = authorize()
        status, _, bad = exchange(bad_code, "wrong-verifier")
        check("OAuth PKCE rejects invalid verifier", status == 400 and bad.get("error") == "invalid_grant")
        status, _, reused = exchange(bad_code)
        check("OAuth authorization code one-time use", status == 400 and reused.get("error") == "invalid_grant")
        status, _, family_a = exchange(authorize())
        check("OAuth authorization_code exchange persisted", status == 200 and family_a.get("refresh_token"))
        status, _, other = exchange(authorize())
        check("OAuth second independent token family", status == 200 and other.get("refresh_token"))
        status, _, mismatch = rotate(family_a["refresh_token"], {"scope":"not-jelly"})
        check("OAuth refresh scope binding", status == 400 and mismatch.get("error") == "invalid_scope")
        status, _, mismatch = rotate(family_a["refresh_token"], {"resource":"https://invalid.invalid/mcp"})
        check("OAuth refresh resource binding", status == 400 and mismatch.get("error") == "invalid_target")
        status, _, rotated = rotate(family_a["refresh_token"])
        check("OAuth refresh token rotation", status == 200 and rotated.get("refresh_token") != family_a["refresh_token"])
        status, _, replay = rotate(family_a["refresh_token"])
        check("OAuth consumed-refresh replay detection", status == 400 and replay.get("error") == "invalid_grant")
        status, _, revoked = rotate(rotated["refresh_token"])
        check("OAuth replay revokes entire token family", status == 400 and revoked.get("error") == "invalid_grant")
        status, _, still_valid = rotate(other["refresh_token"])
        check("OAuth unrelated family survives replay", status == 200 and bool(still_valid.get("refresh_token")))
        oauth_file = runtime / "state/oauth.json"
        require(oauth_file.exists(), "OAuth state absent from isolated runtime")
        stored = json.loads(oauth_file.read_text())
        check("OAuth durable store only under isolated runtime", bool(stored["clients"]) and bool(stored["refresh_tokens"]))
        # An authorized OAuth bearer still works after the unrelated family was rotated.
        result = rpc("tools/list", {}, bearer=still_valid["access_token"])
        check("MCP authorized with isolated OAuth access token", bool(result["tools"]))
        # E12: lose the cached live CDP socket, observe typed failure, then
        # recover through the same MCP worker without restarting MCP itself.
        shutdown(chrome)
        chrome = None
        lost = rpc("tools/call", {"name":"browser-call","arguments": {
            "calls":[{"call":{"jelly":"read-page","params":{}}}]}})
        lost_envelope = lost["structuredContent"]
        check("MCP cached browser session becomes unavailable after CDP loss",
              lost["isError"] and not lost_envelope["ok"] and
              lost_envelope["error"]["kind"] == "browser_unavailable")
        shutil.rmtree(browser_profile, ignore_errors=True)
        browser_profile = Path(tempfile.mkdtemp(prefix="chrome-profile-restart-", dir=sandbox))
        chrome = subprocess.Popen([
            "/usr/bin/chromium", "--headless=new", "--no-sandbox", "--disable-gpu",
            "--disable-background-networking", "--disable-sync", "--no-first-run",
            "--disable-dev-shm-usage", "--remote-allow-origins=*",
            "--remote-debugging-port=0", f"--user-data-dir={browser_profile}",
            fixture,
        ], stdout=chromium_log, stderr=subprocess.STDOUT, env=env, start_new_session=True)
        devtools = browser_profile / "DevToolsActivePort"
        lines = wait_for(cdp_ready, "replacement isolated Chromium CDP")
        port = int(lines[0])
        ws = f"ws://127.0.0.1:{port}{lines[1]}"
        (runtime / "state/cdp_endpoint").write_text(ws)
        (runtime / "state/browser.pid").write_text(str(chrome.pid))
        pages = http(f"http://127.0.0.1:{port}/json/list")[2]
        page = next(p for p in pages if p.get("type") == "page" and p.get("url") == fixture)
        (runtime / "state/page_target_id").write_text(page["id"])
        (runtime / "state/active_target_id").write_text(page["id"])
        recovered = rpc("tools/call", {"name":"browser-call","arguments": {
            "calls":[{"call":{"jelly":"read-page","params":{}}}]}})
        check("MCP cached session reconnects to replacement isolated Chromium",
              not recovered["isError"] and recovered["structuredContent"]["ok"])
        # E16 durability: restart only this sandbox MCP subprocess and verify
        # that the already-persisted refresh/access decisions survive reload.
        shutdown(mcp)
        mcp = subprocess.Popen([str(binaries / "jelly-mcp")], cwd=repo,
                               env=mcp_env, stdout=mcp_log, stderr=subprocess.STDOUT,
                               start_new_session=True)
        wait_for(server_ready, "isolated MCP restart from durable OAuth store")
        persisted_access = rpc("tools/list", {}, bearer=still_valid["access_token"])
        check("OAuth access token remains valid after isolated MCP restart",
              bool(persisted_access["tools"]))
        status, _, replay_after_restart = rotate(family_a["refresh_token"])
        check("OAuth consumed refresh replay remains revoked after restart",
              status == 400 and replay_after_restart.get("error") == "invalid_grant")
        status, _, independent_after_restart = rotate(still_valid["refresh_token"])
        check("OAuth independent refresh family survives store reload",
              status == 200 and bool(independent_after_restart.get("refresh_token")))
        log(f"SUMMARY {len(checks)} integration checks PASS; runtime={runtime}; no systemd actions")
        (sandbox / "integration-result.json").write_text(json.dumps({
            "passed":len(checks), "checks":checks, "runtime":str(runtime),
            "git_snapshot":"ddced5d + working tree", "uses_systemd":False,
        }, indent=2))
    finally:
        shutdown(mcp)
        shutdown(chrome)
        mcp_log.close()
        chromium_log.close()
        shutil.rmtree(browser_profile, ignore_errors=True)


if __name__ == "__main__":
    try:
        main()
    except Exception as exc:
        print("ISOLATED_INTEGRATION_FAILURE", type(exc).__name__, str(exc), file=sys.stderr)
        sys.exit(1)
