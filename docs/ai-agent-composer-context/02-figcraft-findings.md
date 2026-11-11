# FigCraft findings and implementation starting points

These findings come from source inspection at `be991533` and separate GUI observation. Reopen current definitions before using them; symbols/counts are baseline evidence rather than a promise that the next checkout is identical.

## What can be reused

| Existing capability | Source starting point | Why it matters |
|---|---|---|
| One command vocabulary across UI, CLI, control and MCP | [engine command registry](../../crates/engine/src/cmd.rs) | Agent edits can execute real editor behavior instead of maintaining another scene API. |
| Agent-attributed command execution | [session execution](../../crates/engine/src/session/exec.rs) | Reuse attribution, while recognizing that an agent name is not authorization. |
| Batches, step references and grouped undo | [session undo/batches](../../crates/engine/src/session/undo.rs), [batch automation](../../crates/engine/src/automation/batch.rs) | Coherent edits can share one undo step. |
| Transport-neutral MCP backend | [Backend](../../crates/mcp/src/backend.rs), [tools](../../crates/mcp/src/tools.rs) | Existing headless and attached operations provide real tools and image results. |
| Existing original design workflows | [MCP prompts](../../crates/mcp/src/prompts.rs), [resources](../../crates/mcp/src/resources.rs) | Start from repository guidance and command schemas, not extracted incumbent prompts. |
| Live control and inspection | [control protocol](../control-protocol.md) | Useful for attachment and supplemental E2E assertions, while UI traversal still goes through agent-desktop. |
| Rail with Menu/File/Assets/Variables | [left rail](../../crates/ui-egui/src/panels/layers.rs) | Add AI Chat within the current navigation structure. |
| Theme tokens and native widget styling | [theme](../../crates/ui-egui/src/theme.rs) | Keep panel styling, typography, spacing and interaction states consistent. |
| Dependency/platform checks | [layer checker](../../xtask/src/layers.rs), [quality tasks](../../xtask/src/main.rs) | Account for any new native adapter/module in the existing architecture and WASM gates. |

The audit counted 611 command IDs and 19 MCP tools; runtime catalogs vary by target. Discover the running schemas rather than sending every command definition on every turn. Inspection already supports bounded/selected fields and outlines; reuse these for relevant context.

## Critical gaps and limitations

1. **No integrated harness composer at the audit baseline.** A model picker, actual process/session integration and conversation/task UI must be implemented. Existing Cursor chat/menu names do not establish this feature.
2. **Task-scoped review is missing.** Document-global undo is not safe rejection after interleaved human edits. Existing persistent document snapshots/diffs are useful primitives, not a completed task inverse/conflict model.
3. **Rollback is document-oriented.** Batches do not roll back all file writes, clipboard, preferences or external effects. Keep such actions outside canvas-task authority.
4. **Attribution is incomplete across input paths.** Command attribution does not automatically cover pointer/key/text automation. Deny uncontrolled bypass routes for scoped design tasks or implement equivalent enforcement before exposing them.
5. **Change notifications are incomplete.** The event log primarily carries comments; an accepted document topic does not prove a useful document-change stream. Verify revisions/read back state when needed.
6. **MCP progress/cancellation is not a finished task protocol.** Existing ignored notifications do not implement harness interruption, tool admission revocation or UI progress.
7. **Loopback is not task authorization.** The existing control endpoint is unauthenticated automation access. A scoped wrapper cannot leave the same agent an unrestricted bypass to the app.
8. **Attached tests are not live-window E2E.** Fake-app attachment tests do not prove the actual composer, running app and real model work together.

## What the GUI probe established

On the separately installed FigCraft 0.17.0 binary, a fresh synthetic design exercised frame/child creation, simple auto layout, component creation appearing in Assets, a variable collection/color row, generated CSS and a local anchored comment. No integrated agent/provider/browser entry point was found in the bounded inspected UI. A real model-driven edit was not attempted.

That binary was not matched to the source audit commit. Build the candidate you intend to review and record its identity. An initial sparse accessibility tree became richer after interaction; sparse AX output alone is not proof that the app has no accessible controls.

## Existing test patterns to inspect

- Engine batch rollback and step-reference tests for native state/undo invariants.
- UI input-frame/accessibility tests for navigation, keyboard/focus and panel geometry.
- Control capture/document/root tests for correlation, timeouts and target selection.
- Retained-render comparisons for rendering correctness.
- Existing MCP agent-task tests for structured tool behavior, supplemented by real-window review.

Detailed source evidence: [AI-readiness audit](../figcraft-ai-readiness-audit.md). Observed desktop workflow: [FigCraft/Pen comparison](../figcraft-pen-blackbox-comparison.md). Historical runtime recommendations in the audit are superseded by the owner's [local-harness decision](01-product-decisions.md).
