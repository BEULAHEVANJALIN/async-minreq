//! This is a simple example to demonstrate the usage of this library.

#[tokio::main]
async fn main() -> Result<(), async_minreq::Error> {
    let response = async_minreq::get("http://example.com").send().await?;
    let html = response.as_str()?;
    println!("{html}");
    Ok(())
}
