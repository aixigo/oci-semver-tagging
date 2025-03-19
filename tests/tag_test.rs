use assert_json_diff::assert_json_eq;
use clap::Parser;
use log::Level;
use oci_semver_tagging::{run, Args};
use testcontainers::{
    core::{
        logs::consumer::logging_consumer::LoggingConsumer,
        wait::{ExitWaitStrategy, HttpWaitStrategy},
        IntoContainerPort, WaitFor,
    },
    runners::AsyncRunner,
    GenericImage, ImageExt,
};

#[tokio::test]
async fn tag_with_multi_architectures() -> anyhow::Result<()> {
    env_logger::init();

    // TODO: random network name
    let network = "my-network";
    let registry = GenericImage::new("registry", "2")
        .with_exposed_port(5000.tcp())
        .with_wait_for(WaitFor::http(
            HttpWaitStrategy::new("/")
                .with_port(5000.tcp())
                .with_expected_status_code(200u16),
        ))
        .with_log_consumer(
            LoggingConsumer::new()
                .with_stdout_level(Level::Debug)
                .with_stderr_level(Level::Debug),
        )
        .with_network(network.to_string())
        .start()
        .await?;

    let registry_host = &registry.id()[0..12];

    log::info!("Copy postgres image into local registry");
    let _skopeo = GenericImage::new("quay.io/skopeo/stable", "latest")
        .with_wait_for(WaitFor::Exit(ExitWaitStrategy::new().with_exit_code(0)))
        .with_cmd([
            "copy",
            "--dest-tls-verify=false",
            "--multi-arch=all",
            // TODO: how can one avoid that we are pulling all the time from public infra.
            "docker://docker.io/postgres:16.8",
            &format!("docker://{registry_host}:5000/postgres:16.8.0"),
        ])
        .with_network(network.to_string())
        .with_log_consumer(
            LoggingConsumer::new()
                .with_stdout_level(Level::Debug)
                .with_stderr_level(Level::Debug),
        )
        .with_startup_timeout(std::time::Duration::from_secs(180))
        .start()
        .await?;

    let local_registry_port = registry.get_host_port_ipv4(5000.tcp()).await?;

    log::info!("Tag postgres with semver tags");
    let args = Args::parse_from([
        "oci-semver-tagging",
        "--protocol", "http",
        "tag",
        &format!("localhost:{local_registry_port}/postgres:16.8.0"),
    ]);
    run(args).await?;

    log::info!("Inspect tags created by oci-semver-tagging");
    let inspection_16_8_0 = dbg!(inspect(
        network,
        &format!("docker://{registry_host}:5000/postgres:16.8.0"),
    )
    .await?);
    let inspection_16_8 = inspect(
        network,
        &format!("docker://{registry_host}:5000/postgres:16.8"),
    )
    .await?;
    let inspection_16 = inspect(
        network,
        &format!("docker://{registry_host}:5000/postgres:16"),
    )
    .await?;

    assert_json_eq!(inspection_16_8_0, inspection_16_8);
    assert_json_eq!(inspection_16_8, inspection_16);
    // TODO with assert_json_diff() which should check for the architectures

    Ok(())
}

async fn inspect(network: &str, image: &str) -> anyhow::Result<serde_json::Value> {
    let skopeo = GenericImage::new("quay.io/skopeo/stable", "latest")
        .with_wait_for(WaitFor::Exit(ExitWaitStrategy::new().with_exit_code(0)))
        .with_cmd(["inspect", "--tls-verify=false", "--raw", image])
        .with_network(network.to_string())
        .with_log_consumer(
            LoggingConsumer::new()
                .with_stdout_level(Level::Debug)
                .with_stderr_level(Level::Debug),
        )
        .start()
        .await?;

    Ok(serde_json::from_slice(&skopeo.stdout_to_vec().await?)?)
}
