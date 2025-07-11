use crate::{connection::HttpStream, Error};
use bytes::Bytes;
use futures_core::Stream;
use pin_project::pin_project;
use std::collections::HashMap;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::{str, usize};
use tokio::io::{self, AsyncRead, BufReader};
use tokio_stream::StreamExt;
use tokio_util::io::ReaderStream;

const BACKING_READ_BUFFER_LENGTH: usize = 16 * 1024;
const MAX_CONTENT_LENGTH: usize = 16 * 1024;

/// An HTTP response.
///
/// Returned by [`Request::send`](struct.Request.html#method.send).
///
/// # Example
///
/// ```no_run
/// # async fn run() -> Result<(), async_minreq::Error> {
/// let response = async_minreq::get("http://example.com").send().await?;
/// println!("{}", response.as_str()?);
/// # Ok(()) }
/// ```
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Response {
    /// The status code of the response, eg. 404.
    pub status_code: i32,
    /// The reason phrase of the response, eg. "Not Found".
    pub reason_phrase: String,
    /// The headers of the response. The header field names (the
    /// keys) are all lowercase.
    pub headers: HashMap<String, String>,
    /// The URL of the resource returned in this response. May differ from the
    /// request URL if it was redirected or typo corrections were applied (e.g.
    /// <http://example.com?foo=bar> would be corrected to
    /// <http://example.com/?foo=bar>).
    pub url: String,

    body: Vec<u8>,
}

impl Response {
    pub(crate) async fn create(mut parent: ResponseLazy, is_head: bool) -> Result<Response, Error> {
        let mut body = Vec::new();
        if !is_head && parent.status_code != 204 && parent.status_code != 304 {
            while let Some(chunk) = parent.next().await {
                let (bytes, length) = chunk?;
                body.reserve(length);
                body.extend_from_slice(&bytes);
            }
        }

        let ResponseLazy {
            status_code,
            reason_phrase,
            headers,
            url,
            ..
        } = parent;

        Ok(Response {
            status_code,
            reason_phrase,
            headers,
            url,
            body,
        })
    }

    /// Returns the body as an `&str`.
    ///
    /// # Errors
    ///
    /// Returns
    /// [`InvalidUtf8InBody`](enum.Error.html#variant.InvalidUtf8InBody)
    /// if the body is not UTF-8, with a description as to why the
    /// provided slice is not UTF-8.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # async fn run() -> Result<(), Box<dyn std::error::Error>> {
    /// # let url = "http://example.org/";
    /// let response = async_minreq::get(url).send().await?;
    /// println!("{}", response.as_str()?);
    /// # Ok(())
    /// # }
    /// ```
    pub fn as_str(&self) -> Result<&str, Error> {
        match str::from_utf8(&self.body) {
            Ok(s) => Ok(s),
            Err(err) => Err(Error::InvalidUtf8InBody(err)),
        }
    }

    /// Returns a reference to the contained bytes of the body. If you
    /// want the `Vec<u8>` itself, use
    /// [`into_bytes()`](#method.into_bytes) instead.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # async fn run() -> Result<(), Box<dyn std::error::Error>> {
    /// # let url = "http://example.org/";
    /// let response = async_minreq::get(url).send().await?;
    /// println!("{:?}", response.as_bytes());
    /// # Ok(())
    /// # }
    /// ```
    pub fn as_bytes(&self) -> &[u8] {
        &self.body
    }

    /// Turns the `Response` into the inner `Vec<u8>`, the bytes that
    /// make up the response's body. If you just need a `&[u8]`, use
    /// [`as_bytes()`](#method.as_bytes) instead.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # async fn run() -> Result<(), Box<dyn std::error::Error>> {
    /// # let url = "http://example.org/";
    /// let response = async_minreq::get(url).send().await?;
    /// println!("{:?}", response.into_bytes());
    /// // This would error, as into_bytes consumes the Response:
    /// // let x = response.status_code;
    /// # Ok(())
    /// # }
    /// ```
    pub fn into_bytes(self) -> Vec<u8> {
        self.body
    }

