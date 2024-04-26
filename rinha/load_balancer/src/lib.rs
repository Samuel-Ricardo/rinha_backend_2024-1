use std::{
    env,
    sync::{atomic::AtomicUsize, Arc},
    time::Duration,
};

use axum::{body::Body, handler::Handler};
use hyper_util::{
    client::legacy::{connect::HttpConnector, Client},
    rt::TokioExecutor,
};
use tokio::net::TcpListener;

use crate::model::AppState;

pub mod model;
pub mod proxy;
pub mod strategy;

#[tokio::main]
async fn main() {
    let port = env::var("PORT")
        .ok()
        .and_then(|port| port.parse::<u16>().ok())
        .unwrap_or(9999);

    let addrs = env::var("UPSTREAM")
        .ok()
        .map(|upstream| {
            upstream
                .split(",")
                .map(|addr| addr.to_string())
                .collect::<Vec<String>>()
        })
        .unwrap_or(vec![
            String::from("127.0.0.1:8080"),
            String::from("127.0.0.1:8081"),
        ]);

    let listener = TcpListener::bind(("0.0.0.0", port)).await.unwrap();

    let client = {
        let mut connector = HttpConnector::new();

        connector.set_keepalive(Some(Duration::from_secs(60)));
        connector.set_nodelay(true);

        Client::builder(TokioExecutor::new())
            .http2_only(true)
            .build::<_, Body>(connector)
    };

    let round_robin = strategy::RoundRobin {
        addrs: addrs.clone(),
        req_counter: Arc::new(AtomicUsize::new(0)),
    };

    let fixed_load_balancer = strategy::RinhaAccountBalancer {
        addrs: addrs.clone(),
    };

    let app_state = AppState {
        load_balancer: Arc::new(round_robin),
        http_client: client,
    };

    let app = proxy::main.with_state(app_state);

    println!(
        "Listening on 0.0.0.0:{} | Load Balancer: {}",
        port,
        env!("CARGO_PKG_VERSION")
    );

    axum::serve(listener, app).await.unwrap();
}
