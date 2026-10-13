use std::path::PathBuf;

use super::*;
use crate::ports::{Role, Session, SessionId, Tools};
use crate::settings::{AgentHarness, ContextSize, Effort, EndpointUrl, ModelHost, ModelServer};

fn call() -> AgentCall {
    let goose = AgentHarness::Goose(ModelServer {
        host: ModelHost::Url(EndpointUrl::try_from("http://box:11434/v1".to_owned()).unwrap()),
        context: ContextSize::try_from(65_536).unwrap(),
    });
    AgentCall::new(
        Role::Researcher,
        Tools::Research,
        goose,
        "qwen3.8:27b",
        Effort::Low,
        Session::New(SessionId("s".into())),
        PathBuf::from("/k/wt/7"),
        "go",
    )
}

#[test]
fn a_stand_in_goose_refuses_every_call_naming_the_adapter_slice() {
    let goose = GooseCli;
    let call = call();
    let asked = goose.prepare(&call).unwrap_err();
    let ran = goose.run(&call, &Ending::default()).unwrap_err();
    for err in [asked, ran] {
        assert!(matches!(err, AgentError::Setup(_)), "{err:?}");
        let said = err.to_string();
        assert!(
            said.contains("goose is not built yet") && said.contains("#787"),
            "{said}"
        );
    }
}
