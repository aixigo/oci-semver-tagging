use anyhow::{anyhow, Context, Result};
use clap::{Parser, ValueEnum};
use oci_distribution::{client::{ClientConfig, ClientProtocol}, secrets::RegistryAuth, Client, Reference};
use semver::Version;
use std::str::FromStr;

mod tag;

#[derive(Parser, Debug, PartialEq)]
#[command(version, about, long_about = None)]
pub struct Args {
    /// The user that is able to login to the registry
    #[arg(short, long)]
    user: Option<String>,
    // The protocol that the client should use to connect to the registry.
    #[arg(short, long, default_value = "https")]
    protocol: Protocol,
    #[command(flatten)]
    password: Password,
    #[command(subcommand)]
    sub_command: SubCommands,
}

#[derive(Parser, Debug, PartialEq)]
enum SubCommands {
    /// Tags the given images with semantic version tags
    Tag {
        /// The image that shall be tagged with semantic version tags.
        image: Reference,
        /// The version that the image will be tagged with. If not specified, the version will be
        /// parsed from the image's tag.
        tag_version: Option<Version>,
        /// A prefix that will be put in front of the tags to be pushed.
        #[arg(short, long)]
        tag_prefix: Option<String>,
        /// If the tool only outputs only what it would push.
        #[arg(short, long, default_value = "false")]
        dry_run: bool,
    },
    /// Validates the existing tags if they are tagged according to the semantic versioning
    Validate,
}

#[derive(clap::Args, Debug, PartialEq)]
#[group(required = false, multiple = false)]
struct Password {
    /// The user's password will be read from stdin
    #[arg(long = "password-stdin")]
    stdin: bool,
    /// The user's password will be read from the specified environment variable
    #[arg(long = "password-env")]
    env: Option<String>,
}

#[derive(PartialEq, Debug, Clone, ValueEnum)]
enum Protocol {
    Https,
    Http,
}

impl Args {
    fn registry_auth(&self) -> Result<RegistryAuth> {
        match (&self.user, &self.password.stdin, &self.password.env) {
            (None, false, None) => Ok(RegistryAuth::Anonymous),
            (Some(_user), true, None) => {
                todo!()
            }
            (Some(user), false, Some(env_var_name)) => {
                let password = std::env::var(&env_var_name).with_context(|| {
                    format!("Cannot read password from environment variable {env_var_name}.")
                })?;

                Ok(RegistryAuth::Basic(user.clone(), password))
            }
            _ => Err(anyhow!("TODO")),
        }
    }
}

fn version_to_tag(
    image: &Reference,
    cli_version: Option<Version>,
    tag_prefix: &Option<String>,
) -> Result<Version> {
    match cli_version {
        Some(version) => {
            if version.build.is_empty() {
                Ok(version)
            } else {
                Err(anyhow!("{version} contains build metadata which contains characters that are incompatible with distribution spec: https://github.com/opencontainers/distribution-spec/issues/154"))
            }
        }
        None => {
            let tag = image
                .tag()
                .ok_or_else(|| anyhow!("Missing tag for {image}"))?;

            let tag = match tag_prefix.as_ref() {
                None => tag,
                Some(prefix) => {
                    if !tag.starts_with(prefix) {
                        return Err(anyhow!(
                            "The image tag {tag} doesn't start with the prefix {prefix}"
                        ));
                    }
                    tag.trim_start_matches(prefix)
                }
            };
            Version::from_str(tag)
                .with_context(|| format!("Can't parse version from image's tag which is {tag}"))
        }
    }
}

