use std::sync::{atomic::AtomicUsize, Arc};

use axum::{body::Body, extract::Request};
use hyper_util::client::legacy::{connect::HttpConnector, Client};

trait LoadBalancer {
    fn next_server(&self, req: &Request) -> String;
}

#[derive(Clone)]
struct AppState {
    load_balancer: Arc<dyn LoadBalancer + Send + Sync>,
    http_client: Client<HttpConnector, Body>,
}

struct RoundRobin {
    addrs: Vec<String>,
    req_counter: Arc<AtomicUsize>,
}
