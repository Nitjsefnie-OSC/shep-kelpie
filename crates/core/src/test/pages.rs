//! The rig's pages: opens no browser, answers each URL as a test scripted it
//! (a clean page when it scripted none), and keeps every ask

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use crate::ports::{ConsoleError, PageRequest, PageSession, Pages, Visit};

/// One ask for pages
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PagesAsked {
    /// The browser it was to open
    pub session: PageSession,
    /// The URLs, in order
    pub urls: Vec<String>,
    /// The dev server's port as the gate held it when the pages were asked for
    pub gate: Option<u16>,
}

#[derive(Debug, Default)]
struct State {
    by_path: BTreeMap<String, Visit>,
    unopenable: Option<String>,
    asked: Vec<PagesAsked>,
}

/// Stand-in pages that record what is published
#[derive(Debug, Clone, Default)]
pub struct FakePages {
    state: Arc<Mutex<State>>,
}

impl FakePages {
    /// Has the page at `path` (its URL's path and query) come to `visit`
    pub fn script(&self, path: &str, visit: Visit) {
        self.state
            .lock()
            .unwrap()
            .by_path
            .insert(path.to_owned(), visit);
    }

    /// Has the page at `path` load, with a request to `origin` answering `status`
    pub fn answers(&self, path: &str, status: u16) {
        let request = PageRequest {
            method: "GET".to_owned(),
            url: "dev-origin/api/items".to_owned(),
            status: Some(status),
        };
        self.script(
            path,
            Visit {
                requests: vec![request],
                ..clean()
            },
        );
    }

    /// Has the page at `path` log `count` console errors
    pub fn logs_errors(&self, path: &str, count: u32) {
        self.script(
            path,
            Visit {
                console_errors: (0..count)
                    .map(|_| ConsoleError {
                        text: "TypeError: x is undefined".to_owned(),
                        url: Some("dev-origin/app.js".to_owned()),
                    })
                    .collect(),
                ..clean()
            },
        );
    }

    /// Has the page at `path` log the error Chromium logs for a resource at `url` that failed
    /// to load, which is all it logs
    pub fn fails_to_load(&self, path: &str, url: &str) {
        self.script(
            path,
            Visit {
                console_errors: vec![ConsoleError {
                    text: "Failed to load resource: the server responded with a status of 502 \
                           (Bad Gateway)"
                        .to_owned(),
                    url: Some(url.to_owned()),
                }],
                ..clean()
            },
        );
    }

    /// Has every later ask fail for `why`, as a browser that cannot be driven
    pub fn cannot_open(&self, why: &str) {
        self.state.lock().unwrap().unopenable = Some(why.to_owned());
    }

    /// Has later asks open again
    pub fn can_open(&self) {
        self.state.lock().unwrap().unopenable = None;
    }

    /// Every ask, oldest first
    pub fn asked(&self) -> Vec<PagesAsked> {
        self.state.lock().unwrap().asked.clone()
    }
}

/// A page that loaded, made no failing request and logged no error
pub fn clean() -> Visit {
    Visit {
        loaded: Ok(()),
        requests: Vec::new(),
        console_errors: Vec::new(),
    }
}

// What follows the origin in `url`: its path and query.
fn path_of(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    rest.find('/').map_or("/", |at| &rest[at..])
}

impl Pages for FakePages {
    fn visit(&self, session: &PageSession, urls: &[String]) -> Result<Vec<Visit>, String> {
        let mut state = self.state.lock().unwrap();
        state.asked.push(PagesAsked {
            session: session.clone(),
            urls: urls.to_vec(),
            gate: session.gate.get(),
        });
        if let Some(why) = &state.unopenable {
            return Err(why.clone());
        }
        Ok(urls
            .iter()
            .map(|url| {
                let mut visit = state
                    .by_path
                    .get(path_of(url))
                    .cloned()
                    .unwrap_or_else(clean);
                // A scripted request names the dev server's origin as "dev-origin".
                let origin = url.strip_suffix(path_of(url)).unwrap_or(url);
                for request in &mut visit.requests {
                    request.url = request.url.replace("dev-origin", origin);
                }
                for error in &mut visit.console_errors {
                    error.url = (error.url.take()).map(|url| url.replace("dev-origin", origin));
                }
                visit
            })
            .collect())
    }
}
