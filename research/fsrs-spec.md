# FSRS and Anki SM-2

Sources read on 2026-10-06.

## 0. Version pinning

| Component | Version / commit used here | Why it matters |
|---|---|---|
| Anki desktop | 26.09.x (26.09 released 2026-09-14; 26.09.3 on 2026-09-23). Source read at `ankitects/anki@a13d8a6e` (main, 2026-10-06) | The 26.09 release notes say "Update FSRS to 6.6.2" ([release](https://github.com/ankitects/anki/releases/tag/26.09)). `Cargo.toml` pins `fsrs = "6.6.2"` ([Cargo.toml](https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/Cargo.toml)). |
| fsrs-rs (Rust crate used by Anki) | **v6.6.2** (tag commit `6088949b`, released 2026-08-28) | **Implements FSRS-6 (21 parameters)**. Everything in sections 1–6 is taken from this tag. |
| fsrs-rs `main` | `4bc0a097` (2026-09-30) | **Not released yet.** Main now defaults to **FSRS-7 (34 parameters)**: `FSRS::new(&[])` builds FSRS-7, while 17/19/21-parameter vectors still select FSRS-6 ([model.rs@main](https://github.com/open-spaced-repetition/fsrs-rs/blob/4bc0a0979f95dd01cc653031b6f2a01429e4c32b/src/model.rs)). See §12. |
| ts-fsrs | `c8ca282e` (main, 2026-09-02; latest release v5.4.2). `FSRSVersion = "v… using FSRS-6.0"` | Reference TS implementation of FSRS-6. Tests are in §7. |
| py-fsrs | `9446cb06` (main; latest release v6.3.2) | FSRS-6. Tests are in §7. |
| Algorithm wiki | [awesome-fsrs wiki "The Algorithm"](https://github.com/open-spaced-repetition/awesome-fsrs/wiki/The-Algorithm). The fsrs4anki wiki page now just says "Page moved" and points there. | |
| srs-benchmark | `bd9110f7` README | Accuracy figures by version. |

Link prefixes used below:
- `FRS:` = `https://github.com/open-spaced-repetition/fsrs-rs/blob/v6.6.2/src/`
- `ANKI:` = `https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/`
- `TS:` = `https://github.com/open-spaced-repetition/ts-fsrs/blob/c8ca282edc3fe1cdfa1c24912437938b63a25cb3/packages/fsrs/`
- `PY:` = `https://github.com/open-spaced-repetition/py-fsrs/blob/9446cb06605c597a063aeee49f7d188d42e34dc2/`

**Recommendation:** implement FSRS-6 exactly as fsrs-rs v6.6.2 does it (f32 in Rust, f64 is fine in TS, see the precision notes in §7). Accept 17- and 19-parameter vectors through the conversion in §2.3. Keep the forgetting curve and the step function behind an interface so FSRS-7 can be added later.

---

## 1. FSRS-6 formulas (exact, as in fsrs-rs v6.6.2)

Sources: `FRS:model.rs` ([link](https://github.com/open-spaced-repetition/fsrs-rs/blob/v6.6.2/src/model.rs)), `FRS:inference.rs`, and the [wiki](https://github.com/open-spaced-repetition/awesome-fsrs/wiki/The-Algorithm).

Notation:
- G is the grade: 1 = Again, 2 = Hard, 3 = Good, 4 = Easy. 0 means "no rating / padding" (the state is unchanged).
- t is elapsed days, an integer in Anki (see §5.2).
- S is stability in days, D is difficulty, R is retrievability.
- w[0..20] are the parameters.

### 1.1 Constants

| Name | fsrs-rs v6.6.2 | ts-fsrs | py-fsrs | Note |
|---|---|---|---|---|
| S_MIN | `0.001` ([simulation.rs L58](https://github.com/open-spaced-repetition/fsrs-rs/blob/v6.6.2/src/simulation.rs)) | `0.001` ([constant.ts](https://github.com/open-spaced-repetition/ts-fsrs/blob/c8ca282edc3fe1cdfa1c24912437938b63a25cb3/packages/fsrs/src/constant.ts)) | `STABILITY_MIN = 0.001` | fsrs-rs **main** changed this to `0.0001`. |
| S_MAX | `36500.0` | `36500.0` | | |
| D_MIN, D_MAX | `1.0`, `10.0` | clamp(…,1,10) | | |
| INIT_S_MAX | `100.0` ([parameter_initialization.rs](https://github.com/open-spaced-repetition/fsrs-rs/blob/v6.6.2/src/parameter_initialization.rs)) | `100.0` | | Upper clamp for w0–w3. |
| FSRS5_DEFAULT_DECAY | `0.5` | `0.5` | | Used when a 17- or 19-parameter vector is padded to 21. |
| FSRS6_DEFAULT_DECAY | `0.1542` | `0.1542` | | Default w20. |
| R_MIN, R_MAX (simulator / desired-retention search range) | `0.70`, `0.95` | | | |

### 1.2 Forgetting curve (retrievability)

```
DECAY  = -w[20]                       // negative number
FACTOR = 0.9^(1/DECAY) - 1            // fsrs-rs writes it as exp(ln(0.9)/DECAY) - 1
R(t, S) = (1 + FACTOR * t / S)^DECAY
```

Property: R(S, S) = 0.9 for any w20. This is the wiki form `R = (1 + factor·t/S)^(−w20)` with `factor = 0.9^(−1/w20) − 1` ([wiki](https://github.com/open-spaced-repetition/awesome-fsrs/wiki/The-Algorithm)).

- FSRS-4.5 and FSRS-5 use DECAY = -0.5 and FACTOR = 19/81.
- FSRS v4 uses DECAY = -1 and FACTOR = 1/9.

`current_retrievability(state, days_elapsed, decay)` is a public helper in `FRS:inference.rs`. It takes a positive `decay` and computes `factor = 0.9^(1/-decay) - 1` and `R = (days/S*factor + 1)^(-decay)`. Anki calls it with fractional days (seconds/86400) for sorting and display (§9.3). The scheduler itself uses integer days.

### 1.3 Interval from desired retention r

```
I(r, S) = S / FACTOR * (r^(1/DECAY) - 1)
```

`FRS:model.rs next_interval` returns this as an unrounded f32. Callers round it.

- fsrs-rs's doc example: `interval_days = review.interval.round().max(1.0)`.
- Anki rounds and fuzzes (§4).
- ts-fsrs: `next_interval(s) = min(max(1, round(s * intervalModifier)), maximum_interval)`, then fuzz, where `intervalModifier = roundTo((r^(1/DECAY) - 1)/FACTOR, 8)` ([algorithm.ts](https://github.com/open-spaced-repetition/ts-fsrs/blob/c8ca282edc3fe1cdfa1c24912437938b63a25cb3/packages/fsrs/src/algorithm.ts)).

### 1.4 Initial state (first rating of a new card)

```
S0(G) = w[G-1]                              // Again→w0, Hard→w1, Good→w2, Easy→w3
D0(G) = w[4] - exp(w[5] * (G - 1)) + 1      // clamp to [1, 10] when used as a state
```

In fsrs-rs, `step()` treats `nth == 0 && state.stability == 0.0` as the first review. ts-fsrs treats `d === 0 && s === 0` as the first review, and it also floors the initial stability with `Math.max(w[g-1], 0.1)` ([algorithm.ts init_stability](https://github.com/open-spaced-repetition/ts-fsrs/blob/c8ca282edc3fe1cdfa1c24912437938b63a25cb3/packages/fsrs/src/algorithm.ts)). That floor is a ts-fsrs-only quirk; fsrs-rs only clamps to S_MIN.

### 1.5 Difficulty update (linear damping plus mean reversion)

```
ΔD        = -w[6] * (G - 3)
D'        = D + ΔD * (10 - D) / 9                     // linear damping
D''       = w[7] * D0(4) + (1 - w[7]) * D'            // mean reversion toward D0(Easy)
D_new     = clamp(D'', 1, 10)
```

fsrs-rs writes the mean reversion as `w7*(D0(4) - D') + D'`, which is algebraically identical. The difficulty update happens on **every** rated review, including same-day reviews.

### 1.6 Stability after a successful review (G ∈ {2,3,4}, t > 0)

```
S'_r = S * ( exp(w[8]) * (11 - D) * S^(-w[9]) * (exp(w[10] * (1 - R)) - 1)
             * (G == 2 ? w[15] : 1) * (G == 4 ? w[16] : 1) + 1 )
```

D is the **previous** difficulty and R = R(t, S_prev).

### 1.7 Stability after forgetting (G = 1, t > 0)

```
S'_f      = w[11] * D^(-w[12]) * ((S + 1)^w[13] - 1) * exp(w[14] * (1 - R))
S'_f_min  = S / exp(w[17] * w[18])
S_new     = min(S'_f, S'_f_min)
```

The second term caps post-lapse stability so that same-day relearning steps cannot push S above its pre-lapse value.

ts-fsrs computes `clamp(S/exp(w17*w18), S_MIN, S'_f)`. When `enable_short_term=false` it uses w17 = w18 = 0, so the cap is S itself.

### 1.8 Same-day (short-term) stability (t = 0), FSRS-6

```
SInc  = exp(w[17] * (G - 3 + w[18])) * S^(-w[19])
S_new = S * (G >= 2 ? max(SInc, 1) : SInc)
```

- When t == 0 this **replaces** both 1.6 and 1.7, for all four grades.
- The `max(SInc, 1)` for G ≥ 2 ("Non-decreasing SInc(Hard) in same-day reviews") arrived in fsrs-rs v6.0.0 ([PR #376](https://github.com/open-spaced-repetition/fsrs-rs/pull/376)). The wiki says "ensure SInc ≥ 1 when G ≥ 2".
- FSRS-5's version had no `S^(-w19)` term: `S' = S·exp(w17·(G−3+w18))` ([wiki](https://github.com/open-spaced-repetition/awesome-fsrs/wiki/The-Algorithm)). fsrs-rs reproduces FSRS-5 by padding w19 = 0.

### 1.9 Complete step function (fsrs-rs `step`)

```ts
function step(w: number[], deltaT: number, rating: number, state: {s:number,d:number}, nth: number) {
  const lastS = clamp(state.s, S_MIN, S_MAX), lastD = clamp(state.d, 1, 10);
  const r = forgettingCurve(w, deltaT, lastS);
  let newS = rating === 1 ? sAfterFailure(w, lastS, lastD, r) : sAfterSuccess(w, lastS, lastD, r, rating);
  if (deltaT === 0) newS = sShortTerm(w, lastS, rating);
  let newD = clamp(meanReversion(w, nextDifficulty(w, lastD, rating)), 1, 10);
  if (nth === 0 && state.s === 0) {            // first review of a new card
    const g = clamp(rating, 1, 4);
    newS = w[g - 1]; newD = clamp(initDifficulty(w, g), 1, 10);
  }
  if (rating === 0) { newS = lastS; newD = lastD; }
  return { s: clamp(newS, S_MIN, S_MAX), d: newD };
}
```

`next_states(state|None, desired_retention, days_elapsed)` runs `step` for ratings 1..4 and attaches `interval = next_interval(newS, r)` to each result (`FRS:inference.rs`). With `None` it uses state (0, 0) and nth = 0.

Note: in fsrs-rs `step` the stability is clamped but the **returned** `D` is not re-clamped after the rating-0 branch, and `forward_reviews` clamps the initial S/D. TS can clamp everywhere.

---

## 2. Parameters

### 2.1 Default vectors

| Version | Count | Defaults | Source |
|---|---|---|---|
| **FSRS-6** | 21 | `[0.212, 1.2931, 2.3065, 8.2956, 6.4133, 0.8334, 3.0194, 0.001, 1.8722, 0.1666, 0.796, 1.4835, 0.0614, 0.2629, 1.6483, 0.6014, 1.8729, 0.5425, 0.0912, 0.0658, 0.1542]` | `FRS:inference.rs DEFAULT_PARAMETERS`; same in ts-fsrs `default_w` and the wiki |
| FSRS-5 | 19 | `[0.40255, 1.18385, 3.173, 15.69105, 7.1949, 0.5345, 1.4604, 0.0046, 1.54575, 0.1192, 1.01925, 1.9395, 0.11, 0.29605, 2.2698, 0.2315, 2.9898, 0.51655, 0.6621]` | wiki; ts-fsrs FSRS-5.test.ts |
| FSRS-4.5 | 17 | `[0.4872, 1.4003, 3.7145, 13.8206, 5.1618, 1.2298, 0.8975, 0.031, 1.6474, 0.1367, 1.0461, 2.1072, 0.0793, 0.3246, 1.587, 0.2272, 2.8755]` | wiki |
| FSRS v4 | 17 | `[0.4, 0.6, 2.4, 5.8, 4.93, 0.94, 0.86, 0.01, 1.49, 0.14, 0.94, 2.18, 0.05, 0.34, 1.26, 0.29, 2.61]` | wiki |
| FSRS-7 (fsrs-rs main only, unreleased) | 34 | `[0.1104, 2.2395, 3.9221, 11.7841, 6.1686, 0.6457, 3.6807, 1.9795, 0.0, 1.3826, 0.7024, 0.5999, 0.8146, 0.6398, 1.0, 1.3207, 0.6707, 3.8668, 0.4416, 0.0934, 1.8631, 0.6162, 1.0869, 0.1567, 0.0801, 0.2421, 0.9464, 0.1433, 0.7145, 0.0, 0.5667, 0.3734, 0.5333, 0.3048]` | [inference_v7.rs@main](https://github.com/open-spaced-repetition/fsrs-rs/blob/4bc0a0979f95dd01cc653031b6f2a01429e4c32b/src/inference_v7.rs) |

Parameter meanings, from ts-fsrs `CLAMP_PARAMETERS` comments:

| Index | Meaning |
|---|---|
| w0–w3 | S0 for Again/Hard/Good/Easy |
| w4 | D0(Again) |
| w5 | D0 exponent |
| w6 | ΔD multiplier |
| w7 | mean-reversion weight |
| w8–w10 | recall-stability exp, S exponent, R exponent |
| w11–w14 | post-lapse multiplier, D power, S power, R exponent |
| w15 | Hard penalty |
| w16 | Easy bonus |
| w17–w19 | short-term parameters |
| w20 | decay |

### 2.2 Clamp bounds (fsrs-rs v6.6.2 `clip_parameters_in_place`, identical in ts-fsrs)

| i | low | high |
|---|---|---|
| 0–3 | S_MIN (0.001) | INIT_S_MAX (100) |
| 4 | 1.0 | 10.0 |
| 5 | 0.001 | 4.0 |
| 6 | 0.001 | 4.0 |
| 7 | 0.001 | 0.75 |
| 8 | 0.0 | 4.5 |
| 9 | 0.0 | 0.8 |
| 10 | 0.001 | 3.5 |
| 11 | 0.001 | 5.0 |
| 12 | 0.001 | 0.25 |
| 13 | 0.001 | 0.9 |
| 14 | 0.0 | 4.0 |
| 15 | 0.0 | 1.0 |
| 16 | 1.0 | 6.0 |
| 17, 18 | 0.0 | `w17_w18_ceiling` |
| 19 | `enable_short_term ? 0.01 : 0.0` | 0.8 |
| 20 | 0.1 | 0.8 |

```
w17_w18_ceiling = num_relearning_steps > 1
  ? min(2.0, sqrt(max(0.01, -(ln(w11) + ln(2^w13 - 1) + 0.3*w14) / num_relearning_steps)))
  : 2.0
```

Rationale, from the source comment: with D = 1, R = 0.7, S = 1, post-lapse S times `e^(n·w17·w18)` must not exceed S ([parameter_clipper.rs](https://github.com/open-spaced-repetition/fsrs-rs/blob/v6.6.2/src/parameter_clipper.rs)).

`FSRS::new` clips with `num_relearning_steps = 1` (ceiling 2.0) and `enable_short_term = false` (w19 floor 0).

Conflict: ts-fsrs rounds the ceiling to 8 decimal places and clamps it to [0.01, 2.0] instead of applying `max(0.01)` before the sqrt ([default.ts](https://github.com/open-spaced-repetition/ts-fsrs/blob/c8ca282edc3fe1cdfa1c24912437938b63a25cb3/packages/fsrs/src/default.ts)). This makes a minor difference near the bound.

### 2.3 Upgrading older vectors (fsrs-rs `check_and_fill_parameters`)

```
len 0  → DEFAULT_PARAMETERS
len 17 → w4 = w4 + 2*w5;  w5 = ln(1 + 3*w5)/3;  w6 = w6 + 0.5;  append [0, 0, 0, 0.5]
len 19 → append [0, 0.5]                       (w19 = 0, decay 0.5)
len 21 → as is
other  → error; any non-finite value → error
```

fsrs-rs test vector `test_convert_parameters`:
- in: `[0.4, 0.6, 2.4, 5.8, 4.93, 0.94, 0.86, 0.01, 1.49, 0.14, 0.94, 2.18, 0.05, 0.34, 1.26, 0.29, 2.61]`
- out: `[0.4, 0.6, 2.4, 5.8, 6.81, 0.44675013, 1.36, 0.01, 1.49, 0.14, 0.94, 2.18, 0.05, 0.34, 1.26, 0.29, 2.61, 0.0, 0.0, 0.0, 0.5]` ([model.rs tests](https://github.com/open-spaced-repetition/fsrs-rs/blob/v6.6.2/src/model.rs))

ts-fsrs `migrateParameters` does the same but clips **before** converting a 17-length vector and rounds to 8 decimal places.

Anki stores the vectors per preset as `fsrs_params_4`, `fsrs_params_5` and `fsrs_params_6`, and uses the first non-empty one in the order 6 → 5 → 4 ([deckconfig/mod.rs](https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/deckconfig/mod.rs)). The decay is taken from the params: empty → 0.1542; len < 21 → 0.5; else w20 (`get_decay_from_params`, [memory_state.rs](https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/scheduler/fsrs/memory_state.rs)).

### 2.4 Version differences (summary)

| | FSRS v4 | FSRS-4.5 | FSRS-5 | FSRS-6 |
|---|---|---|---|---|
| Parameters | 17 | 17 | 19 | 21 |
| Forgetting curve | (1+t/(9S))^-1 | (1+19/81·t/S)^-0.5 | same as 4.5 | (1+FACTOR·t/S)^-w20, w20 trainable in [0.1, 0.8] |
| D0 | w4 − (G−3)·w5 | same as v4 | w4 − e^{w5(G−1)} + 1 | same as 5 |
| D update | w7·D0(3) + (1−w7)(D − w6(G−3)) | same as v4 | linear damping, mean reversion to **D0(4)** | same as 5 |
| Same-day review | not modelled (ignored) | ignored | S·e^{w17(G−3+w18)} | S·e^{w17(G−3+w18)}·S^{−w19}, SInc ≥ 1 when G ≥ 2 |
| Recall / forget S | §1.6 / §1.7 | same | same (plus post-lapse cap via w17·w18 in fsrs-rs) | same |

Sources: [wiki](https://github.com/open-spaced-repetition/awesome-fsrs/wiki/The-Algorithm) and fsrs-rs code. The benchmark README describes FSRS-5 as "uses the same-day review data" and FSRS-6 as "improved same-day formula + optimizable forgetting-curve flatness" ([srs-benchmark](https://github.com/open-spaced-repetition/srs-benchmark/blob/bd9110f791e5b37282c55a9aa8db35f68f0c4aa2/README.md)).

Benchmark (no same-day reviews, ~10k users / 727M reviews), Log Loss / RMSE(bins) / AUC:

| Model | Log Loss | RMSE(bins) | AUC |
|---|---|---|---|
| FSRS-6 | 0.3460 | 0.0653 | 0.7034 |
| FSRS-rs (FSRS-6 with recency weighting) | 0.3443 | 0.0635 | |
| FSRS-5 | 0.3561 | 0.0742 | |
| FSRS-4.5 | 0.3625 | 0.0764 | |
| FSRS-7 | 0.3401 | 0.0634 | |
| FSRS-7 recency | 0.3370 | 0.0593 | |

---

## 3. Anki's FSRS integration (how the math above is wired into card states)

Source: `ANKI:rslib/src/scheduler/answering/mod.rs` and `states/{learning,review,relearning}.rs`.

### 3.1 Inputs on every answer

- `desired_retention`: the deck-level override if set, else the preset value (`home_deck.effective_desired_retention(&config)`).
- `params = config.fsrs_params()`; `fsrs = FSRS::new(params)`.
- If the card is not New and has no memory state, Anki computes it from the revlog first (§6).
- `days_elapsed` = `next_day_at.elapsed_days_since(card.last_review_time)`. `last_review_time` comes from `card.data.lrt`; if missing, it is the time of the last revlog entry. This is whole days counted against the **day-rollover** boundary (§9.1), not 24-hour periods.
- `fsrs.next_states(card.memory_state, desired_retention, days_elapsed)` produces 4 candidate (memory, interval) pairs.
- `fsrs_allow_short_term` = `params.len() >= 19 ? (w17 > 0 && w18 > 0) : params.is_empty()`. `fsrs_short_term_with_steps` is a global bool config, `FsrsShortTermWithStepsEnabled`, default false.

### 3.2 New / learning cards (FSRS on)

A **New** card is handled as a learning card with `remaining_steps = steps.len()`, `scheduled_secs = 0` and `memory_state = None` (`states/normal.rs`). Its memory state therefore comes from S0/D0.

| Button | If a learning step exists | If no step remains (graduation) |
|---|---|---|
| Again | first step | `interval = states.again.interval` |
| Hard | (see §8.2) | `states.hard.interval` |
| Good | next step | `states.good.interval` |
| Easy | n/a | `ReviewState{ scheduled_days = fuzz(round(max(1, easy.interval)), min = fuzz(good.interval)+1, max) }` |

- If a learning step exists, the card stays in learning and its `memory_state` is still updated to the FSRS result for that button. Because Δt = 0 for same-day steps, S follows the short-term formula of §1.8.
- On graduation: if `fsrs_allow_short_term && (short_term_with_steps || steps.is_empty()) && interval < 0.5`, the card stays in (re)learning with `scheduled_secs = interval*86400`. Otherwise it becomes a Review card with `scheduled_days = with_review_fuzz(round(max(1, interval)), 1, max_ivl)`.
- Easy always graduates (Anki `LearnState::answer_easy`).

### 3.3 Review cards (FSRS on)

From `ReviewState::passing_fsrs_review_intervals`:

```
hard = constrain(hard.interval, minimum = max(1, minFuzz(hard.interval, prev_ivl, max_ivl)))
good = constrain(good.interval, minimum = max(hard+1, minFuzz(good.interval, prev_ivl, max_ivl)))
easy = constrain(easy.interval, minimum = max(good+1, minFuzz(easy.interval, prev_ivl, max_ivl)))
constrain(x, min) = with_review_fuzz(x, clamp(min,1,max_ivl), max(max_ivl,1))   // no interval modifier under FSRS
```

`minimum_review_fuzz_interval(interval, prev, max)` (fuzz.rs):

```
rounded = round(interval); (_, upper) = constrained_fuzz_bounds(interval, 1, max)
if rounded > prev            → prev + 1
else if prev <= upper        → prev          // "don't let fuzz reduce it"
else                         → 0             // interval genuinely shrank (params/DR changed)
```

Test vectors: `(2.7269483, 4, 36500) → 4`; `(2.7269483, 5, 36500) → 0`; `(4.591988, 4, 36500) → 5`.

**Again on a review card:**
- `lapses += 1`.
- Ease is set to `max(1.3, ease − 0.2)`. The SM-2 field is still maintained under FSRS.
- `scheduled_days = round(max(1, again.interval))`.
- If relearning steps exist, the card goes to Relearn at step 0. If there are no steps, short-term is allowed and `again.interval < 0.5`, it goes to Relearn with `scheduled_secs = interval*86400`. Otherwise it becomes Review.

### 3.4 Relearning cards (FSRS on)

Relearning works like learning, but on graduation the card uses `with_review_fuzz(round(max(1, X.interval)), 1, max)` for X = Hard/Good. Easy uses `min = fuzz(good)+1`.

---

## 4. Fuzz (Anki, exact)

Source: [`ANKI:rslib/src/scheduler/states/fuzz.rs`](https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/scheduler/states/fuzz.rs).

```
FUZZ_RANGES = [{start:2.5,end:7,factor:0.15},{start:7,end:20,factor:0.10},{start:20,end:∞,factor:0.05}]
fuzz_delta(ivl) = ivl < 2.5 ? 0 : 1 + Σ factor * max(0, min(ivl, end) - start)
fuzz_bounds(ivl) = (round(ivl - Δ), round(ivl + Δ))
constrained_fuzz_bounds(ivl, min, max):
   min = min(min, max); ivl = clamp(ivl, min, max); (lo, hi) = fuzz_bounds(ivl)
   lo = clamp(lo, min, max); hi = clamp(hi, min, max)
   if hi == lo && hi > 2 && hi < max: hi = lo + 1
with_review_fuzz(factor?, ivl, min, max):
   factor == None → clamp(round(ivl), min, max)
   else (lo, hi) = constrained_fuzz_bounds(ivl, min, max); floor(lo + factor * (1 + hi - lo))
```

**Seed.** `fuzz_seed = card.id + card.reps` (u64 wrapping add; `reps` is the value *before* this answer; for rescheduling it is `reps − 1`). Then `fuzz_factor = StdRng::seed_from_u64(seed).random_range(0.0..1.0)`. Rust's `StdRng` is ChaCha12, so bit-exact reproduction in TS needs a ChaCha port. A different PRNG is fine as long as it is deterministic per (card id, reps).

Fuzz is disabled in Anki's unit tests (`fuzz_seed = None`).

**Load balancer.** When the optional load balancer is enabled (`BoolKey::LoadBalancerEnabled`), `find_interval` picks within the same [lo, hi] range, weighted by due counts and Easy Days. Otherwise the formula above is used.

**Learning-step fuzz.** `secs + uniform[0, min(25% of secs, 300 s))` (`answering/learning.rs learning_ivl_with_fuzz`).

Test vectors (fuzz factor 0 / 0.5 / 0.99 → interval) from `fuzz.rs` tests:

| (interval, min, max) | factor 0 | factor 0.5 | factor 0.99 |
|---|---|---|---|
| (1.0, 1, 1000) | 1 | 1 | 1 |
| (2.49, 1, 1000) | 2 | 2 | 2 |
| (2.5, 1, 1000) | 2 | 3 | 4 |
| (7.0, 1, 1000) | 5 | 7 | 9 |
| (17.0, 1, 1000) | 14 | 17 | 20 |
| (37.0, 1, 1000) | 33 | 37 | 41 |
| (2.0, 3, 1000) | 3 | 4 | 4 |
| (19.9, 3, 1000) | 17 | 20 | 23 |
| (100, 101, 1000) | 101 | 105 | 108 |
| (100, 1, 99) | 92 | 96 | 99 |
| (100, 97, 103) | 97 | 100 | 103 |

With no fuzz: `(1.5,1,100)→2`, `(0.1,1,100)→1`, `(101,1,100)→100`.

Conflict: ts-fsrs and py-fsrs fuzz differ slightly.
- ts-fsrs `get_fuzz_range` uses `min_ivl = max(2, round(ivl−Δ))`, raises min to `elapsed_days+1` when `ivl > elapsed_days`, and uses an Alea PRNG ([help.ts](https://github.com/open-spaced-repetition/ts-fsrs/blob/c8ca282edc3fe1cdfa1c24912437938b63a25cb3/packages/fsrs/src/help.ts)). Fuzz is **off by default** (`default_enable_fuzz = false`).
- py-fsrs uses Python `random` and `round(ivl ± Δ)`.
- Use Anki's version if "behaves like Anki" is the goal.

---

## 5. Anki storage: card.data and revlog

### 5.1 `cards.data` JSON

Source: [`ANKI:rslib/src/storage/card/data.rs`](https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/storage/card/data.rs).

| key | type | meaning | rounding on write |
|---|---|---|---|
| `pos` | u32 | original new-card position (to restore on "forget") | |
| `s` | f32 | FSRS stability (days) | 4 dp |
| `d` | f32 | FSRS difficulty, 1–10 | 3 dp |
| `dr` | f32 | desired retention in effect for this card | 2 dp |
| `decay` | f32 | decay (w20 or 0.5) | 3 dp |
| `lrt` | i64 | last review time, unix **seconds** | |
| `cd` | string | v3 custom-scheduling data (JSON object string, keys ≤ 8 chars) | |

- Keys are omitted when None.
- Invalid values deserialize to default (`default_on_invalid`).
- The memory state exists only when both `s` and `d` are present.
- The source says the scheduler does **not** read `dr` or `decay` from the card. They exist "so that add-ons, card info, stats and browser search/sorts don't need to access the deck config" (memory_state.rs comment). The scheduler takes DR and params from the deck config.

Card columns used by the scheduler:
- `type`: 0 new, 1 learn, 2 review, 3 relearn.
- `queue`: 0 new, 1 learn (`due` = unix secs), 2 review (`due` = day number), 3 day-learn (`due` = day number), 4 preview, −1 suspended, −2 sched-buried, −3 user-buried.
- `ivl` (days), `factor` (ease ×1000, e.g. 2500), `reps`, `lapses`, `left` (remaining steps; `% 1000` is used), `odue`/`odid` (filtered decks).

([card/mod.rs](https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/card/mod.rs))

### 5.2 `revlog` row

Source: [`ANKI:rslib/src/revlog/mod.rs`](https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/revlog/mod.rs).

| column | meaning |
|---|---|
| `id` | answer time in unix **ms** (also the primary key) |
| `cid` | card id |
| `usn` | sync sequence number |
| `ease` | button 1–4; 0 = manual reschedule / reset |
| `ivl` | new interval: >0 days, <0 seconds |
| `lastIvl` | previous interval: >0 days, <0 seconds |
| `factor` | SM-2: ease ×1000. **FSRS: `round((((D−1)/9) + 0.1) * 1000)`**, i.e. 100–1100 (`difficulty_shifted`), so it can be told apart from SM-2 learning (0) |
| `time` | ms taken, capped by "maximum answer seconds" |
| `type` | 0 Learning, 1 Review, 2 Relearning, 3 Filtered (early/cram), 4 Manual, 5 Rescheduled |

Revlog predicates:
- "Reset": `type==Manual && factor==0`.
- "Cramming": `type==Filtered && factor==0`.
- `has_rating`: `ease > 0`.
- Entries that affect scheduling: `has_rating && !cramming`.

### 5.3 Revlog → FSRS items (`reviews_for_fsrs`)

Source: [`params.rs`](https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/scheduler/fsrs/params.rs).

1. Scan the card's entries newest → oldest, skipping cramming.
   - Track `first_of_last_learn_entries`: the oldest Learning entry of the most recent contiguous learning block. Finding one sets `revlogs_complete = true`.
   - Track `first_user_grade_idx`: the oldest graded, inter-day (`ivl ≥ 1` or `ivl ≤ −86400`) entry after the ignore-before cutoff.
   - A Reset entry stops the scan. If a learning block followed the reset, history is complete. Else if a user grade followed, history is incomplete. Else return None.
2. Training: drop the whole card if its last learning block began before "ignore reviews before". When scoring a single card, a learning block before the cutoff instead marks history incomplete.
3. Training requires a learning block (cards without one are skipped). Scoring falls back to `first_user_grade_idx`.
4. Keep only `has_rating_and_affects_scheduling` entries.
5. `days_elapsed(entry) = max(0, floor((next_day_at − entry_secs)/86400))`. Then `delta_t[0] = 0` and `delta_t[i] = days_elapsed(prev) − days_elapsed(cur)`. Days are therefore measured in **scheduler days** (rollover-aligned).
6. Training emits one `FSRSItem` per entry with `idx ≥ 1 && delta_t > 0`, holding all reviews up to and including that one (same-day reviews stay in the history). Scoring emits one item with the full history.

Test vectors (rating 3 everywhere, `NEXT_DAY_AT = 86400*1000`):
- Learning@15d ago, Learning@13d, Review@10d, Review@5d → items `[0,2]`, `[0,2,3]`, `[0,2,3,5]` (delta_t lists).
- Learning@10, Review@9, Filtered(cram)@7, Review@4 → `[0,1]`, `[0,1,5]`.
- A single Learning entry → `None` when training, `[[0]]` when scoring.

### 5.4 Converting SM-2 history to FSRS (enabling FSRS, or a missing memory state)

`update_memory_state` is called when FSRS is turned on or params/DR are saved.

For each non-new card:

1. Build the item as in §5.3 (scoring mode).
2. If `revlogs_complete`: `memory_state(item, starting_state=None)`, i.e. replay from the first learning review.
3. Else (history truncated):
   - `first = filtered_revlogs[0]`
   - `ef = (first.factor == 0 ? 2500 : first.factor) / 1000`
   - `ivl = max(first.ivl, 1)`
   - `starting = memory_state_from_sm2(ef, ivl, historical_retention)`
   - If `ef ≤ 1.1`, the entry was written by FSRS, so `starting.d = (ef − 0.1)*9 + 1`.
   - Drop the first review, then replay the rest from `starting`.
4. If there is no usable revlog but the card is not New and `ivl > 0`: `memory_state_from_sm2(card.factor/1000, card.ivl, historical_retention)`.
5. Write `s`, `d`, `dr`, `decay` into card.data.
6. Optionally ("Reschedule cards on change") recompute `ivl` and `due` from `next_interval(S, DR)`, with fuzz seeded by `reps−1`, and add a `Rescheduled` revlog row.

`memory_state_from_sm2(ease, interval, r)`, with `historical_retention` defaulting to 0.9 ([inference.rs](https://github.com/open-spaced-repetition/fsrs-rs/blob/v6.6.2/src/inference.rs)):

```
S = max(interval, S_MIN) * FACTOR / (r^(1/DECAY) − 1)          // S = interval when r = 0.9
D = clamp(11 − (ease − 1) / (exp(w8) * S^(−w9) * (exp((1 − r)*w10) − 1)), 1, 10)
```

This inverts §1.6 so that one Good review at R = r reproduces SM-2's growth factor `ease`. The fsrs-rs test asserts that `S_after_good / interval ≈ ease ± 0.01` for ease 2.0 and interval 15.

---

## 6. The optimizer (fsrs-rs v6.6.2 `compute_parameters`, as called by Anki)

Sources: [`training.rs`](https://github.com/open-spaced-repetition/fsrs-rs/blob/v6.6.2/src/training.rs), [`parameter_initialization.rs`](https://github.com/open-spaced-repetition/fsrs-rs/blob/v6.6.2/src/parameter_initialization.rs), [`dataset.rs`](https://github.com/open-spaced-repetition/fsrs-rs/blob/v6.6.2/src/dataset.rs), [`cosine_annealing.rs`](https://github.com/open-spaced-repetition/fsrs-rs/blob/v6.6.2/src/cosine_annealing.rs), and Anki [`params.rs compute_params`](https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/scheduler/fsrs/params.rs).

### 6.1 Pipeline

1. **Items.** Build items as in §5.3 for every card in the preset search (default `preset:"<name>" -is:suspended`). Sort all items by revlog id, i.e. chronologically. Reject any rating outside 1–4.
2. **Outlier filter + split** (`prepare_training_data`).
   - Take items with exactly one long-term review (`delta_t > 0` count == 1). These are the "first-interval" items.
   - Group them by (first rating, delta_t of that review).
   - Within each first-rating group, sort subgroups by size descending, ties by delta_t descending. Walk from the **smallest** subgroups and remove them until at least `max(20, total/20)` items have been removed.
   - After that, keep a subgroup only if `size ≥ 6` and `delta_t ≤ 100` (≤ 365 for rating 4). Otherwise mark it removed.
   - The training set drops every item whose (first rating, first long-term delta_t) pair was removed.
3. **Thresholds.**
   - `train_set.len() < 8` → return the **defaults**.
   - Run pretraining (step 4).
   - If `train_set.len() == init_set.len()` (no item has a second long-term review) or `train_set.len() < 64` → return **pretrained w0–w3 + default w4–w20** with no gradient descent.
   - Note: the FAQ says "Do I need 1000 reviews…? That was the case in earlier versions… In Anki 24.06.3 (and newer), the optimizer can be used with any number of reviews" ([FSRS FAQ](https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/docs-site/faqs/frequently-asked-questions-about-fsrs.mdx)). The old 400/1000 limits no longer apply. A code comment still mentions "less than 400 items" but does not enforce it.
4. **Pretrain w0–w3** (`initialize_stability_parameters`).
   - For each first rating g, and for each distinct first long-term delta_t, compute `recall` = mean(pass), where pass = rating > 1. Count `n` as the number of items.
   - Laplace-smooth: `recall' = (recall*n + avg_recall)/(n+1)`, where `avg_recall` is the pass rate over all training items' last reviews.
   - Ternary search S ∈ [S_MIN, 100] (≤ 1000 iterations, stop when `hi−lo ≤ f64::EPSILON`) minimizing `Σ n·BCE(recall', R(t,S)) + |S − default_w[g−1]|/16`, where R uses the **default** decay 0.1542.
   - Then `smooth_and_fill`:
     - Enforce monotonicity: for pairs (1,2),(2,3),(3,4),(1,3),(2,4),(1,4), if S(small) > S(big), overwrite the value of the rating with fewer samples using the other one.
     - Fill missing ratings by the geometric interpolation formulas in the source, with constants w1 = 0.41 and w2 = 0.54. Example: `r1 = r2^(1/w1) * r3^(1 − 1/w1)`; `r4 = r2^(1 − 1/w2) * r3^(1/w2)`; `r2 = r1^w1 * r3^(1−w1)`; `r3 = r2^(1−w2) * r4^w2`.
     - With only one rating present, scale all four defaults by `S(g)/default(g)`.
     - Clamp each to [S_MIN, 100].
   - No pretraining data at all → error `NotEnoughData`.
5. **Weights.** Recency weighting in chronological order: `weight_i = 0.25 + 0.75 * (i/(N−1))^3`. Drop items with `reviews.len() > max_seq_len` (default 256).
6. **Model output per item.** Run `step` over `reviews[0..n−1]` (first review initialises S0/D0, then Δt-driven updates). Prediction is `p = R(Δt_last, S)`; label is `y = rating_last > 1`.
7. **Loss.** Weighted binary cross-entropy, `L = −Σ wᵢ [yᵢ ln pᵢ + (1−yᵢ) ln(1−pᵢ)] / Σ wᵢ`, plus an L2 prior toward the initial (pretrained) parameters:
   ```
   reg = gamma * (batch_size/total_size) * Σ_i (w_i − w_init_i)^2 / PARAMS_STDDEV_i^2
   PARAMS_STDDEV = [6.43, 9.66, 17.58, 27.85, 0.57, 0.28, 0.6, 0.12, 0.39, 0.18, 0.33, 0.3, 0.09, 0.16, 0.57, 0.25, 1.03, 0.31, 0.32, 0.14, 0.27]
   gamma = 1.0
   ```
8. **Optimizer.**
   - Adam with β1 = 0.9, β2 = 0.999, ε = 1e-8 and bias correction (`HostAdam`).
   - lr = 4e-2 with **cosine annealing** to 0 over `T_max = (N/512 + 1) * epochs` steps (PyTorch `CosineAnnealingLR` recurrence, `eta_min = 0`).
   - Batch size 512. Batch order is shuffled each epoch with `StdRng::seed_from_u64(2023)`.
   - `TrainingConfig::default()`: epochs = 5. **Anki passes `num_epochs: 8`.**
   - After **every** step, clip parameters (§2.2) with `num_relearning_steps`. Anki passes the count of relearning steps whose cumulative length stays under 1 day (FsrsOptions.svelte), and `enable_short_term = true` (so the w19 floor is 0.01).
   - Gradient freezing: if `!enable_short_term`, zero the gradients of w0–w3 and w17–w19. Anki always passes `true`, so nothing is frozen.
9. **Post-processing.** Re-run `smooth_and_fill` on the optimized w0–w3 using the pretraining counts.
10. **Anki's acceptance test.**
    - Evaluate log loss of current params vs new params on the same items.
    - If `current ≤ new`, keep current. When there are more than 1 relearning step, keep current only if repeated Good presses on a failed card would not lift S past 1.0.
    - The UI reports "parameters appear optimal" if every value matches to 4 dp.

### 6.2 Evaluate / metrics

- **"Evaluate" button** (`evaluate_params_legacy`) runs `fsrs.evaluate(items)` with the current params on all items (recency-weighted). The UI shows `Log loss: x.xxxx, RMSE(bins): y.yy%`.
- **RMSE(bins)** uses `r_matrix_index` per item:
  ```
  delta_t_bin  = round(2.48 * 3.62^floor(log_3.62(Δt_last)) * 100)
  length_bin   = round(1.99 * 1.89^floor(log_1.89(long_term_review_cnt + 1)))
  lapse_bin    = lapses==0 ? 0 : round(1.65 * 1.73^floor(log_1.73(lapses)))   // lapses = Again with Δt>0 in history
  per bin: pred_mean = Σp/count, real_mean = Σy/count, weight = Σ item weights
  RMSE(bins) = sqrt( Σ_bins weight*(pred_mean − real_mean)^2 / Σ_bins weight )
  ```
- **Health check** (optional, only if > 300 items): `evaluate_with_time_series_splits` with 5 time-ordered splits, train on the past and test on the next. With `r` = pass rate and `c` = item count:
  - `adjusted_logloss = logloss / (0.623*(4r(1−r))^0.738)`
  - `adjusted_rmse = rmse / (0.0135/(r^0.504 − 1.14) + 0.176/((c/1000)^0.825 + 2.22) + 0.101)`
  - It passes if `adjusted_logloss ≤ 1.11 || adjusted_rmse ≤ 1.53`.
- **Reference metrics** (fsrs-rs `test_evaluate` on the bundled Anki sample collection):
  - Default params give `[log_loss, rmse_bins] = [0.20967911, 0.030774858]`.
  - The 21-param set in the test gives `[0.20580745, 0.026005825]`.
  - These need the sample file, so they are not portable.

---

## 7. Reference test vectors (verbatim from source tests)

All values are copied from the named test files. Tolerances are as stated in the source (fsrs-rs `assert_approx_eq` is ~1e-4 relative/absolute; `toBeCloseTo(x, 4)` in ts-fsrs).

I re-derived vectors 7.1–7.6 with an independent f64 implementation of §1 (stdlib Python) and they matched to the printed precision. The exception is the f32/f64 interval difference noted in 7.5. This confirms that the formulas in §1 are complete.

### 7.1 Same-day + long-term sequence, default FSRS-6 params (fsrs-rs, ts-fsrs, py-fsrs agree)

Sources:
- fsrs-rs `test_memory_state` ([inference.rs](https://github.com/open-spaced-repetition/fsrs-rs/blob/v6.6.2/src/inference.rs))
- ts-fsrs `FSRS-6.test.ts` "memory state" ([link](https://github.com/open-spaced-repetition/ts-fsrs/blob/c8ca282edc3fe1cdfa1c24912437938b63a25cb3/packages/fsrs/__tests__/FSRS-6.test.ts))
- py-fsrs `test_memo_state` ([link](https://github.com/open-spaced-repetition/py-fsrs/blob/9446cb06605c597a063aeee49f7d188d42e34dc2/tests/test_basic.py))

```
ratings   = [1, 3, 3, 3, 3, 3]          // Again, Good×5
delta_t   = [0, 0, 1, 3, 8, 21]         // days since previous review (first is the new card)
DR = 0.9, params = DEFAULT (21)
expected final: S = 53.62691,  D = 6.3574867
with w17=w18=w19=0 (short-term frozen) / ts-fsrs enable_short_term=false:
                S = 53.335106, D = 6.3574867
```

Intermediate states (my f64 derivation, for debugging):

| step | S | D |
|---|---|---|
| 1 | 0.212 | 6.4133 |
| 2 | 0.24669 | 6.40212 |
| 3 | 2.02148 | 6.39094 |
| 4 | 7.86370 | 6.37978 |
| 5 | 21.91771 | 6.36863 |
| 6 | 53.6269 | 6.35749 |

### 7.2 First rating of a new card (default params)

From the fsrs-rs doc-test on `next_states` (`FRS:inference.rs`), with `next_states(None, 0.9, 0)`:

| Button | S | D | interval |
|---|---|---|---|
| Again | 0.212 | 6.4133 | 0.212 |
| Hard | 1.2931 | 5.1121707 | 1.2931 |
| Good | 2.3065 | 2.118104 | 2.3065 |
| Easy | 8.2956 | 1.0 | 8.2956 |

ts-fsrs `first repeat` gives the same:
- stability `[0.212, 1.2931, 2.3065, 8.2956]`
- difficulty `[6.4133, 5.11217071, 2.11810397, 1]`
- `scheduled_days = [0, 0, 0, 8]` (learning steps 1m/10m are on, so Again/Hard/Good stay in Learning)
- `states = [Learning, Learning, Learning, Review]`

### 7.3 Formula unit vectors (default params)

From fsrs-rs `model.rs` tests and ts-fsrs `algorithm.test.ts`:

```
next_difficulty(D=5, G=1..4) before mean reversion: [8.354889, 6.6774445, 5.0, 3.3225555]
after mean reversion:                                [8.341763, 6.6659956, 4.990228, 3.3144615]
   (ts-fsrs, rounded to 8 dp:                        [8.34176237, 6.66599536, 4.99022837, 3.31446137])

S_recall(S=5, D=[1,2,3,4], R=[0.9,0.8,0.7,0.6], G=[1,2,3,4]):
   fsrs-rs: [25.602541, 28.226582, 58.656002, 127.226685]
   ts-fsrs: [25.60252118, 28.22657096, 58.65599107, 127.2266925]
   (note: the fsrs-rs test passes D=rating, so D=1..4 paired with G=1..4)
S_forget(S=5, D=[1,2,3,4], R=[0.9,0.8,0.7,0.6]):
   fsrs-rs: [1.0525396, 1.1894329, 1.3680838, 1.584989]
   ts-fsrs: [1.05253961, 1.18943295, 1.36808387, 1.58498896]
S_short_term(S=5, G=1..4): [1.596818, 5.0, 5.0, 8.12961]     (ts-fsrs: [1.596818, 5, 5, 8.12960956])
init_difficulty(G) = w4 − exp(w5*(G−1)) + 1 → [6.4133, 5.1121707, 2.118104, 1 (clamped; raw value ≈ −4.77)]
```

### 7.4 Forgetting curve

| Case | Inputs | Expected R |
|---|---|---|
| Default FSRS-6 decay 0.1542 (fsrs-rs `test_power_forgetting_curve`) | (t, S) = (0,1),(1,2),(2,3),(3,4),(4,4),(5,2) | `[1.0, 0.9403443, 0.9253786, 0.9185229, 0.9, 0.8261359]` |
| Decay 0.1542, S = 1 (ts-fsrs; fsrs-rs pretrain test) | t = 0,1,2,3 | `[1.0, 0.9, 0.84588465, 0.8093881]` (f64: `[1.0, 0.9, 0.8458846447796301, 0.8093881028681906]`) |
| Decay 0.2, S = 1 (`current_retrievability`) | t = 0,1,2,3 | `1.0, 0.9, 0.84028935, 0.7985001` |
| ts-fsrs `get_retrievability` | a card first rated Easy (S = 8.2956) and evaluated at its due date 8 days later | 0.9024733 (`'90.25%'`) |

### 7.5 Interval vs desired retention (S = 1, default params)

From fsrs-rs `test_next_interval`, using `round().max(1)`:

```
DR = 0.1 … 1.0 → [3116766, 34793, 2508, 387, 90, 27, 9, 3, 1, 1]
```

ts-fsrs gets `3116766 + 3` for DR = 0.1 "due to differing numeric precision: fsrs-rs uses f32, while ts-fsrs uses f64". My f64 value was 3116769, which agrees with ts-fsrs.

Also `next_interval(S=121.01552, DR=0.9) == 121.01551` (f32).

### 7.6 FSRS-5-style 19-parameter set (padded to w19 = 0, w20 = 0.5)

From fsrs-rs `test_memo_state` / `test_next_states`.

```
PARAMETERS = [0.6845422, 1.6790825, 4.7349424, 10.042885, 7.4410233, 0.64219797, 1.071918,
              0.0025195254, 1.432437, 0.1544, 0.8692766, 2.0696752, 0.0953, 0.2975, 2.4691248,
              0.19542035, 3.201072, 0.18046261, 0.121442534]
reviews (rating, delta_t) = [(1,0),(3,1),(3,3),(3,8),(3,21)] → S = 31.722992, D = 7.382128
next_states(Some{S=20.925528, D=7.005062}, 0.9, 21).good.memory = {S=40.87456, D=6.9913807}
After [(1,0),(3,1),(3,3),(3,8)], next_states(state, 0.9, 21):
   again {S=2.9691455, D=8.000659}, hard {S=17.091452, D=7.6913934},
   good  {S=31.722992, D=7.382128}, easy {S=71.7502, D=7.0728626}   (interval == S at DR 0.9)
```

### 7.7 SM-2 → FSRS conversion (default params)

From fsrs-rs `test_memory_from_sm2`:

```
memory_state_from_sm2(ease=2.5, ivl=10, r=0.9)  → S=10.0,     D=6.9140563
memory_state_from_sm2(2.5, 10, 0.8)              → S=3.01572,  D=9.393428
memory_state_from_sm2(2.5, 10, 0.95)             → S=24.841097,D=1.2974405
memory_state_from_sm2(1.3, 20, 0.9)              → S=20.0,     D=10.0
```

Anki's own tests ([memory_state.rs tests](https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/scheduler/fsrs/memory_state.rs)) compare rounded values only:
- A truncated history (Review ease 2500, ivl 100 at 99 days ago, then Review today) gives starting state ≈ {S 100, D 5.003576}, and after replay ≈ {S 248.9251, D 4.9938006}.
- A card with no revlog (Review, ivl 100, factor 1300) gives ≈ {S 99.999954, D 9.979899}.

### 7.8 Full scheduler sequences (learning steps 1m/10m, relearning 10m, DR 0.9, no fuzz, default FSRS-6)

ts-fsrs `ivl_history` and py-fsrs `test_review_card`, identical in both:

```
ratings  = [G,G,G,G,G,G,A,A,G,G,G,G,G]
start 2022-12-29 12:30 (ts) / 2022-11-29 12:30 UTC (py); each next review at card.due
scheduled_days history = [0, 2, 11, 46, 163, 498, 0, 0, 2, 4, 7, 12, 21]
```

I reproduced this with §1 plus this state machine:
- New + G → step 1 (10 min).
- G at step 1 with Δt = 0 → short-term S, graduate with `round(S·IM)` (IM = interval modifier from DR).
- Review + A → relearning (10 min).
- Relearning + A → stays at step 0.
- Relearning + G → graduate.

The FSRS-5 equivalent (ts-fsrs `FSRS-5.test.ts`, 19-param defaults):
- `ivl_history = [0, 4, 14, 44, 125, 328, 0, 0, 6, 14, 31, 65, 130]`
- memory-state test S = 48.27115294, D = 7.10441712 (short-term); S = 48.065163 (long-term)
- first repeat difficulty `[7.1949, 6.48830527, 5.28243442, 3.22450159]`, scheduled_days `[0,0,0,16]`

py-fsrs `test_repeated_correct_reviews`: 10 Easy presses one microsecond apart → `difficulty == 1.0`.

### 7.9 Anki SM-2 vectors

From pylib `test_schedv3.py::test_reviews` ([link](https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/pylib/tests/test_schedv3.py)), with fuzz off.

Setup: Review card, ivl = 100, factor = 2500, due 8 days ago, lapses = 1. Expected results:

| Button | New interval | Factor |
|---|---|---|
| Hard | 100 × 1.2 = **120** | 2350 |
| Good | (100 + 8/2) × 2.5 = **260** | 2500 |
| Easy | (100 + 8) × 2.5 × 1.3 = **351** | 2650 |

Leech case: lapses = 7, leech action Suspend, press Again → the card is suspended and the note gets tag `leech` (threshold 8).

`leech_threshold_met(lapses, threshold)`:
- threshold 3: true at 3, 5, 7.
- threshold 8: true at 8, 12, 16; false at 7, 9, 10, 11, 13.
- threshold 1: true at every lapse ≥ 1.
- threshold 0: never.

The fuzz table in §4 also serves as test vectors.

---

## 8. Anki's SM-2 variant (v3 scheduler, FSRS off)

Sources: [`states/review.rs`](https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/scheduler/states/review.rs), `learning.rs`, `relearning.rs`, `steps.rs`, [`deckconfig/mod.rs`](https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/deckconfig/mod.rs) and the [manual](https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/docs-site/manual/deck-options.mdx).

### 8.1 Defaults (`DeckConfig::default`)

| Setting | Default |
|---|---|
| learn steps | `[1, 10]` min |
| relearn steps | `[10]` min |
| new/day | 20 |
| reviews/day | 200 |
| graduating interval (Good) | 1 day |
| easy interval | 4 days |
| starting ease | 2.5 (2500) |
| easy bonus | 1.3 |
| hard interval | 1.2 |
| interval modifier | 1.0 |
| new interval (lapse multiplier) | 0.0 |
| minimum interval | 1 |
| maximum interval | 36500 |
| leech threshold | 8 |
| leech action | Tag Only (enum: Suspend = 0, TagOnly = 1) |
| desired retention | 0.9 |
| historical retention | 0.9 |
| answer-time cap | 60 s |
| bury new / review / interday learning | all false |
| new gather | Deck |
| new sort | Template (card type, then order gathered) |
| review order | Day (due date, then random) |
| new mix / interday learning mix | Mix with reviews |
| Easy days | 1.0 × 7 |

Ease bounds: minimum 1.3. Deltas: Again −0.20, Hard −0.15, Easy +0.15, Good 0 (`review.rs` constants).

### 8.2 Learning steps (`steps.rs`)

Steps are minutes (f32), stored as seconds = `floor(min*60)`. `remaining` is the card's `left % 1000`. `idx = clamp(len − remaining, 0, len−1)`.

| Button | Result |
|---|---|
| Again | first step; `remaining = len` |
| Hard on step 0 | if a 2nd step exists: `(s0 + s1)/2` (integer secs); else `min(s0*1.5, s0 + 1 day)`. Either way, if > 1 day, round to whole days. On other steps, Hard **repeats the current step** |
| Good | step `idx+1` if it exists (`remaining = len − (idx+1)`), else graduate |
| Easy | always graduates |

Example: 1m/10m → Hard on first step = 330 s = 5.5 min.

Conflict: ts-fsrs rounds the Hard interval to whole minutes (6 min) and uses "(first+second)/2" regardless of the current step ([learning_steps.ts](https://github.com/open-spaced-repetition/ts-fsrs/blob/c8ca282edc3fe1cdfa1c24912437938b63a25cb3/packages/fsrs/src/strategies/learning_steps.ts)). py-fsrs asserts 330 s, like Anki.

Graduation (SM-2):
- Good → Review with `scheduled_days = fuzz(graduating_interval_good)`, ease = starting ease.
- Easy → `fuzz(graduating_interval_easy)`.
- With no learning steps, Again/Hard/Good graduate to `graduating_interval_good`.

A step that **reaches or passes the next day rollover** is converted to whole days: `days = (secs − secs_until_rollover)/86400 + 1`. The card goes to queue 3 (DayLearn) with `due = today + days` (`interval_kind.rs`, `answering/learning.rs`).

### 8.3 Review answers (SM-2, not early)

Let `ivl = max(1, scheduled_days)`, `late = max(0, elapsed_days − scheduled_days)`, `IM` = interval modifier, `fz` = with_review_fuzz.

```
hard_min  = hard_mult <= 1 ? 0 : scheduled_days + 1
hard      = fz(ivl * hard_mult * IM,                    min=hard_min)
good_min  = hard_mult <= 1 ? scheduled_days + 1 : hard + 1
good      = fz((ivl + late/2) * ease * IM,              min=good_min)
easy      = fz((ivl + late) * ease * easy_mult * IM,    min=good + 1)
all bounded to [max(1, min), max_ivl]
ease: Hard −0.15 (floor 1.3), Good unchanged, Easy +0.15
```

`elapsed_days` for a review card is `ivl − (due − today)`.

**Early review** (only in filtered decks with rescheduling, when `days_late < 0`). There is no fuzz and no minimum beyond 1:
- hard = `max(elapsed*hard_mult, scheduled*hard_mult/2)`
- good = `max(elapsed*ease, scheduled)`
- easy = `max(elapsed*ease, scheduled) * (easy_mult − (easy_mult−1)/2)`
- Then multiply by IM.

**Lapse (Again):**
- `lapses += 1`, `ease = max(1.3, ease − 0.2)`.
- `new_ivl = fz(max(1, ivl) * lapse_mult, min = minimum_lapse_interval, max)`. With the defaults (0.0 multiplier, min 1) this gives 1 day.
- If relearning steps exist, the card enters Relearn at step 0 and keeps `new_ivl` as its pending review interval. Without steps it goes straight to Review.
- Leech check: `lapses >= threshold && (lapses − threshold) % ceil(threshold/2) == 0`. When it triggers, add tag `leech`; if the action is Suspend, set queue = −1.

**Relearning (SM-2):**
- Steps work as in learning.
- Graduating with Good (or Hard past the steps) restores the pending review state (`scheduled_days = new_ivl`).
- Easy uses `new_ivl + 1`.

The manual adds: "Anki forces a new interval to be at least 1 day longer than it was previously" (deck-options: Interval Modifier). That is the `scheduled_days + 1` minimum above.

---

## 9. Days, queues, counts, display order

### 9.1 Day rollover

Source: [`timing.rs`](https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/scheduler/timing.rs).

- The **Next day starts at** hour defaults to 4 (manual, preferences).
- `next_day_at` = today's rollover datetime in the current UTC offset; if that has already passed, add 1 day.
- `days_elapsed` (the "today" day number) = calendar days between collection creation and now, minus 1 if today's rollover has not passed, floored at 0.
- Review `due` is a day number relative to collection creation. Intraday learning `due` is unix seconds.
- For a fresh design: store `dueDay` as an integer local "scheduler day", where `schedulerDay(ts) = floor((localTime(ts) − rolloverHour*3600)/86400)`. Compute FSRS Δt as `schedulerDay(now) − schedulerDay(lastReview)`. This matches Anki's `elapsed_days_since(next_day_at)` semantics.

### 9.2 Learn-ahead and counts

Source: `queue/builder/mod.rs`, `queue/learning.rs`, `builder/gathering.rs`.

- **Learn ahead limit** defaults to 20 min (manual). Cards with `due ≤ now + learn_ahead_secs` are shown early when nothing else is left (`intraday_ahead_iter`).
- Gather order:
  1. intraday learning (queue 1/4, `due ≤ next_day_at`)
  2. interday learning (queue 3, `due ≤ today`)
  3. reviews (queue 2, `due ≤ today`)
  4. new cards
- Interday learning and reviews both consume the **review limit**. New cards consume the new limit. Unless "New cards ignore review limit" is on, new cards are also capped by the remaining review limit (manual "Daily Limits").
- Limits are per deck plus parents (`LimitTreeMap`). "Limits start from top" applies parent limits when studying a child.
- Displayed counts:
  - `new` = gathered new cards
  - `review` = gathered review cards
  - `learn` = intraday learning with `due ≤ now + learn_ahead` **plus** the count of gathered interday-learning cards. Counts are cards, not remaining steps.
- When the clock advances, `update_learning_cutoff_and_count` adds newly due learning cards to the count.
- A just-answered learning card that would be shown again immediately while the main queue is empty is placed 1 s after the next learning card.
- Burying, when enabled, is per note during gathering: intraday learning first, then interday, review, new. Earlier kinds can bury later ones, not the reverse.

### 9.3 Display order options

Source: [deck_config.proto](https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/proto/anki/deck_config.proto) and the manual.

- **New gather:** Deck (0), DeckThenRandomNotes (5), LowestPosition (1), HighestPosition (2), RandomNotes (3), RandomCards (4).
- **New sort:** Template (0), NoSort/"order gathered" (1), TemplateThenRandom (2), RandomNoteThenTemplate (3), RandomCard (4).
- **New/review mix and interday-learning mix:** MixWithReviews (0, interleaved evenly by an "intersperser"), AfterReviews (1), BeforeReviews (2).
- **Review order** (SQL in [storage/card/mod.rs](https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/storage/card/mod.rs)). Every order is followed by a random tiebreak `fnvhash(id, mod)`.
  - Day (`due`); DayThenDeck; DeckThenDay
  - IntervalsAscending / IntervalsDescending (`ivl`)
  - EaseAscending / EaseDescending (`factor`; under FSRS these become difficulty `d` desc / asc)
  - RetrievabilityAscending (7) / RetrievabilityDescending (11): `extract_fsrs_retrievability(...)`
  - RelativeOverdueness (12):
    - SM-2: `−(1 + (today − due + 0.001)/ivl)` ascending.
    - FSRS: `−(R_now^(−1/decay) − 1)/(DR^(−1/decay) − 1)` ascending, with R_now from `lrt` in fractional days ([sqlite.rs](https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/storage/sqlite.rs)).
  - Random (8), Added (9), ReverseAdded (10).

---

## 10. Desired retention

**Semantics.** DR is the target probability of recall at the moment a card comes due. The interval is `I(DR, S)` from §1.3. The manual says the default 90% "will schedule cards so you have a 90% chance of remembering them" and recommends staying below 97%. Workload "increases very quickly" above 90% ([manual](https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/docs-site/manual/deck-options.mdx)).

DR is per preset and can be overridden per deck (`limits.desiredRetention`). The card's `dr` field mirrors the effective value.

**UI ranges** (FsrsOptions.svelte):
- below 0.70 or above 0.97: danger
- below 0.80 or above 0.95: warning
- otherwise: info

**Compute minimum recommended retention (CMRR).** This was removed in Anki 25.07 (manual: "This feature has been removed as of anki version 25.07").
- It is replaced by the "Help Me Decide" simulator and a workload-change hint. The hint computes `fsrs::expected_workload_with_existing_cards` for DR = 70…99% and reports `cost(DR_new)/cost(DR_old)` ([deckconfig/service.rs](https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/deckconfig/service.rs)).
- The simulator (`simulate_workload`) runs a full simulation per DR 70–99 and returns memorized count, total cost (seconds) and review count.

How CMRR worked, still in fsrs-rs `optimal_retention` and Anki `compute_optimal_retention`, kept here as a design reference:
- **Objective:** minimize `Σ cost_per_day / memorized_cnt_per_day[last]` (seconds per memorized card) over a Monte-Carlo simulation.
- **Search:** Brent's method on DR ∈ [0.70, 0.95], tolerance 0.01, ≤ 64 iterations. Each evaluation averages 16 simulation seeds for a 365-day span (more for shorter spans). The result is clamped to [0.7, 0.95].
- **Simulator defaults** (`SimulatorConfig::default`):
  - `deck_size` 10000, `learn_span` 365, `max_cost_perday` 1800 s
  - `first_rating_prob [0.24, 0.094, 0.495, 0.171]`
  - `review_rating_prob [0.224, 0.631, 0.145]` (Hard/Good/Easy given pass)
  - `state_rating_costs` (seconds, rows learning/review/relearning):
    - learning `[19.58, 18.79, 13.78, 10.71]`
    - review `[19.38, 17.59, 12.38, 8.94]`
    - relearning `[16.44, 15.25, 12.32, 8.03]`
  - learning and relearning step transition matrices
  - Anki replaces these with values extracted from the user's revlog (`extract_simulator_config`).
- py-fsrs's simpler version tries DR ∈ {0.70, 0.75, 0.80, 0.85, 0.90, 0.95} on 1000 simulated cards and needs ≥ 512 review logs with durations.

---

## 11. Differences between reference implementations

They decide which test vectors apply.

| Topic | fsrs-rs / Anki | ts-fsrs | py-fsrs |
|---|---|---|---|
| Rounding | f32, no rounding | rounds most intermediates to 8 dp (`roundTo(x,8)`) | f64 |
| Initial S floor | S_MIN | `max(w, 0.1)` | S_MIN |
| t = 0 with short-term disabled | w17–w19 = 0, so S is unchanged | uses long-term formulas at t = 0 | n/a |
| Hard on learning step | Anki: avg(first, second) secs on step 0, else repeat current | always round avg in minutes | 330 s like Anki |
| Fuzz | Anki ranges, ChaCha seed id+reps, min-fuzz rules §3.3 | same ranges, min ≥ 2, Alea, off by default | same ranges, Python random |
| Elapsed days | scheduler days (rollover-aligned) | `date_diff(now, last_review, 'days')` (calendar days) | timedelta days |
| Post-lapse cap | min(S'_f, S/e^{w17·w18}) | same (w17,w18 = 0 if short-term off) | same |

---

## 12. FSRS-7 (watch item, not shipped in Anki 26.09)

fsrs-rs main (unreleased after v6.6.2) adds FSRS-7. A summary from [model_v7.rs@main](https://github.com/open-spaced-repetition/fsrs-rs/blob/4bc0a0979f95dd01cc653031b6f2a01429e4c32b/src/model_v7.rs):

- **34 parameters.**
- The memory state gains a `stability_fast` component.
- The forgetting curve becomes a weighted mix of two power curves, so retrievability no longer depends on w20:
  ```
  decay1 = −clamp(w23 * S_fast^(w33−0.3), 0.01, 0.95);  factor1 = exp(min(ln(w25)/decay1, 60)) − 1
  r1 = (1 + factor1 * t/S_fast)^decay1
  decay2 = −clamp(w24, 0.01, 0.95); factor2 = w26^(1/decay2) − 1; dts = exp((D−5)(w32−0.3))
  r2 = (1 + factor2 * dts * t/S)^decay2
  weight1 = w27 * S_fast^(−w29); weight2 = w28 * S^w30 * exp((D−5)(w31−0.5))
  R = ((weight1*r1 + weight2*r2)/(weight1+weight2)) * (1 − 2e−5) + 1e−5
  ```
- It works with fractional (same-day) intervals.
- Mean reversion becomes fixed at `0.01*init + 0.99*current`.

The benchmark recommends shipping FSRS-7 "with both recency weighting and scheduling penalties enabled" ([srs-benchmark README](https://github.com/open-spaced-repetition/srs-benchmark/blob/bd9110f791e5b37282c55a9aa8db35f68f0c4aa2/README.md)).

**Design implication:** keep `MemoryState` extensible (add an optional `sFast`), and keep R(t, state) and I(DR, state) as pluggable functions. FSRS-7's interval has no closed form and is solved numerically on main.
