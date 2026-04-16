use semver::{BuildMetadata, Comparator, Prerelease, Version};
use std::{fmt::Display, str::FromStr};

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub enum PartialSemverVersion {
    Major(Comparator),
    MajorMinor(Comparator),
    Full(Version),
}

impl PartialSemverVersion {
    pub fn with_major(major: u64) -> Self {
        Self::Major(Comparator {
            op: semver::Op::Exact,
            major,
            minor: None,
            patch: None,
            pre: Prerelease::EMPTY,
        })
    }

    pub fn major(&self) -> Option<&Comparator> {
        match self {
            Self::Major(comparator) => Some(comparator),
            _ => None,
        }
    }

    pub fn major_unchecked(&self) -> &Comparator {
        self.major().unwrap()
    }

    pub fn to_major(&self) -> Self {
        match self {
            Self::Major(comparator) => Self::Major(comparator.clone()),
            Self::MajorMinor(comparator) => Self::Major(Comparator {
                op: semver::Op::Exact,
                major: comparator.major,
                minor: None,
                patch: None,
                pre: comparator.pre.clone(),
            }),
            Self::Full(version) => Self::Major(Comparator {
                op: semver::Op::Exact,
                major: version.major,
                minor: None,
                patch: None,
                pre: version.pre.clone(),
            }),
        }
    }

    pub fn with_major_minor(major: u64, minor: u64) -> Self {
        Self::MajorMinor(Comparator {
            op: semver::Op::Exact,
            major,
            minor: Some(minor),
            patch: None,
            pre: Prerelease::EMPTY,
        })
    }

    pub fn major_minor(&self) -> Option<&Comparator> {
        match self {
            Self::MajorMinor(comparator) => Some(comparator),
            _ => None,
        }
    }

    pub fn major_minor_unchecked(&self) -> &Comparator {
        self.major_minor().unwrap()
    }

    pub fn to_major_minor(&self) -> Result<Self, String> {
        match self {
            Self::Major(_) => Err(String::from("Cannot turn major into major.minor")),
            Self::MajorMinor(comparator) => Ok(Self::MajorMinor(comparator.clone())),
            Self::Full(version) => Ok(Self::MajorMinor(Comparator {
                op: semver::Op::Exact,
                major: version.major,
                minor: Some(version.minor),
                patch: None,
                pre: version.pre.clone(),
            })),
        }
    }

    pub fn full(&self) -> Option<&Version> {
        match self {
            Self::Full(version) => Some(version),
            _ => None,
        }
    }

    pub fn full_unchecked(&self) -> &Version {
        self.full().unwrap()
    }

    fn to_version(&self) -> Version {
        match self {
            Self::Major(comparator) => Version {
                major: comparator.major,
                minor: 0,
                patch: 0,
                pre: comparator.pre.clone(),
                build: BuildMetadata::EMPTY,
            },
            Self::MajorMinor(comparator) => Version {
                major: comparator.major,
                minor: comparator.minor.expect("Must be set"),
                patch: 0,
                pre: comparator.pre.clone(),
                build: BuildMetadata::EMPTY,
            },
            Self::Full(version) => version.clone(),
        }
    }
}

impl PartialEq<Version> for PartialSemverVersion {
    fn eq(&self, other: &Version) -> bool {
        self.full()
            .map(|version| version.eq(other))
            .unwrap_or(false)
    }
}

impl PartialOrd for PartialSemverVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PartialSemverVersion {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self, other) {
            (Self::Major(_), Self::MajorMinor(_) | Self::Full(_)) => std::cmp::Ordering::Greater,
            (Self::MajorMinor(_), Self::Major(_)) => std::cmp::Ordering::Less,
            (Self::MajorMinor(_), Self::Full(_)) => std::cmp::Ordering::Greater,
            (Self::Full(_), Self::Major(_) | Self::MajorMinor(_)) => std::cmp::Ordering::Less,
            (s, o) => {
                let s = s.to_version();
                let o = o.to_version();

                s.cmp(&o)
            }
        }
    }
}

impl FromStr for PartialSemverVersion {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match Version::from_str(s).map_err(|e| e.to_string()) {
            Ok(version) => Ok(Self::Full(version)),
            Err(err) => match Comparator::from_str(&format!("={s}")) {
                Ok(comparator) => {
                    assert!(comparator.patch.is_none(), "Patch must be none");
                    if comparator.minor.is_some() {
                        Ok(Self::MajorMinor(comparator))
                    } else {
                        Ok(Self::Major(comparator))
                    }
                }
                Err(err2) => Err(format!("Cannot parse {s} as full semver version ({err}) nor as partial semver version ({err2})")),
            },
        }
    }
}

impl Display for PartialSemverVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PartialSemverVersion::Major(comparator) => write!(f, "{}", comparator.major),
            PartialSemverVersion::MajorMinor(comparator) => write!(
                f,
                "{}.{}",
                comparator.major,
                comparator.minor.expect("Must be set in this case")
            ),
            PartialSemverVersion::Full(version) => write!(f, "{version}"),
        }
    }
}

impl From<Version> for PartialSemverVersion {
    fn from(version: Version) -> Self {
        Self::Full(version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use semver::Prerelease;

    #[test]
    fn failed() {
        let psv = PartialSemverVersion::from_str("a").unwrap_err();

        assert_eq!(psv, String::from("Cannot parse a as full semver version (unexpected character 'a' while parsing major version number) nor as partial semver version (unexpected character 'a' while parsing major version number)"));
    }

    #[test]
    fn parse_full_semver() {
        let psv = PartialSemverVersion::from_str("1.2.3").unwrap();

        assert_eq!(psv, PartialSemverVersion::Full(Version::new(1, 2, 3)))
    }

    #[test]
    fn parse_major_minor_semver() {
        let psv = PartialSemverVersion::from_str("1.2").unwrap();

        assert_eq!(
            psv,
            PartialSemverVersion::MajorMinor(Comparator {
                op: semver::Op::Exact,
                major: 1,
                minor: Some(2),
                patch: None,
                pre: Prerelease::EMPTY
            })
        )
    }

    #[test]
    fn parse_major_semver() {
        let psv = PartialSemverVersion::from_str("1").unwrap();

        assert_eq!(
            psv,
            PartialSemverVersion::Major(Comparator {
                op: semver::Op::Exact,
                major: 1,
                minor: None,
                patch: None,
                pre: Prerelease::EMPTY
            })
        )
    }

    #[test]
    fn display() {
        let psv = PartialSemverVersion::from_str("1").unwrap();
        assert_eq!(psv.to_string().as_str(), "1");
        let psv = PartialSemverVersion::from_str("1.0").unwrap();
        assert_eq!(psv.to_string().as_str(), "1.0");
        let psv = PartialSemverVersion::from_str("1.0.0").unwrap();
        assert_eq!(psv.to_string().as_str(), "1.0.0");
    }
}
