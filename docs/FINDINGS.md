# Findings — Verified Technical Review

A consolidated, evidence-backed assessment of `rinha_backend_2024-1`. Findings are grouped by
component and ordered by severity within each. Line references point at paths under `rinha/`.

---

## 0. Completeness verdict (read this first)

The repo is an **infrastructure component workspace**, not a runnable Rinha submission.

| Capability | Present? |
|---|---|
| Custom embedded DB (`espora-db`) | ✅ designed & partially implemented |
| HTTP server helper (`axum-tcp-socket`) | ✅ minimal, works as TCP server |
| HTTP load balancer with strategies | ✅ code exists, but is a **non-runnable lib** |
| Raw TCP load balancer | ✅ code exists, but is a **non-runnable lib** |
| **Banking HTTP API** (`POST /clientes/{id}/transacoes`, `GET /clientes/{id}/extrato`) | ❌ absent |
| Client entities / 5 preloaded accounts / `saldo`-`limite` logic | ❌ absent |
| `docker-compose.yml`, `Dockerfile`, LB config | ❌ absent |
| Build artifacts for member crates | ❌ only `target/debug/rinha.exe` ever built (the stub) |

A repo-wide search for `clientes|transacoes|extrato|saldo|limite|credito|debito|transacao` across
all `.rs`/`.toml`/`.md` files returns **no matches**. `rinha/src/main.rs` is
`fn main() { println!("Hello, world!"); }`. The work assembled the building blocks (DB, server,
two LBs) but never returned to implement the endpoints that the Rinha actually tests.

---

## 1. `espora-db`

### 1.1 Async model does not compile as written — **Blocker (build)**

`model/tokio/database.rs` mixes Windows-only imports with Unix-only APIs and has a type error:

- `use std::os::windows::io::{AsHandle, AsRawHandle};` (Windows-only, **unused** in body)
  vs `self.writer.as_raw_fd()` (Unix-only, and the required `std::os::fd::AsRawFd` trait is **not
  imported**) and `libc::flock` (Unix-only).
- `let offset = (cursor * PAGE_SIZE) as u64;` then `seek(io::SeekFrom::End(-offset))` — unary `-`
  on a `u64` is **`E0600`** (cannot negate unsigned). The sync version casts `as i64` first; the
  async version does not.
- Because `model/mod.rs:4 pub mod tokio;` is **unconditional**, a default `cargo build -p espora-db`
  would fail. (Consistent with `target/` containing only the stub binary.)

### 1.2 Cross-platform locking is broken — **High**

- `linux_lock.rs` is **never declared as a module** (`lib.rs` only has `error/lock/model/test`).
  The Unix `LockHandle { fd }` + `libc::flock(LOCK_UN)` is **orphaned dead code**; only the
  Windows path runs.
- **WinAPI mismatch**: `lock_writes` acquires with `LockFile` (a byte-range file lock on `[0,1)`),
  but `lock.rs` `Drop` releases with `ReleaseMutex` (the API for a `CreateMutex` kernel object —
  **not** a file-range lock). The RAII scoped lock does not do what its lifetime promises; the
  byte-range lock is only truly released when the underlying `File` closes.
- **Inverted success/failure**: `LockFile` returns nonzero on **success**, zero on failure. The
  code maps `0 => Ok(...)` and `_ => Err(...)` — treating failure as success and success as
  failure.
- **Async path**: acquires via `libc::flock(LOCK_EX)` (Unix) but stores a **Windows**
  `LockHandle` (Drop → `ReleaseMutex`). Mismatched acquire/release across platforms. The
  `//TODO: REFACT ALL TO LIBC` confirms it's unfinished.
- Nothing guards regions beyond byte `[0,1)`, so concurrent writers from separate processes could
  still corrupt unlocked regions.

### 1.3 Resume logic can clobber a full page — **High (data loss)**

`from_path` loads the *trailing* page as `current_page`. If that page is **already full**
(`available_rows() == 0`), `from_path` still loads it and repositions the writer to overwrite it.
Because `insert` only branches on a full page **after** writing, the first post-open insert
**rewrites (destroys) the full page** instead of appending a new one. On-full-page should be
detected at open time and start a fresh page.

### 1.4 `Page::insert` has no payload-size guard — **Medium (latent panic)**

If a serialized payload exceeds `ROW_SIZE - 8`, the padding length
`ROW_SIZE - (serialized.len() + 8)` underflows → panic/overflow. Callers must size `ROW_SIZE`
generously; the engine should assert/return an error instead.

### 1.5 Other issues — **Low**

- The `tokio` feature is mis-declared: `async-stream` is referenced in the feature but is **not a
  dependency** (not in `Cargo.lock`), so `--features tokio` errors. `futures`/`tokio` are not
  optional yet use `dep:tokio` semantics. And the async `model::tokio` module is compiled
  **unconditionally** by `model/mod.rs`, so the feature does nothing anyway. The
  `lib.rs`-level `pub mod tokio` points at an **empty** `src/tokio.rs`.
