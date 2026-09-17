//! NestrsCliNewResource1789613851945275000 business logic.

use nestrs::prelude::*;
use super::dto::{NestrsCliNewResource1789613851945275000, CreateNestrsCliNewResource1789613851945275000Dto};

#[derive(Default)]
#[injectable]
pub struct NestrsCliNewResource1789613851945275000Service;

impl NestrsCliNewResource1789613851945275000Service {
    pub async fn list(&self) -> Vec<NestrsCliNewResource1789613851945275000> {
        Vec::new()
    }

    pub async fn create(&self, _input: CreateNestrsCliNewResource1789613851945275000Dto) -> NestrsCliNewResource1789613851945275000 {
        NestrsCliNewResource1789613851945275000 {
            id: uuid::Uuid::new_v4().to_string(),
            name: _input.name,
        }
    }
}
