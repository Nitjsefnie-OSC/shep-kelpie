//! Naming a call's child process in `shep describe`

use std::fmt;

/// Names a runner's child process in `shep describe`
pub trait LambLabels: Send + Sync + fmt::Debug {
    /// Labels the child `pid` with `label`
    fn label(&self, pid: u32, label: &str);
}

impl LambLabels for shep_channel::Shepherd {
    // A label kelpie builds is short and plain, so a refused one is dropped
    // rather than failing the call it names.
    fn label(&self, pid: u32, label: &str) {
        if let Ok(label) = shep_channel::LambLabel::new(label) {
            let _ = self.label_lamb(pid, label);
        }
    }
}
