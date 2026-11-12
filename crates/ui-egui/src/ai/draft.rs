//! The AI composer's draft: its text, the objects it mentions, and its undo history.
//!
//! A mention stands for an object the user clicked on the canvas. It shows as `@Name`, and its
//! authority comes only from the metadata recorded at that click: the label is display text, so
//! typing the same characters never grants access to anything, and renaming the layer afterwards
//! does not retarget the mention.
//!
//! Spans are character indices, never byte offsets, because every editor position egui reports is
//! a character index and a draft holds non-ASCII text.

use std::ops::Range;

use egui::text::{CCursor, CCursorRange, CharIndex};
use egui::{Color32, FontId, TextBuffer};
use figcraft_doc::NodeId;
use serde::{Deserialize, Serialize};

/// How many undo steps the draft keeps.
const MAX_HISTORY: usize = 100;

/// An object the draft mentions, and where its token sits in the draft text.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObjectMention {
    /// First character of the token (`@`).
    pub start_char: usize,
    /// One past the token's last character.
    pub end_char: usize,
    /// The document the object was in when it was mentioned.
    pub doc_uid: u64,
    /// The page the object was on when it was mentioned.
    pub page_id: NodeId,
    /// The object itself.
    pub node_id: NodeId,
    /// The object's name when it was mentioned, so the token reads the same if the layer is
    /// renamed later.
    pub label: String,
}

impl ObjectMention {
    /// The mention for `node_id`, its label as displayed and its token not yet placed in a draft.
    pub fn new(doc_uid: u64, page_id: NodeId, node_id: NodeId, label: impl Into<String>) -> Self {
        Self { start_char: 0, end_char: 0, doc_uid, page_id, node_id, label: label.into() }
    }

    /// The characters that stand for the object in the draft.
    pub fn token(&self) -> String {
        format!("@{}", self.label)
    }

    /// How many characters the token takes.
    pub fn char_len(&self) -> usize {
        1 + self.label.chars().count()
    }

    /// The token's character range.
    pub fn range(&self) -> Range<usize> {
        self.start_char..self.end_char
    }

    /// Whether `at` is strictly inside the token: both edges stay placeable by a caret.
    fn contains(&self, at: usize) -> bool {
        self.start_char < at && at < self.end_char
    }

    /// Whether a deletion of `range` touches the token, so it goes as a whole.
    fn intersects(&self, range: &Range<usize>) -> bool {
        self.start_char < range.end && range.start < self.end_char
    }
}

/// The draft at one moment: the text, the objects it mentions, and the caret.
#[derive(Clone, Debug, Default, PartialEq)]
struct Snapshot {
    text: String,
    mentions: Vec<ObjectMention>,
    /// The selection in characters, normalized so the first is the start.
    selection: (usize, usize),
}

impl Snapshot {
    fn of(text: &str, mentions: &[ObjectMention], selection: (usize, usize)) -> Self {
        Self { text: text.to_owned(), mentions: mentions.to_vec(), selection }
    }
}

/// The draft's objects, caret and history, kept beside its text in [`super::AiState::draft_text`].
#[derive(Debug, Default)]
pub struct DraftMetadata {
    mentions: Vec<ObjectMention>,
    selection: (usize, usize),
    /// The draft as committed: the preedit-free text with its spans and caret as of the last frame
    /// that ended an edit. A running IME composition leaves it alone, so one that is cancelled
    /// restores exactly, and one that is committed is a single history entry.
    committed: Snapshot,
    /// An IME composition is running: its text is in the buffer but is not part of the draft.
    composing: bool,
    /// The next editor sync must replace the editor state rather than adjust it, because a
    /// cancelled composition left a preedit range behind that would otherwise delete real text.
    reset_editor: bool,
    /// Why the draft cannot be sent, shown under it until the next edit.
    pub error: Option<String>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
}

impl DraftMetadata {
    /// The objects the draft mentions, in token order.
    pub fn mentions(&self) -> &[ObjectMention] {
        &self.mentions
    }

    /// The remembered selection, normalized.
    pub fn selection(&self) -> (usize, usize) {
        self.selection
    }