    /// Converts JSON body to a `struct` using Serde.
    ///
    /// # Errors
    ///
    /// Returns
    /// [`SerdeJsonError`](enum.Error.html#variant.SerdeJsonError) if
    /// Serde runs into a problem, or
    /// [`InvalidUtf8InBody`](enum.Error.html#variant.InvalidUtf8InBody)
    /// if the body is not UTF-8.
    ///
    /// # Example
    /// In case compiler cannot figure out return type you might need to declare it explicitly:
    ///
    /// ```no_run
    /// use serde_json::Value;
    ///
    /// # fn main() -> Result<(), async_minreq::Error> {
    /// # let url_to_json_resource = "http://example.org/resource.json";
    /// // Value could be any type that implements Deserialize!
    /// let user = async_minreq::get(url_to_json_resource).send()?.json::<Value>()?;
    /// println!("User name is '{}'", user["name"]);
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(feature = "json-using-serde")]
    pub fn json<'a, T>(&'a self) -> Result<T, Error>
    where
        T: serde::de::Deserialize<'a>,
    {
        let str = match self.as_str() {
            Ok(str) => str,
            Err(_) => return Err(Error::InvalidUtf8InResponse),
        };
        match serde_json::from_str(str) {
            Ok(json) => Ok(json),
            Err(err) => Err(Error::SerdeJsonError(err)),
        }
    }
}

/// An HTTP response, which is loaded lazily.
///
/// In comparison to [`Response`](struct.Response.html), this is
/// returned from
/// [`send_lazy()`](struct.Request.html#method.send_lazy), where as
/// [`Response`](struct.Response.html) is returned from
/// [`send()`](struct.Request.html#method.send).
///
/// In practice, "lazy loading" means that the bytes are only loaded
/// as you iterate through them. The bytes are provided in the form of
/// a `Result<(u8, usize), async_minreq::Error>`, as the reading operation
/// can fail in various ways. The `u8` is the actual byte that was
/// read, and `usize` is how many bytes we are expecting to read in
/// the future (including this byte). Note, however, that the `usize`
/// can change, particularly when the `Transfer-Encoding` is
/// `chunked`: then it will reflect how many bytes are left of the
/// current chunk. The expected size is capped at 16 KiB to avoid
/// server-side DoS attacks targeted at clients accidentally reserving
/// too much memory.
///
/// # Example
/// ```no_run
/// // This is how the normal Response works behind the scenes, and
/// // how you might use ResponseLazy.
/// # async fn run() -> Result<(), async_minreq::Error> {
/// let mut response = async_minreq::get("http://example.com").send_lazy().await?;
/// let mut vec = Vec::new();
/// use tokio_stream::StreamExt;
/// while let Some(result) = response.next().await {
///     let (chunk, length) = result?;
///     vec.reserve(length);
///     vec.extend(chunk);
/// }
/// # Ok(())
/// # }
///
/// ```
#[pin_project]
pub struct ResponseLazy {
    /// The status code of the response, eg. 404.
    pub status_code: i32,
    /// The reason phrase of the response, eg. "Not Found".
    pub reason_phrase: String,
    /// The headers of the response. The header field names (the
    /// keys) are all lowercase.
    pub headers: HashMap<String, String>,
    /// The URL of the resource returned in this response. May differ from the
    /// request URL if it was redirected or typo corrections were applied (e.g.
    /// <http://example.com?foo=bar> would be corrected to
    /// <http://example.com/?foo=bar>).
    pub url: String,

    #[pin]
    stream: HttpStreamBytes,
    state: HttpStreamState,
    max_trailing_headers_size: Option<usize>,
    trailing_content: Option<Trailing>,
}

type HttpStreamBytes = ReaderStream<BufReader<HttpStream>>;

