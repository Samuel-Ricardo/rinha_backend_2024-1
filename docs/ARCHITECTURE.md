# Architecture

This document describes the **intended** and **as-built** architecture of `rinha_backend_2024-1`,
a Rust solution for the Rinha de Backend 2024 Q1 challenge. For the precise contract, see
[CHALLENGE-SPEC.md](CHALLENGE-SPEC.md); for the deficit between design and reality, see
[FINDINGS.md](FINDINGS.md).

## 1. Workspace composition

A Cargo workspace (`rinha/Cargo.toml`) with one binary root + four library members:

| Package | Path | Kind | Responsibility |
|---|---|---|---|
| `rinha` | `rinha/src/main.rs` | bin | The banking HTTP app — **currently a stub** |
| `espora-db` | `rinha/espora-db/` | lib | Custom embedded page-based database |
| `axum-tcp-socket` | `rinha/axum-tcp-socket/` | lib | Axum/Hyper server bound on a TCP path |
| `load_balancer` | `rinha/load_balancer/` | lib | HTTP/2 reverse-proxy load balancer |
| `load_balancer_tcp` | `rinha/load_balancer_tcp/` | lib | Raw TCP byte-stream load balancer |

### Build profile (`rinha/Cargo.toml`)

```toml
[profile.release]
codegen-units = 1   # whole-crate codegen: max inlining, slow compile
lto = true           # cross-crate link-time optimization
panic = "abort"      # no unwinding; smaller binary, panics kill the process
```

These apply workspace-wide. `panic = "abort"` is the right call **for the API** (maximize
throughput, fail fast) but a **hazard for the load balancers** (a single connection error that
`.unwrap()`s will abort the whole LB under abort — see [FINDINGS.md](FINDINGS.md) §"panic=abort").

## 2. Logical architecture (intended)

```
  ┌─────────────┐                          ┌─────────────┐
  │ Client /    │   HTTP   :9999           │  Gatling    │
  │ Gatling     │ ───────────────────────► │  doesn't    │
  └─────────────┘                          └─────────────┘
                  │
        ┌─────────▼──────────┐
        │  Load Balancer     │    load_balancer TCP or HTTP
        │  (port 9999)       │
        └───┬───────────┬───┘
            │           │   round-robin (sticky routing designed)
       ┌────▼────┐  ┌───▼─────┐
       │ API #1  │  │ API #2  │     rinha app (axum Router) — NOT IMPLEMENTED
       └────┬────┘  └───┬─────┘
            │           │
       ┌────▼────┐  ┌───▼─────┐
       │espora-db│ │espora-db │   append-only per-account transaction log
       │ #1     │  │ #2      │
       └────────┘  └─────────┘
```

### Design intent: per-account sharding

`load_balancer::strategy::RinhaAccountBalancer` hashes the request path
(`/clientes/{id}/...`) and deterministically routes to one backend. Because each client ID then
lands on a single instance, that instance becomes the **sole writer** for that account's
`espora-db` log — this removes the need for distributed locking or cross-node coordination on the
hot transaction path. It is the strategically correct choice for the Rinha's 5 accounts, 2
instances shape.

> ⚠ Currently `RinhaAccountBalancer` is **constructed but discarded** in `load_balancer::main`
> in favor of plain `RoundRobin`. See [LOAD-BALANCERS.md](LOAD-BALANCERS.md).

## 3. Request lifecycle (target design)

### `POST /clientes/{id}/transacoes`

1. LB picks an upstream (round-robin / intended: path-hash sticky).
2. API parses `{ valor: i64>0, tipo: "c"|"d", descricao: 1..=10 chars }`; else `422`.
3. Look up client by `id`; unknown → `404`.
4. Compute `novo_saldo = saldo + valor` (crédito `c`) or `saldo - valor` (débito `d`).
5. If `d` and `novo_saldo < -limite` → `422` (no write).
6. Append a row to the client's `espora-db` log, persist (fsync policy), update cached `saldo`.
7. Return `200 { limite, saldo }`.

### `GET /clientes/{id}/extrato`

1. LB → API (same routing).
2. Client by `id`; unknown → `404`.
3. Read current `saldo` + `limite`; read the append-only log reverse and take the first 10 rows.
4. Return `200 { saldo: { total, data_extrato, limite }, ultimas_transacoes: [≤10] }`.

