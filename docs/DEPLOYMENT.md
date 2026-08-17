# Deployment

How the Rinha submission must be packaged and run, and what `rinha_backend_2024-1` is currently
missing to ship. Challenge constraints are from [CHALLENGE-SPEC.md](CHALLENGE-SPEC.md).

## 1. What the challenge requires

A `docker-compose.yml` defining:

- **One load balancer** receiving test traffic on **port 9999**, distributing with **round-robin**
  to **2 web/API instances**.
- **2 API instances** implementing the endpoints.
- **One database** (relational or non-relational; **not** primarily in-memory like Redis). In this
  solution that role is `espora-db` — the embedded engine used **by each API instance**.
- Total resources across **all** services:
  - `deploy.resources.limits.cpu` ≤ **1.5**
  - `deploy.resources.limits.memory` ≤ **550MB**
- A `Dockerfile` per service, plus any LB config (Nginx is no longer mandatory — the custom Rust
  balancer is fine), and a `README.md` with participant info.

## 2. Intended topology for this solution

```
            Gatling → :9999
                 │
        ┌────────▼─────────┐
        │ load_balancer_tcp │   (or load_balancer)
        └───┬──────────┬───┘
   round-robin│          │   (intended: path-hash sticky — RinhaAccountBalancer)
            ┌─▼──┐    ┌──▼─┐
            │API1│    │API2│
            └─┬──┘    └──┬─┘
              │          │
        ┌─────▼────┐┌────▼─────┐
        │esoradb#1 ││esoradb#2 │  per-account append-only logs (page files mounted into the container FS)
        └──────────┘└──────────┘
```

Because `espora-db` is embedded (no server), **no DB container** is needed — each API instance
runs the engine in-process, owning its clients' page files. With `RinhaAccountBalancer` (sticky),
client N's writes always land on its owning instance, so no cross-instance coordination is
required. (Today only `RoundRobin` is wired, which would interleave writes for one client across
both instance-local DBs — keep the intended sticky routing when you wire it up.)

## 3. Resource allocation suggestion (within 1.5 CPU / 550 MB)

A sensible split across 4 services (LB, API×2, plus embedded DB adds 0 services):

| Service | cpu | memory |
|---|---|---|
| load balancer | 0.25 | 60MB |
| api-1 | 0.5 | 220MB |
| api-2 | 0.5 | 220MB |
| **total** | 1.25 ≤ 1.5 | 500MB ≤ 550MB |

Embedded `espora-db` page files can be small; with `ROW_SIZE` tuned to the transaction record (a
few hundred bytes) and a modest number of transactions per client, the on-disk footprint is tiny.

## 4. Building

```bash
# Build all crates in release (LTO + codegen-units=1 + panic=abort)
cargo build --release --manifest-path rinha/Cargo.toml

# Verify the DB unit tests once
cargo test --manifest-path rinha/Cargo.toml -p espora-db
```

> ⚠ `cargo build -p espora-db` currently fails because the async model in
> `model::tokio` does not compile as written (see [FINDINGS.md](FINDINGS.md) F1). Fix that before
> building.

## 5. Running locally (without Docker)

```bash
# API instance 1
PORT=8080 cargo run --release -p rinha        # (once the app binary exists)
# API instance 2
PORT=8081 cargo run --release -p rinha

# Load balancer (TCP)
PORT=9999 UPSTREAMS=127.0.0.1:8080,127.0.0.1:8081 cargo run --release -p load_balancer_tcp
# …or the HTTP balancer
PORT=9999 UPSTREAM=127.0.0.1:8080,127.0.0.1:8081 cargo run --release -p load_balancer
```

> ⚠ Both `load_balancer` and `load_balancer_tcp` are `lib` crates with no `[[bin]]` target, so
> `cargo run -p …` will say "no bin target". Add a `src/main.rs` (or a `[[bin]]` in their
> `Cargo.toml`) first — see [LOAD-BALANCERS.md](LOAD-BALANCERS.md) §5.

## 6. Running via Docker Compose (target shape)

```bash
docker compose up --build        # starts LB(:9999) + api-1 + api-2
curl -s http://localhost:9999/clientes/1/extrato   # Gatling's readiness probe
```

## 7. Reference `docker-compose.yml` (template — not present in the repo)

```yaml
version: "3.8"
services:
  api1:
    build: ./rinha
    environment:
      - PORT=8080
      - ESPORA_DB_PATH=/data/clientes-1.espora
    volumes:
      - api1-data:/data
    deploy:
      resources:
        limits: { cpus: "0.5", memory: 220M }

  api2:
    build: ./rinha
    environment:
      - PORT=8081
      - ESPORA_DB_PATH=/data/clientes-2.espora
    volumes:
      - api2-data:/data
    deploy:
      resources:
        limits: { cpus: "0.5", memory: 220M }

  load-balancer:
    build: ./rinha/load_balancer_tcp       # (or load_balancer)
    ports: ["9999:9999"]
    environment:
      - PORT=9999
      - UPSTREAMS=api1:8080,api2:8081
    depends_on: [api1, api2]
    deploy:
      resources:
        limits: { cpus: "0.25", memory: 60M }

volumes:
  api1-data:
  api2-data:
```

Notes:
- Upstream hostnames must match compose service names (`api1`/`api2`); the example sets
  `UPSTREAMS` accordingly.
- The Rinha LB must listen on **9999** — the `ports: ["9999:9999"]` match satisfies this.
- Each API instance has its own page-file volume (per-account `espora-db` log); with sticky
  routing, client N's writes stay on one instance.

## 8. Gatling (load test)

1. Download Gatling (<https://gatling.io/open-source/>) — needs 64-bit OpenJDK LTS (11/17/21).
2. Set `GATLING_HOME` so `$GATLING_HOME/bin/gatling.sh` (`%GATLING_HOME%\bin\gatling.bat` on
   Windows) is valid.
3. Configure `./executar-teste-local.sh` (or `.ps1`), start the LB on **port 9999**, run the script.
4. Reports save to `./load-test/user-files/results`.
5. **Pre-test readiness**: a probe polls `GET /clientes/1/extrato` for up to **40s** every 2s —
   the API must answer within 40s of container start.

## 9. What's missing here (gap list)

- ❌ `docker-compose.yml` and `Dockerfile`(s) — **none** exist in the repo.
- ❌ A runnable `load_balancer` / `load_balancer_tcp` binary (both are `lib` crates).
- ❌ Any LB/runtime config files (`nginx.conf`-equivalent, `.env`, shell scripts).
- ❌ The Rinha API binary itself (endpoints, 5-client seed, validation) — `main.rs` is a stub.
- ❌ Health/readiness endpoint — the Gatling pre-test needs `GET /clientes/1/extrato` to answer
  within 40s of start; even a cheap "warm" answer before fully ready helps avoid pre-test timeout.

See [FINDINGS.md](FINDINGS.md) for the full defect list and the recommended sequence to close these
gaps.
