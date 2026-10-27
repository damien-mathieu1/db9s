# Contributing

Issues and PRs are welcome.

## Dev setup

```bash
docker compose up -d          # local postgres + mongo, seeded with `users` and `products` (dev/seed.sql, dev/seed.js)
docker compose down -v        # wipe data; next `up` re-seeds
cargo run -- --postgres postgres://db9s:db9s@localhost:55432/db9s --mongo mongodb://localhost:57017
```

## Before opening a PR

```bash
cargo fmt
cargo clippy --all-targets
DB9S_TEST_POSTGRES_URL=postgres://db9s:db9s@localhost:55432/db9s \
DB9S_TEST_MONGO_URL=mongodb://localhost:57017 cargo test
```

`src/datasource/tests.rs` runs the same scenario (list, page, update, query,
delete) against every backend; a backend without its URL env var is skipped.
CI runs them all against real Postgres and MongoDB.

Keep the diff focused on one change. No unused code, no new warnings.

## Adding a backend

Implement the `DataSource` trait (`src/datasource/mod.rs`) in a new module
under `src/datasource/`, then wire it into `NewConnKind` and the `connect()`
helper in `src/app.rs`. Add a `<backend>_conformance` test calling `exercise()`,
a seed file in `dev/`, and a service in `docker-compose.yml` and CI.

## Releasing

Bump `version` in `Cargo.toml`, merge to `main`, then tag:

```bash
git tag v0.2.0 && git push origin v0.2.0
```

The tag triggers two workflows:

- `ci`: runs the tests against real databases, then publishes to crates.io
  (needs the `CARGO_REGISTRY_TOKEN` repo secret). Nothing is published if a test fails.
- `release`: builds Linux/macOS/Windows binaries and attaches them to a GitHub Release.

## Screenshots

`docs/*.png` are generated with [vhs](https://github.com/charmbracelet/vhs) from
`docs/screenshots.tape`, against the seeded `docker compose` stack:

```bash
docker compose up -d && cargo build && vhs docs/screenshots.tape
```
