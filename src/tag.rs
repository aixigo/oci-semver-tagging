use anyhow::{Context, Result};
use oci_distribution::{secrets::RegistryAuth, Client, Reference};
use semver::{Version, VersionReq};
use tokio::task::JoinSet;
use std::str::FromStr as _;

pub async fn tag(
    client: &Client,
    registry_auth: &RegistryAuth,
    image: &Reference,
    existing_tags: &[Version],
    version_to_tag: Version,
    tag_prefix: &Option<String>,
    dry_run: bool,
) -> Result<()> {
    let tags_to_push = tags_to_push(version_to_tag, existing_tags, &tag_prefix);
    if tags_to_push.is_empty() {
        println!("Nothing to push");
        return Ok(());
    }

    let (baseline_manifest, _digest) = client
        .pull_manifest(&image, &registry_auth)
        .await
        .with_context(|| format!("Cannot pull manifest for {}", image))?;

    let mut set = JoinSet::new();

    for tag in tags_to_push {
        let image = Reference::from_str(&format!(
            "{}/{}:{tag}",
            image.registry(),
            image.repository()
        ))
        .expect("Must be valid image string");

        println!("Will push {image}");

        if !dry_run {
            let client = client.clone();
            let baseline_manifest = baseline_manifest.clone();
            set.spawn(async move {
                (
                    client.push_manifest(&image, &baseline_manifest).await,
                    image,
                )
            });
        }
    }

    let mut result = Ok(());
    while let Some(res) = set.join_next().await {
        match res {
            Ok((Ok(url), image)) => {
                println!("Pushed {image} to {url}.");
            }
            Ok((Err(err), image)) => {
                println!("Cannot push image {image}: {err}");
                result = Err(err).with_context(|| format!("{image}"));
            }
            Err(_err) => todo!(),
        }
    }

    result
}

fn tags_to_push(
    version: Version,
    existing_tags: &[Version],
    prefix: &Option<String>,
) -> Vec<String> {
    let mut tags = Vec::with_capacity(3);

    let prefix = prefix.as_ref().map(|s| s.as_str()).unwrap_or("");
    if !existing_tags.iter().any(|v| v == &version) {
        tags.push(format!(
            "{prefix}{}.{}.{}",
            version.major, version.minor, version.patch
        ));

        let version_req = VersionReq::parse(&format!(
            ">={major}.{minor}.{patch}, <{major}.{minor_next}, <{major_next}.0.0",
            major = version.major,
            minor = version.minor,
            patch = version.patch,
            major_next = version.major + 1,
            minor_next = version.minor + 1
        ))
        .expect("Must be valid version requirement");
        if !existing_tags.iter().any(|v| version_req.matches(v)) {
            tags.push(format!("{prefix}{}.{}", version.major, version.minor));

            let version_req = VersionReq::parse(&format!(
                ">={major}.{minor}, <{major_next}.0.0",
                major = version.major,
                minor = version.minor,
                major_next = version.major + 1
            ))
            .expect("Must be valid version requirement");
            if !existing_tags.iter().any(|v| version_req.matches(v)) {
                tags.push(format!("{prefix}{}", version.major));
            }
        }
    }

    tags.reverse();

    tags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_all_tags_for_none_exsting_major_version() {
        assert_eq!(
            tags_to_push(
                Version::from_str("1.2.3").unwrap(),
                &[Version::from_str("3.2.1").unwrap()],
                &None
            ),
            vec![
                String::from("1"),
                String::from("1.2"),
                String::from("1.2.3")
            ]
        )
    }

    #[test]
    fn push_all_tags_for_none_exsting_major_version_with_prefix() {
        assert_eq!(
            tags_to_push(
                Version::from_str("1.2.3").unwrap(),
                &[Version::from_str("3.2.1").unwrap()],
                &Some(String::from("v")),
            ),
            vec![
                String::from("v1"),
                String::from("v1.2"),
                String::from("v1.2.3")
            ]
        )
    }

    #[test]
    fn push_all_tags_except_major() {
        assert_eq!(
            tags_to_push(
                Version::from_str("1.2.3").unwrap(),
                &[
                    Version::from_str("1.3.3").unwrap(),
                    Version::from_str("3.2.1").unwrap()
                ],
                &None
            ),
            vec![String::from("1.2"), String::from("1.2.3")]
        )
    }

    #[test]
    fn push_only_patch_tag() {
        assert_eq!(
            tags_to_push(
                Version::from_str("1.2.3").unwrap(),
                &[Version::from_str("1.2.4").unwrap(),],
                &None
            ),
            vec![String::from("1.2.3")]
        )
    }

    #[test]
    fn push_no_tags() {
        assert_eq!(
            tags_to_push(
                Version::from_str("1.2.3").unwrap(),
                &[Version::from_str("1.2.3").unwrap(),],
                &None
            ),
            Vec::<String>::new()
        )
    }
}
