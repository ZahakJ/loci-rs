# Performance record

These are the numbers recorded for v0.1.0 in the README, reproduced exactly, with the method that produced them.

## Method

Measured on the development desktop (16 cores, 30 GB, Linux) with release builds, while another build job was running on the machine. The engine numbers come from `cargo test --release -p loci-core --test integration -- --ignored perf --nocapture` on a generated 100,000-card legacy `.apkg` (18 MB: 100,000 notes and cards, 437,735 review-log rows after de-duplication).

## Results

| Measurement | Result |
|---|---|
| Cold start (process start to the Home screen interactive, release binary under Xvfb with software rendering, three runs) | 806 ms, 818 ms, 827 ms |
| Import of the 100,000-card `.apkg` | 4.6 s |
| Deck tree with counts on 100,000 cards | 37 ms |
| Browse page of 100 rows sorted by due, offset 0 / offset 99,900 | 91 ms / 86 ms (the table is virtualised; scrolling only ever asks for one page) |
| Browse page sorted by retrievability (`prop:r<0.95`) | 134 ms |
| Study answer round-trip (`answerCard`, engine side, mean of 50) | 0.4 ms |
| Start of a study session on 100,000 cards | 34 ms |
| Statistics for the whole collection | 847 ms |
| Export of the whole collection to `.apkg` | 1.7 s |

Reading the 590-card fixture package takes 7 ms.

## What the numbers mean

Two of them shape daily use. Answering a card costs the engine 0.4 ms, so the time between pressing Good and seeing the next card is the interface's animation, not the database. A Browse page costs about the same at the first row as at the 99,900th, because the table is virtualised and asks the engine for one page of 100 rows at a time, whatever the size of the collection.

The cold-start figure is the time from process start to the moment the Home screen is interactive. The shell records the process start, and the interface reports when it is ready through the `app_ready` command.

## Rerunning the engine measurements

The performance tests live in `crates/loci-core/tests/integration/perf.rs`. They are `#[ignore]`d, so an ordinary `cargo test` skips them. They need a large package, which is not committed: by default they look for `fixtures/large-100k.apkg`, and the environment variable `LOCI_PERF_APKG` points them at any other file. When the package is absent, the large-collection test skips. One way to generate such a package is the TypeScript Loci's `buildLargeCollection`.

```sh
LOCI_PERF_APKG=/path/to/large-100k.apkg \
  cargo test --release -p loci-core --test integration -- --ignored perf --nocapture
```

`--nocapture` lets the timings the tests print reach the terminal. When comparing with the table above, remember that it was recorded with another build job running on the same machine.
