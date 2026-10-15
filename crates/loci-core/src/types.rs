//! Loci domain types — the contract shared by the core, the Tauri app and the CLI.
//!
//! The JSON produced by these types is the JSON the frontend consumes (`camelCase` keys, numeric Anki enums, kebab-case
//! string unions, optional fields omitted when absent), so the frontend's `types.ts` is unchanged.
//! Units follow Anki (see `schema.sql`): ids are milliseconds since the epoch, `mtime` seconds,
//! `due` depends on the queue, `ivl` days (negative = seconds).

use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use std::collections::BTreeMap;

// ───────────────────────── identifiers & time ─────────────────────────
pub type NoteId = i64;
pub type CardId = i64;
pub type DeckId = i64;
pub type NotetypeId = i64;
pub type DeckConfigId = i64;
pub type RevlogId = i64;
/// Seconds since epoch.
pub type Seconds = i64;
/// Milliseconds since epoch.
pub type Millis = i64;
/// Days since the collection's creation day (crt).
pub type CollectionDay = i64;

pub const FIELD_SEPARATOR: char = '\u{1f}';

// ───────────────────────── enums (Anki values) ─────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize_repr, Deserialize_repr, Default)]
#[repr(i32)]
pub enum CardType {
    #[default]
    New = 0,
    Learn = 1,
    Review = 2,
    Relearn = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize_repr, Deserialize_repr, Default)]
#[repr(i32)]
pub enum CardQueue {
    UserBuried = -3,
    SchedBuried = -2,
    Suspended = -1,
    #[default]
    New = 0,
    Learn = 1,
    Review = 2,
    DayLearn = 3,
    Preview = 4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize_repr, Deserialize_repr)]
#[repr(i32)]
pub enum Rating {
    Again = 1,
    Hard = 2,
    Good = 3,
    Easy = 4,
}

impl Rating {
    pub const ALL: [Rating; 4] = [Rating::Again, Rating::Hard, Rating::Good, Rating::Easy];
    pub fn from_i64(v: i64) -> Option<Rating> {
        match v {
            1 => Some(Rating::Again),
            2 => Some(Rating::Hard),
            3 => Some(Rating::Good),
            4 => Some(Rating::Easy),
            _ => None,
        }
    }
    pub fn as_i64(self) -> i64 {
        self as i64
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize_repr, Deserialize_repr, Default)]
#[repr(i32)]
pub enum RevlogType {
    #[default]
    Learn = 0,
    Review = 1,
    Relearn = 2,
    Filtered = 3,
    Manual = 4,
    Rescheduled = 5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize_repr, Deserialize_repr, Default)]
#[repr(i32)]
pub enum NotetypeKind {
    #[default]
    Standard = 0,
    Cloze = 1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize_repr, Deserialize_repr)]
#[repr(i32)]
pub enum GraveType {
    Card = 0,
    Note = 1,
    Deck = 2,
}

/// 0 none, 1 red, 2 orange, 3 green, 4 blue, 5 pink, 6 turquoise, 7 purple.
pub type Flag = u8;

