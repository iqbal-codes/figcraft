# Execution architecture and reasoning

## Responsibility split

| Owner | Responsibilities |
|---|---|
| Local Codex/Claude harness | Model communication, authentication, conversation/context management and the agent tool loop |
| FigCraft host | Target binding, tool authority, native mutation results, task provenance, review and UI presentation |
| Existing engine | Authoritative commands, document/layout relationships, grouped undo and rendering inputs |
| Reviewer | Operate the actual candidate and assess behavior, native correctness and visible result quality |

Keep model transport out of document/layout/render crates. A small native Rust supervisor should hide process/session mechanics and yield bounded events to egui without blocking frames. Names and exact module/crate placement remain design proposals; account for any new dependency in the layer/WASM checks.

## One design turn

1. Capture the displayed document UID, page and selected subtree, or explicit create-new-root page target. Bind authority at the host.
2. Provide a compact revisioned snapshot, relevant fonts/component/variable references and explicit user attachments. Keep unrelated content out and report truncation.
3. Verify the configured FigCraft MCP endpoint is available to the harness. Explicitly supply original bootstrap guidance; automatic loading of all MCP prompts/resources is not guaranteed.
4. Let the harness inspect command schemas and request bounded native edits. Validate actual effects at live-app admission, including nested batches and equivalent generic/input routes.
5. Execute through the existing attributed command path, record task-owned changes and return actual IDs/errors. Only committed state updates the visible canvas.
6. Inspect structure, render the target and send actual image content to a vision-capable model for bounded refinement. Expose unsupported visual capability honestly.
7. Present the changes and verification status for review. Keep user acceptance separate from harness completion.

Use the same operation vocabulary for external MCP and in-app harness tools. Existing guidance is a starting point, not a reason to maintain a second registry or to expose arbitrary JavaScript execution.

## Context and guidance

Distinguish application-supplied instruction, user brief and document/attachment data. Only the host can confer tool authority. Supply enough design guidance to use native layout, hierarchy and real component/variable references; make deeper schemas/guidance discoverable as needed rather than loading the entire registry each turn.

Tool outputs need bounded useful errors and stable object identities. Preserve adapter request/turn/thread IDs separately from FigCraft task/document IDs. Model/effort/image/approval/subagent features vary by harness; capability checks should prevent silent translations or fake parity.

## Mutation, Stop and review

An agent name is attribution, not authorization. Enforce task authority at the actual operation site and eliminate unscoped bypasses to the same session. Canvas scope does not grant arbitrary disk, clipboard, shell or network effects. Prefer a useful bounded operation set over a broad interface whose effects cannot be checked.

One coherent bounded batch is one undo group. A whole turn may contain multiple groups. Completed groups remain after Stop; failed document batches roll back their own document state. Disk/external side effects are not transactionally reversed by that mechanism.

Stop closes admission to new task mutations, requests the harness's supported interruption and reconciles any in-flight result. Process shutdown and turn interruption are separate operations. Inspect uncertain outcomes before retrying; a lost reply can still follow a committed edit.

Record task provenance from the first edit, including before/after property values and ownership/dependencies needed for review. Accept records disposition without reapplying edits. Reject applies safe task inverses, preserving unrelated manual work; overlaps/dependent references produce explicit conflicts requiring revalidation and user choice.

## Sessions, recovery and platform constraints

Store bounded display/task state and harness references atomically in app state, separately from shared design contents and credentials. Resume only explicitly and revalidate targets. On restart, nonterminal or unknown work becomes interrupted/uncertain; no inference request or edit replay is automatic.

The native adapter can launch an installed process. The WASM build cannot claim that capability; preserve the web editor and report the unavailable integration. A browser bridge is separate future work.

Codex-first integration uses the public app-server stdio interface. Claude-next uses its verified supported programmatic CLI mechanisms, with separate evidence for image transport, approvals and interruption. Validate exact schemas against the installed version. References: [Codex app-server](https://developers.openai.com/codex/app-server), [Claude programmatic interface](https://code.claude.com/docs/en/headless), consulted 2026-11-11.

Supporting proposal: [architecture draft](../ai-workspace-architecture.md). Existing seams and pitfalls: [FigCraft findings](02-figcraft-findings.md). The four-ticket contract remains authoritative if an older architecture/audit suggestion conflicts.
