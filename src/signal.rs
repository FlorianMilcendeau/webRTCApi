use anyhow::Result;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Method, Request, Response, Server, StatusCode};
use lazy_static::lazy_static;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

lazy_static! {
    static ref SDP_CHAN_TX_MUTEX: Arc<Mutex<Option<mpsc::Sender<String>>>> =
        Arc::new(Mutex::new(None));
}

async fn remote_handler(req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
    match (req.method(), req.uri().path()) {
        (&Method::POST, "/sdp") => {
            let sdp_str = match std::str::from_utf8(&hyper::body::to_bytes(req.into_body()).await?)
            {
                Ok(s) => s.to_owned(),
                Err(err) => panic!("Error: {}", err),
            };

            {
                let sdp_chan_tx = SDP_CHAN_TX_MUTEX.lock().await;
                if let Some(tx) = &*sdp_chan_tx {
                    let _ = tx.send(sdp_str).await;
                }
            }

            let mut response = Response::new(Body::empty());
            *response.status_mut() = StatusCode::OK;
            Ok(response)
        }
        _ => {
            let mut not_found = Response::default();
            *not_found.status_mut() = StatusCode::NOT_FOUND;
            Ok(not_found)
        }
    }
}

pub async fn http_sdp_server(port: u16) -> mpsc::Receiver<String> {
    let (sdp_channel_tx, sdp_channel_rx) = mpsc::channel::<String>(1);
    {
        let mut tx = SDP_CHAN_TX_MUTEX.lock().await;
        *tx = Some(sdp_channel_tx);
    }

    tokio::spawn(async move {
        let address = SocketAddr::from_str(&format!("0.0.0.0:{}", port)).unwrap();
        let service =
            make_service_fn(|_| async { Ok::<_, hyper::Error>(service_fn(remote_handler)) });
        let server = Server::bind(&address).serve(service);

        if let Err(err) = server.await {
            eprintln!("Server error: {}", err);
        }
    });

    sdp_channel_rx
}

/// TODO - compress
pub fn encode(s: &str) -> String {
    base64::encode(s)
}

/// TODO - decompress
pub fn decode(s: &str) -> Result<String> {
    let b = base64::decode(s)?;

    Ok(String::from_utf8(b)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_test() {
        let str_encoded = encode("Hello world");
        assert_eq!(str_encoded, "SGVsbG8gd29ybGQ=");
    }

    #[test]
    fn decode_test() {
        let str_decoded = decode("SGVsbG8gd29ybGQ=").unwrap();
        assert_eq!(str_decoded, "Hello world");
    }
}
