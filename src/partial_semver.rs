use semver::{Comparator, Version};
use std::str::FromStr;

#[derive(Debug, PartialEq)]
pub enum PartialSemverVersion {
    Major(Comparator),
    MajorMinor(Comparator),
    Full(Version),
}

impl PartialSemverVersion {
    pub fn full(&self) -> Option<&Version> {
        match self {
            Self::Full(version) => Some(version),
            _ => None,
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
}
