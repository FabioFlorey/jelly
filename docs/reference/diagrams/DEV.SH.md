<div id="top"></div>

<div align="center">

# CLI Processes Diagrams

**Jelly CLI workflows, from first-time setup to browser operation and verification.**

</div>

This document defines the main CLI processes for readers who want to understand how Jelly works. Each chapter explains its purpose and shows the decisions, actions, and outcomes in a Mermaid diagram.

**Flowchart conventions:** Pill-shaped **START** and **END** nodes mark process boundaries; rectangles represent actions, diamonds represent decisions, and labeled arrows show the possible paths.

## Table of Contents

- [1. Quickstart: from intention to a working browser](#1-quickstart-from-intention-to-a-working-browser)
- [2. Connection and hosting choices](#2-connection-and-hosting-choices)
- [3. Browser environment and security](#3-browser-environment-and-security)
- [4. Operations and recovery](#4-operations-and-recovery)
- [5. Test and release decisions](#5-test-and-release-decisions)

## 1. Quickstart: from intention to a working browser

This process follows the **real** default `./scripts/dev.sh` command. It checks core prerequisites, asks the user to select or keep a browser backend, runs the MCP identity/hosting wizard, saves settings only after review, and then offers to install and start Jelly. The final verification checks MCP health and readiness; browser/CDP connectivity is a separate operational check.

```mermaid
flowchart TD
    A(["START"])
    A --> B["Open ./scripts/dev.sh in an interactive terminal"]
    B --> C{"Core prerequisites available?"}
    C -->|No| D["Resolve missing prerequisites and retry"]
    D --> B
    C -->|Yes| E{"Where will Chromium run?"}
    E -->|Native| F["Confirm native browser backend"]
    E -->|Docker or Podman| G["Select and review container browser settings"]
    E -->|Existing CDP| H["Enter endpoint, review and register"]
    E -->|Previously configured| I["Reuse selected browser backend"]
    F --> J["Configure MCP credentials, hosting and integrations"]
    G --> J
    H --> J
    I --> J
    J --> K{"Selected prerequisites satisfied?"}
    K -->|No| L["Correct prerequisites or browser selection"]
    L --> E
    K -->|Yes| M["Review redacted MCP configuration"]
    M --> N{"Approve saving settings?"}
    N -->|No| O["Cancel configuration changes"]
    O --> P(["END"])
    N -->|Yes| Q["Save .env configuration securely"]
    Q --> R{"Install and start Jelly now?"}
    R -->|No| S["Keep saved configuration for later startup"]
    S --> T(["END"])
    R -->|Yes| U["Build, install and start user services"]
    U --> V{"Installation succeeded?"}
    V -->|No| W["Review service and build diagnostics"]
    W --> U
    V -->|Yes| X["Check MCP /health and /ready endpoints"]
    X --> Y{"MCP healthy and ready?"}
    Y -->|No| Z["Diagnose services and retry verification"]
    Z --> X
    Y -->|Yes| AA["MCP ready; verify browser/CDP connectivity separately"]
    AA --> AB(["END"])
```

<div align="right"><sub><a href="#top">&uarr; Back to top</a></sub></div>


## 2. Connection and hosting choices

This process defines how `scripts/commands/setup.sh` chooses the MCP connection path for a local client or an external HTTPS client. It covers authentication, public reachability, review of the selected settings, and the choice to start services immediately or save configuration for later. The outcome is either a reachable MCP endpoint or a saved configuration awaiting startup.

```mermaid
flowchart TD
    A(["START"])
    A --> B{"Client location?"}
    B -->|Local MCP client| C["Local HTTP on 127.0.0.1; bearer token"]
    B -->|ChatGPT or other remote HTTPS client| D["Public HTTPS + OAuth"]
    D --> E{"Hosting available?"}
    E -->|Temporary URL| F["Quick Tunnel"]
    E -->|Permanent domain| G["Fixed Cloudflare/domain setup"]
    E -->|IP-based hostname| H["nip.io option, where appropriate"]
    F --> I["Check URL, OAuth settings and callback"]
    G --> I
    H --> I
    C --> J["Review secret redaction and authorization"]
    I --> J
    J --> K{"Save and enable service now?"}
    K -->|Yes| L["Confirm → install/start → verify endpoint"]
    K -->|No| M["Save only; start later"]
    L --> O{"MCP endpoint reachable?"}
    O -->|Yes| N1["MCP endpoint reachable; proceed to browser selection"]
    N1 --> N(["END"])
    O -->|No| P["Inspect service, tunnel and authorization; retry"]
    P --> L
    M --> Q1["Configuration saved; services will be started later"]
    Q1 --> Q(["END"])
```

<div align="right"><sub><a href="#top">&uarr; Back to top</a></sub></div>


## 3. Browser environment and security

This process defines the choices offered by `scripts/commands/browser-container.sh` when configuring Chromium. The user chooses native Chromium, an existing CDP endpoint, or a Docker/Podman container. For containers, engine availability and local CDP evidence inform recommendations, while sandbox, restrictions, profile, and network remain selectable independently. After reviewing and confirming, the CLI either registers a working browser or leaves the configuration unchanged.

```mermaid
flowchart TD
    A(["START"])
    A --> B{"Browser location?"}
    B -->|Native| C["Use host Chromium"]
    B -->|Existing CDP| D["Review external address + CDP risk"]
    B -->|Container| E{"Accessible engine?"}
    E -->|Neither| F["Offer native/external; explain optional install"]
    F --> B
    E -->|Docker and/or Podman| G["Identify mode: rootful/rootless daemon"]
    G --> H["Local CDP PASS first; reference-only separate"]
    H --> I{"Selection?"}
    I -->|Recommended| J["Select tested candidate"]
    I -->|Advanced| K["Any combination, including failed/unverified"]
    I -->|Probe| L["Temporary Chromium/CDP run"]
    L --> M{"CDP ready?"}
    M -->|Yes| N["Record local PASS (not sandbox attestation)"]
    M -->|No| O["Record FAIL + diagnostics; no silent fallback"]
    N --> H
    O --> H
    J --> P["Choose sandbox, restrictions, profile, network separately"]
    K --> P
    C --> Q["Preview before changing browser mode"]
    D --> Q
    P --> Q
    Q --> R{"Confirm?"}
    R -->|No| V1["Change cancelled; browser configuration unchanged"]
    V1 --> V(["END"])
    R -->|Yes| S["Create/register or change backend"]
    S --> T["Restart MCP if required; check routing and status"]
    T --> U{"Browser and MCP routing ready?"}
    U -->|Yes| W1["Browser registered and available to the client"]
    W1 --> W(["END"])
    U -->|No| X["Read CDP/MCP diagnostics; choose another configuration"]
    X --> B
```

<div align="right"><sub><a href="#top">&uarr; Back to top</a></sub></div>


## 4. Operations and recovery

This process describes how an operator uses Jelly's status, log, and service-management commands after installation. Depending on the issue, the user can inspect current state, confirm a start/stop operation, probe a browser configuration, or revisit access settings. Each action returns to verification so that the process ends with a reviewed outcome or a deliberate cancellation.

```mermaid
flowchart TD
    A(["START"])
    A --> B{"What needs attention?"}
    B -->|Status or logs| C["Read service/browser status and diagnostics"]
    B -->|Start or stop| D["Show impact and ask confirmation"]
    B -->|Browser trouble| E["Inspect CDP logs, selected engine, profile, sandbox"]
    B -->|Test new configuration| F["Use disposable probe; no registration"]
    B -->|Change hosting or access| G["Revisit setup and review secrets"]
    D --> H{"Confirm?"}
    H -->|No| I1["Action cancelled; running services unchanged"]
    I1 --> I(["END"])
    H -->|Yes| J["Start/stop; verify actual state"]
    E --> K{"Does CDP work?"}
    K -->|Yes| L["Check browser ID / MCP routing"]
    K -->|No| M["Inspect capabilities, namespaces and profile ownership"]
    M --> E
    L --> C
    F --> C
    G --> C
    J --> C
    C --> N{"Another task?"}
    N -->|Yes| B
    N -->|No| O1["Operator has finished reviewing the service state"]
    O1 --> O(["END"])
```

<div align="right"><sub><a href="#top">&uarr; Back to top</a></sub></div>


## 5. Test and release decisions

This process explains how a maintainer selects the appropriate validation command before accepting a change. Static checks (`./scripts/dev.sh check`), Rust tests (`./scripts/dev.sh test`), browser integration tests, and catalog reviews provide different evidence. Failed executable checks lead back to diagnosis and rerunning the selected test; completed checks lead to a review of results and their scope.

```mermaid
flowchart TD
    A(["START"])
    A --> B{"What type of check?"}
    B -->|Formatting, lint, CLI contracts| C["dev.sh check"]
    B -->|Rust and wrapper unit tests| D["dev.sh test"]
    B -->|Disposable browser/MCP integration| E["test --isolated"]
    B -->|Installed browser behavioral suite| F["test --suite: dedicated test installation"]
    B -->|Test catalog or acceptance coverage| G["--catalog or --acceptance-report"]
    C --> H{"All checks passed?"}
    D --> H
    E --> H
    F --> H
    G --> I["Interpret catalog/coverage; not runtime proof"]
    H -->|Yes| J["Review logs, reports and environment boundaries"]
    H -->|No| K["Inspect failures, correct and rerun"]
    K --> B
    I --> J
    J --> L1["Evidence assembled for review"]
    L1 --> L(["END"])
```

<div align="right"><sub><a href="#top">&uarr; Back to top</a> · <a href="../../INDEX.md">Documentation index</a></sub></div>
