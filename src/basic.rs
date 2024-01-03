use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use s3::Bucket;
use s3::creds::Credentials;

/// result callback
/// - `success` - true if success, false if failed
/// - `message` - error message if failed
pub type ResultCallback = Box<dyn Fn(bool, String) + Send + Sync>;

/// progress callback
/// - `progress` - progress of upload or download, in percentage
pub type ProgressCallback = Arc<Mutex<dyn Fn(f32) + Send + Sync>>;

#[derive(Debug, Serialize, Deserialize)]
pub struct S3Params {
    pub(crate) bucket: String,
    pub(crate) object: String,
    pub(crate) access_key_id: String,
    pub(crate) secret_access_key: String,
    pub(crate) session_token: String,
    pub(crate) security_token: String,
    pub(crate) file_path: String,
    pub(crate) region: Option<String>,
    pub(crate) tries: Option<u32>,
    pub(crate) endpoint: Option<String>,
    pub(crate) ca_cert_path: Option<String>,
}

pub fn get_s3_bucket(params: &S3Params) -> Result<Bucket, Box<dyn std::error::Error>> {
    let mut region: s3::Region = match params.region {
        Some(ref r) => r.parse()?,
        None => s3::Region::UsEast1,
    };
    if params.endpoint.as_ref().is_some_and(|e| e.len() > 0) {
        region = s3::Region::Custom {
            region: params.region.as_ref().unwrap().to_string(),
            endpoint: params.endpoint.as_ref().unwrap().to_string(),
        };
    }
    let mut bucket = Bucket::new(
        params.bucket.as_str(),
        region,
        // Credentials are collected from environment, config, profile or instance metadata
        Credentials::new(
            Some(params.access_key_id.as_str()),
            Some(params.secret_access_key.as_str()),
            None,
            Some(params.session_token.as_str()),
            None,
        )?,
    )?;
    bucket.add_header("x-amz-meta-token", params.security_token.as_str());
    Ok(bucket)
}
