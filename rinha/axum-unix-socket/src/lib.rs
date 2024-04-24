use std::{convert::Infallible, io, path::Path};

use axum::extract::Request;
use axum::response::Response;
use hyper::{body::Incoming, service::Service};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server;
use tokio::fs;

use tokio::net::TcpListener;

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
