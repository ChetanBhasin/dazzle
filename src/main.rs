use shiplift::tty::TtyChunk;
use shiplift::{ContainerOptions, Docker, Error, PullOptions};
use std::io::Write;
use termion::raw::IntoRawMode;
use tokio_stream::{Stream, StreamExt};

const IMAGE: &'static str = "l.gcr.io/google/bazel:latest";

pub type Result<T> = std::result::Result<T, Error>;

async fn build_image(docker: &Docker) {
    let mut stream = docker
        .images()
        .pull(&PullOptions::builder().image(IMAGE).build());
    while let Some(pull_result) = stream.next().await {
        match pull_result {
            Ok(output) => println!("{}", output),
            Err(err) => eprintln!("Error: {}", err),
        }
    }
}

async fn create_container(docker: &Docker, args: Vec<String>) -> Result<String> {
    let args = args.iter().map(|arg| arg.as_str()).collect();
    let dir_map = format!("{}:/src/workspace", std::env::current_dir().unwrap().into_os_string().into_string().unwrap());
    let options = ContainerOptions::builder(IMAGE)
        .working_dir("/src/workspace")
        .attach_stdout(true)
        .attach_stderr(true)
        .attach_stdin(true)
        .tty(true)
        .privileged(true)
        .volumes(vec![dir_map.as_str()])
        .cmd(args)
        .build();
    let response = docker.containers().create(&options).await?;
    if let Some(warnings) = response.warnings {
        for warning in warnings {
            eprintln!("WARNING: {}", warning);
        }
    };
    Ok(response.id)
}

async fn attach_stdout(mut reader: impl Stream<Item = Result<TtyChunk>> + Unpin) {
    println!("Ready to channel logs");
    let mut stdout = std::io::stdout()
        .into_raw_mode()
        .expect("Failed to unwrap raw mode terminal");
    let mut stderr = std::io::stderr()
        .into_raw_mode()
        .expect("Failed to unrap raw mode error into terminal");
    while let Some(tty_result) = reader.next().await {
        match tty_result {
            Ok(chunk) => {
                let result = match chunk {
                    TtyChunk::StdOut(bytes) => stdout.write(bytes.as_ref()),
                    TtyChunk::StdErr(bytes) => stderr.write(bytes.as_ref()),
                    TtyChunk::StdIn(_) => unreachable!(),
                };
                result.expect("Writing to stdout/stderr failed");
            }
            Err(e) => eprintln!("Failed to stream logs to terminal: {}", e),
        };
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let docker = Docker::new();
    build_image(&docker).await;
    let container_id = create_container(&docker, args).await?;
    docker
        .containers()
        .get(container_id.clone())
        .start()
        .await?;
    let log_mux = docker
        .containers()
        .get(container_id.clone())
        .attach()
        .await?;
    docker
        .containers()
        .get(container_id.clone())
        .start()
        .await?;

    let (reader, _writer) = log_mux.split();
    attach_stdout(reader).await;

    Ok(())
}
