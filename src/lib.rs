use std::collections::HashMap;
use std::{fs::File, io::Write, path::PathBuf, process::exit};
use aws_sdk_s3::primitives::ByteStream;
use aws_config::meta::region::RegionProviderChain;
use aws_credential_types::provider::{ProvideCredentials, SharedCredentialsProvider};
use aws_sdk_s3::config::retry::RetryConfig;
use aws_sdk_s3::config::Credentials;
use aws_sdk_s3::{config::Region, meta::PKG_VERSION, Client, Error};
use std::path::Path;

/// A Credentials provider that uses non-standard environment variables `MY_ID`
/// and `MY_KEY` instead of `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY`. This
/// could alternatively load credentials from another secure source of environment
/// secrets, if traditional AWS SDK for Rust credentials providers do not suit
/// your use case.
///
/// WARNING: This example is for demonstration purposes only. Your code should always
/// load credentials in a secure fashion.
#[derive(Debug)]
struct StaticCredentials {
    access_key_id: String,
    secret_access_key: String,
    session_token: String,
}

impl StaticCredentials {
    pub fn new() -> Self {
        let access_key_id = "ASIA2FDNKLKWEVBPTU6N";
        let secret_access_key = "ZhZDJQdZIqQ6cJ8/49kd6muJbjQUIPnc/M1IZl8o";
        let session_token = "IQoJb3JpZ2luX2VjEFIaDmFwLXNvdXRoZWFzdC0xIkcwRQIgTwdMfkdgYz3pxfh5Lmc7Eg76ySwEzvJ/jG7V0MXzdPQCIQDwAH38ha21or8Ut+syUjlwxpJ59IboigmQBnG7Kyp85iqTAwjL//////////8BEAQaDDY5ODE2MTQ1Mzc0MCIModVxp66xdYeSXIgbKucCblG+9afmEO0koHmDNRttINSvQ0/igPo4s70XWewNDcvdit8Ojix34EUu19LwSKDOhvd+vE6uiwwnpBM6Z6W53BFVGF8SanqjaVQiQ8Y0KC7KvVCgQr42dk4jcT3rEBQHGh5UGLykZF1wUMOqysHWMNMC0fJTai2A7/f/Rs/FnnGzVbVMCPAEQxauI0xNLBHdD/qINoE4Nd1OxRrbjQMvzPpHXC6YdarU4Q96fwoVEwmycZphXG/Seljypl8b97vSL1I7oWdsfMfFOQ2yjYVJq+f3bRlxlRUQ5gGYCkf63Zy+R2ftZek7eSpfc3XwKlKJFUbjlpSHhiBskk9NcUH4imN/vi/aXdu4AvDTBVlmIZrRf2vJ8S+hCHRU1NGQo/an/QznSCjlNuzw0reKPxIxTZgfMRo0qEZMliEfv9BaASq6rkruqHweGlECfSTbCJSO5bELW/wh0O2MCUFS6KU8bv4h2FdWdRUwgeuDrAY6nQExKWCvOAFqVNgfzSWlSyvfG5RCz9JEs3q7g1u7MpzsI5/TYWYywsZbn/k74y5AQTM3Zli7PpdavVm/Rid5fIdIPDzqTj2G//bRhKnQZnowk/vRSlbBiOqW3xWe7o7JdJL2Wdz2dMzQzOoIiP65EcYaM1f8B36TNx2KB99BOr/pD+BXhQejC1zjHOq2PFCpWvrohpWeIm6S83mTaXBR";
        Self {
            access_key_id: access_key_id.trim().to_string(),
            secret_access_key: secret_access_key.trim().to_string(),
            session_token: session_token.trim().to_string(),
        }
    }

    async fn load_credentials(&self) -> aws_credential_types::provider::Result {
        Ok(Credentials::new(
            self.access_key_id.clone(),
            self.secret_access_key.clone(),
            Some(self.session_token.clone()),
            None,
            "StaticCredentials",
        ))
    }
}

impl ProvideCredentials for StaticCredentials {
    fn provide_credentials<'a>(
        &'a self,
    ) -> aws_credential_types::provider::future::ProvideCredentials<'a>
    where
        Self: 'a,
    {
        aws_credential_types::provider::future::ProvideCredentials::new(self.load_credentials())
    }
}

async fn testS3() -> Result<(), Box<dyn std::error::Error>> {
    let security_token = "$(Bucket)&$(Object)&$(ContentType)&$(ObjectSize)&11011&177817073&$(md5)&s3app&eyJ0YWciOiJuaW1fZGVmYXVsdF9pbSJ9";
    let object_key = "MTAxMTAxMA==/czNhcHBfMTc3OTE3MDcyXzE3MDI5NTAyNzMzODdfOGIwNmZiNWEtMjI1Ni00ZDUyLWIwN2ItYWQxYTA4ZDZmNmVhJjImMTIwMjYmeXVuLXhpbiZhcC1zb3V0aGVhc3QtMQ==";
    let region = Region::new("ap-southeast-1");
    let tries = 3;
    let verbose = true;
    let upload_file_path = "D:/迅雷下载/test_s3_upload.bin";
    let download_file_path = "D:/迅雷下载/download.bin";

    let region = RegionProviderChain::first_try(region)
        .or_else(Region::new("us-west-2"))
        .region()
        .await
        .expect("parsed region");
    println!();

    if verbose {
        println!("S3 client version: {PKG_VERSION}");
        println!("Region:            {region}");
        println!("Retries:           {tries}");
        println!();
    }

    assert_ne!(tries, 0, "You cannot set zero retries.");

    let shared_config = aws_config::SdkConfig::builder()
        .region(region)
        .credentials_provider(SharedCredentialsProvider::new(StaticCredentials::new()))
        // Set max attempts.
        // If tries is 1, there are no retries.
        .retry_config(RetryConfig::standard().with_max_attempts(tries))
        .build();
    // Construct an S3 client with customized retry configuration.
    let client = Client::new(&shared_config);
    let mut meta_data = HashMap::new();
    meta_data.insert("x-amz-meta-security-token".to_string(), security_token.to_string());
    let body = ByteStream::from_path(Path::new(upload_file_path)).await;
    client.put_object()
        .bucket("yun-xin")
        .set_metadata(Some(meta_data))
        .key(object_key)
        .body(body.unwrap())
        .send()
        .await?;
    println!("Put object succeeded, path: {}", upload_file_path);
    let mut file = File::create(download_file_path)?;
    let mut resp = client
        .get_object()
        .bucket("yun-xin")
        .key(object_key)
        .send()
        .await?;
    let mut byte_count = 0_usize;
    while let Some(bytes) = resp.body.try_next().await? {
        let bytes_len = bytes.len();
        file.write_all(&bytes)?;
        byte_count += bytes_len;
    }
    println!("Got object succeeded, path: {}, size: {}", download_file_path, byte_count);
    Ok(())

}

#[tokio::test]
async fn test_s3() {
    let result = testS3().await;
    if let Err(e) = result {
        println!("Error: {}", e);
    }
}