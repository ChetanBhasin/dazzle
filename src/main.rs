use bollard::container::{Config, RemoveContainerOptions, LogsOptions};
use bollard::Docker;

use bollard::exec::{CreateExecOptions, StartExecResults};
use bollard::image::CreateImageOptions;
use futures_util::stream::StreamExt;
use futures_util::TryStreamExt;
use bollard::exec::StartExecResults::Attached;
use tokio::time::Duration;
use bollard::models::HostConfig;

const IMAGE: &'static str = "l.gcr.io/google/bazel:latest";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + 'static>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let docker = Docker::connect_with_unix_defaults().unwrap();

    docker
        .create_image(
            Some(CreateImageOptions {
                from_image: IMAGE,
                ..Default::default()
            }),
            None,
            None,
        )
        .try_collect::<Vec<_>>()
        .await?;

    let bazel_config = Config {
        image: Some(IMAGE),
        tty: Some(true),
        attach_stderr: Some(true),
        attach_stdout: Some(true),
        attach_stdin: Some(true),
        host_config: Some(HostConfig{
            mounts: None,
            ..Default::default()
        }),
        cmd: Some(args.iter().map(|s| s as &str).collect()),
        ..Default::default()
    };

    let id = docker
        .create_container::<&str, &str>(None, bazel_config)
        .await?
        .id;
    docker.start_container::<String>(&id, None).await?;
    let mut logstream = docker.logs::<String>(&id, Some(LogsOptions {
        follow: true,
        stdout: true,
        stderr: true,
        ..Default::default()
    }));
    while let Some(output) = logstream.next().await {
         match output {
             Ok(log) => println!("{}", log),
             Err(err) => eprintln!("{}", err)
         }
    };

    docker
        .remove_container(
            &id,
            Some(RemoveContainerOptions {
                force: true,
                ..Default::default()
            }),
        )
        .await?;

    Ok(())
}
