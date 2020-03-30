#![feature(proc_macro_hygiene, decl_macro)]

use rocket_contrib::json::Json;
use serde::{Deserialize, Serialize};
#[macro_use]
extern crate rocket;
extern crate chrono;
use base64::encode;
use chrono::{DateTime, Utc};
use reqwest::header::AUTHORIZATION;
use reqwest::header::CONTENT_TYPE;
use reqwest::header::USER_AGENT;
use rocket::response::{self, Response, Responder};
use rocket::http::Status;
use rocket::request::{self, FromRequest, Request};
use rocket::request::LenientForm;
use rocket::Outcome;
use std::env;
use std::fs;

extern crate rocket_multipart_form_data;
use rocket::http::ContentType;
use rocket::Data;
use rocket_multipart_form_data::{
    FileField, MultipartFormData, MultipartFormDataField, MultipartFormDataOptions
};

#[derive(Deserialize, Serialize)]
struct MainEndpointData {
    #[serde(rename(serialize = "media-endpoint"))]
    media_endpoint: String,
}

#[derive(Deserialize, Serialize)]
struct Content {
    message: String,
    content: String,
    path: String,
}

impl Content {
    fn new(message: String, content: String, path: String) -> Content {
        Content {
            message: message,
            content: encode(content),
            path: path,
        }
    }

    fn new_from_image(message: String, content: Vec<u8>, path: String) -> Content {
        Content {
            message: message,
            content: encode(content),
            path: path,
        }
    }
}

#[derive(FromForm)]
struct Post {
    content: String,
}

struct Token(String);

#[derive(Deserialize, Serialize)]
struct CommitResponse {
  content: CommitResponseContent,
}

impl<'r> Responder<'r> for CommitResponse {
  fn respond_to(self, _: &Request) -> response::Result<'r> {
    Response::build()
      .raw_header("Location", self.content.download_url)
      .ok()
  }
}

#[derive(Deserialize, Serialize)]
struct CommitResponseContent {
    download_url: String,
} 

#[derive(Debug)]
enum ApiKeyError {
    BadCount,
    Missing,
    Invalid,
}

impl<'a, 'r> FromRequest<'a, 'r> for Token {
    type Error = ApiKeyError;

    fn from_request(request: &'a Request<'r>) -> request::Outcome<Self, Self::Error> {
        let keys: Vec<_> = request.headers().get("Authorization").collect();
        match keys.len() {
            0 => Outcome::Failure((Status::BadRequest, ApiKeyError::Missing)),
            1 if check_authorization(keys[0].to_string()) => {
                Outcome::Success(Token(keys[0].to_string()))
            }
            1 => Outcome::Failure((Status::BadRequest, ApiKeyError::Invalid)),
            _ => Outcome::Failure((Status::BadRequest, ApiKeyError::BadCount)),
        }
    }
}

#[get("/")]
fn index() -> &'static str {
    "Hello, world!"
}

#[get("/micropub/main")]
fn micropub() -> Json<MainEndpointData> {
    let base_url =
        env::var("BASE_URL").unwrap_or_else(|e| panic!("could not find {}: {}", "BASE_URL", e));

    Json(MainEndpointData {
        media_endpoint: format!("{}/upload/media", base_url),
    })
}

#[post("/upload/media", data = "<data>")]
fn upload_media(content_type: &ContentType, data: Data, _token: Token) -> CommitResponse { 
    let image_path = env::var("IMAGE_UPLOAD_PATH")
        .unwrap_or_else(|e| panic!("could not find {}: {}", "IMAGE_UPLOAD_PATH", e));

    let mut options = MultipartFormDataOptions::new();
    options
        .allowed_fields
        .push(MultipartFormDataField::file("file"));
    let multipart_form_data = MultipartFormData::parse(content_type, data, options).unwrap();

    let photo = multipart_form_data.files.get("file");

    if let Some(photo) = photo {
        match photo {
            FileField::Single(file) => {
                let _content_type = &file.content_type;
                let file_name = &file.file_name;
                let path = &file.path;

                let data = fs::read(path).expect("Unable to read file");

                let content = Content::new_from_image(
                    "Add post".to_string(),
                    data,
                    format!("{}/{}", image_path, file_name.as_ref().unwrap()),
                );

                let response = commit_content(content);
                return response;
            }
            FileField::Multiple(_files) => {
            }
        }
    }
  panic!("No file uploaded")
}

#[post("/micropub/main", data = "<post>")]
fn post(post: LenientForm<Post>, _token: Token) -> String {
    let author_name = env::var("AUTHOR_NAME")
        .unwrap_or_else(|e| panic!("could not find {}: {}", "AUTHOR_NAME", e));
    let now: DateTime<Utc> = Utc::now();
    let filename_date = now.format("%Y-%m-%d-%H-%M");
    let date = now.format("%Y-%m-%d %H:%M");

    let filecontent = format!(
        "---\nauthor: {}\nlayout: status\ndate: {}\n---\n{}",
        author_name, date, post.content
    );

    let content = Content::new(
        "Add post".to_string(),
        filecontent,
        format!("contents/_posts/{}.md", filename_date),
    );

    commit_content(content);
    "posted micropub/main".to_string()
}

fn check_authorization(token: String) -> bool {
    let auth_url = "https://tokens.indieauth.com/token";
    let new_get = reqwest::blocking::Client::new()
        .get(auth_url)
        .header(AUTHORIZATION, token)
        .send()
        .unwrap()
        .text()
        .unwrap();

    let body: Vec<(String, String)> = serde_urlencoded::from_str(&new_get).unwrap();

    let authorized_site = env::var("AUTHORIZED_SITE")
        .unwrap_or_else(|e| panic!("could not find {}: {}", "AUTHORIZED_SITE", e));

    for pair in &body {
        let (key, value) = pair;
        if key == "me" && value == &authorized_site {
            println!("Authorized: {}", value);
            return true;
        } else {
            println!("Not authorized!")
        }
    }

    false
}

fn commit_content(content: Content) -> CommitResponse {
    let repository = env::var("GITHUB_REPOSITORY")
        .unwrap_or_else(|e| panic!("could not find {}: {}", "GITHUB_REPOSITORY", e));

  let post_url = format!(
        "https://api.github.com/repos/{}/contents/{}", repository, 
        content.path
    );
    let token = env::var("GITHUB_ACCESS_TOKEN")
        .unwrap_or_else(|e| panic!("could not find {}: {}", "GITHUB_ACCES_TOKEN", e));
    let new_put = reqwest::blocking::Client::new()
        .put(&post_url)
        .header(USER_AGENT, "pan-rust")
        .header(AUTHORIZATION, format!("Bearer {}", token))
        .header(CONTENT_TYPE, "application/json;charset=UTF-8")
        .body(serde_json::to_string(&content).unwrap())
        .send()
        .unwrap();
        
    let commit_response: CommitResponse = new_put.json().unwrap();
    return commit_response
}

fn main() {
    rocket::ignite()
        .mount("/", routes![index, micropub, post, upload_media])
        .launch();
}
