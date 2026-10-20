use crate::ConnectivityError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectivityState {
    /// No network of any kind is up.
    Offline,
    /// Behind a captive portal; full Internet access needs a login.
    Portal,
    /// Connected to a network, but no route to the Internet.
    Limited,
    /// Connected, with full Internet access.
    Full,
    /// NetworkManager cannot tell.
    Unknown,
}

impl ConnectivityState {
    /// True only when NetworkManager confirms full Internet access.
    ///
    /// `Portal` counts as no Internet (a login is still needed) and `Unknown`
    /// counts as no Internet too, so callers that must not act on a guess
    /// should check for `Unknown` themselves.
    pub fn has_internet(self) -> bool {
        self == ConnectivityState::Full
    }
}

impl TryFrom<u32> for ConnectivityState {
    type Error = ConnectivityError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(ConnectivityState::Unknown),
            1 => Ok(ConnectivityState::Offline),
            2 => Ok(ConnectivityState::Portal),
            3 => Ok(ConnectivityState::Limited),
            4 => Ok(ConnectivityState::Full),
            other => Err(ConnectivityError::UnknownValue(other)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_full_has_internet() {
        assert!(ConnectivityState::Full.has_internet());
        assert!(!ConnectivityState::Limited.has_internet());
        assert!(!ConnectivityState::Offline.has_internet());
        assert!(!ConnectivityState::Portal.has_internet());
        assert!(!ConnectivityState::Unknown.has_internet());
    }

    #[test]
    fn converts_known_numbers() {
        assert_eq!(
            ConnectivityState::try_from(4u32),
            Ok(ConnectivityState::Full)
        );
        assert_eq!(
            ConnectivityState::try_from(1u32),
            Ok(ConnectivityState::Offline)
        );
    }

    #[test]
    fn rejects_unknown_number() {
        assert_eq!(
            ConnectivityState::try_from(7u32),
            Err(ConnectivityError::UnknownValue(7))
        );
    }
}
