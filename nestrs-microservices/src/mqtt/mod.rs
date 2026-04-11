#[cfg(feature = "mqtt")]
mod live;
#[cfg(feature = "mqtt")]
pub use live::{
    MqttMicroserviceOptions, MqttMicroserviceServer, MqttSocketOptions, MqttTlsMode, MqttTransport,
    MqttTransportOptions,
};

#[cfg(not(feature = "mqtt"))]
mod stub;
#[cfg(not(feature = "mqtt"))]
pub use stub::MqttTransport;
