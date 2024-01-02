use aws_config::{retry::RetryConfig, Region};
use aws_credential_types::provider::{ProvideCredentials, SharedCredentialsProvider};
use aws_sdk_s3::{config::Credentials, Client};
use aws_smithy_runtime::client::http::hyper_014::HyperClientBuilder;
use bytes::Bytes;
use http_body::{Body, SizeHint};
use log::info;
use rustls::{Certificate, RootCertStore};
use rustls_pemfile::certs;
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::BufReader,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

/// result callback
/// - `success` - true if success, false if failed
/// - `message` - error message if failed
pub type ResultCallback = Box<dyn Fn(bool, String) + Send + Sync>;

/// progress callback
/// - `progress` - progress of upload or download, in percentage
pub type ProgressCallback = Arc<Mutex<dyn Fn(f32) + Send + Sync>>;

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
    progress_callback: ProgressCallback,
    last_callback_time: std::time::Instant,
}
impl ProgressTracker {
    fn track(&mut self, len: u64) {
        let mut bytes_written = self.bytes_written.lock().unwrap();
        *bytes_written += len;
        let progress = *bytes_written as f32 / self.content_length as f32 * 100.0;
        let progress_callback = self.progress_callback.lock().unwrap();
        if std::time::Instant::now() - self.last_callback_time
            < std::time::Duration::from_millis(500)
            && progress < 100.0
        {
            return;
        }
        self.last_callback_time = std::time::Instant::now();
        progress_callback(progress);
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
    pub fn new(
        body: InnerBody,
        bytes_written: Arc<Mutex<u64>>,
        content_length: u64,
        progress_callback: ProgressCallback,
    ) -> Self {
        Self {
            inner: body,
            progress_tracker: ProgressTracker {
                bytes_written,
                content_length,
                progress_callback,
                last_callback_time: std::time::Instant::now(),
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
    pub(crate) security_token: String,
    pub(crate) file_path: String,
    pub(crate) region: Option<String>,
    pub(crate) tries: Option<u32>,
    pub(crate) endpoint: Option<String>,
    pub(crate) ca_cert_path: Option<String>,
}

fn load_ca_cert(path: &String) -> Result<RootCertStore, Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut root_store = RootCertStore::empty();
    for cert in certs(&mut reader)? {
        root_store.add(&Certificate(cert))?;
    }
    Ok(root_store)
}

pub fn create_s3_client(params: &S3Params) -> Result<Client, Box<dyn std::error::Error>> {
    let mut region = Region::new("ap-southeast-1");
    if let Some(region_str) = &params.region {
        region = Region::new(region_str.clone());
    }
    let credential = NeS3Credential::new(
        params.access_key_id.clone(),
        params.secret_access_key.clone(),
        params.session_token.clone(),
    );
    let mut builder = aws_config::SdkConfig::builder()
        .region(region)
        .credentials_provider(SharedCredentialsProvider::new(credential))
        // Set max attempts.
        // If tries is 1, there are no retries.
        .retry_config(RetryConfig::standard().with_max_attempts(params.tries.unwrap_or(1)));
    if params.ca_cert_path.is_some() {
        info!(
            "use custom ca certs, path: {}",
            params.ca_cert_path.as_ref().unwrap()
        );
        let root_store = load_ca_cert(params.ca_cert_path.as_ref().unwrap())?;
        let config = rustls::ClientConfig::builder()
            .with_safe_defaults()
            .with_root_certificates(root_store)
            .with_no_client_auth();
        let tls_connector = hyper_rustls::HttpsConnectorBuilder::new()
            .with_tls_config(config)
            .https_only()
            .enable_http1()
            .enable_http2()
            .build();
        let hyper_client = HyperClientBuilder::new().build(tls_connector);
        builder.set_http_client(Some(hyper_client));
    }
    if params.endpoint.as_ref().is_some_and(|url| !url.is_empty()) {
        let endpoint = params.endpoint.as_ref().unwrap().clone();
        builder.set_endpoint_url(Some(endpoint));
    }
    let shared_config = builder.build();
    // Construct an S3 client with customized retry configuration.
    Ok(Client::new(&shared_config))
}
