mod split;

use bytes::Bytes;
use futures::{Stream, TryStreamExt};
use rusoto_core::{ByteStream, RusotoError};
use rusoto_s3::{
    CompleteMultipartUploadError, CompleteMultipartUploadOutput, CompleteMultipartUploadRequest,
    CompletedMultipartUpload, CompletedPart, CreateMultipartUploadError,
    CreateMultipartUploadRequest, UploadPartError, UploadPartRequest, S3,
};
use std::ops::RangeInclusive;

// https://docs.aws.amazon.com/AmazonS3/latest/userguide/qfacts.html
pub const PART_SIZE: RangeInclusive<usize> = 5 << 20..=5 << 30;

pub struct MultipartUploadRequest<B, E>
where
    B: Stream<Item = Result<Bytes, E>>,
{
    pub body: B,
    pub bucket: String,
    pub key: String,
}

pub type MultipartUploadOutput = CompleteMultipartUploadOutput;

pub async fn multipart_upload<C, B, E>(
    client: &C,
    input: MultipartUploadRequest<B, E>,
    part_size: RangeInclusive<usize>,
) -> Result<MultipartUploadOutput, E>
where
    C: S3,
    B: Stream<Item = Result<Bytes, E>>,
    E: From<RusotoError<CreateMultipartUploadError>>
        + From<RusotoError<UploadPartError>>
        + From<RusotoError<CompleteMultipartUploadError>>,
{
    let MultipartUploadRequest { body, bucket, key } = input;

    let upload_id = client
        .create_multipart_upload(CreateMultipartUploadRequest {
            bucket: bucket.clone(),
            key: key.clone(),
            ..CreateMultipartUploadRequest::default()
        })
        .await?
        .upload_id
        .unwrap();

    let parts = split::split(body, part_size)
        .and_then(|part| async {
            let e_tag = client
                .upload_part(UploadPartRequest {
                    body: Some(ByteStream::new(futures::stream::iter(
                        part.body.into_iter().map(Ok),
                    ))),
                    bucket: bucket.clone(),
                    content_length: Some(part.content_length as _),
                    content_md5: Some(base64::encode(part.content_md5)),
                    key: key.clone(),
                    part_number: part.part_number as _,
                    upload_id: upload_id.clone(),
                    ..UploadPartRequest::default()
                })
                .await?
                .e_tag;
            Ok(CompletedPart {
                e_tag,
                part_number: Some(part.part_number as _),
            })
        })
        .try_collect()
        .await?;

    Ok(client
        .complete_multipart_upload(CompleteMultipartUploadRequest {
            bucket,
            key,
            multipart_upload: Some(CompletedMultipartUpload { parts: Some(parts) }),
            upload_id,
            ..CompleteMultipartUploadRequest::default()
        })
        .await?)
}

#[cfg(test)]
mod tests;
