# FSRS-6

FSRS (the Free Spaced Repetition Scheduler) schedules each card just before you would forget it, using a small memory model fitted to your reviews. Loci uses FSRS-6, the version Anki ships, ported from fsrs-rs v6.6.2 and checked against its test vectors.

## Three numbers per card

FSRS describes the memory of each card with three numbers.

**Retrievability, R.** The probability that you would recall the card if you saw it right now. It is 1 just after a successful review and falls as time passes. A card with R = 0.9 is one you would answer correctly nine times in ten.

**Stability, S.** How slowly R falls, measured in days. Precisely: S is the number of days it takes for R to fall from 1 to 0.9. A card with S = 30 is one you would still recall with 90% probability a month after the last review. Stability is what reviews build.

**Difficulty, D.** A number from 1 to 10 for how hard this card is for you. It does not set the interval directly; it sets how much each successful review raises stability.

Press `i` while studying to see all three.

## The forgetting curve

How does R fall with time? FSRS-6 models it with a power law that drops quickly at first and then flattens, shaped by one tunable parameter, the decay `w20`:

```text
FACTOR = 0.9^(-1/w20) - 1
R(t, S) = (1 + FACTOR · t / S)^(-w20)
```

Here `t` is the number of days since the last review. FACTOR makes R(S, S) = 0.9 for any decay. With the default decay `w20 = 0.1542`, FACTOR = 0.98035.

FSRS-4.5 and FSRS-5 fixed the decay at 0.5; FSRS-6 fits it to your reviews like every other parameter.

## From desired retention to the next interval

You choose a **desired retention**, DR: the probability of recall you want at the moment a card comes due. The default is 0.90. The next interval is the time at which R falls to DR, which is the forgetting curve solved for `t`:

```text
I(DR, S) = S / FACTOR · (DR^(-1/w20) - 1)
```

At DR = 0.9 this is exactly S: a card with stability 14 days is due in 14 days. Loci rounds the result to whole days and applies a small random spread (the fuzz, described below).

## The first review

A new card has no memory yet. The first answer, G (1 Again, 2 Hard, 3 Good, 4 Easy), sets the initial state:

```text
S0(G) = w[G-1]                              Again → w0, Hard → w1, Good → w2, Easy → w3
D0(G) = w4 - exp(w5 · (G - 1)) + 1          clamped to [1, 10]
```

The first four parameters are the initial stabilities, in days, for each button.

## After a successful review

When you pass a review (Hard, Good or Easy) after `t > 0` days, stability grows:

```text
S' = S · ( exp(w8) · (11 - D) · S^(-w9) · (exp(w10 · (1 - R)) - 1) · hard · easy + 1 )
       hard = w15 if G = 2, else 1
       easy = w16 if G = 4, else 1
```

- `11 - D`: easier cards (low D) grow more.
- `S^(-w9)`: the more stable a memory already is, the smaller its relative growth.
- `exp(w10 · (1 - R)) - 1`: the lower R was at the review, the larger the gain.
- `hard` and `easy`: Hard is a pass with a penalty and Easy a pass with a bonus (by default `w15` = 0.6014 and `w16` = 1.8729).

R is the retrievability at the review and D the difficulty before it.

## After a lapse

When you press Again after `t > 0` days, the memory has failed and stability drops:

```text
S'_f = w11 · D^(-w12) · ((S + 1)^w13 - 1) · exp(w14 · (1 - R))
S'   = min(S'_f, S / exp(w17 · w18))
```

A forgotten card does not start from zero: the new stability still grows with the old one through `(S + 1)^w13`, so a long-held memory keeps more after a lapse than a young one. The second term caps the result, so relearning steps on the same day cannot push stability back above its value before the lapse.

## Reviews on the same day

When `t = 0` (a learning step, or a second look on the same day), FSRS-6 uses a separate short-term rule for all four buttons:

```text
SInc = exp(w17 · (G - 3 + w18)) · S^(-w19)
S'   = S · max(SInc, 1)      for Hard, Good, Easy
S'   = S · SInc              for Again
```

A pass on the same day never lowers stability.

## How difficulty moves

Every rated review, same-day ones included, updates difficulty in two steps:

