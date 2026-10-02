<div align="center" id="top">

# GLOSSARY
`Glossary v1.0`

<sub>

`Select a letter below to browse entries alphabetically`

### [A](#a)　[B](#b)　[C](#c)　[D](#d)　[E](#e)　[H](#h)　[I](#i)　[J](#j)　[L](#l)　[M](#m)　[N](#n)　[O](#o)　[P](#p)　[R](#r)　[S](#s)　[T](#t)　[U](#u)　[V](#v)　[W](#w)

</sub>

</div>

## A

+ **Accessibility Tree**  
  A browser-provided semantic representation of a page focused on roles, names, states, and relationships rather than visual layout. Agents can use it to understand controls and content without relying only on raw HTML.  
  **Acronyms**: AX tree  
  **Synonyms**: accessibility tree, AX tree  
  **References**: [Tool Index](../.agent/tools/index.md)

+ **Active Target**  
  The Chromium target currently attached to jelly's shared browser session. Browser commands and page evaluation are directed to this target until the session switches elsewhere.  
  **Acronyms**: ∅  
  **Synonyms**: current target, attached target  
  **References**: [Architecture](./ARCHITECTURE.md)

+ **Agent**  
  A software component that observes a situation, chooses actions, and uses tools to pursue a goal. In jelly, the agent sits above primitives and routines and decides what to inspect or execute next.  
  **Acronyms**: ∅  
  **Synonyms**: AI agent, autonomous agent  
  **References**: [Architecture](./ARCHITECTURE.md)

<div align="right"><sub><a href="#top">⭐ Go to the top of the document</a></sub></div>

## B

+ **Argument Validation**  
  Checking tool inputs against the primitive's declared argument requirements before the browser operation executes. This catches missing arguments, invalid target forms, and basic type errors at the registry boundary.  
  **Acronyms**: ∅  
  **Synonyms**: input validation, schema validation  
  **References**: [Architecture](./ARCHITECTURE.md)

+ **BrowserSession**  
  jelly's Rust abstraction around an attached Chromium DevTools Protocol session. It owns the active CDP connection, target attachment, command dispatch, and JavaScript evaluation used by browser primitives.  
  **Acronyms**: ∅  
  **Synonyms**: browser session  
  **References**: [Architecture](./ARCHITECTURE.md)

<div align="right"><sub><a href="#top">⭐ Go to the top of the document</a></sub></div>

## C

+ **Capability**  
  A coherent group of tools that solve a related class of tasks, such as browser inspection, input, navigation, tabs, files, or scripting. Capabilities provide a lightweight discovery layer before individual tool schemas are loaded.  
  **Acronyms**: ∅  
  **Synonyms**: capability group  
  **References**: [Tool Discovery](./DISCOVERY.md)

+ **CDP**  
  The protocol used to control and inspect Chromium programmatically. jelly uses CDP for navigation, DOM interaction, input events, screenshots, network inspection, target management, and JavaScript execution.  
  **Acronyms**: Chrome DevTools Protocol  
  **Synonyms**: DevTools Protocol  
  **References**: [Architecture](./ARCHITECTURE.md)

+ **CDP Target**  
  A debuggable entity exposed by Chromium through CDP, such as a page, worker, or browser-level target. jelly primarily attaches to page targets for browser interaction.  
  **Acronyms**: Chrome DevTools Protocol target  
  **Synonyms**: target, debug target  
  **References**: [Architecture](./ARCHITECTURE.md)

+ **Chromium**  
  The browser engine/runtime currently controlled by jelly. The project launches Chromium and communicates with it through CDP.  
  **Acronyms**: ∅  
  **Synonyms**: Chromium browser  
  **References**: [Requirements](./REQUIREMENTS.md)

+ **Composable Primitive**  
  A small operation designed to combine cleanly with other operations instead of encoding an entire site-specific workflow. This is a central jelly design rule: primitives provide capabilities; routines provide workflows.  
  **Acronyms**: ∅  
  **Synonyms**: composable tool, atomic capability  
  **References**: [Routines](./ROUTINES.md)