    /// Remember the selection the editor has, normalized.
    pub fn set_selection(&mut self, start: usize, end: usize) {
        self.selection = (start.min(end), start.max(end));
    }

    /// Whether an IME composition is running, whose text is not part of the draft.
    pub fn composing(&self) -> bool {
        self.composing
    }

    /// Tell the draft whether an IME composition is running this frame. A composition ends with
    /// its commit or with an empty preedit, and its text never becomes part of the draft before.
    pub fn set_composing(&mut self, composing: bool) {
        self.composing = composing;
    }

    /// Whether the draft text is mid-composition, so submitting would send transient text.
    pub fn is_composing(&self) -> bool {
        self.composing
    }

    /// Whether the next editor sync must replace the editor state instead of adjusting it.
    pub fn take_editor_reset(&mut self) -> bool {
        std::mem::take(&mut self.reset_editor)
    }

    /// Drop a running IME composition and restore the committed draft.
    fn cancel_composition(&mut self, target: &mut String) {
        if !self.composing {
            return;
        }
        self.composing = false;
        self.restore(self.committed.clone(), target);
    }

    /// Replace the draft with plain text: no mention survives and the history starts over.
    ///
    /// Typed text never becomes a mention, so this cannot be used to grant authority over an
    /// object.
    pub fn replace_text(&mut self, target: &mut String, text: &str) {
        self.composing = false;
        self.error = None;
        self.mentions.clear();
        self.undo.clear();
        self.redo.clear();
        *target = text.to_owned();
        self.selection = (target.chars().count(), target.chars().count());
        self.committed = Snapshot::of(target, &self.mentions, self.selection);
        self.reset_editor = true;
    }

    /// Insert `mention`'s token where the remembered caret is, replacing the selection, as one
    /// history entry. Nothing about the object is authorized here; this is the draft only.
    pub fn insert_mention(&mut self, target: &mut String, mention: ObjectMention) {
        self.cancel_composition(target);
        self.error = None;

        let before = self.committed.clone();
        let (start, end) = self.selection;
        let total = target.chars().count();
        let start = start.min(total);
        let end = end.min(total).max(start);
        // A selection that reaches into a token takes the whole token, so no half-token is left.
        let (start, end) = expand_deletion(&self.mentions, start, end);
        if start < end {
            self.delete_range(target, start, end);
            self.selection = (start, start);
        }

        // Keep the token from running into neighbouring text; nothing else is added.
        let lead = start > 0 && !char_at(target, start - 1).is_some_and(char::is_whitespace);
        let trail = char_at(target, start).is_some_and(|c| !c.is_whitespace());
        let mut insert = String::with_capacity(mention.label.len() + 3);
        if lead {
            insert.push(' ');
        }
        insert.push_str(&mention.token());
        if trail {
            insert.push(' ');
        }
        let inserted = self.insert_str(target, &insert, start);

        let mut placed = mention;
        placed.start_char = start + usize::from(lead);
        placed.end_char = placed.start_char + placed.char_len();
        self.mentions.push(placed);
        self.mentions.sort_by_key(|m| m.start_char);

        let caret = start + inserted;
        self.selection = (caret, caret);
        self.committed = Snapshot::of(target, &self.mentions, self.selection);
        self.push_undo(before);
    }

    /// End a composer frame: a frame that changed the draft is one history entry.
    pub fn end_frame(&mut self, target: &str) {
        if self.composing {
            // The composition's text is transient, so the draft it started from stays committed.
            return;
        }
        if self.committed.text == target && self.committed.mentions == self.mentions {
            // Only the caret moved.
            self.committed.selection = self.selection;
            return;
        }
        let before = self.committed.clone();
        self.committed = Snapshot::of(target, &self.mentions, self.selection);
        self.push_undo(before);
    }

    /// Undo the last draft change, restoring its text, tokens and caret together.
    pub fn undo(&mut self, target: &mut String) -> bool {
        let Some(previous) = self.undo.pop() else { return false };
        let current = self.committed.clone();
        self.restore(previous, target);
        self.redo.push(current);
        true
    }

    /// Redo the last undone draft change.
    pub fn redo(&mut self, target: &mut String) -> bool {
        let Some(next) = self.redo.pop() else { return false };
        let current = self.committed.clone();
        self.restore(next, target);
        self.undo.push(current);
        true
    }

