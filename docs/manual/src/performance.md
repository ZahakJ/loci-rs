# Performance tests

The performance tests are in `crates/loci-core/tests/integration/perf.rs`, `#[ignore]`d by default. They time import, the deck tree, Browse pages, a study answer, statistics and export on a large package, which is not committed: `fixtures/large-100k.apkg`, or the file named by `LOCI_PERF_APKG`. Without it the large-collection test skips.

```sh
LOCI_PERF_APKG=/path/to/large.apkg \
  cargo test --release -p loci-core --test integration -- --ignored perf --nocapture
```

Cold start is measured from process start to the `app_ready` command the interface sends when Home is interactive.
