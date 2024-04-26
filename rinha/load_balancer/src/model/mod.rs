use std::sync::Arc;

use axum::{body::Body, extract::Request};
use hyper_util::client::legacy::{connect::HttpConnector, Client};

pub trait LoadBalancer {
    fn next_server(&self, req: &Request) -> String;
}

#[derive(Clone)]
pub struct AppState {
    pub load_balancer: Arc<dyn LoadBalancer + Send + Sync>,
    pub http_client: Client<HttpConnector, Body>,
}
