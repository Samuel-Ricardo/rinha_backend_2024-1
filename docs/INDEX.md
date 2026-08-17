# Documentation Index — `rinha_backend_2024-1`

Suggested **reading order** (first → last):

| # | Document | Audience | Summary |
|---|---|---|---|
| 1 | [README.md](../README.md) | everyone | Project pitch, repo map, status, quick start |
| 2 | [CHALLENGE-SPEC.md](CHALLENGE-SPEC.md) | all | The Rinha 2024 Q1 contract the app must satisfy |
| 3 | [ARCHITECTURE.md](ARCHITECTURE.md) | engineers | Intended architecture, data flow, build, lifecycle |
| 4 | [STORAGE-ENGINE.md](STORAGE-ENGINE.md) | engineers | The custom `espora-db` engine in depth |
| 5 | [LOAD-BALANCERS.md](LOAD-BALANCERS.md) | engineers | HTTP + TCP balancers, strategies, config |
| 6 | [HTTP-SERVER.md](HTTP-SERVER.md) | engineers | The `axum-tcp-socket` server helper |
| 7 | [FINDINGS.md](FINDINGS.md) | maintainers | Verified review — bugs, risks, completeness |
| 8 | [DEPLOYMENT.md](DEPLOYMENT.md) | ops | Packaging requirements & gaps |

---

## Quick reference

### The contract in one table

| Endpoint | Success | Bad input | Unknown client |
|---|---|---|---|
| `POST /clientes/{id}/transacoes` | `200 {limite, saldo}` | `422` | `404` |
| `GET /clientes/{id}/extrato` | `200 {saldo, ultimas_transacoes[≤10]}` | n/a | `404` |

### Clients (preloaded)

| id | limite | saldo inicial |
|---:|---:|---:|
| 1 | 100000 | 0 |
| 2 | 80000 | 0 |
| 3 | 1000000 | 0 |
| 4 | 10000000 | 0 |
| 5 | 500000 | 0 |

### Resource cap (Docker Compose)

- `deploy.resources.limits.cpu` total ≤ **1.5**
- `deploy.resources.limits.memory` total ≤ **550MB**
- LB listens on **port 9999**; 2 API instances behind it.

### Pipeline at a glance

- Transaction: serialize request → validate → compute new `saldo` → enforce `saldo ≥ -limite` → append row to `espora-db` → return `{limite, saldo}`.
- Statement: read client's append-only log → take last 10 (reverse) → merge with current `saldo`/`limite`/`data_extrato`.

### Key files

| File | Holds |
|---|---|
| `rinha/Cargo.toml` | Workspace + `[profile.release]` (LTO, codegen-units=1, panic=abort) |
| `rinha/espora-db/src/model/page.rs` | `Page<ROW_SIZE>` + `PAGE_SIZE = 4096` |
| `rinha/espora-db/src/model/database.rs` | sync `Db<T, ROW_SIZE>` engine |
| `rinha/espora-db/src/model/tokio/database.rs` | async `Db<T, ROW_SIZE>` engine |
| `rinha/espora-db/src/model/tokio/builder.rs` | `build_tokio` async builder |
| `rinha/espora-db/src/lock.rs` | Windows `LockHandle` (winapi) |
| `rinha/espora-db/src/linux_lock.rs` | Unix `LockHandle` (libc flock) — **orphaned** |
| `rinha/axum-tcp-socket/src/lib.rs` | `pub async fn server<S>(path, app)` |
| `rinha/load_balancer/src/proxy.rs` | HTTP `proxy::main` request forwarder |
| `rinha/load_balancer/src/strategy/mod.rs` | `RoundRobin` + `RinhaAccountBalancer` |
| `rinha/load_balancer_tcp/src/lib.rs` | TCP `main` (`io::copy_bidirectional`) |

### Symbols to remember

- `espora-db` const-generic is named **`ROM_SIZE`** in the sync struct/builder and **`ROW_SIZE`** in the impl/async/Page — same thing (the fixed byte size of one row slot). Misnamed.
- `PAGE_SIZE = 4096`; rows per page = `PAGE_SIZE / ROW_SIZE` *when aligned* (tests use `ROW_SIZE=2048` → 2 rows/page; `ROW_SIZE=1024` → 4 rows/page).
- Default durability: `sync_writes = Some(Duration::from_secs(0))` → **`fsync` on every insert**. Tunable via `Builder::sync_writes(false)` or `sync_write_interval(d)`.
