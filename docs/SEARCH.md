# Searching in Loci

The Browse search box, saved searches, custom study sessions (`search` kind) and insight
links all use Anki's search syntax. If you know Anki's, everything below works the same way.
The implementation is `src/core/search/` (parser, SQL compiler); this page lists exactly what
it supports.

## Combining terms

| You type | Finds |
|---|---|
| `dog cat` | notes containing both *dog* and *cat* (AND is implicit) |
| `dog and cat` | the same; `and` is optional |
| `dog or cat` | either word |
| `dog (cat or mouse)` | *dog* and either *cat* or *mouse* |
| `-cat` | notes without *cat* |
| `-(cat or mouse)` | notes with neither |

`and` binds tighter than `or`, as in Anki: `a b or c` means `(a and b) or c`. Operators are
case-insensitive.

## Text

| You type | Finds |
|---|---|
| `dog` | *dog* anywhere in the note's text (HTML removed), also inside *doggy* |
| `"a dog"` | the exact phrase *a dog*; cloze markup is read as its text, so `"Paris is in France"` finds `{{c1::Paris}} is in {{c2::France::country}}` |
| `d*g` | `*` matches any run of characters: *dg*, *dog*, *dung* |
| `d_g` | `_` matches one character: *dog*, *dig* |
| `w:dog` | *dog* as a whole word (not *doggy*); `w:dog*` matches words starting with *dog* |
| `nc:uber` | ignores accents and other combining marks: matches *über* |
| `re:(?i)^colou?r` | a regular expression on the raw field HTML of any field |
| `sc:the cat` | text inside cloze deletions counts (`{{c1::cat}}` matches *the cat*) |
| `\*`, `\_`, `\"`, `\\`, `\:`, `\(`, `\)`, `\-` | the literal character |

Plain text searches are case-insensitive. Terms of three characters or more use the full-text
index (fast, case-insensitive in every script, including Arabic and CJK); shorter terms and
terms with wildcards use a scan that folds case for ASCII letters only, as Anki does.

## Fields

| You type | Finds |
|---|---|
| `front:dog` | the field named *Front* is exactly *dog* |
| `front:*dog*` | *Front* contains *dog* |
| `front:` | *Front* is empty |
| `fr*:dog` | any field whose name starts with *fr* is exactly *dog* |
| `front:re:^d.g$` | regular expression on *Front* |
| `front:nc:uber` | *Front* equals *uber*, ignoring accents |

Field searches match the field's HTML, case-insensitively, one field at a time.

## Decks, note types, cards, tags

| You type | Finds |
|---|---|
| `deck:Languages` | cards in *Languages* and all its subdecks |
| `"deck:Languages::Arabic Verbs"` | quote names with spaces |
| `deck:Lang*` | decks matching a wildcard |
| `deck:current` | the deck you are studying or browsing from |
| `-deck:Languages::Arabic::*` | excludes the subdecks of *Arabic* (not *Arabic* itself) |
| `deck:*` | every card |
| `deck:filtered` | cards still in a filtered deck (none after import: Loci returns them home) |
| `preset:Standard` | cards in decks that use the *Standard* preset |
| `note:Basic` | notes of the note type *Basic* (wildcards allowed) |
| `card:1` | the first card type of each note (cloze: `card:3` is cloze 3) |
| `card:Reverse` | cards of the template named *Reverse* |
| `tag:verb` | notes tagged *verb* or any child tag (*verb::irregular*) |
| `tag:verb*` | tags starting with *verb* |
| `tag:none` | notes without tags |
| `tag:re:^ar.*s$` | tags matching a regular expression |

## Card state

| You type | Finds |
|---|---|
| `is:due` | reviews and day-learning cards due today or earlier, and learning cards due now |
| `is:new` | new cards |
| `is:learn` | cards in (re)learning |
| `is:review` | review cards, including ones relearning after a lapse |
| `is:suspended` | suspended cards |
| `is:buried` | buried cards; `is:buried-sibling` and `is:buried-manually` split them |
| `flag:1` | red flag (2 orange, 3 green, 4 blue, 5 pink, 6 turquoise, 7 purple; `flag:0` none) |
| `tag:leech` | cards the scheduler marked as struggling |

## Properties

`prop:` compares a number with `<`, `>`, `<=`, `>=`, `=` or `!=`.

| You type | Finds |
|---|---|
| `prop:ivl>=30` | interval of at least 30 days |
| `prop:due=1` | due tomorrow; `prop:due<0` overdue; `prop:due<=2` within two days |
| `prop:reps>10`, `prop:lapses>=8` | answered more than 10 times; lapsed 8 times or more |
| `prop:ease<2.0` | SM-2 ease under 200% |
| `prop:pos<=100` | new cards at position 100 or earlier |
| `prop:r<0.9` | FSRS retrievability (chance of recall now) below 90%, for every card with a memory state whatever its queue (review, learning, relearning, suspended, buried); days since the last review come from the card's last-review time, else its due day, else its latest answer |
| `prop:s>90` | FSRS stability over 90 days |
| `prop:d>0.8` | FSRS difficulty above 80% (0–1 scale) |
| `prop:rated=-1` | answered yesterday; `prop:resched>-7` rescheduled in the last week |
| `prop:cdn:key>5`, `prop:cds:key=value` | custom data written by Anki add-ons |
| `has-cd:key` | cards that have that custom data key |

## Time

Days count back from the start of today (the day starts at the rollover hour, 4 am by default).

| You type | Finds |
|---|---|
| `rated:1` | answered today; `rated:7` in the last week |
| `rated:7:1` | answered *Again* in the last week (`:2` Hard, `:3` Good, `:4` Easy) |
| `added:1` | cards added today |
| `edited:3` | notes edited in the last three days |
| `introduced:30` | cards first studied in the last 30 days |
| `resched:7` | cards rescheduled by hand (set due date, reset) in the last week |

## Identifiers

| You type | Finds |
|---|---|
| `nid:1700000000000` | a note by id; several: `nid:1,2,3` |
| `cid:…`, `mid:…`, `did:…` | cards, note types, decks by id |
| `dupe:1700000000000,dog` | notes of that note type whose first field is *dog* |

## Examples

- Struggling cards in one deck: `"deck:Medicine::Anatomy" (tag:leech or prop:lapses>=8)`
- What is at risk today: `is:due prop:r<0.8`
- New Arabic verbs: `deck:Languages::Arabic::Verbs is:new`
- Notes you edited this week that contain an image: `edited:7 re:<img`
- Cards you forgot yesterday: `rated:2:1 -rated:1`

## Differences from Anki

- Field searches and `re:` see the field's HTML; plain text searches see the text without HTML.
  This is also Anki's behaviour. Unlike Anki, plain text searches also see cloze deletions as the
  text they reveal (markers and hints removed), so a phrase can span a cloze boundary; search the
  markup itself with `re:` or a field search (`text:*c1::*`).
- `prop:r` covers learning and relearning cards that have a memory state, not only review cards.
- Filtered decks are not kept as decks in Loci, so `deck:filtered` normally finds nothing.
- Searches that need a custom SQL function in Anki (fields, `re:`, `nc:`, `w:`, `sc:`, wildcard tags,
  `dupe:`) are evaluated in two steps: SQL narrows the candidates, then an exact check runs on each.
  The results are the same; very broad regular expressions on huge collections are slower.
