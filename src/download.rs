use crate::basic;
use std::{fs::File, io::Write};

pub async fn get_object(params: &basic::S3Params) -> Result<u64, Box<dyn std::error::Error>> {
    let client = basic::create_s3_client(&params);
    let mut file = File::create(&params.file_path)?;
    let mut resp = client
        .get_object()
        .bucket(&params.bucket)
        .key(&params.object)
        .send()
        .await?;
    let mut byte_count = 0_u64;
    while let Some(bytes) = resp.body.try_next().await? {
        let bytes_len = bytes.len();
        file.write_all(&bytes)?;
        byte_count += bytes_len as u64;
    }
    Ok(byte_count)
}
