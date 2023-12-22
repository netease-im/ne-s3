//! simple s3 client with C interfaces
use std::sync::Mutex;
mod s3;

static mut RUNTIME: Mutex<Option<tokio::runtime::Runtime>> = Mutex::new(None);
/// init tokio runtime
/// run this function before any other functions
pub fn init() {
    let mut runtime = unsafe { RUNTIME.lock().unwrap() };
    if !runtime.is_none() {
        return;
    }
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    *runtime = Some(rt);
}

/// uninit tokio runtime
/// run this function to shutdown
pub fn uninit() {
    let mut runtime = unsafe { RUNTIME.lock().unwrap() };
    if runtime.is_none() {
        return;
    }
    if let Some(rt) = runtime.take() {
        rt.shutdown_background();
    }
}

pub type ResultCallback = Box<dyn Fn(bool, String) + Send + Sync>;
pub type ProgressCallback = Box<dyn Fn(u64) + Send + Sync>;

/// Upload file to s3
/// # Arguments
/// * `params` - The params of upload, json format
/// ** `bucket` - The bucket name
/// ** `object` - The object key
/// ** `access_key_id` - The access key id
/// ** `secret_access_key` - The secret access key
/// ** `file_path` - The file path
/// ** `security_token` - The security token
/// ** `region` - The region
/// ** `tries` - The max retry times
/// ** `verbose` - The verbose flag
/// ** `endpoint` - The endpoint, use default if not set
/// * `result_callback` - The callback function when upload finished
/// ** `success` - The upload succeeded or not
/// ** `message` - The error message if upload failed
/// * `progress_callback` - The callback function when upload progress changed
/// ** `progress` - The progress of upload, in percentage
/// # Return
/// * `0` - Success
pub fn upload(
    params: String,
    result_callback: ResultCallback,
    progress_callback: ProgressCallback,
) {
    let runtime = unsafe { RUNTIME.lock().unwrap() };
    let runtime = match &*runtime {
        Some(runtime) => runtime,
        None => {
            result_callback(false, "runtime not initialized".to_string());
            return;
        }
    };
    let params = match serde_json::from_str::<s3::S3Params>(&params) {
        Ok(params) => params,
        Err(err) => {
            result_callback(false, format!("parse params failed: {}", err));
            return;
        }
    };
    runtime.spawn(async move {
        let result = s3::upload_object(&params).await;
        result_callback(
            result.is_ok(),
            result.err().map(|err| err.to_string()).unwrap_or_default(),
        );
    });
}

#[cfg(test)]
mod tests {
    use std::env;
    use super::*;

    #[test]
    fn test() {
        init();
        {
            let rt = unsafe { RUNTIME.lock().unwrap() };
            if let Some(rt) = &*rt {
                rt.block_on(async {
                    let mut params = s3::S3Params {
                        bucket: env::var("AWS_BUCKET").unwrap(),
                        object: env::var("AWS_OBJECT_KEY").unwrap(),
                        access_key_id: env::var("AWS_ACCESS_KEY_ID").unwrap(),
                        secret_access_key: env::var("AWS_SECRET_ACCESS_KEY").unwrap(),
                        session_token: env::var("AWS_SESSION_TOKEN").unwrap(),
                        file_path: env::var("AWS_UPLOAD_FILE_PATH").unwrap(),
                        security_token: env::var("AWS_SECURITY_TOKEN").unwrap(),
                        region: Some(env::var("AWS_REGION").unwrap_or("ap-southeast-1".to_string())),
                        tries: Some(3),
                        endpoint: None,
                    };
                    println!("uploading begin");
                    let upload_size = s3::upload_object(&params).await.unwrap();
                    println!("uploading finished");
                    params.file_path = env::var("AWS_DOWNLOAD_FILE_PATH").unwrap();
                    println!("downloading begin");
                    let download_size = s3::download_object(&params).await.unwrap();
                    assert_eq!(download_size, upload_size);
                    println!("downloading finished");
                });
            }
        }
        uninit();
    }
}
