extern "C" {
#ifdef _WIN32
#define NE_S3_IMPORT __declspec(dllimport)
#else
#define NE_S3_IMPORT
#endif
/// The callback function when upload/download finished
/// # Arguments
/// - `success` - Whether upload/download success
/// - `message` - The message of upload/download
/// - `user_data` - The user data passed to upload/download function
typedef void (*result_callback)(bool success,
                                const char* message,
                                const char* user_data);

/// The callback function when upload/download progress changed
/// # Arguments
/// - `progress` - The progress of upload/download
/// - `user_data` - The user data passed to upload/download function
typedef void (*progress_callback)(float progress, const char* user_data);

/// init sdk
/// run this function before any other functions
/// # Arguments
/// - `params` - The params of init, in json format
///     - `log_path` - The log path, use stdout if not given
NE_S3_IMPORT void __cdecl ne_s3_init(const char* params);

/// uninit sdk
/// run this function to shutdown
NE_S3_IMPORT void __cdecl ne_s3_uninit();

/// Upload file to s3
/// # Arguments
/// - `params` - The params of upload, json format
///     - `bucket` - The bucket name
///     - `object` - The object key
///     - `access_key_id` - The access key id
///     - `secret_access_key` - The secret access key
///     - `session_token` - The session token
///     - `security_token` - The security token
///     - `file_path` - The file path
///     - `region` - The region
///     - `tries` - The max retry times
///     - `endpoint` - The endpoint, use default if not set
///     - `ca_cert_path` - The ca certs path, use system certs if not set
/// - `result_callback` - The callback function when upload finished
/// - `progress_callback` - The callback function when upload progress changed
/// - `user_data` - The user data, will be passed to callbacks, lifetime must be
///                 longer than callbacks
NE_S3_IMPORT void __cdecl ne_s3_upload(const char* params,
                                       result_callback result,
                                       progress_callback progress,
                                       const char* user_data);

/// download file from s3
/// # Arguments
/// - `params` - The params of download, json format
///     - `bucket` - The bucket name
///     - `object` - The object key
///     - `access_key_id` - The access key id
///     - `secret_access_key` - The secret access key
///     - `session_token` - The session token
///     - `security_token` - The security token
///     - `file_path` - The file path
///     - `region` - The region
///     - `tries` - The max retry times
///     - `endpoint` - The endpoint, use default if not set
///     - `ca_cert_path` - The ca certs path, use system certs if not set
/// - `result_callback` - The callback function when download finished
/// - `user_data` - The user data, will be passed to callbacks, lifetime must be
///                 longer than callbacks
NE_S3_IMPORT void __cdecl ne_s3_download(const char* params,
                                         result_callback result,
                                         const char* user_data);
}