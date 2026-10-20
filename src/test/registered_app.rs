//! The App registered in a kelpie home

use std::path::Path;

use crate::github::{Apps, Owner};
use crate::ports::{AppForges, Reading};
use crate::settings::ForgeSlug;

/// The App registered in a kelpie home, which reads nothing: the login reads every repo, and the
/// App's login is the registered one's
#[derive(Debug, Clone)]
pub(crate) struct RegisteredApp(Apps);

impl RegisteredApp {
    /// The Apps kept in `kelpie_home`
    pub(crate) fn under(kelpie_home: &Path) -> Self {
        Self(Apps::under(kelpie_home))
    }
}

impl AppForges for RegisteredApp {
    fn reading(&self, _: &ForgeSlug) -> Reading {
        Reading::Login
    }

    fn login(&self, repo: &ForgeSlug) -> Option<String> {
        let owner = Owner::of(repo).ok()?;
        Some(self.0.get(&owner).ok()??.slug)
    }
}
