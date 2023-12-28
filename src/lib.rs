//! simple s3 client with C interfaces
use flexi_logger::{with_thread, Age, Cleanup, Criterion, FileSpec, Logger, Naming, WriteMode};
use log::{error, info};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{sync::Mutex, path::Path};
use sysinfo::System;
use urlencoding::decode;
pub use basic::S3Params;
mod basic;
mod download;
mod upload;

#[derive(Debug, Serialize, Deserialize)]
struct InitParams {
    log_path: Option<String>,
}

static mut RUNTIME: Mutex<Option<tokio::runtime::Runtime>> = Mutex::new(None);
/// init tokio runtime
/// run this function before any other functions
/// # Arguments
/// - `params` - The params of init, json format
///     - `log_path` - The log path, use stdout if not set
pub fn init(params_str: String) {
    let mut runtime = unsafe { RUNTIME.lock().unwrap() };
    if !runtime.is_none() {
        return;
    }
    let params = match serde_json::from_str::<InitParams>(&params_str) {
        Ok(params) => params,
        Err(err) => {
            panic!("parse init params failed: {}", err);
        }
    };
    let log_path = params.log_path.as_ref();
    if log_path.is_some_and(|path| Path::new(path).exists()) {
        let log_path = log_path.unwrap();
        let _logger = Logger::try_with_str("info")
            .unwrap()
            .log_to_file(FileSpec::default().directory(log_path))
            .write_mode(WriteMode::Direct)
            .rotate(
                Criterion::Age(Age::Day),
                Naming::Timestamps,
                Cleanup::KeepLogFiles(7),
            )
            .format(with_thread)
            .start()
            .unwrap();
    } else {
        let _logger = Logger::try_with_str("info")
            .unwrap()
            .format(with_thread)
            .start()
            .unwrap();
    }
    info!("init params: {}", params_str);
    let mut system_info = System::new_all();
    system_info.refresh_all();
    let system_info_json = json!({
        "system name": System::name(),
        "system kernel version": System::kernel_version(),
        "system os version": System::os_version(),
        "system host name": System::host_name(),
        "total memory": system_info.total_memory(),
        "used memory": system_info.used_memory(),
    });
    info!("system info: {}", system_info_json);
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(8)
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

/// Upload file to s3
/// # Arguments
/// - `params` - The params of upload, json format
///     - `bucket` - The bucket name
///     - `object` - The object key
///     - `access_key_id` - The access key id
///     - `secret_access_key` - The secret access key
///     - `file_path` - The file path
///     - `security_token` - The security token
///     - `region` - The region
///     - `tries` - The max retry times
///     - `endpoint` - The endpoint, use default if not set
/// - `result_callback` - The callback function when upload finished
///     - `success` - The upload succeeded or not
///     - `message` - The error message if upload failed
/// - `progress_callback` - The callback function when upload progress changed
///     - `progress` - The progress of upload, in percentage
pub fn upload(
    params: String,
    result_callback: basic::ResultCallback,
    progress_callback: basic::ProgressCallback,
) {
    info!("upload params: {}", params);
    let runtime = unsafe { RUNTIME.lock().unwrap() };
    let runtime = match &*runtime {
        Some(runtime) => runtime,
        None => {
            result_callback(false, "runtime not initialized".to_string());
            error!("runtime not initialized");
            return;
        }
    };
    let mut params = match serde_json::from_str::<basic::S3Params>(&params) {
        Ok(params) => params,
        Err(err) => {
            result_callback(false, format!("parse params failed: {}", err));
            error!("parse params failed: {}", err);
            return;
        }
    };
    params.object = match decode(&params.object) {
        Ok(object) => object.to_string(),
        Err(err) => {
            result_callback(false, format!("url decode param object failed: {}", err));
            error!("decode object failed: {}", err);
            return;
        }
    };
    runtime.spawn(async move {
        let result = upload::put_object(&params, progress_callback).await;
        if result.is_ok() {
            info!("upload finished");
            result_callback(true, "".to_string());
        } else {
            let error_descrption = result.err().map(|err| err.to_string()).unwrap_or_default();
            error!("upload failed: {}", error_descrption);
            result_callback(false, error_descrption);
        }
    });
}

/// download file from s3
/// # Arguments
/// - `params` - The params of download, json format
///     - `bucket` - The bucket name
///     - `object` - The object key
///     - `access_key_id` - The access key id
///     - `secret_access_key` - The secret access key
///     - `file_path` - The file path
///     - `security_token` - The security token
///     - `region` - The region
///     - `tries` - The max retry times
///     - `endpoint` - The endpoint, use default if not set
/// - `result_callback` - The callback function when download finished
///     - `success` - The download succeeded or not
///     - `message` - The error message if download failed
pub fn download(params: String, result_callback: basic::ResultCallback) {
    info!("download params: {}", params);
    let runtime = unsafe { RUNTIME.lock().unwrap() };
    let runtime = match &*runtime {
        Some(runtime) => runtime,
        None => {
            error!("runtime not initialized");
            result_callback(false, "runtime not initialized".to_string());
            return;
        }
    };
    let mut params = match serde_json::from_str::<basic::S3Params>(&params) {
        Ok(params) => params,
        Err(err) => {
            error!("parse params failed: {}", err);
            result_callback(false, format!("parse params failed: {}", err));
            return;
        }
    };
    params.object = match decode(&params.object) {
        Ok(object) => object.to_string(),
        Err(err) => {
            result_callback(false, format!("url decode param object failed: {}", err));
            error!("decode object failed: {}", err);
            return;
        }
    };
    runtime.spawn(async move {
        let result = download::get_object(&params).await;
        if result.is_ok() {
            info!("download finished");
            result_callback(true, "".to_string());
        } else {
            let error_descrption = result.err().map(|err| err.to_string()).unwrap_or_default();
            error!("download failed: {}", error_descrption);
            result_callback(false, error_descrption);
        }
    });
}