impl ResponseLazy {
    pub(crate) async fn from_stream(
        stream: HttpStream,
        max_headers_size: Option<usize>,
        max_status_line_len: Option<usize>,
    ) -> Result<ResponseLazy, Error> {
        let mut stream =
            ReaderStream::new(BufReader::with_capacity(BACKING_READ_BUFFER_LENGTH, stream));
        let ResponseMetadata {
            status_code,
            reason_phrase,
            headers,
            state,
            max_trailing_headers_size,
            trailing_content,
        } = read_metadata(&mut stream, max_headers_size, max_status_line_len).await?;

        let trailing_content = if trailing_content.len() == 0 {
            None
        } else {
            Some(trailing_content)
        };

        Ok(ResponseLazy {
            status_code,
            reason_phrase,
            headers,
            url: String::new(),
            stream,
            state,
            max_trailing_headers_size,
            trailing_content,
        })
    }
}

impl Stream for ResponseLazy {
    type Item = Result<(Bytes, usize), Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        use HttpStreamState::*;

        let this = self.project();
        let stream: Pin<&mut HttpStreamBytes> = this.stream;

        let mut prefix = Trailing::new();
        if let Some(trailing) = this.trailing_content {
            prefix = trailing.clone();
            *this.trailing_content = None;
        }

        match this.state {
            EndOnClose => read_until_closed(prefix, stream, cx),
            ContentLength(ref mut length) => {
                let dbg = read_with_content_length(prefix, stream, length, cx);
                dbg
            }
            Chunked(ref mut expecting_chunks, ref mut length, ref mut content_length) => {
                read_chunked(
                    prefix,
                    stream,
                    this.headers,
                    expecting_chunks,
                    length,
                    content_length,
                    *this.max_trailing_headers_size,
                    cx,
                )
            }
        }
    }
}

impl AsyncRead for ResponseLazy {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let this = self.project();
        let stream: Pin<&mut HttpStreamBytes> = this.stream;

        if let Some(prefix) = this.trailing_content {
            buf.put_slice(&prefix);
            *this.trailing_content = None;
        }

        match stream.poll_next(cx) {
            Poll::Ready(Some(Ok(bytes))) => {
                let len = std::cmp::min(buf.remaining(), bytes.len());
                buf.put_slice(&bytes[..len]); // Copy bytes into the buffer
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Some(Err(e))) => Poll::Ready(Err(e)), // Error from the stream
            Poll::Ready(None) => Poll::Ready(Ok(())),         // Stream is closed, return EOF
            Poll::Pending => Poll::Pending,                   // No data available yet
        }
    }
}

fn process_prefix(prefix: &[u8]) -> Poll<Option<<ResponseLazy as Stream>::Item>> {
    Poll::Ready(Some(Ok((Bytes::copy_from_slice(prefix), prefix.len()))))
}

fn read_until_closed(
    prefix: Trailing,
    bytes: Pin<&mut HttpStreamBytes>,
    cx: &mut Context<'_>,
) -> Poll<Option<<ResponseLazy as Stream>::Item>> {
    if prefix.len() > 0 {
        return process_prefix(&prefix);
    }
    match bytes.poll_next(cx) {
        Poll::Ready(some_chunk) => match some_chunk {
            Some(Ok(chunk)) => {
                let len = chunk.len();
                Poll::Ready(Some(Ok((chunk, len))))
            }
            Some(Err(err)) => Poll::Ready(Some(Err(Error::IoError(err)))),
            None => Poll::Ready(None),
        },
        Poll::Pending => Poll::Pending,
    }
}

fn read_with_content_length(
    prefix: Trailing,
    bytes: Pin<&mut HttpStreamBytes>,
    content_length: &mut usize,
    cx: &mut Context<'_>,
) -> Poll<Option<<ResponseLazy as Stream>::Item>> {
    let prefix_len = prefix.len();
    if prefix_len > 0 {
        *content_length -= prefix_len;
        return process_prefix(&prefix);
    }
    if *content_length > 0 {
        match bytes.poll_next(cx) {
            Poll::Ready(some_chunk) => match some_chunk {
                Some(Ok(chunk)) => {
                    let len = chunk.len();
                    *content_length -= len;
                    Poll::Ready(Some(Ok((
                        chunk,
                        (*content_length).min(MAX_CONTENT_LENGTH) + len,
                    ))))
                }
                Some(Err(err)) => Poll::Ready(Some(Err(Error::IoError(err)))),
                None => Poll::Ready(None),
            },
            Poll::Pending => Poll::Pending,
        }
    } else {
        Poll::Ready(None)
    }
}

