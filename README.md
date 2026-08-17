# rinha_backend_2024-1

> A [**Rinha de Backend 2024 Q1**](https://github.com/zanfranceschi/rinha-de-backend-2024-q1) solution sketch in Rust.
> "A solution for Rinha de Backend 2024.1 based on @Navarro."

This repository builds the **infrastructure layer** of a high-concurrency "créditos" banking
API: a **custom embedded database** (`espora-db`), an Axum/Hyper socket-server helper, and **two
load balancers** (HTTP/2-aware and raw TCP). Everything is hand-written in Rust, optimized for
RPS under the Rinha's tight resource budget (1.5 CPU / 550 MB RAM).

> **Status — "infra scaffold, app not wired".** The components are designed and partially
> implemented; the banking HTTP application that the Rinha actually tests is **not present**
> (`rinha/src/main.rs` is still `println!("Hello, world!")`), and the deployment files (Docker
> Compose, Dockerfile, LB config) are missing. See [`docs/FINDINGS.md`](docs/FINDINGS.md) for a
> full completeness assessment and the list of defects to fix. This documentation describes
> **what exists today** and **what is required to make it a runnable submission**.

---

## What the Rinha asks for (TL;DR)

Two HTTP endpoints, 5 preloaded clients, strong concurrency under a CPU/RAM cap:

| Method & Path | Purpose |
|---|---|
| `POST /clientes/{id}/transacoes` | Register a credit (`c`) or debit (`d`) transaction; returns `{ limite, saldo }`. Reject debit that would breach `saldo < -limite` with **422**; unknown client → **404**; malformed body → **422**. |
| `GET /clientes/{id}/extrato` | Return current balance + last 10 transactions with timestamps. Unknown client → **404**. |

Required clients: `id 1..5` with limits `100000, 80000, 1000000, 10000000, 500000`, all initial
balance `0` (and **no** `id 6`). A load balancer must listen on **port 9999**, fan out to **2 API
instances**, with `cpu ≤ 1.5` and `memory ≤ 550MB` total (Docker Compose). See
[`docs/CHALLENGE-SPEC.md`](docs/CHALLENGE-SPEC.md).

---

## Architecture (intended)

```
                 Gatling load test
                        │  (port 9999)
                        ▼
        ┌──────────────────────────────────┐
        │   Load Balancer (Rust, custom)   │   ← load_balancer / load_balancer_tcp
        │   strategy: RoundRobin          │   (RinhaAccountBalancer designed but UNUSED)
        └───────┬──────────────────┬──────┘
        HTTP/2  │                  │  (or raw TCP copy)
                ▼                  ▼
      ┌─────────────────┐  ┌─────────────────┐
      │  API instance 1 │  │  API instance 2 │   ← NOT YET IMPLEMENTED (rinha app)
      │  axum-tcp-socket│  │  axum-tcp-socket│
      └────────┬────────┘  └────────┬────────┘
               │ accounts          │ accounts
               ▼                    ▼
      ┌─────────────────┐  ┌─────────────────┐
      │   espora-db     │  │   espora-db     │   ← custom embedded DB
      │  page-based log │  │  page-based log │     (4096B pages, bitcode, fsync-tunable)
      └─────────────────┘  └─────────────────┘
```

The clever intended design: **account-sticky routing** (`RinhaAccountBalancer`) pins each client
ID to a fixed backend, so that backend owns the client's append-only transaction log —
single-writer-per-account, no distributed lock, no cross-node coordination. That strategy is
implemented but currently **not wired into** the running balancer.

Full design in [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).

## Repository map

```
rinha_backend_2024-1/
├─ README.md                 ← this file
├─ LICENSE
├─ docs/                     ← full structured documentation
└─ rinha/                    ← Cargo workspace
   ├─ Cargo.toml             ← workspace + root binary (release: LTO, codegen-units=1, panic=abort)
   ├─ src/main.rs            ← STUB ("Hello, world!") — the Rinha app is not implemented
   ├─ espora-db/             ← ★ custom embedded page-based database (bitcode, sync+tokio, file locks)
   ├─ axum-tcp-socket/       ← Axum/Hyper server bound on a TCP path (one async fn)
   ├─ load_balancer/         ← HTTP reverse proxy/LB (axum + hyper-util), RoundRobin + sticky balancer
   └─ load_balancer_tcp/     ← raw TCP byte-stream proxy (tokio io::copy_bidirectional)
```

| Crate | Lines | Role | Compiles today? |
|---|---:|---|---|
| `espora-db` | ~600 | Custom embedded DB engine | sync model: Windows only; async model: likely no (see findings) |
| `axum-tcp-socket` | 44 | Hyper `serve_connection_with_upgrades` helper | Yes (on any platform) |
| `load_balancer` | ~90 | HTTP/2 round-robin proxy | Yes, but it's a library, not a binary |
| `load_balancer_tcp` | 52 | Raw TCP round-robin proxy | Yes, but it's a library, not a binary |
| `rinha` (root) | 3 | The banking API | Compiles (stub); the real app is missing |

## Documentation index

| Doc | What you'll find |
|---|---|
| [`docs/INDEX.md`](docs/INDEX.md) | Reading order & quick reference |
| [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) | Intended design, data flow, workspace/build, request lifecycle |
| [`docs/STORAGE-ENGINE.md`](docs/STORAGE-ENGINE.md) | `espora-db` deep dive: pages, slots, bitcode, durability, locking, tests |
| [`docs/LOAD-BALANCERS.md`](docs/LOAD-BALANCERS.md) | HTTP + TCP balancers, strategies, proxy behavior, env config |
| [`docs/HTTP-SERVER.md`](docs/HTTP-SERVER.md) | The `axum-tcp-socket` server helper crate |
| [`docs/CHALLENGE-SPEC.md`](docs/CHALLENGE-SPEC.md) | The exact Rinha 2024 Q1 contract (endpoints, rules, clients, constraints) |
| [`docs/FINDINGS.md`](docs/FINDINGS.md) | Verified technical review: bugs, risks, completeness verdict |
| [`docs/DEPLOYMENT.md`](docs/DEPLOYMENT.md) | How the submission must be packaged & run; what's missing here |

## Quick start

```bash
# Build everything (workspace)
cargo build --release --manifest-path rinha/Cargo.toml

# Run the (still-stubbed) root binary — currently prints "Hello, world!"
cargo run   --release --manifest-path rinha/Cargo.toml

# Database unit tests
cargo test  --manifest-path rinha/Cargo.toml -p espora-db
```

The load balancers are `lib` crates without a `[[bin]]` target — `cargo run -p load_balancer`
will say "no bin target". To run them, add an `src/main.rs` (or a `[[bin]]`) that calls into the
library; see [`docs/FINDINGS.md`](docs/FINDINGS.md) §"Load balancers are libraries".

## Crates & dependency surface

| Crate | Key dependencies |
|---|---|
| `espora-db` | `bitcode` 0.6 (serde), `tokio` 1.37 (fs/io/sync), `futures`, `libc`, `winapi` (synchapi/fileapi) |
| `axum-tcp-socket` | `axum` 0.7, `hyper` 1.3 (full), `hyper-util` 0.1 (tokio/server-auto/http1), `tokio`, `tower` |
| `load_balancer` | `axum` 0.7, `hyper-util` 0.1 (client-legacy/http2/http1), `tokio` |
| `load_balancer_tcp` | `tokio` 1.37 (full) — that's it |
| `rinha` (root) | `serde`, `time` (scaffolding for the unwritten JSON DTOs) |

## Tech stack

- **Rust 2021 edition**, tokio async runtime throughout
- **axum + hyper 1.x** for HTTP
- **bitcode** binary serialization (smaller & faster than JSON) for on-disk rows
- **Custom page-based storage** instead of an external database — the standout design choice
- **Release profile**: `lto = true`, `codegen-units = 1`, `panic = "abort"` for a tight binary

## License

See [`LICENSE`](LICENSE).
