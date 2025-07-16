//! This example demonstrates the `json-using-serde` feature.

#[tokio::main]
async fn main() -> Result<(), async_minreq::Error> {
    let response = async_minreq::get("http://httpbin.org/anything")
        .with_body("Hello, world!")
        .send()
        .await?;

    // httpbin.org/anything returns the body in the json field "data":
    let json: serde_json::Value = response.json()?;
    println!("\"Hello, world!\" == {}", json["data"]);

    Ok(())
}