fn read_trailers(
    mut bytes: Pin<&mut HttpStreamBytes>,
    headers: &mut HashMap<String, String>,
    mut max_headers_size: Option<usize>,
    cx: &mut Context<'_>,
    prefix: Trailing,
) -> Poll<Result<(), Error>> {
    let mut trailing = prefix;
    loop {
        let trailer_line = read_line(
            bytes.as_mut(),
            max_headers_size,
            Error::HeadersOverflow,
            cx,
            trailing,
        )?;
        match trailer_line {
            Poll::Ready((trailer_line, trailing_)) => {
                trailing = trailing_;
                if let Some(ref mut max_headers_size) = max_headers_size {
                    *max_headers_size -= trailer_line.len() + 2;
                }
                if let Some((header, value)) = parse_header(trailer_line) {
                    headers.insert(header, value);
                } else {
                    break;
                }
            }
            Poll::Pending => return Poll::Pending,
        }
    }
    Poll::Ready(Ok(()))
}

fn read_chunked(
    prefix: Trailing,
    mut bytes: Pin<&mut HttpStreamBytes>,
    headers: &mut HashMap<String, String>,
    expecting_more_chunks: &mut bool,
    chunk_length: &mut usize,
    content_length: &mut usize,
    max_trailing_headers_size: Option<usize>,
    cx: &mut Context<'_>,
) -> Poll<Option<<ResponseLazy as Stream>::Item>> {
    if !*expecting_more_chunks && *chunk_length == 0 {
        return Poll::Ready(None);
    }

    if *chunk_length == 0 {
        // Max length of the chunk length line is 1KB: not too long to
        // take up much memory, long enough to tolerate some chunk
        // extensions (which are ignored).

        // Get the size of the next chunk
        let (length_line, trailing) = match read_line(
            bytes.as_mut(),
            Some(1024),
            Error::MalformedChunkLength,
            cx,
            prefix,
        ) {
            Poll::Ready(Ok(line)) => line,
            Poll::Ready(Err(err)) => return Poll::Ready(Some(Err(err))),
            Poll::Pending => return Poll::Pending,
        };

        // Note: the trim() and check for empty lines shouldn't be
        // needed according to the RFC, but we might as well, it's a
        // small change and it fixes a few servers.
        let incoming_length = if length_line.is_empty() {
            0
        } else {
            let length = if let Some(i) = length_line.find(';') {
                length_line[..i].trim()
            } else {
                length_line.trim()
            };
            match usize::from_str_radix(length, 16) {
                Ok(length) => length,
                Err(_) => return Poll::Ready(Some(Err(Error::MalformedChunkLength))),
            }
        };

        if incoming_length == 0 {
            match read_trailers(
                bytes.as_mut(),
                headers,
                max_trailing_headers_size,
                cx,
                trailing,
            ) {
                Poll::Ready(Err(err)) => return Poll::Ready(Some(Err(err))),
                Poll::Pending => return Poll::Pending,
                _ => {}
            }

            *expecting_more_chunks = false;
            headers.insert("content-length".to_string(), (*content_length).to_string());
            headers.remove("transfer-encoding");
            return Poll::Ready(None);
        }
        *chunk_length = incoming_length;
        *content_length += incoming_length;
    }

    if *chunk_length > 0 {
        *chunk_length -= 1;
        match bytes.poll_next(cx) {
            // TODO
            Poll::Ready(some_chunk) => match some_chunk {
                Some(Ok(chunk)) => {
                    let len = chunk.len();
                    Poll::Ready(Some(Ok((chunk, len))))
                }
                Some(Err(err)) => Poll::Ready(Some(Err(Error::IoError(err)))),
                None => Poll::Ready(None),
            },
            Poll::Pending => Poll::Pending,
        }
    } else {
        Poll::Ready(None)
    }
}