    /// `start..end` with both ends moved out of any token they sit inside, so an editor never
    /// shows a caret in the middle of a name. A single caret picks the nearer edge.
    pub fn snap_range(&self, start: usize, end: usize) -> (usize, usize) {
        let (start, end) = (start.min(end), start.max(end));
        if start == end {
            let at = self.snap_caret(start);
            return (at, at);
        }
        let mut start = start;
        let mut end = end;
        for mention in &self.mentions {
            if mention.contains(start) {
                start = mention.start_char;
            }
            if mention.contains(end) {
                end = mention.end_char;
            }
        }
        (start, end)
    }

    /// The character the caret would sit at nearest to `at`: a caret never sits inside a token.
    fn snap_caret(&self, at: usize) -> usize {
        for mention in &self.mentions {
            if mention.contains(at) {
                // Ties choose the end, so the caret leaves the token it was closest to.
                return if at - mention.start_char < mention.end_char - at { mention.start_char } else { mention.end_char };
            }
        }
        at
    }

    /// Insert `text` at `at`, snapping out of a token. Returns the characters inserted.
    fn insert_str(&mut self, target: &mut String, text: &str, at: usize) -> usize {
        self.error = None;
        let count = text.chars().count();
        if count == 0 {
            return 0;
        }
        let total = target.chars().count();
        let at = self.snap_caret(at.min(total));
        let byte = char_to_byte(target, at);
        target.insert_str(byte, text);
        for mention in &mut self.mentions {
            if mention.start_char >= at {
                mention.start_char += count;
                mention.end_char += count;
            }
        }
        let shift = |v: usize| if v >= at { v + count } else { v };
        self.selection = (shift(self.selection.0), shift(self.selection.1));
        count
    }

    /// Delete `start..end`, widened to whole tokens. Returns the caret the deletion leaves.
    fn delete_range(&mut self, target: &mut String, start: usize, end: usize) -> usize {
        self.error = None;
        let total = target.chars().count();
        let start = start.min(total);
        let end = end.min(total).max(start);
        let (start, end) = expand_deletion(&self.mentions, start, end);
        if start >= end {
            return start;
        }
        let removed = end - start;
        target.replace_range(char_to_byte(target, start)..char_to_byte(target, end), "");
        self.mentions.retain(|mention| !mention.intersects(&(start..end)));
        for mention in &mut self.mentions {
            if mention.start_char >= end {
                mention.start_char -= removed;
                mention.end_char -= removed;
            }
        }
        let fix = |v: usize| {
            if v > end {
                v - removed
            } else if v >= start {
                start
            } else {
                v
            }
        };
        self.selection = (fix(self.selection.0), fix(self.selection.1));
        start
    }

    /// Make `snapshot` the draft again, text, tokens and caret together.
    fn restore(&mut self, snapshot: Snapshot, target: &mut String) {
        *target = snapshot.text.clone();
        self.mentions = snapshot.mentions.clone();
        self.selection = snapshot.selection;
        self.committed = snapshot;
        self.error = None;
        // The text was replaced wholesale, so the editor's own state has to be rebuilt rather
        // than adjusted.
        self.reset_editor = true;
    }

    fn push_undo(&mut self, snapshot: Snapshot) {
        self.undo.push(snapshot);
        if self.undo.len() > MAX_HISTORY {
            self.undo.remove(0);
        }
        self.redo.clear();
    }
}

/// A draft and its spans as one [`TextBuffer`], so the editor's own edits keep the tokens true.
pub struct DraftBuffer<'a> {
    pub text: &'a mut String,
    pub meta: &'a mut DraftMetadata,
}

