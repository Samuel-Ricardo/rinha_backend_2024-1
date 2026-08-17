# Load Balancers

Two hand-written Rust load balancers, both in the `rinha` workspace, both currently `lib` crates
(no `[[bin]]` / `src/main.rs`, so `cargo run -p …` finds **no bin target** — see the note at the
end). They front the two API instances required by the Rinha:

| Crate | Awareness | Default port | Env | Upstream wire |
|---|---|---|---|---|
| `load_balancer` | HTTP/1+2 client | 9999 | `PORT`, `UPSTREAM` (comma list) | HTTP/2 pooled |
| `load_balancer_tcp` | none (byte-stream) | 9999 | `PORT`, `UPSTREAMS` | raw TCP copy |

---

## 1. `load_balancer` — HTTP reverse proxy (`src/`)

### 1.1 Trait & shared state (`model/mod.rs`)

```rust
pub trait LoadBalancer {
    fn next_server(&self, req: &Request) -> String;
}

#[derive(Clone)]
pub struct AppState {
    pub load_balancer: Arc<dyn LoadBalancer + Send + Sync>,
    pub http_client: Client<HttpConnector, Body>,
}
```

`LoadBalancer` has a single method `next_server(&self, req) -> String` returning an upstream
address. `AppState` holds the chosen strategy (trait-dispatched through `Arc<dyn …>`) and a
shared **pooled** `hyper-util` legacy `Client<HttpConnector, Body>`.

### 1.2 Strategies (`strategy/mod.rs`)

#### `RoundRobin` (the one actually used)

```rust
pub struct RoundRobin {
    pub addrs: Vec<String>,
    pub req_counter: Arc<AtomicUsize>,
}
impl LoadBalancer for RoundRobin {
    fn next_server(&self, _req: &Request) -> String {
        let count = self.req_counter.fetch_add(1, Ordering::Relaxed);
        self.addrs[count % self.addrs.len()].clone()
    }
}
```

Lock-free atomic counter — uniform distribution, request-content-agnostic, cheap.

#### `RinhaAccountBalancer` (account-sticky, **NOT used**)

```rust
pub struct RinhaAccountBalancer {
    pub addrs: Vec<String>,
}
impl LoadBalancer for RinhaAccountBalancer {
    fn next_server(&self, req: &Request) -> String {
        let path = req.uri().path();
        let hash = {
            let mut hasher = DefaultHasher::new();
            path.hash(&mut hasher);
            hasher.finish() as usize
        };
        self.addrs[hash % self.addrs.len()].to_string()
    }
}
```

**Path-hash sticky routing**: hashes the request path (`/clientes/{id}/...`) and deterministically
picks a backend. Because the only varying segment is the client ID, each client is pinned to one
backend → that backend owns the client's append-only `espora-db` log → **single-writer-per-account,
no distributed lock, no cross-node coordination**. This is the strategically correct design for the
Rinha's 5 clients / 2 instances shape — and is exactly what the storage engine rewards.

> ⚠ Despite being the better choice, `RinhaAccountBalancer` is **constructed then discarded** in
> `load_balancer::main` (see §1.4) — the running balancer uses plain `RoundRobin`.

### 1.3 Request forwarding (`proxy.rs`)

```rust
pub async fn main(
    State(AppState { load_balancer, http_client }): State<AppState>,
    mut req: Request,
) -> impl IntoResponse {
    let addr = load_balancer.next_server(&req);

    *req.uri_mut() = {
        let mut parts = req.uri().clone().into_parts();
        parts.authority = Authority::from_str(addr.as_str()).ok();
        parts.scheme = Some(Scheme::HTTP);
        Uri::from_parts(parts).unwrap()
    };

    match http_client.request(req).await {
        Ok(res) => Ok(res),
        Err(_) => Err(StatusCode::BAD_GATEWAY),
    }
}
```

- Preserves **path, query, method, headers, body**; rewrites only `authority` (chosen upstream)
  and forces `scheme = HTTP`.
- Forwards through the pooled client → **`502 BAD_GATEWAY`** on upstream error.
- **No retries, no circuit-breaker, no health checks, no backoff.**

### 1.4 Wiring (`lib.rs`)

```rust
#[tokio::main]
async fn main() {
    let port = env::var("PORT").ok().and_then(|p| p.parse::<u16>().ok()).unwrap_or(9999);
    let addrs = env::var("UPSTREAM").ok()
        .map(|u| u.split(",").map(String::from).collect())
        .unwrap_or(vec!["127.0.0.1:8080".into(), "127.0.0.1:8081".into()]);
    let listener = TcpListener::bind(("0.0.0.0", port)).await.unwrap();

    let client = {
        let mut connector = HttpConnector::new();
        connector.set_keepalive(Some(Duration::from_secs(60)));
        connector.set_nodelay(true);
        Client::builder(TokioExecutor::new())
            .http2_only(true)
            .build::<_, Body>(connector)
    };

    let round_robin = RoundRobin { addrs: addrs.clone(), req_counter: Arc::new(AtomicUsize::new(0)) };
    let fixed_load_balancer = RinhaAccountBalancer { addrs: addrs.clone() };   // built...

    let app_state = AppState { load_balancer: Arc::new(round_robin), http_client: client }; // …ignored
    let app = proxy::main.with_state(app_state);
    axum::serve(listener, app).await.unwrap();
}
```

**Behavior:**

- `PORT` (default **9999**), `UPSTREAM` comma list (default `127.0.0.1:8080,8081`).
- Upstream client: `HttpConnector` keepalive 60s + `nodelay` + **`http2_only(true)`** → all
  upstream traffic forced HTTP/2, multiplexed over pooled connections.

