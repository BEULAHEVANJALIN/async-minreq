extern crate async_minreq;
mod setup;

use self::setup::*;
use std::io;

#[tokio::test]
#[cfg(any(feature = "rustls", feature = "openssl", feature = "native-tls"))]
async fn test_https() {
    // TODO: Implement this locally.
    assert_eq!(
        get_status_code(async_minreq::get("https://example.com").send().await),
        200,
    );
}

#[tokio::test]
#[cfg(feature = "json-using-serde")]
async fn test_json_using_serde() {
    const JSON_SRC: &str = r#"{
        "str": "Json test",
        "num": 42
    }"#;

    let original_json: serde_json::Value = serde_json::from_str(JSON_SRC).unwrap();
    let response = async_minreq::post(url("/echo"))
        .with_json(&original_json)
        .unwrap()
        .send()
        .await
        .unwrap();
    let actual_json: serde_json::Value = response.json().unwrap();
    assert_eq!(&actual_json, &original_json);
}

#[tokio::test]
async fn test_timeout_too_low() {
    setup();
    let result = async_minreq::get(url("/slow_a"))
        .with_body("Q".to_string())
        .with_timeout(1)
        .send()
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_timeout_high_enough() {
    setup();
    let body = get_body(
        async_minreq::get(url("/slow_a"))
            .with_body("Q".to_string())
            .with_timeout(3)
            .send()
            .await,
    );
    assert_eq!(body, "j: Q");
}

#[tokio::test]
async fn test_headers() {
    setup();
    let body = get_body(
        async_minreq::get(url("/header_pong"))
            .with_header("Ping", "Qwerty")
            .send()
            .await,
    );
    assert_eq!("Qwerty", body);
}

#[tokio::test]
async fn test_custom_method() {
    use async_minreq::Method;
    setup();
    let body = get_body(
        async_minreq::Request::new(Method::Custom("GET".to_string()), url("/a"))
            .with_body("Q")
            .send()
            .await,
    );
    assert_eq!("j: Q", body);
}

#[tokio::test]
async fn test_get() {
    setup();
    let body = get_body(async_minreq::get(url("/a")).with_body("Q").send().await);
    assert_eq!(body, "j: Q");
}

#[tokio::test]
async fn test_redirect_get() {
    setup();
    let body = get_body(
        async_minreq::get(url("/redirect"))
            .with_body("Q")
            .send()
            .await,
    );
    assert_eq!(body, "j: Q");
}

#[tokio::test]
async fn test_redirect_post() {
    setup();
    // POSTing to /redirect should return a 303, which means we should
    // make a GET request to the given location. This test relies on
    // the fact that the test server only responds to GET requests on
    // the /a path.
    let body = get_body(
        async_minreq::post(url("/redirect"))
            .with_body("Q")
            .send()
            .await,
    );
    assert_eq!(body, "j: Q");
}

#[tokio::test]
async fn test_redirect_with_fragment() {
    setup();
    let original_url = url("/redirect#foo");
    let res = async_minreq::get(original_url).send().await.unwrap();
    // Fragment should stay the same, otherwise redirected
    assert_eq!(res.url.as_str(), url("/a#foo"));
}

#[tokio::test]
async fn test_redirect_with_overridden_fragment() {
    setup();
    let original_url = url("/redirect-baz#foo");
    let res = async_minreq::get(original_url).send().await.unwrap();
    // This redirect should provide its own fragment, overriding the initial one
    assert_eq!(res.url.as_str(), url("/a#baz"));
}

#[tokio::test]
async fn test_infinite_redirect() {
    setup();
    let body = async_minreq::get(url("/infiniteredirect")).send().await;
    assert!(body.is_err());
}

#[tokio::test]
async fn test_relative_redirect_get() {
    setup();
    let body = get_body(
        async_minreq::get(url("/relativeredirect"))
            .with_body("Q")
            .send()
            .await,
    );
    assert_eq!(body, "j: Q");
}

#[tokio::test]
async fn test_head() {
    setup();
    assert_eq!(
        get_status_code(async_minreq::head(url("/b")).send().await),
        418
    );
}

#[tokio::test]
async fn test_post() {
    setup();
    let body = get_body(async_minreq::post(url("/c")).with_body("E").send().await);
    assert_eq!(body, "l: E");
}

#[tokio::test]
async fn test_put() {
    setup();
    let body = get_body(async_minreq::put(url("/d")).with_body("R").send().await);
    assert_eq!(body, "m: R");
}

#[tokio::test]
async fn test_delete() {
    setup();
    assert_eq!(
        get_body(async_minreq::delete(url("/e")).send().await),
        "n: "
    );
}

#[tokio::test]
async fn test_trace() {
    setup();
    assert_eq!(get_body(async_minreq::trace(url("/f")).send().await), "o: ");
}

#[tokio::test]
async fn test_options() {
    setup();
    let body = get_body(async_minreq::options(url("/g")).with_body("U").send().await);
    assert_eq!(body, "p: U");
}

#[tokio::test]
async fn test_connect() {
    setup();
    let body = get_body(async_minreq::connect(url("/h")).with_body("I").send().await);
    assert_eq!(body, "q: I");
}

#[tokio::test]
async fn test_patch() {
    setup();
    let body = get_body(async_minreq::patch(url("/i")).with_body("O").send().await);
    assert_eq!(body, "r: O");
}

#[tokio::test]
async fn tcp_connect_timeout() {
    let _listener = std::net::TcpListener::bind("127.0.0.1:32162").unwrap();
    let resp = async_minreq::Request::new(async_minreq::Method::Get, "http://127.0.0.1:32162")
        .with_timeout(1)
        .send()
        .await;
    assert!(resp.is_err());
    if let Some(async_minreq::Error::IoError(err)) = resp.err() {
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
    } else {
        panic!("timeout test request did not return an error");
    }
}

#[tokio::test]
async fn test_header_cap() {
    setup();
    let body = async_minreq::get(url("/long_header"))
        .with_max_headers_size(999)
        .send()
        .await;
    assert!(body.is_err());
    assert!(matches!(
        body.err(),
        Some(async_minreq::Error::HeadersOverflow)
    ));

    let body = async_minreq::get(url("/long_header"))
        .with_max_headers_size(1500)
        .send()
        .await;
    assert!(body.is_ok());
}

#[tokio::test]
async fn test_status_line_cap() {
    setup();
    let expected_status_line = "HTTP/1.1 203 Non-Authoritative Information";

    let body = async_minreq::get(url("/long_status_line"))
        .with_max_status_line_length(expected_status_line.len() + 1)
        .send()
        .await;
    assert!(body.is_err());
    assert!(matches!(
        body.err(),
        Some(async_minreq::Error::StatusLineOverflow)
    ));

    let body = async_minreq::get(url("/long_status_line"))
        .with_max_status_line_length(expected_status_line.len() + 2)
        .send()
        .await;
    assert!(body.is_ok());
}

#[tokio::test]
async fn test_massive_content_length() {
    use tokio::task;
    use tokio::time::{sleep, Duration};
    setup();
    task::spawn(async {
        // If async_minreq trusts Content-Length, this should crash pretty much straight away.
        let _ = async_minreq::get(url("/massive_content_length"))
            .send()
            .await;
    });

    sleep(Duration::from_millis(500)).await;
    // If it were to crash, it would have at this point. Pass!
}