```text
D'  = D - w6 · (G - 3) · (10 - D) / 9          linear damping
D'' = w7 · D0(4) + (1 - w7) · D'                mean reversion
D_new = clamp(D'', 1, 10)
```

Again and Hard raise D, Easy lowers it, Good leaves the first step unchanged. The factor `(10 - D) / 9` damps the change near 10. Mean reversion pulls D slightly toward `D0(4)`, the initial difficulty of an Easy card, so one bad start does not stick.

## The default parameters

These are the 21 FSRS-6 defaults, as defined in `crates/loci-core/src/collection/presets.rs` (identical to fsrs-rs v6.6.2):

| Parameters | Default values | Where they appear |
|---|---|---|
| w0, w1, w2, w3 | 0.212, 1.2931, 2.3065, 8.2956 | initial stability for Again, Hard, Good, Easy |
| w4, w5 | 6.4133, 0.8334 | initial difficulty |
| w6 | 3.0194 | how far one answer moves difficulty |
| w7 | 0.001 | mean reversion of difficulty |
| w8, w9, w10 | 1.8722, 0.1666, 0.796 | stability growth after a pass |
| w11, w12, w13, w14 | 1.4835, 0.0614, 0.2629, 1.6483 | stability after a lapse |
| w15 | 0.6014 | Hard penalty |
| w16 | 1.8729 | Easy bonus |
| w17, w18, w19 | 0.5425, 0.0912, 0.0658 | same-day reviews (w17 and w18 also cap stability after a lapse) |
| w20 | 0.1542 | decay of the forgetting curve |

Stability is clamped to [0.001, 36500] days and difficulty to [1, 10]. Older 17- and 19-parameter vectors (FSRS-4.5 and FSRS-5) are converted and padded to 21 when they are loaded, as fsrs-rs does.

## A worked example

Follow one card through two reviews at DR = 0.90. The numbers were computed from the formulas above with the default parameters, in double precision, and rounded for display.

