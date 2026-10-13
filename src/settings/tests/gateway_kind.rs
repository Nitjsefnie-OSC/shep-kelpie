//! `kind` on `[kelpie.gateways.<name>]`, which says how kelpie talks to the gateway

use crate::settings::{Gateway, GatewayKind};

const BASE: &str = "url = \"http://gpu:8700\"\nkey_env = \"PADDOCK_KEY\"\n";

#[test]
fn an_absent_gateway_kind_means_paddock() {
    let gateway: Gateway = toml::from_str(BASE).unwrap();
    assert_eq!(gateway.kind, GatewayKind::Paddock);
    let spelled: Gateway = toml::from_str(&format!("kind = \"paddock\"\n{BASE}")).unwrap();
    assert_eq!(spelled, gateway);
}

#[test]
fn an_unknown_gateway_kind_is_refused_naming_it() {
    let err = toml::from_str::<Gateway>(&format!("kind = \"litellm\"\n{BASE}"))
        .unwrap_err()
        .to_string();
    assert!(err.contains("litellm"), "{err}");
}

#[test]
fn an_openai_gateway_kind_is_read_by_its_name() {
    let gateway: Gateway = toml::from_str(&format!("kind = \"openai\"\n{BASE}")).unwrap();
    assert_eq!(gateway.kind, GatewayKind::Openai);
}
