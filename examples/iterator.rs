//! This example demonstrates probably the most complicated part of
//! `async_minreq`. Useful when making loading bars, for example.

use tokio_stream::StreamExt;

#[tokio::main]
async fn main() -> Result<(), async_minreq::Error> {
    let mut buffer: Vec<u8> = Vec::new();
    let mut response_lazy = async_minreq::get("http://example.com").send_lazy().await?;
    while let Some(chunk) = response_lazy.next().await {
        // The connection could have a problem at any point during the
        // download, so each byte needs to be unwrapped.
        let (bytes, len) = chunk?;

        // The `byte` is the current u8 of data we're iterating
        // through.
        print!("{}", String::from_utf8((bytes).to_vec()).unwrap());

        // The `len` is the expected amount of incoming bytes
        // including the current one: this will be the rest of the
        // body if the server provided a Content-Length header, or
        // just the size of the remaining chunk in chunked transfers.
        buffer.reserve(len);
        buffer.extend(&bytes);

        // Flush the printed text so each char appears on your
        // terminal right away.
        flush();

        // Wait for 50ms so the data doesn't appear instantly fast
        // internet connections, to demonstrate that the body is being
        // printed char-by-char.
        sleep();
    }
    Ok(())
}

// Helper functions

fn flush() {
    use std::io::{stdout, Write};
    stdout().lock().flush().ok();
}

fn sleep() {
    use std::thread::sleep;
    use std::time::Duration;

    sleep(Duration::from_millis(2));
}
