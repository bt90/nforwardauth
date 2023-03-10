use std::net::SocketAddr;
use std::collections::HashSet;
use std::cell::RefCell;
use bytes::{Buf, Bytes};
use http_body_util::{BodyExt, Full};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{body::Incoming as IncomingBody, Request, Response};
use hyper::{Method, StatusCode};
use tokio::net::TcpListener;
use cookie::{Cookie, SameSite};
use secrets::Secret;

type GenericError = Box<dyn std::error::Error + Send + Sync>;
type Result<T> = std::result::Result<T, GenericError>;
type BoxBody = http_body_util::combinators::BoxBody<Bytes, hyper::Error>;

static INDEX_DOCUMENT: &str = "public/index.html";
static INDEX_SCRIPT: &str = "public/script.js";
static NOT_FOUND: &[u8] = b"Not Found";
static UNAUTHORIZED: &[u8] = b"Unauthorized";
static AUTHORIZED: &[u8] = b"Authorized";

// Initialize token bucket
// thread_local!(static TOKEN_BUCKET: HashSet<String>= HashSet::new());
thread_local!(static TOKEN_BUCKET: RefCell<HashSet<String>> = RefCell::new(HashSet::new()));

async fn api(req: Request<hyper::body::Incoming>) -> Result<Response<BoxBody>> {
    match (req.method(), req.uri().path()) {
        (&Method::GET, "/") | (&Method::GET, "/index.html") => serve_file(INDEX_DOCUMENT).await,
        (&Method::GET, "/script.js") => serve_file(INDEX_SCRIPT).await,
        (&Method::POST, "/login") => api_login(req).await,
        (&Method::GET, "/forward") => api_forward_auth(req).await,
        _ => {
            // 404, not found
            Ok(Response::builder()
               .status(StatusCode::NOT_FOUND)
               .body(full(NOT_FOUND))
               .unwrap())
        }
    }
}

// ForwardAuth route
async fn api_forward_auth(req: Request<IncomingBody>) -> Result<Response<BoxBody>> {
    // Get cookie from request headers
    let headers = req.headers();
    let cookies = headers["cookie"].to_str().unwrap();
    // Check if valid cookie exists
    let mut found = false;
    for cookie in Cookie::split_parse(cookies) {
        let cookie = cookie.unwrap();
        TOKEN_BUCKET.with(|token_bucket| {
            if token_bucket.borrow().contains(cookie.value()) {
                // If valid cookie has been found return OK
                found = true;
            }
        });
        if found {
            return Ok(Response::builder()
               .status(StatusCode::OK)
               .body(full(AUTHORIZED))
               .unwrap());
        }
    }

    // No valid cookie found, return unauthorized
    Ok(Response::builder()
       .status(StatusCode::UNAUTHORIZED)
       .body(full(UNAUTHORIZED))
       .unwrap())
}

// Login route
async fn api_login(req: Request<IncomingBody>) -> Result<Response<BoxBody>> {
    // Aggregate request body
    let body = req.collect().await?.aggregate();
    // Decode JSON
    let data: serde_json::Value = serde_json::from_reader(body.reader())?;

    // Process login
    if data["username"] == "test" && data["password"] == "test" {
        // Correct login, respond with OK
        let jwt = "12hd1928hd28d";
        // let cookie = format!("{}={}; SameSite={}; {};", "simple-forward-auth", jwt, "Strict", "HttpOnly");
        let cookie = Cookie::build("simple-forward-auth", jwt)
            .domain("tux.tonydu.co")
            .http_only(true)
            .same_site(SameSite::Strict)
            .finish();
        Ok(Response::builder()
           .status(StatusCode::OK)
           .header(hyper::header::SET_COOKIE, &cookie.to_string())
           .body(full(AUTHORIZED))
           .unwrap())
    } else {
        // Incorrect login, respond with unauthorized
        Ok(Response::builder()
           .status(StatusCode::UNAUTHORIZED)
           .body(full(UNAUTHORIZED))
           .unwrap())
    }
}

// Serve file route
async fn serve_file(filename: &str) -> Result<Response<BoxBody>> {
    if let Ok(contents) = tokio::fs::read(filename).await {
        let body = contents.into();
        return Ok(Response::new(Full::new(body).map_err(|never| match never {}).boxed()));
    }

    // 404, not found
    Ok(Response::builder()
       .status(StatusCode::NOT_FOUND)
       .body(full(NOT_FOUND))
       .unwrap())
}

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    // Setup secret and token bucket
    TOKEN_BUCKET = HashSet::new();
    // Create TcpListener and bind to 127.0.0.1:3000
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    let listener = TcpListener::bind(addr).await?;
    println!("Listening on http://{}", addr);

    // Start loop to continuously accept incoming connections
    loop {
        let (stream, _) = listener.accept().await?;

        // Spawn a tokio task to serve multiple connections concurrently
        tokio::task::spawn(async move {
            // Finally, bind the incoming connection to our index service
            if let Err(err) = http1::Builder::new()
                // Convert function to service
                .serve_connection(stream, service_fn(api))
                .await
            {
                println!("Error serving connection: {:?}", err);
            }
        });
    }
}

// Helper function to convert full to BoxBody
fn full<T: Into<Bytes>>(chunk: T) -> BoxBody {
    Full::new(chunk.into())
        .map_err(|never| match never {})
        .boxed()
}