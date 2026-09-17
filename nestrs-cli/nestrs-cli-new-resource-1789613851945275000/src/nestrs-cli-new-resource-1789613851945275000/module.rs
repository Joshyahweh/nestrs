//! NestrsCliNewResource1789613851945275000 feature module — wire controller + service into the host app.

use nestrs::prelude::*;
use super::controller::NestrsCliNewResource1789613851945275000Controller;
use super::service::NestrsCliNewResource1789613851945275000Service;

#[module(
    controllers = [NestrsCliNewResource1789613851945275000Controller],
    providers = [NestrsCliNewResource1789613851945275000Service],
    exports = [NestrsCliNewResource1789613851945275000Service],
)]
pub struct NestrsCliNewResource1789613851945275000Module;
