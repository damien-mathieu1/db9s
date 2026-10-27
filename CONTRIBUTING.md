# Contributing

Issues and PRs are welcome.

## Dev setup

```bash
docker compose up -d          # local postgres + mongo + redis + rabbitmq, seeded (dev/seed*)
docker compose down -v        # wipe data; next `up` re-seeds
cargo run -- --postgres postgres://db9s:db9s@localhost:55432/db9s --mongo mongodb://localhost:57017 \
  --redis redis://localhost:56379 --rabbitmq http://db9s:db9s@localhost:55672
```

## Before opening a PR

```bash
cargo fmt
cargo clippy --all-targets
DB9S_TEST_POSTGRES_URL=postgres://db9s:db9s@localhost:55432/db9s \
DB9S_TEST_MONGO_URL=mongodb://localhost:57017 \
DB9S_TEST_RABBITMQ_URL=http://db9s:db9s@localhost:55672 \
DB9S_TEST_AMQP_URL=amqp://db9s:db9s@localhost:55673/%2F \
DB9S_TEST_REDIS_URL=redis://localhost:56379 cargo test
```

`src/datasource/tests.rs` runs the same scenario (list, page, update, query,
delete) against every database backend, and `exercise_queue()` runs the broker
scenario (publish, peek, purge) against every queue backend (RabbitMQ over the
management API and over AMQP). Redis has its own test, one check per key type. A backend without
its URL env var is skipped. CI runs them all against real services.

Keep the diff focused on one change. No unused code, no new warnings.

## Adding a backend

1. Implement the `DataSource` trait (`src/datasource/mod.rs`) in a new module
   under `src/datasource/`. Message brokers (Kafka, NATS, ...) return `true`
   from `is_queue()`: the UI then shows publish/purge and disables edit.
2. Add a variant to `Kind` in `src/config.rs` (and to `Kind::ALL` / `label()`),
   then an arm in `connect()` in `src/app.rs`. The add-connection form, config
   file and footer hints pick it up from there.
3. Add a `<backend>_conformance` test calling `exercise()` (databases) or
   `exercise_queue()` (brokers), a seed in `dev/`, and a service in
   `docker-compose.yml` and CI.

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

`docs/demo.gif` (README hero) and `docs/*.png` are generated with
[vhs](https://github.com/charmbracelet/vhs) from `docs/demo.tape` and
`docs/screenshots.tape`, against the seeded `docker compose` stack:

```bash
docker compose up -d && cargo build && vhs docs/demo.tape && vhs docs/screenshots.tape
```
