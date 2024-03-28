use aws_sdk_s3 as s3;
use base64::{engine::general_purpose, Engine as _};
use lambda_http::{service_fn, Error, Request, RequestExt, Response};
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
    let mut config = aws_config::load_from_env().await;
    // aws_config::Region::new("us-west-2");

    let client = s3::Client::new(&config);

    let _context = request.lambda_context_ref();

    let code = request
        .query_string_parameters_ref()
        .and_then(|params| params.first("code"))
        .ok_or(Error::from("missing authorization code"))?;

    let redirect_uri = request
        .query_string_parameters_ref()
        .and_then(|params| params.first("redirect_uri"))
        .ok_or(Error::from("missing redirect_uri"))?;

    let client_id = request
        .query_string_parameters_ref()
        .and_then(|params| params.first("client_id"))
        .ok_or(Error::from("missing client_id"))?;

    let client_secret = env::var("CLIENT_SECRET")?;
    let bucket = env::var("S3_BUCKET")?;

    let encoded_key = code.split('.').nth(1).unwrap();
    let decoded_key = general_purpose::STANDARD.decode(encoded_key).unwrap();

    let value: JwtKeyData = serde_json::from_slice(decoded_key.as_slice()).unwrap();

    let keys = match awss3::list_objects(&client, &bucket).await {
        Ok(list) => list,
        Err(e) => {
            let resp = Response::builder()
                .status(404)
                .header("content-type", "application/json")
                .body(e.to_string())
                .map_err(Box::new)?;
            return Ok(resp);
        }
    };
    let key = &format!("jobber-tokens/{}/{}.json", &value.app_id, &value.user_id);

    let token_data = if keys.contains(key) {
        println!("Found token");
        let data = match awss3::get_object(&client, &bucket, key).await {
            Ok((_size, bytes)) => bytes,
            Err(e) => {
                let resp = Response::builder()
                    .status(404)
                    .header("content-type", "application/json")
                    .body(e.to_string())
                    .map_err(Box::new)?;
                return Ok(resp);
            }
        };

        let json_token: Token = serde_json::from_slice(&data).unwrap();
        let access_token = json_token.access_token;
        let encoded_token = access_token.split('.').nth(1).unwrap();
        let decoded_token = general_purpose::STANDARD.decode(encoded_token).unwrap();
        let token_value: JwtKeyData = serde_json::from_slice(decoded_token.as_slice()).unwrap();
        if is_expired(token_value.exp + 1800) {
            println!("Token is expired. Using refresh token");
            let request_path = format!(
                "client_id={}&client_secret={}&grant_type=refresh_token&refresh_token={}",
                client_id, &client_secret, json_token.refresh_token
            );
            match request_token(&request_path).await {
                Ok(token) => token,
                Err(e) => {
                    let resp = Response::builder()
                        .status(404)
                        .header("content-type", "application/json")
                        .body(e.to_string())
                        .map_err(Box::new)?;
                    return Ok(resp);
                }
            }
        } else {
            String::from_utf8(data).unwrap()
        }
    } else {
        let request_query = format!(
            "client_id={}&client_secret={}&grant_type=authorization_code&code={}&redirect_uri={}",
            client_id, client_secret, code, redirect_uri
        );
        match request_token(&request_query).await {
            Ok(response) => response,
            Err(e) => {
                let resp = Response::builder()
                    .status(404)
                    .header("content-type", "application/json")
                    .body(e.to_string())
                    .map_err(Box::new)?;
                return Ok(resp);
            }
        }
    };

    match awss3::upload_object(&client, &bucket, &token_data, key).await {
        Ok(_) => {}
        Err(e) => {
            let resp = Response::builder()
                .status(404)
                .header("content-type", "application/json")
                .body(e.to_string())
                .map_err(Box::new)?;
            return Ok(resp);
        }
    }

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
            return (Err(e.into()));
        }
    };

    if response.status() != 200 {
        return Err(make_error(&response.text().await.unwrap()));
    }
    Ok(response.text().await.unwrap())
}