+ **CLI**  
  A command-oriented interface used to invoke jelly tools from a shell. Examples include `agent-run`, `agent-discover`, and individual `agent-*` binaries.  
  **Acronyms**: Command-Line Interface  
  **Synonyms**: command-line interface  
  **References**: [README](../README.md)

<div align="right"><sub><a href="#top">⭐ Go to the top of the document</a></sub></div>

## D

+ **Discovery**  
  The process by which an agent learns what capabilities and tools are available without loading the complete tool catalog up front. jelly supports capability listing, lightweight tool search, and full schema lookup.  
  **Acronyms**: ∅  
  **Synonyms**: tool discovery, capability discovery  
  **References**: [Tool Discovery](./DISCOVERY.md)

+ **Deferred Loading**  
  Delaying the loading of complete tool definitions until discovery indicates that those tools are relevant. This reduces the amount of tool metadata an agent must consider at once.  
  **Acronyms**: ∅  
  **Synonyms**: deferred tool loading, lazy tool loading  
  **References**: [Tool Discovery](./DISCOVERY.md)

+ **DOM**  
  The in-memory tree representation of a web document exposed by the browser. Many jelly inspection and interaction primitives ultimately operate on DOM elements.  
  **Acronyms**: Document Object Model  
  **Synonyms**: document tree  
  **References**: [Architecture](./ARCHITECTURE.md)

<div align="right"><sub><a href="#top">⭐ Go to the top of the document</a></sub></div>

## E

+ **Escape Hatch**  
  A lower-level capability intentionally available when normal primitives are insufficient. In jelly, JavaScript evaluation and injection are escape hatches rather than the preferred implementation for common interactions.  
  **Acronyms**: ∅  
  **Synonyms**: low-level fallback  
  **References**: [Development](./DEVELOPMENT.md)

+ **Execution Context**  
  The JavaScript environment associated with a particular document or frame. Navigation can destroy an execution context, which matters when evaluating scripts or waiting on page conditions.  
  **Acronyms**: ∅  
  **Synonyms**: JavaScript execution context  
  **References**: [Architecture](./ARCHITECTURE.md)

<div align="right"><sub><a href="#top">⭐ Go to the top of the document</a></sub></div>

## H

+ **Headed Mode**  
  Running Chromium with a visible graphical browser window. This is useful when a human needs to observe or participate in the automation.  
  **Acronyms**: ∅  
  **Synonyms**: headed browser  
  **References**: [Runtime Layout](./RUNTIME.md)

+ **Headless Mode**  
  Running Chromium without a visible browser window. This is the typical mode for unattended automation and does not require a graphical desktop session.  
  **Acronyms**: ∅  
  **Synonyms**: headless browser  
  **References**: [Requirements](./REQUIREMENTS.md)

+ **Handler**  
  The Rust function associated with a primitive in the registry. After arguments are validated, the handler performs the actual browser operation.  
  **Acronyms**: ∅  
  **Synonyms**: primitive handler  
  **References**: [Architecture](./ARCHITECTURE.md)

+ **HITL**  
  A workflow in which execution pauses so a person can review, approve, provide input, or continue an automated process. jelly routines can preserve continuation state for this purpose.  
  **Acronyms**: Human in the Loop  
  **Synonyms**: human approval, human handoff  
  **References**: [Routines](./ROUTINES.md)

<div align="right"><sub><a href="#top">⭐ Go to the top of the document</a></sub></div>

## I

+ **In-Process Execution**  
  Executing a primitive directly inside an existing process and shared BrowserSession instead of spawning another command-line process. Routines use this for browser primitives to preserve session continuity and reduce overhead.  
  **Acronyms**: ∅  
  **Synonyms**: direct execution  
  **References**: [Routines](./ROUTINES.md)