async fn present_semver_tags(
    client: &Client,
    registry_auth: &RegistryAuth,
    image: &Reference,
    prefix: &Option<String>,
) -> Result<Vec<Version>> {
    let tag_respones = client
        .list_tags(image, registry_auth, None, None)
        .await
        .with_context(|| format!("Cannot resolve tags for {image}."))?;

    Ok(tag_respones
        .tags
        .into_iter()
        .flat_map(|tag| {
            let tag = match prefix.as_ref() {
                None => tag.as_str(),
                Some(prefix) => {
                    if !tag.starts_with(prefix) {
                        return None;
                    }
                    tag.trim_start_matches(prefix)
                }
            };
            Version::from_str(tag).ok()
        })
        .collect())
}

pub async fn run(args: Args) -> Result<()> {
    let client = Client::new(ClientConfig {
        protocol: match &args.protocol {
            Protocol::Https => ClientProtocol::Https,
            Protocol::Http => ClientProtocol::Http,
        },
        ..Default::default()
    });

    let registry_auth = args.registry_auth()?;

    match args.sub_command {
        SubCommands::Validate => todo!(),
        SubCommands::Tag {
            image,
            tag_version,
            tag_prefix,
            dry_run,
        } => {
            let version_to_tag = version_to_tag(&image, tag_version, &tag_prefix)?;

            let existing_tags = present_semver_tags(
                &client,
                &registry_auth,
                &Reference::from_str(&format!("{}/{}", image.registry(), image.repository(),))
                    .expect("Must be valid image string"),
                &tag_prefix,
            )
            .await?;

            tag::tag(
                &client,
                &registry_auth,
                &image,
                &existing_tags,
                version_to_tag,
                &tag_prefix,
                dry_run,
            )
            .await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefer_version_cli_instead_of_parsing_image_tag_version() {
        assert_eq!(
            version_to_tag(
                &Reference::from_str("hello-world:16.0.0").unwrap(),
                Some(Version::from_str("1.2.3").unwrap()),
                &None
            )
            .unwrap(),
            Version::from_str("1.2.3").unwrap()
        )
    }

    #[test]
    fn parse_version_from_image_tag() {
        assert_eq!(
            version_to_tag(
                &Reference::from_str("hello-world:16.0.0").unwrap(),
                None,
                &None
            )
            .unwrap(),
            Version::from_str("16.0.0").unwrap()
        )
    }

    #[test]
    fn parse_version_from_image_tag_with_prefix() {
        assert_eq!(
            version_to_tag(
                &Reference::from_str("hello-world:v16.0.0").unwrap(),
                None,
                &Some(String::from("v"))
            )
            .unwrap(),
            Version::from_str("16.0.0").unwrap()
        )
    }

    #[test]
    fn fail_on_build_meta_data_semver() {
        let err = version_to_tag(
            &Reference::from_str("hello-world:latest").unwrap(),
            Some(Version::from_str("0.8.1+zstd.1.5.0").unwrap()),
            &None,
        )
        .unwrap_err();

        assert_eq!(err.to_string(), "0.8.1+zstd.1.5.0 contains build metadata which contains characters that are incompatible with distribution spec: https://github.com/opencontainers/distribution-spec/issues/154")
    }

    #[test]
    fn fail_on_none_matching_version_prefix() {
        let err = version_to_tag(
            &Reference::from_str("hello-world:1.2.3").unwrap(),
            None,
            &Some(String::from("v")),
        )
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "The image tag 1.2.3 doesn't start with the prefix v"
        )
    }

    mod parse_args {
        use super::*;

        #[test]
        fn without_auth() -> Result<()> {
            let args = Args::try_parse_from([
                "oci-semver-tagging",
                "tag",
                "localhost:5135/postgres:15.8.0",
            ])?;

            assert_eq!(
                args,
                Args {
                    user: None,
                    password: Password {
                        stdin: false,
                        env: None
                    },
                    protocol: Protocol::Https,
                    sub_command: SubCommands::Tag {
                        image: Reference::from_str("localhost:5135/postgres:15.8.0")?,
                        tag_version: None,
                        tag_prefix: None,
                        dry_run: false
                    }
                }
            );

            Ok(())
        }
    }
}
