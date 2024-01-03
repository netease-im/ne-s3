use crate::basic;
use log::info;

pub async fn get_object(params: &basic::S3Params) -> Result<u64, Box<dyn std::error::Error>> {
    let bucket = basic::get_s3_bucket(params)?;
    let mut async_output_file = tokio::fs::File::create(&params.file_path).await?;
    let status_code = bucket.get_object_to_writer(&params.object, &mut async_output_file).await?;
    info!("get_object status code: {}", status_code);
    Ok(0)
}