- Const-generic named `ROM_SIZE` (sync struct/builder) vs `ROW_SIZE` (impl/Page/async) — same
  value, misleading name.
- `model/database.rs:12` imports `LOCKFILE_EXCLUSIVE_LOCK` (unused); `:6` imports `FileExt`
  (unused). `model/tokio/database.rs:3` imports `AsHandle`/`AsRawHandle` (unused, Windows-only).
- Tests (`test/tokio/database.rs`) open the **same** `temp_dir().join("test.espora")` with no
  cleanup → second-run failures (resume appends onto stale state).

### 1.6 Reliability summary (DB)

- Durability default = `sync_writes: Some(Duration::from_secs(0))` → **fsync every insert**.
  Correct for safety; an RPS cap for the Rinha unless tuned via `sync_writes(false)` /
  `sync_write_interval(d)`.
- Full 4 KiB page rewritten on every insert (write amplification); extra `Vec::concat` allocation
  per insert.
- Read path `read_exact`-by-page with a `seek` per page — no mmap, no streaming.

---

## 2. `axum-tcp-socket`

- **Name vs impl mismatch**: does `fs::remove_file(&path)` (Unix-domain-socket cleanup) but binds
  a `TcpListener::bind(path)` (TCP). `TcpListener::bind` needs a `host:port`; a path string fails
  to resolve at runtime. The crate is a TCP server with a Unix-socket-style name + vestigial file
  removal (`src/lib.rs:18-21`) — either should be `UnixListener` (Linux only) or the naming/removal
  removed.
- `while let Ok((socket, _)) = listener.accept().await` — swallows accept errors and exits
  (returns `Ok(())`), so a transient accept failure stops the server.
- `auto::Builder` (HTTP/1 vs HTTP/2 negotiator) is built with **only** the `http1` feature —
  serves HTTP/1 only. No concern by itself, but see the HTTP LB below.

---

## 3. `load_balancer`

### 3.1 The strategically-best strategy is dead code — **High (design)**

`RinhaAccountBalancer` (path-hash sticky routing) is constructed in `main` then **discarded**;
`app_state.load_balancer = Arc::new(round_robin)`. The whole point of account-sticky sharding —
single-writer-per-account, no distributed lock — is forfeited. Fix: swap the two lines (and
ideally make the strategy configurable via env).

### 3.2 HTTP/2-only upstream vs HTTP/1-only server — **High (integration)**

`Client::builder(...).http2_only(true)` (upstream client) is mismatched with `axum-tcp-socket`,
which compiles only the `http1` feature. If the LB fronts `axum-tcp-socket`, upstream negotiation
fails. Either enable `http2` upstream or downgrade the client.

### 3.3 Not a runnable binary — **Medium (usability)**

`load_balancer/src/lib.rs` declares a **private** `#[tokio::main] async fn main()` inside a
**library** crate; no `src/main.rs`, no `[[bin]]`. `cargo run -p load_balancer` → "no bin target".
Fix: add a `src/main.rs` (and expose a `run()`), or add a `[[bin]]` target.

### 3.4 Reliability gaps — **Medium**

- No health checks, no failure marking, no retries, no circuit-breaker. A dead upstream is
  re-selected every request → `502`.
- `axum::serve(listener, app).await.unwrap()` at the top level — `bind(...).unwrap()` too. Under
  `panic = "abort"` a bind failure on startup aborts (acceptable) but a per-request `Uri::from_parts(...)
  .unwrap()` could abort on unusual inputs.

---

## 4. `load_balancer_tcp`

### 4.1 `panic = "abort"` + `.unwrap()` — **High (reliability)**

The most serious LB defect: `TcpStream::connect(addr).await.unwrap()` and
`io::copy_bidirectional(&mut downstream, &mut upstream).await.unwrap()` run inside spawned tasks.
With `panic = "abort"` (workspace `[profile.release]`), a panic **cannot be isolated** by tokio —
the whole LB process aborts. A **single** backend-down connection kills the balancer. In a default
(unwinding) build, tokio would isolate it. For an LB under the Rinha barrage, this is a guaranteed
crash. Fix: replace `.unwrap()` with proper error handling, or relax `panic = "abort"` for this crate.

### 4.2 Off-by-one — **Low**

`counter += 1` happens **before** indexing, so the **first** accepted connection maps to
`addrs[1]`, skipping `addrs[0]`. Use `let idx = counter; counter += 1; addrs[idx % len]`.

### 4.3 Other — Low

- `Box::leak(addr.into_boxed_str()) as &'static str` per upstream — bounded (config read once),
  but technically a leak.
- Default `UPSTREAMS` are socket-style paths (`./rinha-app1.socket`) that `TcpStream::connect`
  can't resolve → must supply real `host:port` (and note the env name is plural `UPSTREAMS` vs the
  HTTP LB's singular `UPSTREAM`).
