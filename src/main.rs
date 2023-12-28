use clap::Parser;
use log::info;
use ne_s3::{download, init, uninit, upload};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::{Arc, Mutex, Condvar};

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

fn main() {
    let args = Args::parse();
    init(
        json!(
            {
                "log_path": args.log_path,
            }
        )
        .to_string(),
    );
    let flag = Arc::new(Mutex::new(false));
    let cond = Arc::new(Condvar::new());
    match args.command.as_str() {
        "upload" => {
            let cflag = flag.clone();
            let ccond = cond.clone();
            let result_callback = move |success: bool, message: String| {
                info!("upload finished: {}", success);
                info!("upload message: {}", message);
                let mut lock = cflag.lock().unwrap();
                *lock = true;
                ccond.notify_one();
            };
            let progress_callback = |progress: f64| {
                info!("put object progress: {:.2}%", progress);
            };
            upload(
                serde_json::to_string(&args).unwrap(),
                Box::new(result_callback),
                Arc::new(Mutex::new(progress_callback)),
            );
            let mut lock = flag.lock().unwrap();
            while !*lock {
                lock = cond.wait(lock).unwrap();
            }
        }
        "download" => {
            let cflag = flag.clone();
            let ccond = cond.clone();
            download(
                serde_json::to_string(&args).unwrap(),
                Box::new(move |success: bool, message: String| {
                    info!("download finished: {}", success);
                    info!("download message: {}", message);
                    let mut lock = cflag.lock().unwrap();
                    *lock = true;
                    ccond.notify_one();
                }),
            );
            let mut lock = flag.lock().unwrap();
            while !*lock {
                lock = cond.wait(lock).unwrap();
            }
        }
        _ => {
            println!("unknown command: {}", args.command);
        }
    }
    uninit();
}
