# HTTP Server — `axum-tcp-socket`

The smallest crate in the workspace: a single helper function that runs Axum/Hyper over a Tokio
TCP listener. Crate root: `rinha/axum-tcp-socket/src/lib.rs` (44 lines).

## 1. Dependencies

```toml
axum = "0.7.5"
hyper = { version = "1.3.1", features = ["full"] }
hyper-util = { version = "0.1.3", features = ["tokio", "server-auto", "http1"] }
tokio = { version = "1.37.0", features = ["full"] }
tower = { version = "0.4.13", features = ["full"] }
```

Note `hyper-util` is built with **`http1` only** (no `http2`), so the server is **HTTP/1-only**.

## 2. The `server` function (verbatim)

```rust
pub async fn server<S>(path: impl AsRef<Path>, app: S) -> io::Result<()>
where
    S: Service<Request<Incoming>, Response = Response, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send,
{
    let path = path.as_ref();

    fs::remove_file(&path).await.ok();

    let listener = TcpListener::bind(path.to_str().unwrap()).await?;

    while let Ok((socket, _addr)) = listener.accept().await {
        let service = app.clone();

        tokio::spawn(async move {
            let socket = TokioIo::new(socket);

            let hyper_service =
                hyper::service::service_fn(move |request: hyper::Request<Incoming>| {
                    service.clone().call(request)
                });

            if let Err(err) = server::conn::auto::Builder::new(TokioExecutor::new())
                .serve_connection_with_upgrades(socket, hyper_service)
                .await
            {
                eprintln!("server error: {}", err);
            }
        });
    }

    Ok(())
}
```

## 3. What it does, step by step

1. **`fs::remove_file(&path).await.ok()`** — best-effort delete of any pre-existing file at `path`.
2. **`TcpListener::bind(path.to_str().unwrap())`** — bind a Tokio TCP listener whose address is
   the `path` value.
3. **Accept loop** — `listener.accept().await`; on each new socket, clone the service and
   `tokio::spawn` a task that:
   - wraps the stream with `TokioIo::new(socket)` (adapts `tokio::TcpStream` to hyper's IO trait);
   - builds a `hyper::service::service_fn` that delegates to `service.clone().call(request)`;
   - runs `server::conn::auto::Builder::new(TokioExecutor::new()).serve_connection_with_upgrades(...)`
     — the `auto` builder chooses HTTP/1 vs HTTP/2, but only **HTTP/1** is compiled in (see deps);
   - logs per-connection errors to stderr via `eprintln!` without affecting the loop.

## 4. The generic bound

`S: Service<Request<Incoming>, Response = Response, Error = Infallible> + Clone + Send + 'static`.
This is exactly the bound Axum's `Router`/`Service` satisfies, so an `axum::Router` plugs directly
in. `Error = Infallible` means the service is infallible at the HTTP layer (Axum converts route
rejections into responses).

## 5. The naming/implementation mismatch (important)

The crate is named `axum-tcp-socket` and the code does `fs::remove_file` (the textbook Unix
domain-socket cleanup, since `UnixListener` binds to a path and the file must not pre-exist).
**But** it then binds a **`TcpListener`** (TCP), not a `UnixListener`:

- `TcpListener::bind` requires an address parseable by `ToSocketAddrs` (`host:port`). A path like
  `/tmp/rinha.sock` will **fail to resolve** at runtime.
- Conversely `ffile"-style defaults in `load_balancer_tcp` (`"./rinha-app1.socket"`) imply the
  intended design was Unix-domain sockets between LB and API (zero-copy, no port allocation) — but
  the server implements only TCP.
- On Windows, `std::os::unix::net` is unavailable, which is likely why `TcpListener` was used —
  but that abandons the socket-file goal.

**Net:** to honor the crate's name you'd switch to `tokio::net::UnixListener` (Linux) behind a
`cfg` and only use TCP on Windows; today it's a TCP server with a misleading name and a vestigial
`remove_file`.

## 6. Reliability notes

- **`while let Ok(...)` silently swallows accept errors** — on a transient accept failure the loop
  ends and the function returns `Ok(())` (the server stops). A robust server would log and retry.
- **`fs::remove_file(...).ok()`** swallows cleanup errors (permission, parent missing) — only the
  bind error surfaces via `?`.
- Per-connection panics under `panic = "abort"` would abort the whole process (compare the LBs);
  here the bodies are infallible hyper tasks, so the risk is lower, but `path.to_str().unwrap()`
  and the `while`-loop pattern are sharp edges.

## 7. How it would be used

```rust
let app = Router::new()
    .route("/clientes/:id/transacoes", post(transacoes))
    .route("/clientes/:id/extrato", get(extrato))
    .with_state(state);

axum_tcp_socket::server("app1", app).await?;   // (TCP) or a path you don't mind removing
```

In practice the Rinha app would more likely use `axum::serve(listener, app)` directly; this crate
adds the `serve_connection_with_upgrades` + socket-file-cleanup plumbing over a single function.
