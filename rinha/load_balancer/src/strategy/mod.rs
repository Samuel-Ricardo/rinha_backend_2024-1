use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use axum::extract::Request;

use crate::model::LoadBalancer;

struct RoundRobin {
    addrs: Vec<String>,
    req_counter: Arc<AtomicUsize>,
}

impl LoadBalancer for RoundRobin {
    fn next_server(&self, req: &Request) -> String {
        let count = self.req_counter.fetch_add(1, Ordering::Relaxed);
        self.addrs[count % self.addrs.len()].clone()
    }
}
