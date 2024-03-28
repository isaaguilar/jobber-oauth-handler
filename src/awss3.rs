use aws_sdk_s3 as s3;
use aws_sdk_s3::Client;
use lambda_http::Error;
use s3::{error::SdkError, operation::put_object::PutObjectError, primitives::ByteStream};

pub async fn list_objects(client: &Client, bucket: &str) -> Result<Vec<String>, Error> {
    let mut response = client
        .list_objects_v2()
        .bucket(bucket.to_owned())
        .prefix("jobber-tokens/")
        .max_keys(10) // In this example, go 10 at a time.
        .into_paginator()
        .send();

    let mut keys: Vec<String> = vec![];
    while let Some(result) = response.next().await {
        match result {
            Ok(output) => {
                let mut data = output
                    .contents()
                    .into_iter()
                    .map(|item| item.key().unwrap_or("Unknown").to_string())
                    .collect::<Vec<_>>();

                keys.append(&mut data);
            }
            Err(err) => {
                return Err(err.into());
            }
        }
    }

    Ok(keys)
}

pub async fn upload_object(
    client: &Client,
    bucket_name: &str,
    data: &str,
    key: &str,
) -> Result<(), SdkError<PutObjectError>> {
    let bs = ByteStream::from(data.as_bytes().to_vec());
    client
        .put_object()
        .bucket(bucket_name)
        .key(key)
        .body(bs)
        .send()
        .await?;

    Ok(())
}

pub async fn get_object(
    client: &Client,
    bucket: &str,
    key: &str,
) -> Result<(usize, Vec<u8>), Box<dyn std::error::Error + Send + Sync>> {
    let mut object = client.get_object().bucket(bucket).key(key).send().await?;

    let mut buf = vec![];
    let mut byte_count = 0_usize;
    while let Some(bytes) = object.body.try_next().await? {
        let bytes_len = bytes.len();
        buf.extend_from_slice(&bytes);
        byte_count += bytes_len;
    }

    Ok((byte_count, buf))
}
