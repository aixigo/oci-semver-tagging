use anyhow::Result;
use clap::Parser;
use oci_semver_tagging::{run, Args};

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    run(args).await
}
