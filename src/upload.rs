use crate::basic;
use log::info;

pub async fn put_object(
    params: &basic::S3Params,
    proress_callback: basic::ProgressCallback,
) -> Result<(), Box<dyn std::error::Error>> {
    let bucket = basic::get_s3_bucket(params)?;
    let mut async_output_file = tokio::fs::File::open(&params.file_path).await?;
    let response = bucket.put_object_stream(&mut async_output_file, &params.object).await?;
    info!("put_object status code: {}", response.status_code());
    Ok(())
}