// ───────────────────────── note types ─────────────────────────
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
#[derive(Default)]
pub struct FieldConfig {
    pub sticky: bool,
    pub rtl: bool,
    /// '' = inherit the app card font
    pub font: String,
    /// 0 = inherit
    pub size: i64,
    /// placeholder shown in the editor
    pub description: String,
    /// edit as plain text (no HTML)
    pub plain_text: bool,
    pub collapsed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Field {
    pub ord: i64,
    pub name: String,
    pub config: FieldConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct TemplateConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deck_override: Option<DeckId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Template {
    pub ord: i64,
    pub name: String,
    pub qfmt: String,
    pub afmt: String,
    #[serde(default)]
    pub bqfmt: String,
    #[serde(default)]
    pub bafmt: String,
    #[serde(default)]
    pub config: TemplateConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReqKind {
    All,
    Any,
    None,
}

/// Anki's card-generation requirement: `[ord, 'all'|'any'|'none', fieldOrds[]]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Requirement(pub i64, pub ReqKind, pub Vec<i64>);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BuiltinKind {
    Basic,
    BasicReversed,
    BasicTyped,
    Cloze,
    ImageOcclusion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum OcclusionMode {
    #[default]
    HideAll,
    HideOne,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct NotetypeConfig {
    /// Anki's card-generation requirements, kept verbatim for export.
    pub req: Vec<Requirement>,
    /// One of the built-in kinds, if this note type was created by Loci.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub builtin: Option<BuiltinKind>,
    /// Cloze: reveal clozes one by one on the back.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloze_one_by_one: Option<bool>,
    /// Image occlusion mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub occlusion_mode: Option<OcclusionMode>,
    /// Anki id this note type was imported from, if different (collisions).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_id: Option<i64>,
    /// Anki `originalStockKind` (1 basic, 2 basic+reversed, 3 optional reversed, 4 typing, 5 cloze, 6 image occlusion).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_stock_kind: Option<i64>,
    /// Keep this note type's own colours and fonts (see template/css).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub use_own_styling: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Notetype {
    pub id: NotetypeId,
    pub name: String,
    pub kind: NotetypeKind,
    pub css: String,
    pub latex_pre: String,
    pub latex_post: String,
    pub sort_field: i64,
    pub fields: Vec<Field>,
    pub templates: Vec<Template>,
    pub config: NotetypeConfig,
    pub mtime: Seconds,
    pub usn: i64,
}

/// `saveNotetype` input: a note type without `mtime`/`usn`, `id` optional (new when absent).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotetypeInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<NotetypeId>,
    pub name: String,
    pub kind: NotetypeKind,
    #[serde(default)]
    pub css: String,
    #[serde(default)]
    pub latex_pre: String,
    #[serde(default)]
    pub latex_post: String,
    #[serde(default)]
    pub sort_field: i64,
    pub fields: Vec<Field>,
    pub templates: Vec<Template>,
    #[serde(default)]
    pub config: NotetypeConfig,
}

// ───────────────────────── decks & presets ─────────────────────────
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct DeckToday {
    pub day: CollectionDay,
    pub new: i64,
    pub rev: i64,
    pub lrn: i64,
    pub time_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Deck {
    pub id: DeckId,
    pub name: String,
    pub config_id: DeckConfigId,
    pub description: String,
    pub collapsed: bool,
    pub today: DeckToday,
    pub mtime: Seconds,
    pub usn: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeckTreeNode {
    pub deck: Deck,
    pub children: Vec<DeckTreeNode>,
    pub level: i64,
    /// counts for today after limits, including children
    pub new_count: i64,
    pub learn_count: i64,
    pub review_count: i64,
    /// estimated minutes to clear, from recent seconds-per-card
    pub estimated_minutes: i64,
    pub total_cards: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum SchedulerKind {
    #[default]
    Fsrs,
    Sm2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum NewCardOrder {
    #[default]
    Due,
    Random,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum NewCardGather {
    #[default]
    Deck,
    DeckRandomNotes,
    Ascending,
    Descending,
    RandomNotes,
    RandomCards,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum NewCardSort {
    #[default]
    TemplateThenOrder,
    Order,
    TemplateThenRandom,
    RandomNote,
    RandomCard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewOrder {
    #[default]
    DueThenRandom,
    DueThenDeck,
    DeckThenDue,
    AscendingIntervals,
    DescendingIntervals,
    AscendingEase,
    DescendingEase,
    RelativeOverdue,
    Random,
    Added,
    ReverseAdded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum NewReviewMix {
    #[default]
    Mix,
    ReviewFirst,
    NewFirst,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum LeechAction {
    #[default]
    Suspend,
    Tag,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeckConfig {
    pub scheduler: SchedulerKind,
    /// Daily limits
    pub new_per_day: i64,
    pub reviews_per_day: i64,
    /// Loci: minutes target per day used for Home estimates and catch-up (not a hard cap).
    pub minutes_per_day: i64,
    /// Learning steps in minutes (fractional allowed)
    pub learn_steps: Vec<f64>,
    pub relearn_steps: Vec<f64>,
    /// days, SM-2
    pub graduating_interval: i64,
    /// days, SM-2
    pub easy_interval: i64,
    pub new_order: NewCardOrder,
    pub new_gather: NewCardGather,
    pub new_sort: NewCardSort,
    pub review_order: ReviewOrder,
    pub new_review_mix: NewReviewMix,
    pub interday_learning_mix: NewReviewMix,
    /// SM-2: permille, 2500
    pub initial_ease: i64,
    pub easy_bonus: f64,
    pub interval_modifier: f64,
    pub hard_multiplier: f64,
    pub lapse_multiplier: f64,
    /// days, 1
    pub minimum_lapse_interval: i64,
    /// days, 36500
    pub maximum_interval: i64,
    /// FSRS: 21 numbers (17/19 accepted and upgraded)
    pub fsrs_params: Vec<f64>,
    /// 0.70–0.99
    pub desired_retention: f64,
    /// used for SM-2 → FSRS conversion
    pub historical_retention: f64,
    pub auto_optimise: bool,
    /// 400
    pub optimise_every_reviews: i64,
    /// Lapses: 8
    pub leech_threshold: i64,
    pub leech_action: LeechAction,
    /// Burying
    pub bury_siblings_new: bool,
    pub bury_siblings_review: bool,
    pub bury_siblings_interday: bool,
    /// Audio
    pub autoplay_audio: bool,
    pub replay_question_audio: bool,
    /// Timer: 60
    pub max_answer_seconds: i64,
    pub show_timer: bool,
    /// Display
    pub show_intervals_on_buttons: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeckConfigPreset {
    pub id: DeckConfigId,
    pub name: String,
    pub config: DeckConfig,
    pub mtime: Seconds,
    pub usn: i64,
}

/// `savePreset` input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeckConfigPresetInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<DeckConfigId>,
    pub name: String,
    pub config: DeckConfig,
}

// ───────────────────────── notes & cards ─────────────────────────
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: NoteId,
    pub guid: String,
    pub ntid: NotetypeId,
    pub mtime: Seconds,
    pub usn: i64,
    pub tags: Vec<String>,
    pub fields: Vec<String>,
}

/// `cards.data` JSON as Anki writes it. Unknown keys are preserved through `extra` so a
/// round-trip through Loci never drops what Anki or an add-on stored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct CardData {
    /// FSRS stability (days)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub s: Option<f64>,
    /// FSRS difficulty (1..10)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub d: Option<f64>,
    /// desired retention at last scheduling
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dr: Option<f64>,
    /// FSRS-6 decay
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decay: Option<f64>,
    /// original new-card position
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pos: Option<i64>,
    /// custom data (Anki `cd`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cd: Option<String>,
    /// last review time, unix seconds (Anki `lrt`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lrt: Option<i64>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    pub id: CardId,
    pub nid: NoteId,
    pub did: DeckId,
    pub ord: i64,
    pub mtime: Seconds,
    pub usn: i64,
    #[serde(rename = "type")]
    pub ctype: CardType,
    pub queue: CardQueue,
    pub due: i64,
    pub ivl: i64,
    pub factor: i64,
    pub reps: i64,
    pub lapses: i64,
    pub left: i64,
    pub odue: i64,
    pub odid: DeckId,
    pub flags: i64,
    pub data: CardData,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevlogEntry {
    pub id: RevlogId,
    pub cid: CardId,
    pub usn: i64,
    pub ease: i64,
    pub ivl: i64,
    pub last_ivl: i64,
    pub factor: i64,
    pub time: i64,
    #[serde(rename = "type")]
    pub rtype: RevlogType,
}

// ───────────────────────── rendering ─────────────────────────
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RenderedSide {
    /// HTML for the shadow root (sanitised by the UI)
    pub html: String,
    /// audio/video filenames in play order (`[sound:]` tags)
    pub media: Vec<String>,
    /// true when the side contains a `{{type:}}` field
    pub has_type_input: bool,
    /// expected typed answer (plain text) when has_type_input
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_answer: Option<String>,
    /// cloze ordinals present on this card (for one-by-one)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloze_ords: Option<Vec<i64>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Dir {
    #[default]
    Auto,
    Rtl,
    Ltr,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderedCard {
    pub card_id: CardId,
    pub note_id: NoteId,
    pub notetype_id: NotetypeId,
    pub template_name: String,
    pub front: RenderedSide,
    pub back: RenderedSide,
    /// CSS from the note type, already scoped for the shadow root
    pub css: String,
    /// direction hint for the whole card
    pub dir: Dir,
    /// image occlusion payload when applicable
    #[serde(skip_serializing_if = "Option::is_none")]
    pub occlusion: Option<OcclusionPayload>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShapeKind {
    Rect,
    Ellipse,
    Polygon,
    Text,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcclusionShape {
    pub kind: ShapeKind,
    /// cloze ordinal
    pub ord: i64,
    /// normalised 0..1 coordinates relative to the image
    #[serde(skip_serializing_if = "Option::is_none")]
    pub left: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<[f64; 2]>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcclusionPayload {
    pub image: String,
    pub shapes: Vec<OcclusionShape>,
    pub mode: OcclusionMode,
    pub active_ord: i64,
}

// ───────────────────────── study ─────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StudyCounts {
    pub new: i64,
    pub learn: i64,
    pub review: i64,
}

/// `Record<Rating, T>`: JSON keys "1".."4".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct RatingMap<T> {
    #[serde(rename = "1")]
    pub again: T,
    #[serde(rename = "2")]
    pub hard: T,
    #[serde(rename = "3")]
    pub good: T,
    #[serde(rename = "4")]
    pub easy: T,
}

impl<T> RatingMap<T> {
    pub fn get(&self, r: Rating) -> &T {
        match r {
            Rating::Again => &self.again,
            Rating::Hard => &self.hard,
            Rating::Good => &self.good,
            Rating::Easy => &self.easy,
        }
    }
    pub fn get_mut(&mut self, r: Rating) -> &mut T {
        match r {
            Rating::Again => &mut self.again,
            Rating::Hard => &mut self.hard,
            Rating::Good => &mut self.good,
            Rating::Easy => &mut self.easy,
        }
    }
    pub fn from_fn(mut f: impl FnMut(Rating) -> T) -> Self {
        RatingMap { again: f(Rating::Again), hard: f(Rating::Hard), good: f(Rating::Good), easy: f(Rating::Easy) }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulingPreview {
    /// human interval label per rating, e.g. '<10m', '3d', '1.2mo'
    pub labels: RatingMap<String>,
    /// seconds until each rating's next due
    pub seconds_until: RatingMap<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StudyCard {
    pub card: Card,
    pub note: Note,
    pub rendered: RenderedCard,
    pub preview: SchedulingPreview,
    pub counts: StudyCounts,
    pub deck_name: String,
    pub is_leech: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnswerResult {
    pub counts: StudyCounts,
    pub next: Option<StudyCard>,
    pub undo_label: String,
    /// set when the answer made the card a leech
    #[serde(skip_serializing_if = "Option::is_none")]
    pub became_leech: Option<bool>,
    /// true when the session has nothing left
    pub finished: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardExplanation {
    pub retrievability: Option<f64>,
    pub stability: Option<f64>,
    pub difficulty: Option<f64>,
    pub due_date: Option<Seconds>,
    pub interval: i64,
    pub reps: i64,
    pub lapses: i64,
    pub average_seconds: f64,
    pub history: Vec<RevlogEntry>,
    pub sentence: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub cards: i64,
    pub seconds: i64,
    pub accuracy: f64,
    pub again: i64,
    pub hard: i64,
    pub good: i64,
    pub easy: i64,
    pub tomorrow_due: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub insight: Option<Insight>,
}

/// Custom study kinds (`startCustomStudy`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub enum CustomStudyKind {
    Ahead { days: i64 },
    Forgotten { days: i64 },
    Preview,
    Search { query: String, limit: i64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LeechResolution {
    Reset,
    Suspend,
    Keep,
}

// ───────────────────────── backlog / catch-up ─────────────────────────
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BacklogReport {
    pub due: i64,
    pub overdue: i64,
    pub max_overdue_days: i64,
    pub daily_average: f64,
    pub estimated_minutes: i64,
    pub days_at_current_pace: i64,
    pub triggered: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub enum CatchUpPlan {
    Spread { days: i64 },
    TodayShare,
    LowerRetention { retention: f64, days: i64 },
    StudyAll,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatchUpResult {
    pub postponed: i64,
    pub kept_today: i64,
    pub undo_label: String,
}

// ───────────────────────── browse / search ─────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum BrowseMode {
    #[default]
    Cards,
    Notes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BrowseColumn {
    SortField,
    Question,
    Answer,
    Deck,
    Notetype,
    Template,
    Tags,
    Due,
    Interval,
    Ease,
    Difficulty,
    Stability,
    Retrievability,
    Reps,
    Lapses,
    Created,
    Modified,
    CardCount,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseSort {
    pub column: BrowseColumn,
    pub descending: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseRequest {
    pub query: String,
    pub mode: BrowseMode,
    pub sort: BrowseSort,
    pub offset: i64,
    pub limit: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseRow {
    pub id: i64,
    pub card_id: CardId,
    pub note_id: NoteId,
    pub cells: BTreeMap<BrowseColumn, String>,
    pub flag: Flag,
    pub suspended: bool,
    pub buried: bool,
    pub is_leech: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseResult {
    pub rows: Vec<BrowseRow>,
    pub total: i64,
    pub ids: Vec<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SidebarKind {
    Saved,
    Today,
    Flag,
    Deck,
    Notetype,
    Tag,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SidebarItem {
    pub kind: SidebarKind,
    pub label: String,
    pub query: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<SidebarItem>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum BulkAction {
    ChangeDeck {
        deck_id: DeckId,
    },
    AddTags {
        tags: Vec<String>,
    },
    RemoveTags {
        tags: Vec<String>,
    },
    Suspend,
    Unsuspend,
    Bury,
    Unbury,
    Flag {
        flag: Flag,
    },
    Reposition {
        start: i64,
        step: i64,
        randomize: bool,
        shift: bool,
    },
    /// Anki syntax: '0', '1!', '3-7'
    SetDue {
        days: String,
    },
    Forget {
        reset_counts: bool,
        restore_position: bool,
    },
    Delete,
    FindReplace {
        find: String,
        replace: String,
        regex: bool,
        field: Option<String>,
        match_case: bool,
    },
}

// ───────────────────────── stats ─────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Suggest,
    Warn,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InsightAction {
    pub label: String,
    pub route: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Insight {
    pub id: String,
    pub severity: Severity,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<InsightAction>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StatsRequest {
    /// null = whole collection
    #[serde(default)]
    pub deck_id: Option<DeckId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StatsToday {
    pub studied: i64,
    pub seconds: i64,
    pub again: i64,
    pub correct_pct: f64,
    pub learned: i64,
    pub relearned: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForecastDay {
    pub day: i64,
    pub due: i64,
    pub with_new: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetentionRow {
    pub label: String,
    pub true_retention: Option<f64>,
    pub desired: f64,
    pub reviews: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewsDay {
    pub day: CollectionDay,
    pub count: i64,
    pub seconds: i64,
    pub learn: i64,
    pub review: i64,
    pub relearn: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeatCell {
    /// ISO date
    pub date: String,
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bucket {
    pub bucket: String,
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HourRow {
    pub hour: i64,
    pub reviews: i64,
    pub correct_pct: f64,
}

/// again/hard/good/easy counts per maturity
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ButtonStats {
    pub learn: Vec<i64>,
    pub young: Vec<i64>,
    pub mature: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CardCounts {
    pub new: i64,
    pub learning: i64,
    pub young: i64,
    pub mature: i64,
    pub suspended: i64,
    pub buried: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Momentum {
    pub days_studied_last30: i64,
    pub current_run: i64,
    pub longest_run: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub today: StatsToday,
    /// next 90 days
    pub forecast: Vec<ForecastDay>,
    /// 7d/30d/90d/1y/all
    pub retention: Vec<RetentionRow>,
    /// 365 days
    pub reviews_per_day: Vec<ReviewsDay>,
    /// 365 days
    pub heatmap: Vec<HeatCell>,
    pub intervals: Vec<Bucket>,
    pub stability: Vec<Bucket>,
    pub difficulty: Vec<Bucket>,
    pub hourly: Vec<HourRow>,
    pub buttons: ButtonStats,
    pub card_counts: CardCounts,
    pub momentum: Momentum,
    pub insights: Vec<Insight>,
}

// ───────────────────────── import / export ─────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PackageLayout {
    Legacy1,
    Legacy2,
    Latest,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    pub layout: PackageLayout,
    pub is_collection_package: bool,
    pub notes: i64,
    pub cards: i64,
    pub decks: Vec<String>,
    pub notetypes: Vec<String>,
    pub reviews: i64,
    pub media: i64,
    pub has_scheduling: bool,
    pub fsrs_cards: i64,
    pub source_crt: Seconds,
    pub warnings: Vec<String>,
    /// notes whose guid is already in this collection, and notes that would be new
    pub notes_existing: i64,
    pub notes_new: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ImportMode {
    #[default]
    Merge,
    Replace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ConflictPolicy {
    #[default]
    Newer,
    Keep,
    Theirs,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportOptions {
    /// 'merge' into this collection by note guid; 'replace' = restore-like replacement of the whole collection.
    pub mode: ImportMode,
    pub keep_scheduling: bool,
    /// when a note exists with the same guid: 'newer' wins (default), 'keep' ours, 'theirs'
    #[serde(default)]
    pub conflict: ConflictPolicy,
}

impl Default for ImportOptions {
    fn default() -> Self {
        ImportOptions { mode: ImportMode::Merge, keep_scheduling: true, conflict: ConflictPolicy::Newer }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub notes_added: i64,
    pub notes_updated: i64,
    pub notes_skipped: i64,
    pub cards_added: i64,
    pub reviews_added: i64,
    pub media_added: i64,
    pub decks_added: i64,
    pub notetypes_added: i64,
    pub warnings: Vec<String>,
    pub duration_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportOptions {
    pub deck_id: Option<DeckId>,
    pub include_scheduling: bool,
    pub include_media: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub phase: String,
    pub done: i64,
    pub total: i64,
}

// ───────────────────────── backups ─────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BackupKind {
    Daily,
    Weekly,
    Monthly,
    Manual,
    PreRestore,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    pub name: String,
    pub at: Seconds,
    pub bytes: i64,
    pub cards: Option<i64>,
    pub kind: BackupKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct IntegrityReport {
    pub ok: bool,
    pub problems: Vec<String>,
    pub repaired: Vec<String>,
}

// ───────────────────────── optimiser ─────────────────────────
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParamsMetrics {
    pub params: Vec<f64>,
    pub log_loss: f64,
    pub rmse: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OptimiseResult {
    pub config_id: DeckConfigId,
    pub reviews: i64,
    pub before: ParamsMetrics,
    pub after: ParamsMetrics,
    pub adopted: bool,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Evaluation {
    pub log_loss: f64,
    pub rmse: f64,
    pub reviews: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadForecast {
    pub reviews_per_day: Vec<f64>,
    pub minutes_per_day: Vec<f64>,
    pub sentence: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaintenanceReport {
    pub optimised: Vec<OptimiseResult>,
    pub backed_up: bool,
}

// ───────────────────────── collection-wide settings ─────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemePref {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CardFont {
    #[default]
    Serif,
    Sans,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CardAlign {
    #[default]
    Start,
    Center,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ReducedMotion {
    #[default]
    System,
    On,
    Off,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionSettings {
    pub rollover_hour: i64,
    pub learn_ahead_minutes: i64,
    pub locale: String,
    pub theme: ThemePref,
    pub card_font: CardFont,
    pub card_font_size: i64,
    pub card_align: CardAlign,
    pub show_keyboard_hints: bool,
    pub reduced_motion: ReducedMotion,
    pub typed_answer_ignore_case: bool,
    pub typed_answer_ignore_diacritics: bool,
}

/// `Partial<CollectionSettings>` for `setSettings`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct CollectionSettingsPatch {
    pub rollover_hour: Option<i64>,
    pub learn_ahead_minutes: Option<i64>,
    pub locale: Option<String>,
    pub theme: Option<ThemePref>,
    pub card_font: Option<CardFont>,
    pub card_font_size: Option<i64>,
    pub card_align: Option<CardAlign>,
    pub show_keyboard_hints: Option<bool>,
    pub reduced_motion: Option<ReducedMotion>,
    pub typed_answer_ignore_case: Option<bool>,
    pub typed_answer_ignore_diacritics: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enums_serialise_as_anki_numbers() {
        assert_eq!(serde_json::to_string(&CardQueue::UserBuried).unwrap(), "-3");
        assert_eq!(serde_json::to_string(&CardType::Relearn).unwrap(), "3");
        assert_eq!(serde_json::from_str::<Rating>("4").unwrap(), Rating::Easy);
    }

    #[test]
    fn string_unions_match_the_typescript_literals() {
        assert_eq!(serde_json::to_string(&NewCardGather::DeckRandomNotes).unwrap(), "\"deck-random-notes\"");
        assert_eq!(serde_json::to_string(&BrowseColumn::SortField).unwrap(), "\"sortField\"");
        assert_eq!(serde_json::to_string(&BackupKind::PreRestore).unwrap(), "\"pre-restore\"");
        assert_eq!(serde_json::to_string(&BuiltinKind::ImageOcclusion).unwrap(), "\"image-occlusion\"");
    }

    #[test]
    fn tagged_unions_use_kind() {
        let a: BulkAction = serde_json::from_str(r#"{"kind":"changeDeck","deckId":5}"#).unwrap();
        assert_eq!(a, BulkAction::ChangeDeck { deck_id: 5 });
        let p: CatchUpPlan = serde_json::from_str(r#"{"kind":"lower-retention","retention":0.85,"days":7}"#).unwrap();
        assert_eq!(p, CatchUpPlan::LowerRetention { retention: 0.85, days: 7 });
        let k: CustomStudyKind = serde_json::from_str(r#"{"kind":"search","query":"deck:x","limit":50}"#).unwrap();
        assert_eq!(k, CustomStudyKind::Search { query: "deck:x".into(), limit: 50 });
    }

    #[test]
    fn rating_map_keys_are_strings_1_to_4() {
        let m = RatingMap::from_fn(|r| r.as_i64() * 10);
        assert_eq!(serde_json::to_string(&m).unwrap(), r#"{"1":10,"2":20,"3":30,"4":40}"#);
    }

    #[test]
    fn card_data_preserves_unknown_keys_and_omits_absent_ones() {
        let d: CardData = serde_json::from_str(r#"{"s":12.5,"d":5.2,"foo":1}"#).unwrap();
        assert_eq!(d.s, Some(12.5));
        assert_eq!(d.extra.get("foo"), Some(&serde_json::json!(1)));
        assert_eq!(serde_json::to_string(&d).unwrap(), r#"{"s":12.5,"d":5.2,"foo":1}"#);
        assert_eq!(serde_json::to_string(&CardData::default()).unwrap(), "{}");
    }

    #[test]
    fn card_type_and_queue_fields_are_named_type_and_queue() {
        let c = Card {
            id: 1,
            nid: 2,
            did: 1,
            ord: 0,
            mtime: 0,
            usn: -1,
            ctype: CardType::New,
            queue: CardQueue::New,
            due: 0,
            ivl: 0,
            factor: 0,
            reps: 0,
            lapses: 0,
            left: 0,
            odue: 0,
            odid: 0,
            flags: 0,
            data: CardData::default(),
        };
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(v["type"], 0);
        assert_eq!(v["queue"], 0);
        assert!(v.get("ctype").is_none());
    }
}
