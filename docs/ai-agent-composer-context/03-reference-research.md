# Reference research: what Pen taught us

The research question was how a design editor connects natural-language work, native canvas operations and visual feedback. The useful result is a methodology and workflow inventory; it is not a requirement to clone Pen's implementation or visual assets.

## Evidence classes

| Evidence | Established | Did not establish |
|---|---|---|
| Official Pen documentation and advertised tool schemas | State/design-guidance/style/execution tools; integrated composer and external MCP workflows | Actual invocation success or design quality in this installation |
| Direct agent-desktop observation of Pen 1.2.22 | Visible UI, mode switching and a small synthetic browser/design probe | Live model generation, cancellation reliability, parallel isolation or broad import fidelity |
| Explicitly requested selected ASAR inspection | Static harness adapters and selected context/tool configuration paths | Runtime execution, policy enforcement, exact full context/token behavior or quality guarantees |

The original public/black-box reports remain separate from the later source-derived research. Proprietary source/prompt text and screenshots are not included in this context pack. Original public documentation and FigCraft source support implementation decisions independently.

## Observed workspace

Pen's left sidebar exposes Chat, Chat history, Layers, Slides, Components and Libraries. Switching modes changed the upper content while a lower composer retained its visible object chip/model controls. The canvas and right inspector remained present. Chat history showed recency grouping; existing conversations were not opened or deleted during the follow-up probe.

This confirmed the value of a persistent composition surface while inspecting designs. The owner then specified FigCraft's own rail: Menu, AI Chat, File, Assets, Variables. Follow that correction. Sessions is inside AI Chat, not a direct copy of Pen's top-level switcher.

Typed-draft persistence, active-run continuity, history resumption and collapse/reopen were not verified in Pen. They are explicit FigCraft requirements with their own tests, not inherited evidence.

## Exercised context/browser behavior

An original local HTML fixture loaded in Browser View. Full-page capture created a raster object; reproduce/import created a separate root whose inspector exposed layout properties. Selecting that root produced a composer context chip. Coding Mode changed workspace presentation and could be exited.

These observations distinguish capture, structural import and context attachment. Individual imported-child editability and general website reconstruction were not established. Browser work remains outside the first composer tickets.

## How tool-based design is assembled

Public Pen tooling separates application state, design instructions, style reference values and execution. Its documentation explains that skills supply instructions while MCP tools read/change documents. The later selected static inspection showed application-owned adapter/configuration work around existing Claude, Codex, Cursor and Pi harnesses: available tools are filtered, host/conversation context is bound, guidance/context are prepared and events are mapped to the editor.

The static inspected Codex integration used its SDK thread/stream interface; that does not establish that Pen uses app-server. FigCraft's recommended app-server integration comes from Codex's own public UI-integration interface, fitting a Rust subprocess client. Pen also contains bundled-runtime support; our owner chose user-installed CLIs instead.

| Reference capability | Independent FigCraft approach |
|---|---|
| Current application state | Bounded document/selection/revision inspection |
| Design instructions/skills | Original guidance plus existing FigCraft MCP workflows/resources |
| Style references | Explicit brief and actual document fonts/styles/components/variables |
| Canvas execution | Existing typed native tools and engine commands, bounded batches |
| Screenshot feedback | Actual rendered image in a vision-capable model's tool/input context |
| Host integration | Rust session supervisor, task-bound tool admission and real UI events |

A broad tool catalog alone does not produce attractive UI. The proposed quality loop combines a clear brief, relevant design-system context, useful native operations, rendering and critique. It remains a hypothesis to evaluate on real synthetic design tasks.

## Sources and unresolved questions

- [Public research report](../pen-dev-public-research.md): documented behavior and boundaries.
- [Black-box observation](../pen-dev-blackbox-observation.md): exercised versus inspected UI behavior, including the left-sidebar follow-up.
- [Desktop comparison](../figcraft-pen-blackbox-comparison.md): FigCraft behavior and bounded gaps.
- Official references, consulted 2026-11-11: [Pen AI integration](https://docs.pen.dev/getting-started/ai-integration), [agents and skills](https://docs.pen.dev/core-concepts/ai-agents).

Unknowns include actual runtime tool enforcement, live auth precedence, model-specific quality, retry/cancellation races and parallel orchestration. The original disconnected MCP attempts are historical failures, not evidence that the app remained unavailable after the successful GUI probes. No real-model quality benchmark has been completed.
