//! Plugin API versioning (DESIGN §11.4).
//!
//! The host exposes a `major.minor` API version. A plugin declares the version
//! it targets; it is compatible when the major matches and the host's minor is
//! at least the plugin's (additive, backward-compatible minor bumps).

/// A `major.minor` plugin-API version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiVersion {
    pub major: u32,
    pub minor: u32,
}

impl ApiVersion {
    /// The plugin-API version this build of the host implements.
    pub const CURRENT: ApiVersion = ApiVersion { major: 0, minor: 1 };

    pub fn new(major: u32, minor: u32) -> Self {
        ApiVersion { major, minor }
    }

    pub fn parse(s: &str) -> Option<ApiVersion> {
        let s = s.trim().trim_start_matches('v');
        let mut parts = s.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next().unwrap_or("0").parse().ok()?;
        Some(ApiVersion { major, minor })
    }

    /// Can the host (`self`) run a plugin targeting `plugin`?
    pub fn supports(&self, plugin: ApiVersion) -> bool {
        self.major == plugin.major && self.minor >= plugin.minor
    }
}

impl std::fmt::Display for ApiVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compatibility_rules() {
        let host = ApiVersion::new(0, 2);
        assert!(host.supports(ApiVersion::new(0, 1))); // older minor: ok
        assert!(host.supports(ApiVersion::new(0, 2))); // same: ok
        assert!(!host.supports(ApiVersion::new(0, 3))); // newer minor: no
        assert!(!host.supports(ApiVersion::new(1, 0))); // different major: no
    }

    #[test]
    fn parsing() {
        assert_eq!(ApiVersion::parse("1.4"), Some(ApiVersion::new(1, 4)));
        assert_eq!(ApiVersion::parse("v2"), Some(ApiVersion::new(2, 0)));
        assert_eq!(ApiVersion::parse("nonsense"), None);
    }
}