impl TextBuffer for DraftBuffer<'_> {
    fn type_id(&self) -> std::any::TypeId {
        std::any::TypeId::of::<DraftBuffer<'static>>()
    }

    fn is_mutable(&self) -> bool {
        true
    }

    fn as_str(&self) -> &str {
        self.text
    }

    fn insert_text(&mut self, text: &str, char_index: CharIndex) -> usize {
        self.meta.insert_str(self.text, text, char_index.0)
    }

    fn delete_char_range(&mut self, char_range: Range<CharIndex>) {
        self.meta.delete_range(self.text, char_range.start.0, char_range.end.0);
    }

    /// Insert and move the caret to just after what was inserted, which a snap out of a token
    /// makes different from the requested position.
    fn insert_text_at(&mut self, ccursor: &mut CCursor, text_to_insert: &str, char_limit: usize) {
        let at = self.meta.snap_caret(ccursor.index.0);
        ccursor.index = CharIndex(at);
        let text = if char_limit < usize::MAX {
            let cutoff = char_limit.saturating_sub(self.text.chars().count());
            match text_to_insert.char_indices().nth(cutoff) {
                None => text_to_insert,
                Some((idx, _)) => &text_to_insert[..idx],
            }
        } else {
            text_to_insert
        };
        let inserted = self.meta.insert_str(self.text, text, at);
        ccursor.index = CharIndex(at + inserted);
    }

    /// The caret after a deletion is where the text actually ends up, which is not egui's range
    /// start once the deletion was widened to a whole token.
    fn delete_selected_ccursor_range(&mut self, [min, max]: [CCursor; 2]) -> CCursor {
        let (start, end) = if min.index.0 <= max.index.0 { (min.index.0, max.index.0) } else { (max.index.0, min.index.0) };
        CCursor::new(self.meta.delete_range(self.text, start, end))
    }

    /// Surrounding deletion as one range, so its caret stays true when a token is widened into.
    fn delete_surrounding_chars(&mut self, cursor_range: CCursorRange, before_chars: usize, after_chars: usize) -> CCursorRange {
        let [min, max] = cursor_range.sorted_cursors();
        let start = min.index.0.saturating_sub(before_chars);
        let end = max.index.0.saturating_add(after_chars);
        let caret = self.meta.delete_range(self.text, start, end);
        CCursorRange::one(CCursor::new(caret))
    }
}

/// The layout job for one frame of the draft.
pub fn draft_layout(
    text: &str,
    mentions: &[ObjectMention],
    font: &FontId,
    text_color: Color32,
    token_background: Color32,
    wrap_width: f32,
) -> egui::text::LayoutJob {
    let plain = egui::TextFormat { font_id: font.clone(), color: text_color, ..Default::default() };
    let marked = egui::TextFormat { font_id: font.clone(), color: text_color, background: token_background, ..Default::default() };
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = wrap_width.max(1.0);
    // A label longer than the field still has to fit: breaking inside it beats overflowing the
    // composer, and the caret geometry stays the token's own.
    job.wrap.break_anywhere = true;
    job.break_on_newline = true;

    let mut cursor = 0;
    for mention in mentions {
        let start = mention.start_char.max(cursor);
        let end = mention.end_char.min(text.chars().count()).max(start);
        if start > cursor {
            job.append(&chars_between(text, cursor, start), 0.0, plain.clone());
        }
        if end > start {
            job.append(&chars_between(text, start, end), 0.0, marked.clone());
        }
        cursor = end;
    }
    if cursor < text.chars().count() {
        job.append(&chars_between(text, cursor, text.chars().count()), 0.0, plain);
    }
    job
}

/// The characters of `text` in `start..end`.
fn chars_between(text: &str, start: usize, end: usize) -> String {
    text.chars().skip(start).take(end.saturating_sub(start)).collect()
}

/// The byte offset of character `at`.
fn char_to_byte(text: &str, at: usize) -> usize {
    text.char_indices().nth(at).map_or(text.len(), |(byte, _)| byte)
}

/// The character at `at`, if the text is that long.
fn char_at(text: &str, at: usize) -> Option<char> {
    text.chars().nth(at)
}

/// `start..end` widened to cover every token it touches, so a token is never half deleted.
fn expand_deletion(mentions: &[ObjectMention], start: usize, end: usize) -> (usize, usize) {
    let mut range = start..end;
    loop {
        let mut widened = false;
        for mention in mentions {
            if mention.intersects(&range) {
                let (start, end) = (range.start.min(mention.start_char), range.end.max(mention.end_char));
                if (start, end) != (range.start, range.end) {
                    range = start..end;
                    widened = true;
                }
            }
        }
        if !widened {
            return (range.start, range.end);
        }
    }
}
