#[cfg(feature = "rabbitmq")]
mod live;
#[cfg(feature = "rabbitmq")]
pub use live::{
    RabbitMqMicroserviceOptions, RabbitMqMicroserviceServer, RabbitMqTransport,
    RabbitMqTransportOptions,
};

#[cfg(not(feature = "rabbitmq"))]
mod stub;
#[cfg(not(feature = "rabbitmq"))]
pub use stub::RabbitMqTransport;
