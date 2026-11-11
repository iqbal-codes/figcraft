# Mandatory review-session desktop E2E

The owner explicitly requires review through **agent-desktop with real AI**, including actual canvas changes and inspection of both editor UI and generated design. This gate applies to every ticket's relevant behavior and to the complete feature at final review. These instructions describe future review; no such implementation review has run yet.

## 1. Ready — bind the review candidate

Build/launch the actual candidate and record commit, dirty-state scope, application version/platform and CLI/model identity. The previously explored installed FigCraft 0.17.0 is not a substitute for a fresh identified candidate. Read agent-desktop's current skill/help and inspect permissions before interaction. Use a new synthetic document and original reference material; preserve the user's unrelated documents/settings.

Check that the real configured harness/model is usable under the applicable authorization. Missing build, permissions, auth or model makes the affected scenario blocked/unverified, not passed. Controlled subprocess fixtures remain useful for rare protocol failures but do not replace real AI design execution.

**Ready is complete when:** candidate/build, platform, harness/model, synthetic input and desktop permissions are recorded, and the ticket's applicable acceptance scenarios are selected. Keep a required scenario open when its prerequisite is unavailable.

## 2. Exercise — operate the real workflow

| Scenario | User-facing action | Evidence to inspect |
|---|---|---|
| Workspace/readiness | Open AI Chat from the rail; inspect setup/model state | Correct rail order/two wrappers; File/Assets/Menu/Variables preserved; right inspector usable |
| Native generation | Type/send an original brief from the visible composer | Real model/tool activity; new native frame with editable text/shapes/layout; actual result IDs |
| Scoped revision | Select an existing frame and request a concrete change | Requested properties changed; siblings and bindings preserved; tab/selection switches do not retarget |
| Reference/design system | Attach original image/text and request named resources | Actual attachment input; real component instances/variable bindings; missing resources explained |
| Visual correction | Ask to fix known clipping/overflow or hierarchy | Actual rendered image reaches a vision-capable model; corrected output re-rendered; limitations truthful |
| Navigation | Switch AI Chat/File/Assets, Menu/Variables, Sessions/conversation; resize/collapse | Draft/attachments/model preserved; correct focus/IME; Sessions nested; reachable task-specific Stop |
| Interruption | Stop during editing and while a result settles | No new mutation admitted; retained edits and uncertain outcomes reconciled; no duplicate replay |
| Task review | Interleave a human edit, accept/reject and exercise an overlap | Accurate changed objects; human work retained; explicit conflict resolution and revalidation |
| Sessions/restart | New/resume conversation, restart and explicitly resume/review | Correct thread/document; bounded history; interrupted state; no automatic inference or edit replay |
| Regression | Continue drawing/inspecting, undo/redo, save/reopen; inspect theme/width/scaling | Native relationships survive; editor remains responsive; supported platform behavior remains intact |

Perform UI actions through agent-desktop. Supplemental engine/MCP/control reads can prove native properties; they cannot substitute for typing into and operating the actual composer. Read back state/screenshots after actions; a reported click delivery alone does not prove its effect. Use semantic accessibility operations where supported and follow the tool's authorization requirements for physical input.

**Exercise is complete when:** each selected scenario has recorded actions and observed results, including native document assertions where applicable. Screenshots, tool replies and model summaries are supporting observations; evaluate them in the inspection step before assigning a pass.

## 3. Inspect — assess editor and design separately

**Editor workspace:** layout follows the approved rail/panel structure, controls remain reachable, typography is readable, state/focus/selection are clear, resizing/scaling/narrow widths do not hide critical actions and loading/error/permission states are honest.

**Generated design:** result follows the brief/reference direction, has a readable hierarchy, coherent type and spacing/alignment, meaningful requested content, no unintended clipping/overlap and genuine requested component/variable reuse. Verify native editability and save/reopen separately. A model's summary or screenshot existence alone is not visual acceptance.

Use a vision-capable reviewer/model where visual interpretation is claimed. For a non-vision product model, explicitly mark its visual verification unavailable while still inspecting the user preview in review.

**Inspection is complete when:** the relevant window screenshots and affected-frame renders have been opened, both visual checks have findings, native editability/save-reopen has been checked where required, and defects or verification limits are named. A captured image that nobody inspected leaves the visual check unverified.

## 4. Record — leave reproducible evidence

Record these fields per applicable scenario:

- Candidate commit/build, worktree scope, platform, app/CLI/model/capabilities.
- Synthetic document and exact original brief/reference identity.
- Desktop actions, expected behavior and observed behavior.
- Before/after window screenshots and affected-frame renders that were actually opened and inspected.
- Native document assertions, changed object IDs and save/reopen outcome where relevant.
- Result: passed, failed or blocked/unverified; concrete defect/blocker and reproduction steps.
- Fix candidate and rerun result for affected scenarios. Evidence from an earlier build is not evidence for a changed candidate.

Keep incumbent screenshots and private history/source material in ignored local research. Share only original synthetic evidence that can be redistributed; keep credentials and unrelated user content out. Store the evidence in the review's ordinary artifact location and link it from the ticket/PR report rather than committing product screenshots as application assets.

**Evidence is complete when:** every selected scenario has a result, candidate identity, observed behavior and supporting artifacts or a concrete blocker. Fixes require evidence from the revised candidate and a rerun of the affected scenarios.

## 5. Decide — close only verified work

Run the ticket's required repository checks and its applicable real-desktop scenarios. Mark a ticket complete only when its acceptance and review evidence pass, or report specifically what remains incomplete. Preserve the distinction between unit/protocol checks, real-model behavior, native document correctness and visible quality.

For final ticket 04, run the combined Codex scenario set and independently verify the supported Claude workflow. Codex evidence cannot establish Claude parity. No extra implementation ticket is needed merely to file the review checklist; defects belong to the relevant one of the approved four tickets.

Authoritative scenarios: [feature spec AT01–AT12](../ai-agent-composer-spec.md). Task navigation and scope context: [workspace behavior](04-workspace-behavior.md), [execution architecture](05-execution-architecture.md).