+ **Instrumentation**  
  The practice of exposing observable and controllable interfaces around a system. jelly instruments Chromium so agents can inspect state, issue browser actions, verify outcomes, and record traces.  
  **Acronyms**: ∅  
  **Synonyms**: browser instrumentation  
  **References**: [Architecture](./ARCHITECTURE.md)

<div align="right"><sub><a href="#top">⭐ Go to the top of the document</a></sub></div>

## J

+ **JavaScript Injection**  
  Executing custom JavaScript inside the active page. jelly treats this as an escape hatch for unusual or exploratory behavior rather than the default way to implement common browser operations.  
  **Acronyms**: JS injection  
  **Synonyms**: script injection  
  **References**: [Tool Index](../.agent/tools/index.md)

+ **JSON Schema**  
  A machine-readable description of a tool's accepted arguments and structure. jelly can expose primitive metadata as schemas for agent runtimes or protocol adapters.  
  **Acronyms**: ∅  
  **Synonyms**: tool schema  
  **References**: [Tool Discovery](./DISCOVERY.md)

<div align="right"><sub><a href="#top">⭐ Go to the top of the document</a></sub></div>

## L

+ **Large Surface**
  Jelly's expanded MCP publication mode, selected with `JELLY_MCP_SURFACE=large-surface`. It publishes individual semantic browser primitives as top-level MCP tools together with the current system-tool set. When raw CDP is enabled, it also publishes the dedicated `cdp-call` tool. Large surface is useful for compatibility, diagnostics, and clients that prefer individually published browser operations.
  **Acronyms**: ∅
  **Synonyms**: large-surface, expanded MCP surface
  **References**: [MCP Server](./MCP.md), [Tool Discovery](./DISCOVERY.md)

<div align="right"><sub><a href="#top">⭐ Go to the top of the document</a></sub></div>

## M

+ **MCP**
  The Model Context Protocol used by Jelly to publish authenticated browser and system capabilities to remote AI clients. Jelly exposes a validated Agent Tool Catalog through `tools/list`; the selected MCP surface controls which browser-facing tools are published and executable.
  **Acronyms**: Model Context Protocol
  **Synonyms**: ∅
  **References**: [MCP Server](./MCP.md), [Tool Discovery](./DISCOVERY.md)

<div align="right"><sub><a href="#top">⭐ Go to the top of the document</a></sub></div>

## N

+ **Network Capture**  
  Recording browser request and response metadata from CDP network events. jelly can use this to inspect API calls, response status, MIME types, and URLs produced during a browser workflow.  
  **Acronyms**: ∅  
  **Synonyms**: network inspection, traffic capture  
  **References**: [Runtime Layout](./RUNTIME.md)

<div align="right"><sub><a href="#top">⭐ Go to the top of the document</a></sub></div>

## O

+ **Observe → Act → Verify Loop**  
  An agent-control pattern where the system first observes browser state, performs one or more actions, then inspects the resulting state to confirm the expected outcome. This loop is a core behavioral model for reliable jelly automation.  
  **Acronyms**: OAV loop  
  **Synonyms**: observe-act-verify, inspect-act-check loop  
  **References**: [Architecture](./ARCHITECTURE.md)

<div align="right"><sub><a href="#top">⭐ Go to the top of the document</a></sub></div>

## P

+ **Primitive**  
  A small, generic browser capability that performs one focused operation, such as clicking, reading a page, waiting for a condition, or switching tabs. Primitives are intended to be composable rather than workflow-specific.  
  **Acronyms**: ∅  
  **Synonyms**: browser primitive, tool  
  **References**: [Tool Index](../.agent/tools/index.md)

+ **Persistent Injection**  
  JavaScript saved by jelly so it can be applied again after navigation initiated through jelly. It is useful for page instrumentation that must survive controlled navigations.  
  **Acronyms**: ∅  
  **Synonyms**: persistent script injection  
  **References**: [Tool Index](../.agent/tools/index.md)