enum HttpStreamState {
    // No Content-Length, and Transfer-Encoding != chunked, so we just
    // read unti lthe server closes the connection (this should be the
    // fallback, if I read the rfc right).
    EndOnClose,
    // Content-Length was specified, read that amount of bytes
    ContentLength(usize),
    // Transfer-Encoding == chunked, so we need to save two pieces of
    // information: are we expecting more chunks, how much is there
    // left of the current chunk, and how much have we read? The last
    // number is needed in order to provide an accurate Content-Length
    // header after loading all the bytes.
    Chunked(bool, usize, usize),
}

// This struct is just used in the Response and ResponseLazy
// constructors, but not in their structs, for api-cleanliness
// reasons. (Eg. response.status_code is much cleaner than
// response.meta.status_code or similar.)
struct ResponseMetadata {
    status_code: i32,
    reason_phrase: String,
    headers: HashMap<String, String>,
    state: HttpStreamState,
    max_trailing_headers_size: Option<usize>,
    trailing_content: Trailing,
}

type Trailing = Vec<u8>;

async fn read_line_async(
    stream: &mut HttpStreamBytes,
    max_len: Option<usize>,
    overflow_error: Error,
    prefix: Trailing,
) -> Result<(String, Trailing), Error> {
    let mut bytes = Vec::with_capacity(32);
    let mut new_line_flag = true;
    let mut trailing = Trailing::new();
    let max_len = max_len.unwrap_or(usize::MAX);

    for byte in prefix {
        if new_line_flag {
            if bytes.len() >= max_len {
                return Err(overflow_error);
            }
            if byte == b'\n' {
                if let Some(b'\r') = bytes.last() {
                    bytes.pop();
                }
                new_line_flag = false;
            } else {
                bytes.push(byte);
            }
        } else {
            trailing.push(byte);
        }
    }

    if new_line_flag {
        while let Some(some_chunk) = stream.next().await {
            match some_chunk {
                Ok(chunk) => {
                    for byte in chunk {
                        if new_line_flag {
                            if bytes.len() >= max_len {
                                return Err(overflow_error);
                            }
                            if byte == b'\n' {
                                if let Some(b'\r') = bytes.last() {
                                    bytes.pop();
                                }
                                new_line_flag = false;
                            } else {
                                bytes.push(byte);
                            }
                        } else {
                            trailing.push(byte);
                        }
                    }
                    if !new_line_flag {
                        break;
                    }
                }
                Err(err) => return Err(Error::IoError(err)),
            }
        }
    }

    match String::from_utf8(bytes) {
        Ok(line) => Ok((line, trailing)),
        Err(_) => Err(Error::InvalidUtf8InResponse),
    }
}

async fn read_metadata(
    stream: &mut HttpStreamBytes,
    mut max_headers_size: Option<usize>,
    max_status_line_len: Option<usize>,
) -> Result<ResponseMetadata, Error> {
    let mut trailing = Trailing::new();
    let (line, trailing_) = read_line_async(
        stream,
        max_status_line_len,
        Error::StatusLineOverflow,
        trailing,
    )
    .await?;
    trailing = trailing_;
    let (status_code, reason_phrase) = parse_status_line(&line);

    let mut headers = HashMap::new();
    loop {
        let (line, trailing_) =
            read_line_async(stream, max_headers_size, Error::HeadersOverflow, trailing).await?;
        trailing = trailing_;
        if line.is_empty() {
            // Body starts here
            break;
        }
        if let Some(ref mut max_headers_size) = max_headers_size {
            *max_headers_size -= line.len() + 2;
        }
        if let Some(header) = parse_header(line) {
            headers.insert(header.0, header.1);
        }
    }
    let mut chunked = false;
    let mut content_length = None;
    for (header, value) in &headers {
        // Handle the Transfer-Encoding header
        if header.to_lowercase().trim() == "transfer-encoding"
            && value.to_lowercase().trim() == "chunked"
        {
            chunked = true;
        }

        // Handle the Content-Length header
        if header.to_lowercase().trim() == "content-length" {
            match str::parse::<usize>(value.trim()) {
                Ok(length) => content_length = Some(length),
                Err(_) => return Err(Error::MalformedContentLength),
            }
        }
    }

    let state = if chunked {
        HttpStreamState::Chunked(true, 0, 0)
    } else if let Some(length) = content_length {
        HttpStreamState::ContentLength(length)
    } else {
        HttpStreamState::EndOnClose
    };

    Ok(ResponseMetadata {
        status_code,
        reason_phrase,
        headers,
        state,
        max_trailing_headers_size: max_headers_size,
        trailing_content: trailing,
    })
}

