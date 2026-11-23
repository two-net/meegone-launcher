//! Which features have stopped working (ARCH-09, ADR 0043).
//!
//! A feature stops for good when its code panics, wherever it runs: an intent, a listener, what
//! its views show, or a reader of the host's. The host may also find a bug in a feature's own
//! views and report it with [`ShellIntent::Failed`](crate::ShellIntent::Failed). The rest of the
//! app carries on, and the feature shows a card that says it stopped working.

/// A part of MeeGone that fails on its own. The host names one by its [`index`](Self::index).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Feature {
    Applications,
    Events,
    Recent,
    LockScreenImage,
    Settings,
}

impl Feature {
    pub const ALL: [Self; 5] = [
        Self::Applications,
        Self::Events,
        Self::Recent,
        Self::LockScreenImage,
        Self::Settings,
    ];

    /// Its place in [`Feature::ALL`], which the host mirrors.
    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_index(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }

    fn bit(self) -> u8 {
        1 << self.index()
    }
}

/// The features that have stopped working.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Failures(u8);

impl Failures {
    /// These, and `feature`.
    #[must_use]
    pub fn with(self, feature: Feature) -> Self {
        Self(self.0 | feature.bit())
    }

    pub fn contains(self, feature: Feature) -> bool {
        self.0 & feature.bit() != 0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// One bit for each, at its [`Feature::index`], for the host.
    pub fn bits(self) -> u8 {
        self.0
    }

    /// The features among these that aren't among `earlier`.
    pub fn since(self, earlier: Self) -> impl Iterator<Item = Feature> {
        Feature::ALL
            .into_iter()
            .filter(move |feature| self.contains(*feature) && !earlier.contains(*feature))
    }
}

impl FromIterator<Feature> for Failures {
    fn from_iter<I: IntoIterator<Item = Feature>>(features: I) -> Self {
        features
            .into_iter()
            .fold(Self::default(), |failures, feature| failures.with(feature))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failures_name_every_feature_that_stopped() {
        let failures: Failures = [Feature::Events, Feature::Settings].into_iter().collect();

        assert!(failures.contains(Feature::Events));
        assert!(!failures.contains(Feature::Recent));
        assert_eq!(failures.bits(), 0b1_0010);
        assert_eq!(
            failures
                .with(Feature::Recent)
                .since(failures)
                .collect::<Vec<_>>(),
            [Feature::Recent]
        );
        assert!(Failures::default().is_empty());
        for feature in Feature::ALL {
            assert_eq!(Feature::from_index(feature.index()), Some(feature));
        }
        assert_eq!(Feature::from_index(5), None);
    }
}
