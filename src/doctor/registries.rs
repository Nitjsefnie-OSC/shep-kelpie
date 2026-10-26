//! How the registries of a project's package managers are reached
//!
//! A worker installs packages in its sandbox, which reaches a registry only
//! when `worker.package_registries` adds it or `worker.allowed_domains` lists
//! it. The managers are the ones the checkout keeps a lockfile or manifest for.

use super::Line;
use super::mcp::allowed;
use crate::package_managers::{self, PackageManager};
use crate::settings::Settings;

/// A line naming each package manager the checkout keeps files for and how its registry is
/// reached: nothing where it keeps none
///
/// A manager whose registry neither setting reaches makes the line `MISSING`, with the hosts to
/// add, since its workers could not install anything.
pub(super) fn line(subject: String, settings: &Settings) -> Option<Line> {
    let found = package_managers::found_in(&settings.git.checkout);
    if found.is_empty() {
        return None;
    }
    let domains = &settings.worker.allowed_domains;
    let (mut reached, mut lacking) = (Vec::new(), Vec::new());
    for manager in found {
        if settings.worker.package_registries {
            reached.push(format!("{} by package_registries", manager.name));
        } else if let Some(hosts) = unlisted(manager, domains) {
            lacking.push((manager.name, hosts));
        } else {
            reached.push(format!("{} by allowed_domains", manager.name));
        }
    }
    let reached = reached.join(", ");
    if lacking.is_empty() {
        return Some(Line::ok(subject, reached));
    }
    let named: Vec<String> = lacking
        .iter()
        .map(|(name, hosts)| format!("{name} lacks {}", hosts.join(", ")))
        .collect();
    let mut add: Vec<&str> = lacking
        .iter()
        .flat_map(|(_, hosts)| hosts.iter().copied())
        .collect();
    add.sort_unstable();
    add.dedup();
    let reached = if reached.is_empty() {
        String::new()
    } else {
        format!("{reached}; ")
    };
    Some(Line::missing(
        subject,
        format!("{reached}{}", named.join("; ")),
        format!(
            "add {} to `worker.allowed_domains`, or turn on `worker.package_registries`",
            add.join(", ")
        ),
    ))
}

// The registries of `manager` that `domains` leaves out, or None when they cover every one.
fn unlisted(
    manager: &PackageManager,
    domains: &[crate::settings::NonBlank],
) -> Option<Vec<&'static str>> {
    let lacking: Vec<&'static str> = (manager.hosts.iter().copied())
        .filter(|host| !allowed(host, domains))
        .collect();
    (!lacking.is_empty()).then_some(lacking)
}
