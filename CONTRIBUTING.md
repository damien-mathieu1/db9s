# Contributing

Issues and PRs are welcome.

## Dev setup

```bash
docker compose up -d          # local postgres + mongo, seeded with a `users` table/collection
cargo run -- --postgres postgres://db9s:db9s@localhost:55432/db9s --mongo mongodb://localhost:57017
```

## Before opening a PR

```bash
cargo fmt
cargo clippy --all-targets
cargo test
```

Keep the diff focused on one change. No unused code, no new warnings.

## Adding a backend

Implement the `DataSource` trait (`src/datasource/mod.rs`) in a new module
under `src/datasource/`, then wire it into `NewConnKind` and the `connect()`
helper in `src/app.rs`.
