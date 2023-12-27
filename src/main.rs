use clap::Parser;
use log::info;
use ne_s3::{download, init, uninit, upload};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::{Arc, Mutex};

#[derive(Parser, Debug, Serialize, Deserialize)]
#[command(author, version, about, long_about = None)]
struct Args {
    command: String,
    #[arg(long)]
    bucket: String,
    #[arg(long)]
    object: String,
    #[arg(long)]
    access_key_id: String,
    #[arg(long)]
    secret_access_key: String,
    #[arg(long)]
    session_token: String,
    #[arg(long)]
    file_path: String,
    #[arg(long)]
    security_token: String,
    #[arg(long, default_value_t = String::new())]
    ca_certs_path: String,
    #[arg(long, default_value_t = String::new())]
    region: String,
    #[arg(long, default_value_t = 3)]
    tries: u32,
    #[arg(long, default_value_t = String::new())]
    endpoint: String,
    #[arg(long, default_value_t = String::new())]
    log_path: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    init(
        json!(
            {
                "log_path": args.log_path,
            }
        )
        .to_string(),
    );
    match args.command.as_str() {
        "upload" => {
            let result_callback = |success: bool, message: String| {
                info!("upload finished: {}", success);
                info!("upload message: {}", message);
            };
            let progress_callback = |progress: f64| {
                info!("put object progress: {:.2}%", progress);
            };
            upload(
                serde_json::to_string(&args).unwrap(),
                Box::new(result_callback),
                Arc::new(Mutex::new(progress_callback)),
            );
        }
        "download" => {
            download(
                serde_json::to_string(&args).unwrap(),
                Box::new(|success: bool, message: String| {
                    info!("download finished: {}", success);
                    info!("download message: {}", message);
                }),
            );
        }
        _ => {
            println!("unknown command: {}", args.command);
        }
    }
    uninit();
    Ok(())
}
