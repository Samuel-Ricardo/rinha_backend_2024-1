# Challenge Spec — Rinha de Backend 2024 Q1

Source: the [official challenge repo](https://github.com/zanfranceschi/rinha-de-backend-2024-q1)
(`@zanfranceschi`). This document is the **authoritative contract** the `rinha_backend_2024-1`
solution must satisfy. It is reproduced here so the project docs are self-contained.

> The Rinha is a community challenge: build a backend API that handles a specific spec under
> **controlled resource constraints**, evaluated with a **Gatling** load test that checks both
> performance and correctness (including concurrency-correct saldo/limite logic).

## 1. Endpoints

Two HTTP endpoints, per client (by integer `id`):

### 1.1 Transactions — `POST /clientes/{id}/transacoes`

**Request body**

```json
{ "valor": 1000, "tipo": "c", "descricao": "descricao" }
```

- `id` — integer in the URL identifying the client.
- `valor` — positive integer, **cents** (no fractions of a cent). R$ 10 = `1000`.
- `tipo` — exactly `"c"` (crédito, increases balance) or `"d"` (débito, decreases balance).
- `descricao` — string, **1 to 10 characters**.

**Success response — `HTTP 200 OK`**

```json
{ "limite": 100000, "saldo": -9098 }
```

- `limite` — the client's registered limit.
- `saldo` — the **new** balance after the transaction.

> A successful transaction **must** return HTTP 200.

**Rules**

- A debit **must never** make the client's balance less than `-limite`. A client with limit 1000
  may never have `saldo < -1000`; `saldo = -1001` or lower is a Rinha inconsistency.
- A debit that would leave the balance inconsistent → **HTTP 422**, and the transaction is **not**
  completed.
- Malformed payload (e.g. `descricao` longer than 10 chars, `tipo` other than `"c"`/`"d"`, or a
  non-integer `valor`) → **HTTP 422** (or 400 for a non-integer `valor`).
- Unknown client `id` → **HTTP 404**. (Do **not** return 2xx with a "not found" body — that fails
  the test.)

### 1.2 Statement — `GET /clientes/{id}/extrato`

**Request**

- `id` — integer in the URL identifying the client.

**Success response — `HTTP 200 OK`**

```json
{
  "saldo": {
    "total": -9098,
    "data_extrato": "2024-01-17T02:34:41.217753Z",
    "limite": 100000
  },
  "ultimas_transacoes": [
    { "valor": 10, "tipo": "c", "descricao": "descricao", "realizada_em": "2024-01-17T02:34:38.543030Z" },
    { "valor": 90000, "tipo": "d", "descricao": "descricao", "realizada_em": "2024-01-17T02:34:38.543030Z" }
  ]
}
```

- `saldo`
  - `total` — the **current** total balance (not only the ones listed).
  - `data_extrato` — date/time of the query (ISO 8601, UTC `Z`).
  - `limite` — the client's registered limit.
- `ultimas_transacoes` — the **last 10 transactions**, ordered by date/time **descending**, each
  with `valor`, `tipo ("c"/"d")`, `descricao`, and `realizada_em` (ISO 8601 UTC).

**Rule**

- Unknown client `id` → **HTTP 404** (again, never 2xx).

## 2. Preloaded clients (mandatory)

Exactly five clients must exist before the test, with these IDs, limits and initial balances:

| id | limite | saldo inicial |
|---:|---:|---:|
| 1 | 100000 | 0 |
| 2 | 80000 | 0 |
| 3 | 1000000 | 0 |
| 4 | 10000000 | 0 |
| 5 | 500000 | 0 |

> **Do not** register a client with id **6**: part of the test verifies that `id 6` does not
> exist and the API returns **404**.

## 3. Minimal architecture

- A **load balancer** distributing traffic with **round-robin**. Nginx is no longer mandatory —
  you may pick or build any LB (e.g. HAProxy). **The LB must receive test traffic on port 9999.**
- **2 web server instances** serving the HTTP requests (behind the LB).
- A database — relational or non-relational — **except** anything primarily in-memory (e.g. Redis).

## 4. Resource constraints (Docker Compose)

In `docker-compose.yml`, limit **all** services so the totals don't exceed:

- `deploy.resources.limits.cpu` ≤ **1.5** (one-and-a-half CPU, shared by all services)
- `deploy.resources.limits.memory` ≤ **550MB** (shared by all services)

## 5. Load test & evaluation

- **Gatling** is used (download from <https://gatling.io/open-source/>; needs a 64-bit OpenJDK LTS
  — 11, 17, or 21) with `GATLING_HOME` set so `$GATLING_HOME/bin/gatling.sh`
  (`%GATLING_HOME%\bin\gatling.bat` on Windows) is valid.
- Configure `./executar-teste-local.sh` (or `.ps1`), start the API/LB on **port 9999**, then run
  the script. Reports save to `./load-test/user-files/results`.
- **Pre-test**: before measuring, a script polls `GET /clientes/1/extrato` for up to **40 seconds**
  (every 2s) waiting for the API to be ready. Ensure services start and answer within 40s.
- The simulation **also validates the saldo/limite business logic** (unusual for a perf test); so
  correctness under concurrency is graded, not just throughput.

## 6. Submission contents (per challenge)

Each submission (a PR adding a `participantes/...` dir) must include:

- `docker-compose.yml` defining the services within resource constraints.
- `README.md` with participant info and implementation details.
- Any other files required for the containerized solution to run.

## 7. How `rinha_backend_2024-1` maps onto the spec

| Spec requirement | This repo |
|---|---|
| `POST /clientes/{id}/transacoes` | **Missing** — no handler, no router, no DTO (only `serde`+`time` deps scaffolded) |
| `GET /clientes/{id}/extrato` | **Missing** |
| 5 preloaded clients (no id 6) | **Missing** — no seed/client entity at all |
| LB on port 9999, round-robin | `RoundRobin` exists & defaults to 9999 ✓ (but as a lib, not a runnable bin) |
| 2 API instances | components exist; the API binary is a stub |
| Database (non-in-memory) | `espora-db` custom embedded DB ✓ (the standout) |
| `docker-compose.yml` within 1.5 CPU / 550 MB | **Missing** — no Dockerfile/compose |
| Works on Gatling pre-test within 40s | **Untestable** — app not runnable |
| Correctness of saldo/limite under concurrency | **Untestable** — app not implemented |

See [FINDINGS.md](FINDINGS.md) for the full gap analysis.
