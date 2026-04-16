use crate::PartialSemverVersion;
use anyhow::{Context, Result};
use oci_distribution::{manifest::OciManifest, secrets::RegistryAuth, Client, Reference};
use semver::Version;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fmt::Display,
    str::FromStr,
};
use tokio::task::JoinSet;

pub async fn validate(
    client: &Client,
    registry_auth: &RegistryAuth,
    image: &Reference,
    tag_prefix: &Option<String>,
    existing_tags: &[PartialSemverVersion],
) -> Result<()> {
    println!(
        "Validating for {image} if the tags have correct partial semver tagging: {}",
        existing_tags
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    );

    let manifests =
        fetch_manifests(client, registry_auth, image, tag_prefix, existing_tags).await?;

    detect_miss_placed_tags(existing_tags, manifests).map_err(|errors| {
        anyhow::anyhow!(errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("\n"))
    })?;
    Ok(())
}

#[derive(thiserror::Error, Debug, PartialEq, Eq)]
enum ValidationError {
    MissingMajor {
        latest_version: Version,
    },
    MissingMajorMinor {
        latest_version: Version,
    },
    MissPlaced {
        major_or_major_minor: PartialSemverVersion,
        should_point_to: Version,
        pointing_to_instead: Version,
    },
}

impl Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingMajor { latest_version } => write!(f,"There is no partial major tag '{}' for '{latest_version}'", PartialSemverVersion::from(latest_version.clone()).to_major()),
            Self::MissingMajorMinor { latest_version } => write!(f,"There is no partial major.minor tag '{}' for {latest_version}", PartialSemverVersion::from(latest_version.clone()).to_major_minor().unwrap()),
            Self::MissPlaced {
                major_or_major_minor,
                should_point_to,
                pointing_to_instead,
            } => write!(f,"The {major_or_major_minor} tag points to {pointing_to_instead} instead to {should_point_to}"),
        }
    }
}

