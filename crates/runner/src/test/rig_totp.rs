//! The rig's authenticator secret and the codes it makes

use super::*;

impl Rig {
    /// The rig's authenticator secret: RFC 6238's SHA-1 key, in base32
    pub const TOTP_SECRET: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";

    /// Writes [`Self::TOTP_SECRET`] where `shep kelpie totp` would, for its owner alone
    pub fn write_totp_secret(&self) {
        use std::os::unix::fs::PermissionsExt;
        let folder = self.paths().totp;
        crate::totp::private_dir(&folder).unwrap();
        let secret = folder.join("secret");
        std::fs::write(&secret, format!("{}\n", Self::TOTP_SECRET)).unwrap();
        std::fs::set_permissions(&secret, std::fs::Permissions::from_mode(0o600)).unwrap();
    }

    /// The maintainer's authenticator code at `at`
    pub fn code_at(&self, at: Timestamp) -> String {
        let secret = crate::totp::Secret::load(&self.paths().totp.join("secret"));
        let secret = secret.unwrap().expect("the rig writes a secret");
        format!("{:06}", secret.code(crate::totp::step_of(at)))
    }

    /// Writes `text` ending with the code of the moment to the topic, as the
    /// maintainer's phone would, and returns the code
    pub fn reply(&self, text: &str) -> String {
        let now = self.clock.now();
        let code = self.code_at(now);
        self.alerts.reply(&format!("{text} {code}"), now);
        code
    }
}
