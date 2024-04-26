use std::{
    hash::{DefaultHasher, Hash, Hasher},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
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