fn detect_miss_placed_tags(
    existing_tags: &[PartialSemverVersion],
    manifests: BTreeMap<PartialSemverVersion, OciManifest>,
) -> std::result::Result<(), Vec<ValidationError>> {
    assert!(
        existing_tags.iter().collect::<BTreeSet<_>>() == manifests.keys().collect::<BTreeSet<_>>(),
        "Tags and manifests must be equal"
    );

    let mut errors = Vec::new();

    let mut manifests_grouped_by_major =
        HashMap::<PartialSemverVersion, BTreeMap<Version, &OciManifest>>::new();
    let mut manifests_grouped_by_major_minor =
        HashMap::<PartialSemverVersion, BTreeMap<Version, &OciManifest>>::new();
    let mut full_tags_without_major = BTreeMap::<PartialSemverVersion, &Version>::new();
    let mut full_tags_without_major_minor = BTreeMap::<PartialSemverVersion, &Version>::new();

    for full_tag in existing_tags.iter().filter(|psv| psv.full().is_some()) {
        let major_minor = full_tag
            .to_major_minor()
            .expect("full must be convertible to major.minor");

        match manifests.get(&major_minor) {
            Some(manifest) => {
                manifests_grouped_by_major_minor
                    .entry(major_minor)
                    .and_modify(|e| {
                        e.insert(full_tag.full_unchecked().clone(), manifest);
                    })
                    .or_insert_with(|| {
                        BTreeMap::from([(full_tag.full_unchecked().clone(), manifest)])
                    });
            }
            None => {
                let version = full_tag.full_unchecked();
                full_tags_without_major_minor
                    .entry(major_minor)
                    .and_modify(|e| {
                        if *e < version {
                            *e = version;
                        }
                    })
                    .or_insert(version);
            }
        }

        let major = full_tag.to_major();
        match manifests.get(&major) {
            Some(manifest) => {
                manifests_grouped_by_major
                    .entry(major)
                    .and_modify(|e| {
                        e.insert(full_tag.full_unchecked().clone(), manifest);
                    })
                    .or_insert_with(|| {
                        BTreeMap::from([(full_tag.full_unchecked().clone(), manifest)])
                    });
            }
            None => {
                let version = full_tag.full_unchecked();
                full_tags_without_major
                    .entry(major)
                    .and_modify(|e| {
                        if *e < version {
                            *e = version;
                        }
                    })
                    .or_insert(version);
            }
        }
    }

    errors.extend(full_tags_without_major.into_values().map(|version| {
        ValidationError::MissingMajor {
            latest_version: version.clone(),
        }
    }));
    errors.extend(full_tags_without_major_minor.into_values().map(|version| {
        ValidationError::MissingMajorMinor {
            latest_version: version.clone(),
        }
    }));

    fn check_misplaced(
        partial_tag: PartialSemverVersion,
        versions_and_manifests: BTreeMap<Version, &OciManifest>,
        manifests: &BTreeMap<PartialSemverVersion, OciManifest>,
    ) -> Option<ValidationError> {
        let (version, manifest) = versions_and_manifests
            .iter()
            .last()
            .expect("There must be at least one entry");

        match manifests.get(&PartialSemverVersion::from(version.clone())) {
            Some(full_version_manifest) => {
                let manifest = serde_json::to_value(manifest).unwrap();
                let full_version_manifest = serde_json::to_value(full_version_manifest).unwrap();

                if manifest != full_version_manifest {
                    let pointing_to_instead = versions_and_manifests
                        .iter()
                        .rev()
                        // we compared the last entry already
                        .skip(1)
                        .find_map(|(version, m)| {
                            let m = serde_json::to_value(m).unwrap();
                            if m == manifest {
                                Some(version)
                            } else {
                                None
                            }
                        })
                        .cloned()
                        .unwrap();

                    Some(ValidationError::MissPlaced {
                        major_or_major_minor: partial_tag,
                        should_point_to: version.clone(),
                        pointing_to_instead,
                    })
                } else {
                    None
                }
            }
            None => todo!(),
        }
    }

    errors.extend(manifests_grouped_by_major.into_iter().filter_map(
        |(major, versions_and_manifests)| {
            check_misplaced(major, versions_and_manifests, &manifests)
        },
    ));

    errors.extend(manifests_grouped_by_major_minor.into_iter().filter_map(
        |(major_minor, versions_and_manifests)| {
            check_misplaced(major_minor, versions_and_manifests, &manifests)
        },
    ));

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

async fn fetch_manifests(
    client: &Client,
    registry_auth: &RegistryAuth,
    image: &Reference,
    tag_prefix: &Option<String>,
    existing_tags: &[PartialSemverVersion],
) -> Result<BTreeMap<PartialSemverVersion, OciManifest>> {
    let mut set = JoinSet::new();

    for tag in existing_tags.iter().cloned() {
        let tagged_image = Reference::from_str(&format!(
            "{}/{}:{}{tag}",
            image.registry(),
            image.repository(),
            tag_prefix.as_ref().map(|t| t.as_str()).unwrap_or("")
        ))
        .expect("Must be valid image string");

        let auth = registry_auth.clone();
        let client = client.clone();
        set.spawn(async move { (tag, client.pull_manifest(&tagged_image, &auth).await) });
    }

    let mut manifests = BTreeMap::new();
    while let Some(res) = set.join_next().await {
        match res {
            Ok((tag, Ok((manifest, _digest)))) => {
                manifests.insert(tag, manifest);
            }
            Ok((tag, Err(err))) => {
                eprintln!("Cannot fetch manifest of {image}:{tag}: {err}");
                return Err(err).with_context(|| format!("{image}"));
            }
            Err(_) => todo!(),
        }
    }

    Ok(manifests)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::collections::BTreeMap;

    fn nextcloud_32_0_0_manifest() -> OciManifest {
        serde_json::from_value(serde_json::json!({
           "schemaVersion": 2,
           "mediaType": "application/vnd.oci.image.index.v1+json",
           "manifests": [
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 5336,
                 "digest": "sha256:ad214130d3ab539033e757ef16485b6e3478bc56fbc127c27f9ae089b11fa648",
                 "platform": {
                    "architecture": "amd64",
                    "os": "linux"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 567,
                 "digest": "sha256:705d08959c87babcaaa22f934f3f681ef246726c597a4b65c8d666998f3af12b",
                 "platform": {
                    "architecture": "unknown",
                    "os": "unknown"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 5337,
                 "digest": "sha256:61b6490a46b5fdc5133e43d39abfc69505e84eaceef8452aa628e99f0a5afc76",
                 "platform": {
                    "architecture": "arm",
                    "os": "linux",
                    "variant": "v5"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 567,
                 "digest": "sha256:ccff9e7f1dcde97cb0eb656562978c7f57099eb543f4d8f4940fe67cd9083d44",
                 "platform": {
                    "architecture": "unknown",
                    "os": "unknown"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 5337,
                 "digest": "sha256:6a3387ea92f651133babffaf4039e298de32a24599d85cd4005f332f55b1bbe7",
                 "platform": {
                    "architecture": "arm",
                    "os": "linux",
                    "variant": "v7"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 567,
                 "digest": "sha256:72a12507a9474f82502d7f60944adfc5838cea7f5f29174a905d96fee04e264a",
                 "platform": {
                    "architecture": "unknown",
                    "os": "unknown"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 5338,
                 "digest": "sha256:cd94cd8deb7621fd4d934848bf88211ed25233352dfaafeed5f58df9b1a7c329",
                 "platform": {
                    "architecture": "arm64",
                    "os": "linux",
                    "variant": "v8"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 567,
                 "digest": "sha256:e8e7a727202936ad20142a0186a82ccbe3a61d6fbe8326ef7b88c217a0041b69",
                 "platform": {
                    "architecture": "unknown",
                    "os": "unknown"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 5335,
                 "digest": "sha256:944baa02504e4bc62dab37567fc8d2620d98d61a595c4ec5a6440ba0fa8e4dae",
                 "platform": {
                    "architecture": "386",
                    "os": "linux"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 567,
                 "digest": "sha256:2079bdc1437ea51330763f3c04a97a766e4faf6a2d228c9505407a420c6ff19a",
                 "platform": {
                    "architecture": "unknown",
                    "os": "unknown"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 5338,
                 "digest": "sha256:31965a20e25da29a3f37d70f7bb29555e1aa505a60ad7d27e54e5a8df545f5fb",
                 "platform": {
                    "architecture": "ppc64le",
                    "os": "linux"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 567,
                 "digest": "sha256:80227f8be29ec253075989cde7d908f9055d09054cebeefa9fe1aef7555ea785",
                 "platform": {
                    "architecture": "unknown",
                    "os": "unknown"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 5338,
                 "digest": "sha256:a2cbc703357d6672d4bda00d5fa09c1d9ec8bb062e634259e9fc852f3f7cbcb0",
                 "platform": {
                    "architecture": "riscv64",
                    "os": "linux"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 567,
                 "digest": "sha256:7b3b0888e4e76f2909e7749049a2b670151808a6bc8184a1f015f777f65dfc10",
                 "platform": {
                    "architecture": "unknown",
                    "os": "unknown"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 5335,
                 "digest": "sha256:8f5bb6b196128783ec2cf6c957c636f5be778c791cfb291090cfc0853d74c073",
                 "platform": {
                    "architecture": "s390x",
                    "os": "linux"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 567,
                 "digest": "sha256:4e12b0d80c90d74a0b418b757a5b95bba06b86a090d9e96ca05bd2008830696c",
                 "platform": {
                    "architecture": "unknown",
                    "os": "unknown"
                 }
              }
           ]
        })).unwrap()
    }

    fn nextcloud_32_0_1_manifest() -> OciManifest {
        serde_json::from_value(serde_json::json!({
           "schemaVersion": 2,
           "mediaType": "application/vnd.oci.image.index.v1+json",
           "manifests": [
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 5336,
                 "digest": "sha256:d2c9c96ce7a38c61674a2e0389e75c47e9820250b00f77137238014417a31201",
                 "platform": {
                    "architecture": "amd64",
                    "os": "linux"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 567,
                 "digest": "sha256:cd8550c2485dc05d543a48bdf5ad5680b6499963cc503adaf4af17289e630aa3",
                 "platform": {
                    "architecture": "unknown",
                    "os": "unknown"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 5337,
                 "digest": "sha256:fd47d1cee1d5359a38851d1229e6c6862d719f34700bcd4fbbc72af377b2e2f0",
                 "platform": {
                    "architecture": "arm",
                    "os": "linux",
                    "variant": "v5"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 567,
                 "digest": "sha256:447fa72bac35bb0cba8c9a896506c42764420f0b9bcdd5c2111f1d5a061aa58c",
                 "platform": {
                    "architecture": "unknown",
                    "os": "unknown"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 5337,
                 "digest": "sha256:bca3cd131cc42f847bd93d94c52dc416f6389ad1e5c79f16105f1c2dae680abc",
                 "platform": {
                    "architecture": "arm",
                    "os": "linux",
                    "variant": "v7"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 567,
                 "digest": "sha256:5532e2ce9d0cd8999ba49b68cea7eec53aa2fdaa70b6c631e8b459d8ea842da4",
                 "platform": {
                    "architecture": "unknown",
                    "os": "unknown"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 5338,
                 "digest": "sha256:7d2c1ebed7ad8bfe233c6f7f909e4664f8990719d67108a82a9bf6c1505fc0ce",
                 "platform": {
                    "architecture": "arm64",
                    "os": "linux",
                    "variant": "v8"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 567,
                 "digest": "sha256:2f071b4956263871c24e7e2b66a8d3ba8fadd1fb38ebb8f2babe3c322e458eb7",
                 "platform": {
                    "architecture": "unknown",
                    "os": "unknown"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 5335,
                 "digest": "sha256:50debca8924498cda5312a52952729d1b5f65e9e74eb99c5593e4b74edd2dc43",
                 "platform": {
                    "architecture": "386",
                    "os": "linux"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 567,
                 "digest": "sha256:70097b7e839ef998c93da1ce41207a7e9278ba52706438399b9d720b8bcc88a0",
                 "platform": {
                    "architecture": "unknown",
                    "os": "unknown"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 5338,
                 "digest": "sha256:2de96383f2ff44a0205af9654af4782e4680d67741715a6b01031d8a16ae0733",
                 "platform": {
                    "architecture": "ppc64le",
                    "os": "linux"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 567,
                 "digest": "sha256:a2efd1525dfb5f07594cb19ebf16907a4ea4e5bf72fef9cec0e68159cad5d1d0",
                 "platform": {
                    "architecture": "unknown",
                    "os": "unknown"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 5338,
                 "digest": "sha256:e3d7b70185b5d7e76826b244730a72a1e45842c58b60e1d6aab00f58cc3bbbd6",
                 "platform": {
                    "architecture": "riscv64",
                    "os": "linux"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 567,
                 "digest": "sha256:47d1128981dd789dd0d0ddf5e70ef25deffd24e8bfb08635277c2626ee6d4aca",
                 "platform": {
                    "architecture": "unknown",
                    "os": "unknown"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 5335,
                 "digest": "sha256:93671b9b15e0a69d555093fca0f9dac1e2441228300fb3fc763478179a4a8a4f",
                 "platform": {
                    "architecture": "s390x",
                    "os": "linux"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 567,
                 "digest": "sha256:d684110391b649b32139fbce1a7124be26f517c19b656e3dfb7e0e74c324894e",
                 "platform": {
                    "architecture": "unknown",
                    "os": "unknown"
                 }
              }
           ]
        })).unwrap()
    }

    fn nextcloud_32_0_2_manifest() -> OciManifest {
        serde_json::from_value(serde_json::json!({
           "schemaVersion": 2,
           "mediaType": "application/vnd.oci.image.index.v1+json",
           "manifests": [
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 5336,
                 "digest": "sha256:4f0ea71dbe72cae030fa8de9dda67c728eab189becf27e40f6a380c47789ac47",
                 "platform": {
                    "architecture": "amd64",
                    "os": "linux"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 567,
                 "digest": "sha256:586fab872e04a76eab9c8e4e6623e65fbb95a779eae7620c25af3ff963f77c71",
                 "platform": {
                    "architecture": "unknown",
                    "os": "unknown"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 5337,
                 "digest": "sha256:b4c50ac8a940fcf4fda405323035ef03c26b59572625f9a9d9f71970ecde8769",
                 "platform": {
                    "architecture": "arm",
                    "os": "linux",
                    "variant": "v5"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 567,
                 "digest": "sha256:572cf2fa3c346620e0ef2efeadf9f0341a550550418c66a60666514a553b93f7",
                 "platform": {
                    "architecture": "unknown",
                    "os": "unknown"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 5337,
                 "digest": "sha256:58f656f5a7682e92540cd3ffeebfb91a50d8d992d62a49b8c720cb3eb2905620",
                 "platform": {
                    "architecture": "arm",
                    "os": "linux",
                    "variant": "v7"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 567,
                 "digest": "sha256:7a580b58547e8eab894ba8167d0bf4f5e00fb6b32f74e3add0969adfc856c9af",
                 "platform": {
                    "architecture": "unknown",
                    "os": "unknown"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 5338,
                 "digest": "sha256:ffa8ff44fb01593706611c1eea21f35cc429fb9ff43d502ccf38de77665d7c8c",
                 "platform": {
                    "architecture": "arm64",
                    "os": "linux",
                    "variant": "v8"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 567,
                 "digest": "sha256:6c0e5336d9542889e8b079a495ec34628d871543a7760b010cde2e5786ef47ff",
                 "platform": {
                    "architecture": "unknown",
                    "os": "unknown"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 5335,
                 "digest": "sha256:401c178e39664d27a50d77c4d8f512d8ee98ca1315adb35541c8209266e13261",
                 "platform": {
                    "architecture": "386",
                    "os": "linux"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 567,
                 "digest": "sha256:a1dad8a6958bd4e04f6b5b766893db39b8c51736f8397e646f9e4aaec0b22c30",
                 "platform": {
                    "architecture": "unknown",
                    "os": "unknown"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 5338,
                 "digest": "sha256:a3c712461c3821c76343ff044078f9e812c700930d72e26fada4634eccdff7bb",
                 "platform": {
                    "architecture": "ppc64le",
                    "os": "linux"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 567,
                 "digest": "sha256:8adfb3ee4f8559b616cd38cfe3d32f9a514dbb91f802f49408a50180cace0b05",
                 "platform": {
                    "architecture": "unknown",
                    "os": "unknown"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 5338,
                 "digest": "sha256:0bd042ed7b64a9c1ce0ff38e100d0626d5727601d0ae67447d868ffc73af005b",
                 "platform": {
                    "architecture": "riscv64",
                    "os": "linux"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 567,
                 "digest": "sha256:9ebc4d8e79499c0957c3c8370d8c8bc6c6664c7d2b7d2fe585a73d8997e79715",
                 "platform": {
                    "architecture": "unknown",
                    "os": "unknown"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 5335,
                 "digest": "sha256:8d03aa09137f89ad4fcd7e8c74a889aaffba2683b08e770a9de000a8de6fdc11",
                 "platform": {
                    "architecture": "s390x",
                    "os": "linux"
                 }
              },
              {
                 "mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "size": 567,
                 "digest": "sha256:544d912a0031fb5aaab39d152d13b67e2fb337c1537158e188a9bfed6747056e",
                 "platform": {
                    "architecture": "unknown",
                    "os": "unknown"
                 }
              }
           ]
        })).unwrap()
    }

    #[test]
    fn detect_missisng_partial_semver_tags() {
        assert_eq!(
            detect_miss_placed_tags(
                &[PartialSemverVersion::from(Version::new(32, 0, 1))],
                BTreeMap::from([(
                    PartialSemverVersion::from(Version::new(32, 0, 1)),
                    nextcloud_32_0_1_manifest(),
                )]),
            ),
            Err(vec![
                ValidationError::MissingMajor {
                    latest_version: Version::new(32, 0, 1)
                },
                ValidationError::MissingMajorMinor {
                    latest_version: Version::new(32, 0, 1)
                }
            ])
        );
    }

    #[test]
    fn detect_missing_major_only() {
        assert_eq!(
            detect_miss_placed_tags(
                &[
                    PartialSemverVersion::with_major_minor(32, 0),
                    PartialSemverVersion::from(Version::new(32, 0, 0)),
                    PartialSemverVersion::from(Version::new(32, 0, 1)),
                    PartialSemverVersion::from(Version::new(32, 0, 2)),
                ],
                BTreeMap::from([
                    (
                        PartialSemverVersion::from(Version::new(32, 0, 0)),
                        nextcloud_32_0_0_manifest(),
                    ),
                    (
                        PartialSemverVersion::from(Version::new(32, 0, 1)),
                        nextcloud_32_0_1_manifest(),
                    ),
                    (
                        PartialSemverVersion::from(Version::new(32, 0, 2)),
                        nextcloud_32_0_2_manifest(),
                    ),
                    (
                        PartialSemverVersion::with_major_minor(32, 0),
                        nextcloud_32_0_2_manifest(),
                    )
                ]),
            ),
            Err(vec![ValidationError::MissingMajor {
                latest_version: Version::new(32, 0, 2)
            },])
        );
    }

    #[test]
    fn detect_missing_major_minor_only() {
        assert_eq!(
            detect_miss_placed_tags(
                &[
                    PartialSemverVersion::with_major(32),
                    PartialSemverVersion::from(Version::new(32, 0, 0)),
                    PartialSemverVersion::from(Version::new(32, 0, 1))
                ],
                BTreeMap::from([
                    (
                        PartialSemverVersion::from(Version::new(32, 0, 0)),
                        nextcloud_32_0_0_manifest(),
                    ),
                    (
                        PartialSemverVersion::from(Version::new(32, 0, 1)),
                        nextcloud_32_0_1_manifest(),
                    ),
                    (
                        PartialSemverVersion::with_major(32),
                        nextcloud_32_0_1_manifest(),
                    )
                ]),
            ),
            Err(vec![ValidationError::MissingMajorMinor {
                latest_version: Version::new(32, 0, 1)
            },])
        );
    }

    #[test]
    fn detect_miss_placed_major() {
        assert_eq!(
            detect_miss_placed_tags(
                &[
                    PartialSemverVersion::with_major(32),
                    PartialSemverVersion::with_major_minor(32, 0),
                    PartialSemverVersion::from(Version::new(32, 0, 0)),
                    PartialSemverVersion::from(Version::new(32, 0, 1))
                ],
                BTreeMap::from([
                    (
                        PartialSemverVersion::from(Version::new(32, 0, 0)),
                        nextcloud_32_0_0_manifest(),
                    ),
                    (
                        PartialSemverVersion::from(Version::new(32, 0, 1)),
                        nextcloud_32_0_1_manifest(),
                    ),
                    (
                        PartialSemverVersion::with_major_minor(32, 0),
                        nextcloud_32_0_1_manifest(),
                    ),
                    (
                        PartialSemverVersion::with_major(32),
                        // should have been 32_0_1
                        nextcloud_32_0_0_manifest(),
                    )
                ]),
            ),
            Err(vec![ValidationError::MissPlaced {
                major_or_major_minor: PartialSemverVersion::with_major(32),
                should_point_to: Version::new(32, 0, 1),
                pointing_to_instead: Version::new(32, 0, 0)
            },])
        );
    }

    #[test]
    fn detect_miss_placed_major_minor() {
        assert_eq!(
            detect_miss_placed_tags(
                &[
                    PartialSemverVersion::with_major(32),
                    PartialSemverVersion::with_major_minor(32, 0),
                    PartialSemverVersion::from(Version::new(32, 0, 0)),
                    PartialSemverVersion::from(Version::new(32, 0, 1))
                ],
                BTreeMap::from([
                    (
                        PartialSemverVersion::from(Version::new(32, 0, 0)),
                        nextcloud_32_0_0_manifest(),
                    ),
                    (
                        PartialSemverVersion::from(Version::new(32, 0, 1)),
                        nextcloud_32_0_1_manifest(),
                    ),
                    (
                        PartialSemverVersion::with_major_minor(32, 0),
                        // should have been 32_0_1
                        nextcloud_32_0_0_manifest(),
                    ),
                    (
                        PartialSemverVersion::with_major(32),
                        nextcloud_32_0_1_manifest(),
                    )
                ]),
            ),
            Err(vec![ValidationError::MissPlaced {
                major_or_major_minor: PartialSemverVersion::with_major_minor(32, 0),
                should_point_to: Version::new(32, 0, 1),
                pointing_to_instead: Version::new(32, 0, 0)
            },])
        );
    }
}
