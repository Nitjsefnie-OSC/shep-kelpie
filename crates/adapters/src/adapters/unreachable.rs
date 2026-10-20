//! Whether an agent's error says its model host could not be reached, for the
//! adapters whose errors are the SDK's or the provider's own text

// Whether `detail` says the model host could not be reached: a status in
// `statuses` as the text leads with it, or a refused or reset connection, or
// any of `signs`, lower case. Any other error is the call's own, and so is one
// that leads with another status, whatever its text goes on to mention.
pub(super) fn reaches_no_host(detail: &str, statuses: &[&str], signs: &[&str]) -> bool {
    let lower = detail.trim_start().to_ascii_lowercase();
    let digits = lower.bytes().take_while(u8::is_ascii_digit).count();
    let leads_with_status =
        digits == 3 && !lower[digits..].starts_with(|c: char| c.is_ascii_alphanumeric());
    if leads_with_status {
        return statuses.contains(&&lower[..digits]);
    }
    [
        "econnrefused",
        "econnreset",
        "connection refused",
        "connection reset",
    ]
    .iter()
    .chain(signs)
    .any(|sign| lower.contains(sign))
}

#[cfg(test)]
mod tests;
