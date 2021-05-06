use bollard::container::{Config, LogsOptions, RemoveContainerOptions};
use bollard::Docker;

use bollard::image::CreateImageOptions;
use bollard::models::HostConfig;
use futures_util::stream::StreamExt;
use futures_util::TryStreamExt;
use signal_hook::consts::TERM_SIGNALS;
use signal_hook::iterator::exfiltrator::SignalOnly;
use signal_hook::iterator::SignalsInfo;
use std::path::Path;
use std::sync::Arc;

const IMAGE: &'static str = "l.gcr.io/google/bazel:latest";

type ErrBox = Box<dyn std::error::Error + 'static>;

async fn build_docker_image(docker: &Docker) -> Result<(), Box<dyn std::error::Error + 'static>> {
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
    Ok(())
}

fn build_bazel_config(cmd_args: Vec<String>, current_dir: String) -> Config<String> {
    let mut args: Vec<String> = vec![];
    args.push("--output_user_root=/tmp/dazzle".into());
    cmd_args.iter().for_each(|x| args.push(x.to_owned()));
    Config {
        image: Some(IMAGE.into()),
        tty: Some(true),
        attach_stderr: Some(true),
        attach_stdout: Some(true),
        attach_stdin: Some(true),
        open_stdin: Some(true),
        stdin_once: Some(true),
        working_dir: Some("/src/workspace".into()),
        host_config: Some(HostConfig {
            mounts: None,
            binds: Some(vec![
                format!("{}:/src/workspace", current_dir).into(),
                "/tmp/dazzle:/tmp/dazzle".into(),
            ]),
            ..Default::default()
        }),
        cmd: Some(args),
        ..Default::default()
    }
}

async fn run_container(docker: &Docker, config: Config<String>) -> Result<String, ErrBox> {
    let container = docker
        .create_container::<&str, String>(None, config)
        .await?;
    let id = container.id;
    docker.start_container::<String>(&id, None).await?;
    Ok(id)
}

async fn stop_container(docker: &Docker, container_id: &String) -> Result<(), ErrBox> {
    docker
        .remove_container(
            &container_id,
            Some(RemoveContainerOptions {
                force: true,
                ..Default::default()
            }),
        )
        .await?;
    Ok(())
}

async fn map_logs(docker: Arc<Docker>, container_id: &String) {
    let mut logstream = docker.logs::<String>(
        container_id,
        Some(LogsOptions {
            follow: true,
            stdout: true,
            stderr: true,
            ..Default::default()
        }),
    );

    while let Some(output) = logstream.next().await {
        match output {
            Ok(log) => println!("{}", log),
            Err(err) => eprintln!("{}", err),
        }
    }
}

fn create_default_dirs() -> Result<(), ErrBox> {
    let dazzle_dir = Path::new("/tmp/dazzle");
    std::fs::create_dir_all(dazzle_dir)?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), ErrBox> {
    create_default_dirs()?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let current_dir: String = std::env::current_dir()?
        .into_os_string()
        .into_string()
        .expect("Could not convert OS String to string");
    let docker = Arc::new(Docker::connect_with_unix_defaults().unwrap());
    build_docker_image(&docker).await?;
    let bazel_config = build_bazel_config(args, current_dir);
    let container_id = run_container(&docker, bazel_config).await?;

    let job_end = {
        let docker = docker.clone();
        let container_id = container_id.clone();
        tokio::spawn(async move { map_logs(docker, &container_id).await; })
    };

    let term_sig = tokio::spawn(async move {
        let mut signals =
            SignalsInfo::<SignalOnly>::new(TERM_SIGNALS).expect("Failed to fetch signals");
        for _ in &mut signals {
            break;
        };
    });

    tokio::select! {
        _val = job_end => {},
        _val = term_sig => {}
    }

    stop_container(&docker, &container_id).await?;

    Ok(())
}
