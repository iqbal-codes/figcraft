# AI Agent composer: session entry point

This pack preserves the reasoning and research behind the approved composer feature. The feature is specified, not implemented or E2E-verified by this handoff. Begin with the ticket whose blockers are complete.

## 1. Ready — establish the task

Read the repository agent instructions and required roadmap/gap context, then [product decisions](01-product-decisions.md), [workspace behavior](04-workspace-behavior.md), the [feature spec](../ai-agent-composer-spec.md) and the selected [ticket](../../.scratch/ai-agent-composer/README.md). Preserve the user's unrelated worktree changes.

**Ready is complete when:** you have identified the candidate revision/worktree, the ticket outcome, its blocker dispositions and the accepted UI/harness constraints. A blocker without completion evidence stays open. Follow the routing table below for the work you will touch.

## Reference routing

| Trigger | Read before acting |
|---|---|
| Commands, MCP, document state, undo or test seams | [FigCraft findings](02-figcraft-findings.md); reopen the linked current definitions |
| Pen-derived claims, design inspiration or earlier investigation conclusions | [Reference research](03-reference-research.md); preserve its evidence/unknown distinction |
| Processes, session identity, tool admission, context, image transport or task review | [Execution architecture](05-execution-architecture.md) |
| Review session or any ticket-completion claim | [Desktop E2E procedure](06-desktop-e2e-review.md); complete its evidence gates |

The spec owns behavior; tickets own outcome grouping and blockers; this pack owns rationale and research interpretation. An older audit proposal does not override an accepted decision. Resolve a concrete contradiction against the owner's latest direction and record the resulting choice.

## 2. Implement — deliver the ticket outcome

Use current repository patterns and the ticket's acceptance criteria. Keep internal implementation steps inside the approved four tickets. Baseline scope enforcement, honest state and Stop safety belong with the first mutating workflow.

**Implementation is complete when:** every applicable ticket behavior is implemented, the relevant repository checks have actually run and their results are recorded. This permits review; it does not establish real-model or visual acceptance.

## 3. Review — exercise the candidate

Follow the [desktop E2E procedure](06-desktop-e2e-review.md) through the actual composer using agent-desktop and real AI. Inspect native document results, editor screenshots and generated-design screenshots. Fix defects and rerun affected scenarios on the revised candidate.

**Review is complete when:** every applicable acceptance scenario has candidate-bound passing evidence. Report failed/blocked/unverified scenarios explicitly; leave completion open while any required scenario lacks evidence. Final ticket 04 covers the combined Codex workflow and independent supported Claude behavior.

## Ticket frontier

| Ticket | Outcome | Blocked by |
|---|---|---|
| [01](../../.scratch/ai-agent-composer/issues/01-ai-chat-codex-live-canvas.md) | Codex creates/revises real editable canvas designs from AI Chat | None |
| [02](../../.scratch/ai-agent-composer/issues/02-reference-guided-visual-design.md) | References, component/variable reuse and visual refinement | 01 |
| [03](../../.scratch/ai-agent-composer/issues/03-sessions-review-recovery.md) | Sessions, Stop, conflict-aware review and recovery | 01 |
| [04](../../.scratch/ai-agent-composer/issues/04-claude-and-complete-workflow.md) | Claude adapter and complete-workflow validation | 02, 03 |

Initial frontier: 01. Read actual completion evidence before advancing it. `ready-for-agent` means a ticket is specified; implementation/review status is separate.

## Handoff prompt

> Implement ticket 01 from the AI Agent composer local tracker. Begin with the context pack's Ready gate; read the routed references for the work you touch. Deliver the complete ticket outcome, then run its desktop E2E review with agent-desktop and real AI. Leave candidate-bound evidence and unresolved blockers. Preserve the four-ticket scope.

Change the ticket number only when its blockers are complete. Source/binary provenance and unverified behavior are documented in [FigCraft findings](02-figcraft-findings.md) and [reference research](03-reference-research.md); this handoff is not evidence of a passing implementation.

## Local handoff availability

The [local tracker](../../.scratch/ai-agent-composer/README.md) records publication state. These artifacts have not been committed/pushed by the handoff. Another session in this workspace can read them directly. For a different checkout, transfer this pack, its linked spec/research/architecture documents and four ticket files with the repository layout intact. Keep ignored incumbent screenshots/source-derived notes and unrelated user work out of the transfer.