+ **Primitive Registry**  
  The declarative Rust registry that acts as jelly's source of truth for primitive names, descriptions, usage, categories, arguments, validation, schemas, and handlers.  
  **Acronyms**: ∅  
  **Synonyms**: registry, tool registry  
  **References**: [Architecture](./ARCHITECTURE.md)

<div align="right"><sub><a href="#top">⭐ Go to the top of the document</a></sub></div>

## R

+ **Routine**  
  A reusable workflow composed from multiple primitives. Routines keep browser operations organized at a higher level while preserving a shared browser session.  
  **Acronyms**: ∅  
  **Synonyms**: workflow  
  **References**: [Routines](./ROUTINES.md)

+ **RPA**  
  Software automation that performs repetitive user-facing tasks by interacting with applications and interfaces. jelly overlaps with RPA, but is designed specifically around agent-driven inspection, reasoning, and browser control.  
  **Acronyms**: Robotic Process Automation  
  **Synonyms**: process automation  
  **References**: [README](../README.md)

+ **Runtime**  
  The mutable state produced while jelly is operating, including browser profiles, PIDs, logs, screenshots, network captures, and continuation state. Runtime data is intentionally kept outside the repository.  
  **Acronyms**: ∅  
  **Synonyms**: runtime state  
  **References**: [Runtime Layout](./RUNTIME.md)

<div align="right"><sub><a href="#top">⭐ Go to the top of the document</a></sub></div>

## S

+ **Schema**  
  Structured metadata describing a primitive's interface. A schema tells an agent or integration what a tool is called, what it does, which arguments it accepts, and which arguments are required.  
  **Acronyms**: ∅  
  **Synonyms**: tool schema  
  **References**: [Tool Discovery](./DISCOVERY.md)

+ **Semantic Discovery**  
  Finding tools by their meaning and described purpose rather than requiring an exact tool name. jelly's lightweight search uses names, categories, usage, and descriptions to approximate this behavior deterministically.  
  **Acronyms**: ∅  
  **Synonyms**: semantic tool search  
  **References**: [Tool Discovery](./DISCOVERY.md)

+ **Selector**  
  A locator used to identify a browser element. jelly supports stable references, CSS selectors, and exact visible text through its typed Target abstraction.  
  **Acronyms**: ∅  
  **Synonyms**: element locator  
  **References**: [Tool Index](../.agent/tools/index.md)

+ **Side Effect**  
  A tool action that intentionally changes browser or external state rather than only observing it. Examples include clicking, typing, uploading a file, submitting a form, or sending a message.  
  **Acronyms**: ∅  
  **Synonyms**: state-changing operation  
  **References**: [README](../README.md)

+ **Single Source of Truth**  
  A design principle where one authoritative representation owns a piece of information. In jelly, the browser primitive registry owns semantic browser-capability metadata and the system tool registry owns lifecycle/integration metadata. Internal discovery and generated capability documentation consume those registries; the MCP Agent Tool Catalog is a separate validated projection that decides what is remotely published and executable.<br>
  **Acronyms**: SSOT  
  **Synonyms**: canonical source  
  **References**: [Architecture](./ARCHITECTURE.md)

+ **Small Surface**
  Jelly's default MCP publication mode, selected automatically when `JELLY_MCP_SURFACE` is unset or explicitly with `small-surface`. Instead of publishing every browser primitive individually, it exposes the browser facade tools `browser-schema`, `browser-call`, and `browser-events`, while retaining the same current system-tool set as large surface. Raw CDP remains separately opt-in and, when enabled, is exposed through `browser-call`.
  **Acronyms**: ∅
  **Synonyms**: small-surface, browser facade
  **References**: [MCP Server](./MCP.md), [Tool Discovery](./DISCOVERY.md)

+ **Span**  
  One timed operation inside a larger trace. Primitive executions become spans so nested work can be related to the routine or tool invocation that caused it.  
  **Acronyms**: ∅  
  **Synonyms**: trace span  
  **References**: [Development](./DEVELOPMENT.md)

