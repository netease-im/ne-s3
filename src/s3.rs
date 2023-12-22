use aws_config::{retry::RetryConfig, Region};
use aws_credential_types::provider::{ProvideCredentials, SharedCredentialsProvider};
use aws_sdk_s3::{
    config::Credentials,
    operation::{
        create_multipart_upload::CreateMultipartUploadOutput, upload_part::UploadPartOutput,
    },
    primitives::{ByteStream, SdkBody},
    types::{CompletedMultipartUpload, CompletedPart},
    Client,
};
use aws_smithy_runtime_api::http::Request;
use aws_smithy_types::byte_stream::Length;
use bytes::Bytes;
use http_body::{Body, SizeHint};
use serde::{Deserialize, Serialize};
use std::{
    convert::Infallible,
    fs::File,
    io::Write,
    path::Path,
    pin::Pin,
    sync::{Mutex, Arc},
    task::{Context, Poll},
};
use tokio::task::JoinHandle;

const DEFAULT_CHUNK_SIZE: u64 = 1024 * 1024 * 5;
const MAX_CHUNK_COUNT: u64 = 10000;

#[derive(Debug)]
pub struct NeS3Credential {
    access_key_id: String,
    secret_access_key: String,
    session_token: String,
}

impl NeS3Credential {
    pub fn new(access_key_id: String, secret_access_key: String, session_token: String) -> Self {
        Self {
            access_key_id,
            secret_access_key,
            session_token,
        }
    }

    async fn load_credentials(&self) -> aws_credential_types::provider::Result {
        Ok(Credentials::new(
            self.access_key_id.clone(),
            self.secret_access_key.clone(),
            Some(self.session_token.clone()),
            None,
            "NeS3Credential",
        ))
    }
}

impl ProvideCredentials for NeS3Credential {
    fn provide_credentials<'a>(
        &'a self,
    ) -> aws_credential_types::provider::future::ProvideCredentials<'a>
    where
        Self: 'a,
    {
        aws_credential_types::provider::future::ProvideCredentials::new(self.load_credentials())
    }
}

// ProgressTracker prints information as the upload progresses.
struct ProgressTracker {
    bytes_written: Arc<Mutex<u64>>,
    content_length: u64,
}
impl ProgressTracker {
    fn track(&mut self, len: u64) {
        let mut bytes_written = self.bytes_written.lock().unwrap();
        *bytes_written += len;
        let progress = *bytes_written as f64 / self.content_length as f64;
        println!("Read {} bytes, progress: {:.2}%", len, progress * 100.0);
    }
}

// snippet-start:[s3.rust.put-object-progress-body]
// A ProgressBody to wrap any http::Body with upload progress information.
#[pin_project::pin_project]
pub struct ProgressBody<InnerBody> {
    #[pin]
    inner: InnerBody,
    // prograss_tracker is a separate field so it can be accessed as &mut.
    progress_tracker: ProgressTracker,
}

impl<InnerBody> ProgressBody<InnerBody>
where
    InnerBody: Body<Data = Bytes, Error = aws_smithy_types::body::Error>,
{
    pub fn new(body: InnerBody, bytes_written: Arc<Mutex<u64>>, content_length: u64) -> Self {
        Self {
            inner: body,
            progress_tracker: ProgressTracker {
                bytes_written,
                content_length,
            },
        }
    }
}

impl<InnerBody> Body for ProgressBody<InnerBody>
where
    InnerBody: Body<Data = Bytes, Error = aws_smithy_types::body::Error>,
{
    type Data = Bytes;

    type Error = aws_smithy_types::body::Error;

    // Our poll_data delegates to the inner poll_data, but needs a project() to
    // get there. When the poll has data, it updates the progress_tracker.
    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        let this = self.project();
        match this.inner.poll_data(cx) {
            Poll::Ready(Some(Ok(data))) => {
                this.progress_tracker.track(data.len() as u64);
                Poll::Ready(Some(Ok(data)))
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Pending => Poll::Pending,
        }
    }

    // Delegate utilities to inner and progress_tracker.
    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<http::HeaderMap>, Self::Error>> {
        self.project().inner.poll_trailers(cx)
    }

    fn size_hint(&self) -> http_body::SizeHint {
        SizeHint::with_exact(self.progress_tracker.content_length)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct S3Params {
    pub(crate) bucket: String,
    pub(crate) object: String,
    pub(crate) access_key_id: String,
    pub(crate) secret_access_key: String,
    pub(crate) session_token: String,
    pub(crate) file_path: String,
    pub(crate) security_token: String,
    pub(crate) region: Option<String>,
    pub(crate) tries: Option<u32>,
    pub(crate) endpoint: Option<String>,
}

fn create_s3_client(params: &S3Params) -> Client {
    let mut region = Region::new("ap-southeast-1");
    if let Some(region_str) = &params.region {
        region = Region::new(region_str.clone());
    }
    let credential = NeS3Credential::new(
        params.access_key_id.clone(),
        params.secret_access_key.clone(),
        params.session_token.clone(),
    );
    let shared_config = aws_config::SdkConfig::builder()
        .region(region)
        .credentials_provider(SharedCredentialsProvider::new(credential))
        // Set max attempts.
        // If tries is 1, there are no retries.
        .retry_config(RetryConfig::standard().with_max_attempts(params.tries.unwrap_or(1)))
        .build();
    // Construct an S3 client with customized retry configuration.
    Client::new(&shared_config)
}

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

pub async fn upload_object(params: &S3Params) -> Result<u64, Box<dyn std::error::Error>> {
    let client = create_s3_client(&params);
    let multipart_upload_res: CreateMultipartUploadOutput = client
        .create_multipart_upload()
        .bucket(&params.bucket)
        .key(&params.object)
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
    println!("upload_chunk_info: {:?}", upload_chunk_info);
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
                    let value = value.map(move |body| {
                        let body = ProgressBody::new(body, uploaded_size.clone(), file_size);
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
    println!("upload_parts finished");
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

pub async fn download_object(params: &S3Params) -> Result<u64, Box<dyn std::error::Error>> {
    let client = create_s3_client(&params);
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
