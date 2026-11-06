use std::sync::Arc;

use crate::Driver;

/// All connectors compiled into this build.
#[derive(Default, Clone)]
pub struct DriverRegistry {
    drivers: Vec<Arc<dyn Driver>>,
}

impl DriverRegistry {
    pub fn register(&mut self, driver: impl Driver) {
        self.drivers.push(Arc::new(driver));
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn Driver>> {
        self.drivers.iter().find(|d| d.info().id == id).cloned()
    }

    pub fn all(&self) -> &[Arc<dyn Driver>] {
        &self.drivers
    }
}
