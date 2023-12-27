use crate::basic;
use aws_sdk_s3::{
    operation::create_multipart_upload::CreateMultipartUploadOutput,
    primitives::{ByteStream, SdkBody},
    types::{CompletedMultipartUpload, CompletedPart},
};
use aws_smithy_runtime_api::http::Request;
use aws_smithy_types::byte_stream::Length;
use log::info;
use std::{
    convert::Infallible,
    path::Path,
    sync::{Arc, Mutex},
};

const DEFAULT_CHUNK_SIZE: u64 = 1024 * 1024 * 5;
const MAX_CHUNK_COUNT: u64 = 10000;

#[derive(Debug)]
struct MultiUploadChunkInfo {
    chunk_count: u64,
    chunk_size: u64,
    size_of_last_chunk: u64,
}

fn get_multiupload_chunk_info(
    file_size: u64,
    default_chunk_size: u64,
    max_chunk_count: u64,
) -> MultiUploadChunkInfo {
    let mut chunk_size = default_chunk_size;
    if file_size > default_chunk_size * max_chunk_count {
        chunk_size = file_size / (max_chunk_count - 1).max(1);
    }
    let mut size_of_last_chunk = file_size % chunk_size;
    let mut chunk_count = file_size / chunk_size + 1;
    if size_of_last_chunk == 0 {
        size_of_last_chunk = chunk_size;
        chunk_count = (chunk_count - 1).max(1)
    }
    MultiUploadChunkInfo {
        chunk_count,
        chunk_size,
        size_of_last_chunk,
    }
}

pub async fn put_object(
    params: &basic::S3Params,
    proress_callback: basic::ProgressCallback,
) -> Result<u64, Box<dyn std::error::Error>> {
    let client = basic::create_s3_client(&params)?;
    let mut metadata_header = std::collections::HashMap::new();
    metadata_header.insert(
        "x-amz-meta-token".to_string(),
        params.security_token.clone(),
    );
    let multipart_upload_res: CreateMultipartUploadOutput = client
        .create_multipart_upload()
        .bucket(&params.bucket)
        .key(&params.object)
        .set_metadata(Some(metadata_header))
        .send()
        .await?;
    let upload_id = multipart_upload_res.upload_id();
    if upload_id.is_none() {
        return Err("upload_id is none".into());
    }
    let file_path = Path::new(&params.file_path);
    let upload_id = upload_id.unwrap();
    let file_size = tokio::fs::metadata(file_path).await?.len();
    if file_size == 0 {
        return Err("file size is 0".into());
    }
    let upload_chunk_info =
        get_multiupload_chunk_info(file_size, DEFAULT_CHUNK_SIZE, MAX_CHUNK_COUNT);
    info!("upload_chunk_info: {:?}", upload_chunk_info);
    let mut upload_parts = Vec::new();
    let uploaded_size = Arc::new(Mutex::new(0_u64));
    for chunk_index in 0..upload_chunk_info.chunk_count {
        let this_chunk = if upload_chunk_info.chunk_count - 1 == chunk_index {
            upload_chunk_info.size_of_last_chunk
        } else {
            upload_chunk_info.chunk_size
        };
        let stream = ByteStream::read_from()
            .path(file_path)
            .offset(chunk_index * upload_chunk_info.chunk_size)
            .length(Length::Exact(this_chunk))
            .build()
            .await?;
        // chunk index needs to start at 0, but part numbers start at 1.
        // chunk_count is less than MAX_CHUNKS, so this can't overflow.
        let part_number = (chunk_index as i32) + 1;
        let uploaded_size = uploaded_size.clone();
        let proress_callback = proress_callback.clone();
        let customized = client
            .upload_part()
            .key(&params.object)
            .bucket(&params.bucket)
            .upload_id(upload_id)
            .body(stream)
            .part_number(part_number)
            .customize()
            .map_request(
                move |value: Request<SdkBody>| -> Result<Request<SdkBody>, Infallible> {
                    let uploaded_size = uploaded_size.clone();
                    let proress_callback = proress_callback.clone();
                    let value = value.map(move |body| {
                        let body = basic::ProgressBody::new(
                            body,
                            uploaded_size.clone(),
                            file_size,
                            proress_callback.clone(),
                        );
                        SdkBody::from_body_0_4(body)
                    });
                    Ok(value)
                },
            );
        let upload_part = tokio::task::spawn(async move { customized.send().await });
        upload_parts.push((upload_part, part_number));
    }
    let mut upload_part_res_vec: Vec<CompletedPart> = Vec::new();
    for (handle, part_number) in upload_parts {
        let upload_part_res = handle.await??;
        upload_part_res_vec.push(
            CompletedPart::builder()
                .e_tag(upload_part_res.e_tag.unwrap_or_default())
                .part_number(part_number)
                .build(),
        );
    }
    info!("upload_parts finished");
    let completed_multipart_upload: CompletedMultipartUpload = CompletedMultipartUpload::builder()
        .set_parts(Some(upload_part_res_vec))
        .build();
    client
        .complete_multipart_upload()
        .bucket(&params.bucket)
        .key(&params.object)
        .multipart_upload(completed_multipart_upload)
        .upload_id(upload_id)
        .send()
        .await?;
    Ok(file_size)
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_get_multiupload_chunk_info() {
        use super::get_multiupload_chunk_info;
        let mut chunk_info = get_multiupload_chunk_info(10, 2, 5);
        assert_eq!(chunk_info.chunk_count, 5);
        assert_eq!(chunk_info.chunk_size, 2);
        assert_eq!(chunk_info.size_of_last_chunk, 2);
        chunk_info = get_multiupload_chunk_info(10, 2, 4);
        assert_eq!(chunk_info.chunk_count, 4);
        assert_eq!(chunk_info.chunk_size, 3);
        assert_eq!(chunk_info.size_of_last_chunk, 1);
        chunk_info = get_multiupload_chunk_info(10, 2, 1);
        assert_eq!(chunk_info.chunk_count, 1);
        assert_eq!(chunk_info.chunk_size, 10);
        assert_eq!(chunk_info.size_of_last_chunk, 10);
    }
}
