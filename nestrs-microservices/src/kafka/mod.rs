#[cfg(feature = "kafka")]
mod connection;
#[cfg(feature = "kafka")]
mod live;
#[cfg(feature = "kafka")]
pub use connection::{KafkaConnectionOptions, KafkaSaslOptions, KafkaTlsOptions};
#[cfg(feature = "kafka")]
pub use live::{
    kafka_cluster_reachable, kafka_cluster_reachable_with, KafkaMicroserviceOptions, KafkaMicroserviceServer,
    KafkaTransport, KafkaTransportOptions,
};

#[cfg(not(feature = "kafka"))]
mod stub;
#[cfg(not(feature = "kafka"))]
pub use stub::KafkaTransport;