- Not a runnable binary (same as the HTTP LB).

---

## 5. Cross-cutting

### 5.1 `panic = "abort"` tradeoff

`Rinha`-appropriate for the **API** (smallest binary, fail-fast). Hazard for **both LBs** because
they `.unwrap()` connection errors inside spawned tasks — abort kills the process. Either keep
abort for the API binary only and use unwinding for the LBs, or scrub the `.unwrap()`s.

### 5.2 Missing deployment artifacts (challenge blocker)

No `docker-compose.yml`, `Dockerfile`, `nginx.conf` / LB config, `.env`, or shell scripts anywhere
in the repo (verified by repo-wide search). The Rinha requires compose with `cpu ≤ 1.5` /
`memory ≤ 550MB`, 2 API instances + LB on `9999`. See [DEPLOYMENT.md](DEPLOYMENT.md).

### 5.3 `unsafe` usage

All `unsafe` is at OS-lock FFI boundaries:
- `lock.rs` `ReleaseMutex`, `database.rs` `LockFile`, `linux_lock.rs` `flock`, async path `flock`,
- `Box::leak` (safe API, intentional `'static`).

No pointer arithmetic. The unsafe is **reasonable in scope** but, given §1.2's bugs, semantically
**incorrect** (wrong release API, inverted result). No raw secrets or injection surfaces (input
parsing would live in the unwritten app).

### 5.4 Security note

There are no input-handling surfaces yet (the API isn't implemented), so no SQLi/PII/credential
concerns in the current code. `bitcode` deserialization of untrusted data is the future risk:
`bitcode` is a binary format optimized for trusted inputs — **do not** deserialize
 attacker-controlled bytes without validating provenance and length; the engine has **no**
payload-size guard (§1.4) so a malicious oversized payload could panic the storage task.

---

## 6. Defect register (priority)

| # | Component | Severity | Defect |
|---|---|---|---|
| F1 | espora-db | Blocker | async model doesn't compile (`-u64`, mixed win/unix APIs) |
| F2 | espora-db | High | locking broken (orphaned Linux impl, `ReleaseMutex` release, inverted `LockFile` result, flock/ReleaseMutex mismatch) |
| F3 | espora-db | High | `from_path` clobbers a trailing full page on first insert |
| F4 | load_balancer | High | `RinhaAccountBalancer` dead code (sticky routing not used) |
| F5 | load_balancer | High | HTTP/2-only upstream vs HTTP/1-only server |
| F6 | load_balancer_tcp | High | `panic=abort` + `.unwrap()` → single error aborts the LB |
| F7 | both LBs | Medium | non-runnable `lib` crates (no `[[bin]]`/`main.rs`) |
| F8 | espora-db | Medium | `Page::insert` no payload-size guard |
| F9 | axum-tcp-socket | Medium | named after a socket but binds TCP; `remove_file` vestigial |
| F10 | both LBs, server | Medium | `while let Ok(...)` swallows accept errors → silent stop |
| F11 | overall | Blocker | no banking API, no client seed, no compose/Dockerfile |
| F12 | espora-db | Low | `tokio` feature mis-declared (`async-stream` not a dep); async module compiled unconditionally; `ROM_SIZE` vs `ROW_SIZE` naming |
| F13 | espora-db | Low | tests not isolated (shared temp file, no cleanup) |
| F14 | load_balancer_tcp | Low | off-by-one in round-robin indexing |
| F15 | load_balancer/most | Low | unused imports; leak-via-`Box::leak` |

---

## 7. Recommended path to a runnable submission

1. **Fix `espora-db` async compile (F1)** and unify locking behind `#[cfg]` (Linux: `flock`;
   Windows: corrected `LockFile`/`UnlockFile` or a Mutex), correcting the inverted `LockFile`
   result and the wrong release API (F2).
2. **Fix `from_path` full-page clobber (F3)** and add a `Page::insert` size guard (F8).
3. **Wire `RinhaAccountBalancer` instead of `RoundRobin`** (F4) and reconcile the upstream HTTP
   version (F5) — enable `http2` on the server, or drop `.http2_only(true)`.
4. **Add `main.rs`/`[[bin]]`** to both LBs (F7) and replace LB `.unwrap()`s with error handling
   (or relax `panic=abort` for LBs) (F6).
5. **Implement the Rinha API** in `rinha/src/main.rs` (or a new crate): the `Router`, the
   `Transacao`/`Saldo`/`Extrato` DTOs (`serde` + `time` are already declared for this), the
   5-client in-memory table, the credit/debit + `limite` enforcement (422/404/200), and the
   append-via-`espora-db` + last-10-extrato read (F11). Seed clients 1–5.
6. **Add `Dockerfile` + `docker-compose.yml`** under 1.5 CPU / 550 MB, LB on 9999, 2 API
   instances, mounting `espora-db` page files (F11).
7. Run the Gatling pre-test harness / local test to validate readiness under 40s startup.
