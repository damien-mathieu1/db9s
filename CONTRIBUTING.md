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

Versions are CalVer `vYYYY.M.N` (N = release number in the month). To release,
go to **Actions → Create Release → Run workflow** on `main`. It:

1. runs the CI tests against real databases (nothing happens if they fail),
2. computes the next version, bumps `Cargo.toml`, commits `chore(release): vX`
   to `main` and tags it,
3. creates the GitHub Release with a changelog built from conventional commits
   (`cliff.toml`, same as git-cliff),
4. builds Linux/macOS/Windows binaries and attaches them to the release,
5. publishes to crates.io (needs the `CARGO_REGISTRY_TOKEN` repo secret).

## Screenshots

`docs/*.png` are generated with [vhs](https://github.com/charmbracelet/vhs) from
`docs/screenshots.tape`, against the seeded `docker compose` stack:

```bash
docker compose up -d && cargo build && vhs docs/screenshots.tape
```
