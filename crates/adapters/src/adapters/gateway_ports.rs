//! The port to a gateway, by its kind

use super::openai_gateway::OpenAi;
use super::paddock::Paddock;
use crate::ports::ModelGateway;
use crate::settings::{Gateway, GatewayKind, GatewayName, Gateways};

/// The port to `gateway`, for its kind, with a paddock's leases on `gateways`' schedule
pub fn port(gateways: &Gateways, gateway: &Gateway) -> Box<dyn ModelGateway> {
    match gateway.kind {
        GatewayKind::Paddock => Box::new(Paddock {
            timing: gateways.paddock_timing(),
        }),
        GatewayKind::Openai => Box::new(OpenAi),
    }
}

/// The port to the gateway `name`
///
/// # Errors
///
/// The reason when kelpie's settings lack a gateway of that name.
pub fn port_named(
    gateways: &Gateways,
    name: &GatewayName,
) -> Result<Box<dyn ModelGateway>, String> {
    Ok(port(gateways, gateways.named(name)?))
}
