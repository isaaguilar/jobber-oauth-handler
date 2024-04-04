use aws_sdk_s3 as s3;
use base64::{engine::general_purpose, Engine as _};
use lambda_http::{service_fn, Error, Request, RequestExt, Response};
use reqwest::StatusCode;
use serde::Deserialize;
use std::env;
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

mod awss3;

// Define a simple error type that contains a message.
#[derive(Debug)]
struct SimpleError {
    msg: String,
}

// Implement the Error trait for SimpleError.
impl std::error::Error for SimpleError {}

// Implement the Display trait for SimpleError to display the error message.
impl fmt::Display for SimpleError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.msg)
    }
}

// Function to create a SimpleError from a given message.
fn make_error(msg: &str) -> Box<dyn std::error::Error + Send + Sync> {
    Box::new(SimpleError {
        msg: msg.to_string(),
    })
}

#[derive(Deserialize)]
struct JwtKeyData {
    user_id: u64,
    app_id: String,
    exp: u64,
}

#[derive(Deserialize)]
struct Token {
    access_token: String,
    refresh_token: String,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    lambda_http::run(service_fn(hello)).await?;
    Ok(())
}

async fn hello(request: Request) -> Result<Response<String>, Error> {
    let _context = request.lambda_context_ref();

    println!("Setting up s3 client");
    let config = aws_config::load_from_env().await;
    let client = s3::Client::new(&config);

    println!("Parsing data from url");
    let code = match request
        .query_string_parameters_ref()
        .and_then(|params| params.first("code"))
    {
        Some(s) => s,
        None => return respond_with_message(StatusCode::UNPROCESSABLE_ENTITY, "missing code"),
    };

    let redirect_uri = match request
        .query_string_parameters_ref()
        .and_then(|params| params.first("redirect_uri"))
    {
        Some(s) => s,
        None => {
            return respond_with_message(StatusCode::UNPROCESSABLE_ENTITY, "missing redirect_uri")
        }
    };

    let client_id = match request
        .query_string_parameters_ref()
        .and_then(|params| params.first("client_id"))
    {
        Some(s) => s,
        None => return respond_with_message(StatusCode::UNPROCESSABLE_ENTITY, "missing client_id"),
    };

    println!("Loading environment variables");
    let client_secret = match env::var("CLIENT_SECRET") {
        Ok(s) => s,
        Err(e) => {
            return respond_with_message(
                StatusCode::UNPROCESSABLE_ENTITY,
                &format!("missing CLIENT_SECRET: {}", e.to_string()),
            )
        }
    };
    let bucket = match env::var("S3_BUCKET") {
        Ok(s) => s,
        Err(e) => {
            return respond_with_message(
                StatusCode::UNPROCESSABLE_ENTITY,
                &format!("missing S3_BUCKET: {}", e.to_string()),
            )
        }
    };

    let encoded_key = code.split('.').nth(1).unwrap();
    let decoded_key = match general_purpose::STANDARD_NO_PAD.decode(encoded_key) {
        Ok(s) => s,
        Err(e) => {
            return respond_with_message(
                StatusCode::UNPROCESSABLE_ENTITY,
                &format!("failed to decode authorization code: {}", e.to_string()),
            )
        }
    };
    let value: JwtKeyData = match serde_json::from_slice(decoded_key.as_slice()) {
        Ok(s) => s,
        Err(e) => {
            return respond_with_message(
                StatusCode::UNPROCESSABLE_ENTITY,
                &format!(
                    "failed to parse authorization code into json: {}",
                    e.to_string()
                ),
            )
        }
    };
    let keys = match awss3::list_objects(&client, &bucket).await {
        Ok(s) => s,
        Err(e) => {
            return respond_with_message(
                StatusCode::UNPROCESSABLE_ENTITY,
                &format!("failed to list s3 bucket: {}", e.to_string()),
            )
        }
    };
    let key = &format!("jobber-tokens/{}/{}.json", &value.app_id, &value.user_id);

    let token_data = if keys.contains(key) {
        println!("Found token");
        let data = match awss3::get_object(&client, &bucket, key).await {
            Ok((_, s)) => s,
            Err(e) => {
                return respond_with_message(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    &format!("failed to fetch item from s3: {}", e.to_string()),
                )
            }
        };

        let json_token: Token = match serde_json::from_slice(&data) {
            Ok(s) => s,
            Err(e) => {
                return respond_with_message(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    &format!(
                        "failed to parse token data from s3 object: {}",
                        e.to_string()
                    ),
                )
            }
        };
        let access_token = json_token.access_token;
        let encoded_token = match access_token.split('.').nth(1) {
            Some(s) => s,
            None => {
                return respond_with_message(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "failed to extract the data section from the jwt token",
                )
            }
        };
        let decoded_token = match general_purpose::STANDARD_NO_PAD.decode(encoded_token) {
            Ok(s) => s,
            Err(e) => {
                return respond_with_message(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    &format!(
                        "failed to decode the data section from the jwt token: {}",
                        e.to_string()
                    ),
                )
            }
        };
        let token_value: JwtKeyData = match serde_json::from_slice(decoded_token.as_slice()) {
            Ok(s) => s,
            Err(e) => {
                return respond_with_message(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    &format!(
                        "failed to parse to decoded data section from the jwt token: {}",
                        e.to_string()
                    ),
                )
            }
        };
        if is_expired(token_value.exp - 1800) {
            println!("Token is expired. Using refresh token");
            let request_path = format!(
                "client_id={}&client_secret={}&grant_type=refresh_token&refresh_token={}",
                client_id, &client_secret, json_token.refresh_token
            );
            match request_token(&request_path).await {
                Ok(s) => s,
                Err(_) => {
                    println!("Making a request for a new token");
                    let request_query = format!(
                        "client_id={}&client_secret={}&grant_type=authorization_code&code={}&redirect_uri={}",
                        client_id, client_secret, code, redirect_uri
                    );
                    match request_token(&request_query).await {
                        Ok(s) => s,
                        Err(e) => {
                            return respond_with_message(
                                StatusCode::UNPROCESSABLE_ENTITY,
                                &format!("failed to get new token: {}", e.to_string()),
                            )
                        }
                    }
                }
            }
        } else {
            match String::from_utf8(data) {
                Ok(s) => s,
                Err(e) => {
                    return respond_with_message(
                        StatusCode::UNPROCESSABLE_ENTITY,
                        &format!(
                            "failed to convert s3 object back to string: {}",
                            e.to_string()
                        ),
                    )
                }
            }
        }
    } else {
        println!("Making a request for a new token");
        let request_query = format!(
            "client_id={}&client_secret={}&grant_type=authorization_code&code={}&redirect_uri={}",
            client_id, client_secret, code, redirect_uri
        );
        match request_token(&request_query).await {
            Ok(s) => s,
            Err(e) => {
                return respond_with_message(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    &format!("failed to get new token: {}", e.to_string()),
                )
            }
        }
    };

    println!("Uploading key to s3");
    match awss3::upload_object(&client, &bucket, &token_data, key).await {
        Ok(s) => s,
        Err(e) => {
            return respond_with_message(
                StatusCode::UNPROCESSABLE_ENTITY,
                &format!("failed to upload token data to s3: {}", e.to_string()),
            )
        }
    }

    println!("Finishing up...");
    let resp = Response::builder()
        .status(200)
        .header("content-type", "text/plain")
        .body(String::new())
        .map_err(Box::new)?;
    Ok(resp)
}

fn is_expired(epoch_timestamp: u64) -> bool {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(now) => now.as_secs() > epoch_timestamp,
        Err(_) => false, // An error means the timestamp is in the future
    }
}

async fn request_token(
    request_query: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let request = reqwest::Client::new();
    let response = match request
        .post(&format!(
            "https://api.getjobber.com/api/oauth/token?{}",
            request_query
        ))
        .send()
        .await
    {
        Ok(response) => response,
        Err(e) => {
            return Err(e.into());
        }
    };

    if response.status() != 200 {
        return Err(make_error(&response.text().await.unwrap()));
    }
    Ok(response.text().await.unwrap())
}

fn respond_with_message(status_code: StatusCode, msg: &str) -> Result<Response<String>, Error> {
    let resp = Response::builder()
        .status(status_code)
        .header("content-type", "text/plain")
        .body(msg.to_string())
        .map_err(Box::new)?;
    return Ok(resp);
}
