//! DTOs for the NestrsCliNewResource1789613851945275000 resource.

use nestrs::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NestrsCliNewResource1789613851945275000 {
    pub id: String,
    pub name: String,
}

#[dto]
pub struct CreateNestrsCliNewResource1789613851945275000Dto {
    #[IsString]
    #[Length(min = 1, max = 255)]
    pub name: String,
}

#[nestrs::partial_type]
pub struct UpdateNestrsCliNewResource1789613851945275000Dto {
    #[IsString]
    #[Length(min = 1, max = 255)]
    pub name: String,
}