fn read_line(
    mut stream: Pin<&mut HttpStreamBytes>,
    max_len: Option<usize>,
    overflow_error: Error,
    cx: &mut Context<'_>,
    prefix: Trailing,
) -> Poll<Result<(String, Trailing), Error>> {
    let mut bytes = Vec::with_capacity(32);
    let mut new_line_flag = true;
    let mut trailing = Trailing::new();

    for byte in prefix {
        if new_line_flag {
            if byte == b'\n' {
                if let Some(b'\r') = bytes.last() {
                    bytes.pop();
                }
                new_line_flag = false;
            } else {
                bytes.push(byte);
            }
        } else {
            trailing.push(byte);
        }
    }

    if new_line_flag {
        loop {
            match stream.as_mut().poll_next(cx) {
                Poll::Ready(some_chunk) => match some_chunk {
                    Some(Ok(chunk)) => {
                        if let Some(max_len) = max_len {
                            if bytes.len() >= max_len {
                                return Poll::Ready(Err(overflow_error));
                            }
                        }

                        for byte in chunk {
                            if new_line_flag {
                                if byte == b'\n' {
                                    if let Some(b'\r') = bytes.last() {
                                        bytes.pop();
                                    }
                                    new_line_flag = false;
                                } else {
                                    bytes.push(byte);
                                }
                            } else {
                                trailing.push(byte);
                            }
                        }
                        if !new_line_flag {
                            break;
                        }
                    }
                    Some(Err(err)) => return Poll::Ready(Err(Error::IoError(err))),
                    None => break,
                },
                Poll::Pending => return Poll::Pending,
            }
        }
    }
    let res = match String::from_utf8(bytes) {
        Ok(line) => Ok((line, trailing)),
        Err(_) => Err(Error::InvalidUtf8InResponse),
    };
    Poll::Ready(res)
}

fn parse_status_line(line: &str) -> (i32, String) {
    // sample status line format
    // HTTP/1.1 200 OK
    let mut status_code = String::with_capacity(3);
    let mut reason_phrase = String::with_capacity(2);

    let mut spaces = 0;

    for c in line.chars() {
        if spaces >= 2 {
            reason_phrase.push(c);
        }

        if c == ' ' {
            spaces += 1;
        } else if spaces == 1 {
            status_code.push(c);
        }
    }

    if let Ok(status_code) = status_code.parse::<i32>() {
        return (status_code, reason_phrase);
    }

    (503, "Server did not provide a status line".to_string())
}

fn parse_header(mut line: String) -> Option<(String, String)> {
    if let Some(location) = line.find(':') {
        // Trim the first character of the header if it is a space,
        // otherwise return everything after the ':'. This should
        // preserve the behavior in versions <=2.0.1 in most cases
        // (namely, ones where it was valid), where the first
        // character after ':' was always cut off.
        let value = if let Some(sp) = line.get(location + 1..location + 2) {
            if sp == " " {
                line[location + 2..].to_string()
            } else {
                line[location + 1..].to_string()
            }
        } else {
            line[location + 1..].to_string()
        };

        line.truncate(location);
        // Headers should be ascii, I'm pretty sure. If not, please open an issue.
        line.make_ascii_lowercase();
        return Some((line, value));
    }
    None
}
