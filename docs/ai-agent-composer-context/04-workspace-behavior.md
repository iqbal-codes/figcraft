# Workspace and interaction rationale

## The layout the owner chose

In the default left-to-right workspace, the far-left narrow rail and the content panel beside it remain separate wrappers. The canvas stays central and the property inspector stays on the right. Preserve the existing supported mirrored-layout behavior.

| Rail item | Behavior |
|---|---|
| Menu | Existing main-menu popup; preserve the underlying panel and draft. |
| AI Chat | Agent panel with conversation and Sessions subviews. |
| File | Existing pages/layer tree and navigation. Keep the label File. |
| Assets | Existing components/assets and supported library access. |
| Variables | Existing variable editor. Source currently opens this separately; moving it into a new tab is not required. |

The owner's screenshot illustrates a left-side conversation area with a bottom composer and an independent inspector. Its role is a behavioral/layout reference. Create original FigCraft controls using existing theme tokens and native widgets; incumbent imagery/iconography is not an asset source.

## AI Chat and Sessions

Conversation view contains text, actual tool activity, run status, permission/input requests and task review entry points. Sessions is reached from within AI Chat. It shows identifiable conversations, real run status, New and Resume. Merely viewing the list does not switch the harness thread or launch inference.

Selecting a session restores the matching conversation; revalidate its document/scope before another request. A session shown in the panel and an already running task are distinct identities. Viewing a different session cannot redirect the active task. The initial contract admits one mutating run per document.

## The composer while navigating

The bottom composer stays available in expanded non-modal AI Chat/File/Assets panel views, including the Sessions subview. Preserve unsent text, attachments, model/harness choice, context and conversation identity. Retain each panel's scroll/search/expanded-tree state. Menu/Variables overlays may cover content but must not discard that state.

Selection changes can update an unsent draft's visible context. Sending binds the task to the displayed document/page/target IDs. Later selection or tab changes do not broaden or redirect it. Sending from File/Assets activates the matching AI Chat conversation while retaining the previous panel's navigation state.

Enter submits, Shift+Enter inserts a newline and active IME composition is respected. Prevent duplicate submission while a turn runs. A user may draft a follow-up; it is not automatically submitted when a previous turn finishes.

Collapsing the panel or switching modes does not stop or start a run. Provide a reachable activity/Stop entry when the composer is hidden, clearly associated with the task it affects. Restore focus appropriately and preserve the existing keyboard/shortcut/accessibility behavior.

## Empty, active and failed states

- Empty: show the chosen target and actionable harness setup/readiness. Opening the panel sends no model request.
- Running: display actual streamed text/tool outcomes, responsive controls and Stop. Partial tool arguments are not committed canvas changes.
- Needs input/permission: show the real action/target and supported answer controls. Unsupported interactions fail visibly.
- Failed/interrupted/unknown: identify committed edits and incomplete verification; avoid pretending interruption reversed the document.
- Reviewable: summarize task-owned changes and offer supported Accept/Reject/conflict resolution. Harness completion does not equal user acceptance.

## Original design-quality workflow

A useful synthetic brief states the screen's purpose, intended platform/frame size, content, reference direction, required components/variables and constraints such as preserving siblings. For example: design a compact operations dashboard with a clear primary action and readable status hierarchy using the document's existing button and color variables. That example is an evaluation input, not the default design for every user.

The agent should inspect current context, choose a coherent hierarchy/type/spacing/layout approach, create native structure, render and correct visible issues. User-selected direction and actual resources lead; a fixed palette/grid/card recipe should not dominate unrelated requests.

Evaluate the editor UI and generated design separately. A beautiful generated frame cannot compensate for lost draft text or a broken Stop control; a polished composer cannot substitute for native editable output.

Detailed acceptance remains in the [spec](../ai-agent-composer-spec.md), especially AT01, AT02–AT05 and AT12. Review procedure: [desktop E2E](06-desktop-e2e-review.md).
