use std::sync::{atomic::AtomicUsize, Arc};

struct RoundRobin {
    addrs: Vec<String>,
    req_counter: Arc<AtomicUsize>,
}