+ **Stable Reference**  
  A jelly identifier assigned during inspection so later actions can refer to the same discovered element without repeating a selector. The default runtime uses document-scoped refs such as `@eabc123-7`; rollback mode uses numeric refs such as `@e7`, and `inspect-images` exposes DOM-backed image refs such as `@img2`.<br>
  **Acronyms**: ∅  
  **Synonyms**: element reference, jelly ref  
  **References**: [Tool Index](../.agent/tools/index.md)

<div align="right"><sub><a href="#top">⭐ Go to the top of the document</a></sub></div>

## T

+ **Stateful Session**  
  An execution model where browser state and the active CDP attachment persist across multiple operations. This allows a workflow to inspect, act, navigate, and continue without recreating browser state for every primitive.  
  **Acronyms**: ∅  
  **Synonyms**: persistent session  
  **References**: [Routines](./ROUTINES.md)

+ **Target**  
  A typed description of the browser element an operation should act on. jelly supports runtime/DOM-backed references, CSS selectors such as `css:#submit`, `text:<exact text>`, and plain exact visible text.<br>
  **Acronyms**: ∅  
  **Synonyms**: element target  
  **References**: [Tool Index](../.agent/tools/index.md)

+ **Tool**  
  A callable capability. Internally Jelly has semantic browser primitives and system/integration tools; the remotely agent-callable MCP set is the selected Agent Tool Catalog projection and may not publish each internal capability as a top-level tool.<br>
  **Acronyms**: ∅  
  **Synonyms**: callable, primitive  
  **References**: [Tool Discovery](./DISCOVERY.md)

+ **Tool Catalog**  
  A validated set of agent-facing tool metadata and bindings. Jelly's MCP Agent Tool Catalog is a publication/execution projection over internal capabilities plus native Agent API builtins; it is distinct from the generated internal capability index.<br>
  **Acronyms**: ∅  
  **Synonyms**: primitive catalog, capability catalog  
  **References**: [Tool Discovery](./DISCOVERY.md)

+ **Tool Search**  
  Selecting relevant capabilities from a larger set based on a task or query. `agent-discover` searches internal registries; small-surface remote agents use `browser-schema` for semantic browser-operation search and `tools/list` for the published MCP surface.
  **Acronyms**: ∅  
  **Synonyms**: tool discovery search  
  **References**: [Tool Discovery](./DISCOVERY.md)

+ **Trace**  
  A structured record connecting related operations across one higher-level action or routine. Traces allow jelly to relate parent operations to child spans for debugging and observability.  
  **Acronyms**: ∅  
  **Synonyms**: execution trace  
  **References**: [Development](./DEVELOPMENT.md)

<div align="right"><sub><a href="#top">⭐ Go to the top of the document</a></sub></div>

## U

+ **User Data Directory**  
  The directory Chromium uses for profile state such as cookies, preferences, cache, local storage, and browser databases. jelly keeps headed and headless profiles in separate runtime directories.  
  **Acronyms**: ∅  
  **Synonyms**: browser profile directory, Chromium profile  
  **References**: [Runtime Layout](./RUNTIME.md)

<div align="right"><sub><a href="#top">⭐ Go to the top of the document</a></sub></div>

## V

+ **Viewport**  
  The currently visible rectangular area of a web page. Pointer coordinates, scrolling behavior, and viewport screenshots are all defined relative to this visible browser area.  
  **Acronyms**: ∅  
  **Synonyms**: visible page area  
  **References**: [Tool Index](../.agent/tools/index.md)

<div align="right"><sub><a href="#top">⭐ Go to the top of the document</a></sub></div>

## W

+ **WebSocket**  
  A persistent bidirectional connection used by CDP. jelly communicates with Chromium's DevTools endpoint over WebSocket connections.  
  **Acronyms**: WS  
  **Synonyms**: WebSocket connection  
  **References**: [Architecture](./ARCHITECTURE.md)

<div align="right"><sub><a href="#top">⭐ Go to the top of the document</a></sub></div>

<div align="center">

<sub>[⭐ Documentation Index](./README.md)</sub>

</div>
