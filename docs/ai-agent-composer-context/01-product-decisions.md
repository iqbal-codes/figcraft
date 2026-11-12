# Product intent, decisions and boundaries

## Outcome and rationale

The owner wants to stay inside FigCraft while an agent turns a brief into native editable UI, then inspect and continue editing the result. Existing document/layout/component/variable operations make that possible; the missing product behavior is the integrated composer-to-tool-to-canvas workflow.

The discussion began with broader Pen-like AI and browser capabilities, then narrowed to the composer first. The [feature spec](../ai-agent-composer-spec.md) owns the behavioral contract. This document records why its decisions were made.

## Accepted decisions and why they matter

| Choice | Reason / implementation consequence |
|---|---|
| Rust/egui with the existing engine | Reuse editor behavior and one authoritative mutation path. |
| Installed Codex first; Claude Code next | Reuse harness authentication, context management and the model/tool loop. Complete one workflow before supporting a second protocol. A local harness may call a cloud model. |
| FigCraft's own two-wrapper navigation | The owner corrected the early right-side drawer and later rejected copying Pen's top-level mode arrangement. Follow [workspace behavior](04-workspace-behavior.md) for the rail, nested Sessions, persistent composer and inspector. |
| Native editable output and task-owned review | Continue manual editing and reject agent changes while retaining unrelated human work. Snapshots/diffs are primitives, not proof that task rejection is safe. |
| Four outcome-based tickets | The owner reduced 13 drafts to four to limit feature drift. Implementation subtasks stay inside those tickets. |
| Real-AI desktop E2E in review sessions | Completion needs actual composer interaction, native results and inspected editor/design images. Follow the [review procedure](06-desktop-e2e-review.md). |

Research-worker provider/model settings apply to investigation workers only. Product model choice comes from the selected installed harness's supported capabilities.

## Open implementation choices

- A Rust harness supervisor above the engine is the recommended seam. New crate/module names remain proposals; choose them from current repository conventions and actual consumers.
- Codex app-server stdio is the verified public integration direction. Confirm schemas and capabilities against the installed supported executable during implementation.
- Original compact design guidance should supplement existing FigCraft MCP workflows. Exact wording/packaging and its effect on visual quality remain untested.
- History uses bounded app-state persistence. Exact retention values and lifecycle mechanics need an implementation choice consistent with the spec and existing storage conventions.

When choosing modules or transport, read [execution architecture](05-execution-architecture.md). When a proposal conflicts with a confirmed outcome, resolve that specific conflict; accepted UI/harness choices are not an invitation to restart product discovery.

## Deferred ideas

Browser navigation/capture/editable reconstruction, parallel agents, repeated design alternatives, arbitrary repository writing, deployment, additional MCP integrations and broader design-to-code automation remain future work. Their appearance in research does not create dependencies for these four tickets.

A repository-specific egui development skill was also discussed. The owner clarified that this spec concerns the composer product feature; creating that developer skill is separate work.

## Implementation alignment

Use local harnesses for model communication and their agent loop, existing native commands for edits, FigCraft's selected navigation pattern for presentation and candidate-bound evidence for completion. Keep scope enforcement and interruption safety in the first mutating ticket. Generated code/raster output alone cannot satisfy the native-editability contract.

Supporting sources: [source findings](02-figcraft-findings.md), [reference research](03-reference-research.md), [architecture draft](../ai-workspace-architecture.md). The last document proposes structure; it does not establish an implemented module or a passing test.