The example uses a preset with no learning steps, so the first answer graduates the card at once. (With the Standard preset's 1m and 10m steps, the first day adds a same-day Good: its SInc of 0.9945 is floored to 1, so S stays 2.3065, and mean reversion moves D to 2.1112.)

### Day 0: the first answer is Good

The card is new, so the initial-state formulas apply with G = 3:

```text
S = w2 = 2.3065 days
D = w4 - exp(w5 · 2) + 1 = 6.4133 - exp(1.6668) + 1 = 6.4133 - 5.2952 + 1 = 2.1181
```

The interval at DR = 0.9 equals stability, 2.3065 days, which rounds to **2 days**. Intervals below 2.5 days get no fuzz.

### Day 3: the second answer is Good

You come back three days later, one day after the card fell due. First, how likely were you to remember it?

```text
FACTOR · t / S = 0.98035 · 3 / 2.3065 = 1.27511
R = (1 + 1.27511)^(-0.1542) = 0.8809
```

An 88% chance, below the 90% target because the review is a day late. You answer Good:

```text
exp(w8)                  = exp(1.8722)              = 6.5026
11 - D                   = 11 - 2.1181              = 8.8819
S^(-w9)                  = 2.3065^(-0.1666)         = 0.8700
exp(w10 · (1 - R)) - 1   = exp(0.796 · 0.1191) - 1  = 0.0994

product = 6.5026 · 8.8819 · 0.8700 · 0.0994 = 4.9948
S' = 2.3065 · (4.9948 + 1) = 13.8269 days
```

Stability grew about sixfold. Difficulty barely moves: Good leaves the damping step unchanged and the mean-reversion weight `w7` is only 0.001:

```text
D'  = 2.1181                                         (G = 3: no change)
D'' = 0.001 · D0(4) + 0.999 · 2.1181
    = 0.001 · (-4.7716) + 0.999 · 2.1181 = 2.1112
```

`D0(4)` is the initial difficulty of an Easy card before clamping, 6.4133 − exp(0.8334 · 3) + 1 = −4.7716.

The next interval at DR = 0.9 is S' itself: 13.83 days, which rounds to **14 days**. The fuzz window for an interval of 13.83 days is ±2.36 days, so before Anki's rule that keeps the four buttons in order, the card lands somewhere from day 11 to day 16 after this review.

### What the other buttons would have given

At the same review (t = 3, R = 0.8809), the four buttons lead to these states:

| Button | New S (days) | New D | Unrounded interval |
|---|---|---|---|
| Again | 0.6369 | 7.3945 | 0.64 |
| Hard | 9.2349 | 4.7529 | 9.23 |
| Good | 13.8269 | 2.1112 | 13.83 |
| Easy | 23.8831 | 1.0 | 23.88 |

Again uses the lapse formula: the long-term term gives 0.6369, well under the cap of 2.3065 / exp(0.5425 · 0.0912) = 2.1952. On a review card, Again then moves the card to the Standard preset's 10-minute relearning step. Easy pushes difficulty below 1, and the clamp holds it at 1.

## Desired retention and workload

Because the interval formula is linear in S, desired retention changes every interval by the same factor. With the default decay, the interval is this multiple of stability:

| Desired retention | Interval as a multiple of S | For the example card (S = 13.83) |
|---|---|---|
| 0.80 | 3.32 | 45.8 days |
| 0.85 | 1.91 | 26.4 days |
| 0.90 | 1.00 | 13.8 days |
| 0.93 | 0.61 | 8.5 days |
| 0.95 | 0.40 | 5.6 days |
| 0.97 | 0.22 | 3.1 days |

Going from 0.90 to 0.95 shortens every interval to 40% of its length: roughly two and a half times as many reviews of a mature card, to forget half as often (5% of due cards instead of 10%). Going down to 0.85 nearly doubles the intervals. Loci's presets sit at 0.85 (Relaxed), 0.90 (Standard) and 0.93 (Intense), and the setting accepts values from 0.70 to 0.99.

Before you change desired retention or new cards per day, Loci shows a workload forecast: a 90-day sparkline of predicted reviews per day, simulated on your actual cards.

## What the optimiser does

The optimiser (fsrs-rs v6.6.2, as Anki calls it) fits the 21 parameters to your history, per preset:

1. **Items.** Each card's review log becomes training items: answers and the days between them, with the last answer as the outcome to predict.
2. **Outlier filter.** Cards are grouped by their first answer and first interval. The smallest groups are dropped until at least 20 items or 5%, whichever is more, are gone, and so is any group under 6 cards or with a first interval over 100 days (365 after Easy).
3. **Thresholds.** With fewer than 8 training items, the defaults stay. With fewer than 64, or when no card has a second long-term review, only the initial stabilities w0 to w3 are fitted (pretraining). Otherwise the full fit runs.
4. **The fit.** Adam on the log-loss: the model predicts R at each item's last review, and the loss measures how far those predictions are from what happened. Eight epochs, batches of 512, learning rate 0.04 with cosine annealing, recent reviews weighted more, an L2 prior toward the starting values, and parameters clipped to their ranges after every step.
5. **Acceptance.** The new parameters are kept only if their log-loss on your reviews is lower than the current ones'. Each run records log-loss and RMSE(bins) before and after in the collection's `optimise_log`.

*Optimise* and *Evaluate* sit in each preset's settings, next to the parameters and the last run's metrics. An *Optimise* that adopted new parameters can be undone: the preset's previous parameters and every refreshed memory state come back.

Each preset has an auto-optimise switch (on, threshold 400 reviews). The engine's `maintenance` pass optimises the FSRS preset with the most new reviews since its last run, if that is at least 400; automatic runs are not undoable. The app does not run `maintenance` by itself: use *Optimise* in the preset's settings.

## The fuzz

Fuzz spreads cards learned together with a small random offset, by Anki's rule:

| Part of the interval | Spread added |
|---|---|
| below 2.5 days | none |
| 2.5 to 7 days | 15% |
| 7 to 20 days | 10% |
| beyond 20 days | 5% |

The window's half-width is 1 day plus those percentages of each part of the interval. The day inside it comes from Anki's own generator, `StdRng::seed_from_u64(card.id + reps)` (ChaCha12), so a card gets the same day as in Anki.

## SM-2

Each preset has a scheduler switch. SM-2 behaves as in Anki's v3 scheduler.