**Issues:**

- **`http2_only(true)` vs HTTP/1-only server**: the only HTTP server in the repo
  (`axum-tcp-socket`) is built with the `http1` feature only (no `http2`), so LB→upstream would
  fail negotiation if `axum-tcp-socket` were the backend. The real Rinha app would need `http2`
  enabled upstream, or the LR needs to downgrade.
- **`RinhaAccountBalancer` is dead code** — built and dropped.
- **No health checks / failure marking** — a dead upstream is re-selected every request → 502.

## 2. `load_balancer_tcp` — raw TCP proxy (`src/lib.rs`)

```rust
#[tokio::main]
async fn main() -> io::Result<()> {
    let prot = env::var("PORT").ok().and_then(|p| p.parse::<u16>().ok()).unwrap_or(9999);

    let addrs = env::var("UPSTREAMS").ok()
        .map(|u| u.split(",").map(|a| a.trim().to_owned()).collect())
        .unwrap_or(vec!["./rinha-app1.socket".into(), "./rinha-app2.socket".into()])
        .into_iter()
        .map(|addr| Box::leak(addr.into_boxed_str()) as &'static str)
        .collect::<Vec<_>>();

    let listner = TcpListener::bind("0.0.0.0:".to_owned() + &prot.to_string()).await.unwrap();
    let mut counter = 0;
    println!("TCP lb ({}) ready 9999", env!("CARGO_PKG_VERSION"));

    while let Ok((mut downstream, _)) = listner.accept().await {
        downstream.set_nodelay(true)?;
        counter += 1;

        let addr = addrs[counter % addrs.len()];
        tokio::spawn(async move {
            let mut upstream = TcpStream::connect(addr).await.unwrap();
            io::copy_bidirectional(&mut downstream, &mut upstream).await.unwrap();
        });
    }
    Ok(())
}
```

**Behavior:**

- `PORT` (default **9999**), `UPSTREAMS` (plural, default socket-style names
  `./rinha-app1.socket`/`./rinha-app2.socket` — only valid if set to real `host:port` strings).
- Round-robin via a plain mutable `counter` (single acceptor task; no atomics).
- Per connection: `set_nodelay`, spawn a task that `TcpStream::connect`s the chosen upstream and
  pumps bytes both ways with `io::copy_bidirectional` — kernel-assisted copy of the full
  downstream↔upstream byte streams. **Protocol-agnostic.**

**Issues:**

- **`panic = "abort"` + `.unwrap()`**: any single connection error (backend down) `unwrap`s
  inside the spawned task; under `panic = "abort"` tokio **cannot isolate** it → the whole LB
  process aborts. This is the most serious reliability defect for any LB in abort mode.
- **Off-by-one**: `counter` increments *before* indexing, so the **first** accepted connection
  maps to `addrs[1]`, skipping `addrs[0]`.
- **`Box::leak`** of each upstream string for `'static` — bounded (read once), but a leak.
- **No health checks / retries** — `TcpStream::connect` and `copy_bidirectional` both `.unwrap()`.
- **`while let Ok(...)`** swallows accept errors and ends the loop → a transient accept failure
  kills the server.

## 3. The two balancers compared

| | HTTP LB | TCP LB |
|---|---|---|
| Layer | L7 (parses nothing, but rewrites URI) | L4 (bytes only) |
| Strategy | `RoundRobin` (sticky available, unused) | plain counter round-robin |
| Upstream transport | HTTP/2 pooled, keepalive 60s | raw TCP |
| Overhead per request | minimal (replace authority) | minimal (`copy_bidirectional`) |
| Failure handling | `502` (no retry) | `unwrap` → can abort process |
| Health checks | none | none |

The **TCP balancer** is the leaner choice (no HTTP parsing, true passthrough), but is more fragile
under `panic = "abort"`. The **HTTP balancer** is the more "correct" reverse proxy but forces
HTTP/2 upstream mismatched with the current `axum-tcp-socket` server.

## 4. Environmental configuration

| Variable | Crate | Purpose | Default |
|---|---|---|---|
| `PORT` | both | listen port | `9999` |
| `UPSTREAM` | HTTP LB | comma-separated upstreams | `127.0.0.1:8080,127.0.0.1:8081` |
| `UPSTREAMS` | TCP LB | comma-separated upstreams | `./rinha-app1.socket,./rinha-app2.socket` |

> Note the **singular vs plural** env name between the two balancers. Default `UPSTREAMS` values
> are socket-style paths that `TcpStream::connect` cannot resolve — supply real `host:port`.

## 5. Why the LBs don't run (`cargo run -p load_balancer` → "no bin target")

Both `load_balancer/src/lib.rs` and `load_balancer_tcp/src/lib.rs` declare `#[tokio::main] async
fn main()` **inside a library** crate, with no `src/main.rs` and no `[[bin]]` in their `Cargo.toml`.
A crate's `lib.rs` `main` is **not** an entry point — the linker never calls it. To run either:

- Add `src/main.rs` that calls the (currently private) library entry, e.g.
  ```rust
  // load_balancer/src/main.rs
  fn main() { /* call into the library */ }
  ```
  and make `lib.rs` export the runnable function (rename `main` → `run` and expose it); **or**
- Add a `[[bin]]` target in `Cargo.toml` pointing at a new `main.rs`.

See [FINDINGS.md](FINDINGS.md) §"Load balancers are libraries".
