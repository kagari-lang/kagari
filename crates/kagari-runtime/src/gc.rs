#[derive(Debug, Clone, Copy)]
pub struct GcHeapConfig {
    pub nursery_bytes: usize,
    pub large_object_threshold: usize,
}

impl Default for GcHeapConfig {
    fn default() -> Self {
        Self {
            nursery_bytes: 1024 * 1024,
            large_object_threshold: 8 * 1024,
        }
    }
}

#[derive(Debug)]
pub struct GcHeap {
    config: GcHeapConfig,
    allocated_objects: usize,
}

impl GcHeap {
    pub fn new(config: GcHeapConfig) -> Self {
        Self {
            config,
            allocated_objects: 0,
        }
    }

    pub fn config(&self) -> GcHeapConfig {
        self.config
    }

    pub fn allocated_objects(&self) -> usize {
        self.allocated_objects
    }
}