> None of steps 1–7 are implemented in the workspace today — the API binary is a stub. The
> components below exist to support exactly this flow.

## 4. Component roles

### 4.1 `espora-db` — the storage engine

A hand-written embedded database. Pages are 4096 bytes; each row is a fixed-size slot
`[8-byte BE length][bitcode payload][zero-pad]`. Rows are **append-only**; the last page is
rewritten in place on each insert, and a new page is appended when the current one fills.
Durability is tunable (`sync_writes(false)` disables fsync; `sync_write_interval(d)` batches it).
Both a synchronous (`std::fs`) and an asynchronous (`tokio::fs`) variant exist, sharing the
`Page` type and a `Builder`. Read [STORAGE-ENGINE.md](STORAGE-ENGINE.md).

### 4.2 `axum-tcp-socket` — the HTTP server helper

A single generic function `server<S: Service>(path, app)` that binds a `tokio::TcpListener`,
spawns a task per connection, and runs `hyper` `serve_connection_with_upgrades` over
`TokioIo`. Despite the name and the `fs::remove_file` socket-cleanup idiom, it binds a **TCP**
listener (`path` must be `host:port`), not a Unix domain socket. Read
[HTTP-SERVER.md](HTTP-SERVER.md).

### 4.3 `load_balancer` — HTTP reverse proxy

Accepts `PORT` (default 9999) and `UPSTREAM` (comma list, default `127.0.0.1:8080,8081`),
builds a pooled `hyper-util` legacy `Client` with HTTP/2-only upstream, keepalive 60s, nodelay,
and forwards each `Request` by rewriting its authority/scheme while preserving path, query,
method, headers, and body. Upstream failure → `502`. Read [LOAD-BALANCERS.md](LOAD-BALANCERS.md).

### 4.4 `load_balancer_tcp` — raw TCP proxy

Accepts `PORT` (default 9999) and `UPSTREAMS`; for each accepted connection it spawns a task
that `TcpStream::connect`s the chosen upstream and `io::copy_bidirectional`s bytes both ways —
protocol-agnostic, no HTTP awareness. Read [LOAD-BALANCERS.md](LOAD-BALANCERS.md).

## 5. Concurrency model

- **Tokio multi-thread runtime** is implied everywhere (`tokio = { features = ["full"] }` in
  the network crates).
- **One task per connection** in `axum-tcp-socket` and both balancers — accepts run sequentially
  on a single acceptor, connection handling is concurrent via `tokio::spawn`.
- **`espora-db` does not manage its own internal concurrency.** `insert`/`pages`/`rows` take
  `&mut self`, so the engine relies on the caller (e.g. `Arc<Mutex<Db>>` or
  `tokio::sync::Mutex<Db>`) to serialize access. Inter-process exclusion is an opt-in manual
  `lock_writes()` advisory call (currently broken — see the storage engine doc).
- **Account-sticky routing** would make per-account locking unnecessary (one writer per
  account), which is the architectural payoff of `RinhaAccountBalancer`.

## 6. Performance design (winning-the-Rinha levers)

| Lever | Where | Effect |
|---|---|---|
| `bitcode` (not JSON) for on-disk rows | `espora-db` | compact, fast (de)serialize |
| 4096-byte page-aligned I/O | `espora-db` | OS-page-friendly writes |
| fsync-tunable durability | `Builder::sync_writes` | trade safety for RPS |
| Account-sticky routing | `RinhaAccountBalancer` | single-writer-per-account, no coordination |
| Pooled HTTP/2 upstream (keepalive 60s, nodelay) | `load_balancer` | connection reuse LB→API |
| Raw-TCP passthrough (`copy_bidirectional`) | `load_balancer_tcp` | zero parsing overhead |
| `LTO` + `codegen-units=1` + `panic=abort` | `[profile.release]` | tightest binary |

### Known anti-throughput items

- `fsync`-on-every-insert is the **default** (`sync_writes: Some(Duration::from_secs(0))`).
- Full 4 KiB page rewrite on **every** insert (write amplification).
- A `Vec::concat`-and-memcpy per insert in `Db::insert` (could be two `write_all` calls).
- Page-by-page `read_exact` with a `seek` per page on the hot read path (no mmap).
- `RinhaAccountBalancer` not wired → locality lost.
