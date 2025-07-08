use crate::request::ParsedRequest;
use crate::{Error, Method, ResponseLazy};
use std::env;
use std::future::Future;
use std::net::ToSocketAddrs;
use std::pin::Pin;
use std::time::Duration;
use tokio::io::{self, AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{timeout, Instant};
#[cfg(any(feature = "rustls-webpki", feature = "rustls"))]
use webpki_roots::TLS_SERVER_ROOTS;
#[cfg(all(
    not(feature = "rustls"),
    any(feature = "openssl", feature = "native-tls")
))]
use {
    crate::native_tls::TlsConnector, crate::tokio_native_tls::TlsConnector as TokioTlsConnector,
    crate::tokio_native_tls::TlsStream,
};
#[cfg(feature = "rustls")]
use {
    once_cell::sync::Lazy,
    rustls_pki_types::ServerName,
    std::convert::TryFrom,
    std::sync::Arc,
    tokio_rustls::client::TlsStream,
    tokio_rustls::rustls::{ClientConfig, RootCertStore},
    tokio_rustls::TlsConnector,
};

#[cfg(feature = "rustls")]
static CONFIG: Lazy<Arc<ClientConfig>> = Lazy::new(|| {
    let mut root_certificates = RootCertStore::empty();
    root_certificates.extend(TLS_SERVER_ROOTS.iter().cloned());

    // Try to load native certs
    #[cfg(feature = "https-rustls-probe")]
    {
        let rustls_native_certs::CertificateResult { certs, .. } =
            rustls_native_certs::load_native_certs();
        for cert in certs {
            // Ignore erroneous OS certificates, there's nothing
            // to do differently in that situation anyways.
            let _ = root_certificates.add(cert);
        }
    }
    let config = ClientConfig::builder()
        .with_root_certificates(root_certificates)
        .with_no_client_auth();
    Arc::new(config)
});

type UnsecuredStream = TcpStream;
#[cfg(feature = "rustls")]
type SecuredStream = TlsStream<TcpStream>;
#[cfg(all(
    not(feature = "rustls"),
    any(feature = "openssl", feature = "native-tls")
))]
type SecuredStream = TlsStream<TcpStream>;

pub(crate) enum HttpStream {
    Unsecured(UnsecuredStream, Option<Instant>),
    #[cfg(any(feature = "rustls", feature = "openssl", feature = "native-tls"))]
    Secured(Box<SecuredStream>, Option<Instant>),
}

impl HttpStream {
    fn create_unsecured(reader: UnsecuredStream, timeout_at: Option<Instant>) -> HttpStream {
        HttpStream::Unsecured(reader, timeout_at)
    }

    #[cfg(any(feature = "rustls", feature = "openssl", feature = "native-tls"))]
    fn create_secured(reader: SecuredStream, timeout_at: Option<Instant>) -> HttpStream {
        HttpStream::Secured(Box::new(reader), timeout_at)
    }
}

fn timeout_err() -> io::Error {
    io::Error::new(
        io::ErrorKind::TimedOut,
        "the timeout of the request was reached",
    )
}

fn timeout_at_to_duration(timeout_at: Option<Instant>) -> Result<Option<Duration>, io::Error> {
    if let Some(timeout_at) = timeout_at {
        if let Some(duration) = timeout_at.checked_duration_since(Instant::now()) {
            Ok(Some(duration))
        } else {
            Err(timeout_err())
        }
    } else {
        Ok(None)
    }
}

impl AsyncRead for HttpStream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            HttpStream::Unsecured(ref mut stream, _) => {
                // Delegate the read operation to the TcpStream
                Pin::new(stream).poll_read(cx, buf)
            }
            _ => todo!(),
        }
    }
}

/// A connection to the server for sending
/// [`Request`](struct.Request.html)s.
pub struct Connection {
    request: ParsedRequest,
    timeout_at: Option<Instant>,
}

impl Connection {
    /// Creates a new `Connection`. See [Request] and [ParsedRequest]
    /// for specifics about *what* is being sent.
    pub(crate) fn new(request: ParsedRequest) -> Connection {
        let timeout = request
            .config
            .timeout
            .or_else(|| match env::var("MINREQ_TIMEOUT") {
                Ok(t) => t.parse::<u64>().ok(),
                Err(_) => None,
            });
        let timeout_at = timeout.map(|t| Instant::now() + Duration::from_secs(t));
        Connection {
            request,
            timeout_at,
        }
    }

    /// Returns the timeout duration for operations that should end at
    /// timeout and are starting "now".
    ///
    /// The Result will be Err if the timeout has already passed.
    fn timeout(&self) -> Result<Option<Duration>, io::Error> {
        let timeout = timeout_at_to_duration(self.timeout_at);
        log::trace!("Timeout requested, it is currently: {:?}", timeout);
        timeout
    }

    /// Sends the [`Request`](struct.Request.html), consumes this
    /// connection, and returns a [`Response`](struct.Response.html).
    #[cfg(feature = "rustls")]
    pub(crate) async fn send_https(self) -> Result<ResponseLazy, Error> {
        match self.timeout()? {
            None => self.send_https_without_timeout().await,
            Some(duration) => match timeout(duration, self.send_https_without_timeout()).await {
                Ok(result) => result,
                Err(_) => Err(Error::IoError(timeout_err())),
            },
        }
    }

    #[cfg(feature = "rustls")]
    async fn send_https_without_timeout(mut self) -> Result<ResponseLazy, Error> {
        self.request.url.host = ensure_ascii_host(self.request.url.host)?;
        let bytes = self.request.as_bytes();

        // Rustls setup
        log::trace!("Setting up TLS parameters for {}.", self.request.url.host);
        let dns_name = match ServerName::try_from(self.request.url.host.clone()) {
            Ok(result) => result,
            Err(err) => return Err(Error::IoError(io::Error::new(io::ErrorKind::Other, err))),
        };
        let connector = TlsConnector::from(CONFIG.clone());

        log::trace!("Establishing TCP connection to {}.", self.request.url.host);
        let tcp = self.connect().await?;

        // Send request
        log::trace!("Establishing TLS session to {}.", self.request.url.host);
        let mut tls = connector.connect(dns_name, tcp).await?;
        log::trace!("Writing HTTPS request to {}.", self.request.url.host);
        tls.write_all(&bytes).await?;

        // Receive request
        log::trace!("Reading HTTPS response from {}.", self.request.url.host);
        let response = ResponseLazy::from_stream(
            HttpStream::create_secured(tls, self.timeout_at),
            self.request.config.max_headers_size,
            self.request.config.max_status_line_len,
        )
        .await?;
        handle_redirects(self, response).await
    }

    // / Sends the [`Request`](struct.Request.html), consumes this
    // / connection, and returns a [`Response`](struct.Response.html).
    #[cfg(all(
        not(feature = "rustls"),
        any(feature = "openssl", feature = "native-tls")
    ))]
    pub(crate) async fn send_https(mut self) -> Result<ResponseLazy, Error> {
        let duration = timeout_at_to_duration(self.timeout_at)?;
        let native = TlsConnector::builder()
            .build()
            .map_err(|e| Error::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        let connector = TokioTlsConnector::from(native);

        let func = async move {
            self.request.url.host = ensure_ascii_host(self.request.url.host)?;
            let bytes = self.request.as_bytes();

            log::trace!("Setting up TLS parameters for {}.", self.request.url.host);
            let dns_name = self.request.url.host.as_str();
            /*
            let mut builder = TlsConnector::builder();
            ...
            let sess = match builder.build() {
            */

            log::trace!("Establishing TCP connection to {}.", self.request.url.host);
            let tcp = self.connect().await?;

            // Send request
            log::trace!("Establishing TLS session to {}.", self.request.url.host);
            let mut tls = connector
                .connect(dns_name, tcp)
                .await
                .map_err(|e| Error::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

            // let _ = tls.get_ref().set_write_timeout(self.timeout()?);

            log::trace!("Writing HTTPS request to {}.", self.request.url.host);
            // let _ = tls.get_ref().set_write_timeout(self.timeout()?);
            tls.write_all(&bytes).await?;

            // Receive request
            log::trace!("Reading HTTPS response from {}.", self.request.url.host);
            let response = ResponseLazy::from_stream(
                HttpStream::create_secured(tls, self.timeout_at),
                self.request.config.max_headers_size,
                self.request.config.max_status_line_len,
            )
            .await?;

            Ok(handle_redirects(self, response).await?)
        };

        if let Some(dur) = duration {
            match timeout(dur, func).await {
                Ok(res) => res,
                Err(_) => Err(Error::IoError(timeout_err())),
            }
        } else {
            func.await
        }
    }

    /// Sends the [`Request`](struct.Request.html), consumes this
    /// connection, and returns a [`Response`](struct.Response.html).
    pub(crate) fn send(self) -> Pin<Box<dyn Future<Output = Result<ResponseLazy, Error>> + Send + 'static>> {
       Box::pin(async move { match self.timeout()? {
            None => self.send_without_timeout().await,
            Some(duration) => match timeout(duration, self.send_without_timeout()).await {
                Ok(result) => result,
                Err(_) => Err(Error::IoError(timeout_err())),
            },
        }
    })
    }

    fn send_without_timeout(mut self) -> Pin<Box<dyn Future<Output = Result<ResponseLazy, Error>> + Send + 'static>> {
       Box::pin(async move {  self.request.url.host = ensure_ascii_host(self.request.url.host)?;
        let bytes = self.request.as_bytes();

        log::trace!("Establishing TCP connection to {}.", self.request.url.host);
        let tcp = self.connect().await?;

        // Send request
        log::trace!("Writing HTTP request.");
        loop {
            // Wait for the socket to be writable
            tcp.writable().await?;

            match tcp.try_write(&bytes) {
                Ok(_n) => {
                    break;
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    continue;
                }
                Err(e) => {
                    return Err(e.into());
                }
            }
        }

        // Receive response
        log::trace!("Reading HTTP response.");
        let stream = HttpStream::create_unsecured(tcp, self.timeout_at);
        let response = ResponseLazy::from_stream(
            stream,
            self.request.config.max_headers_size,
            self.request.config.max_status_line_len,
        )
        .await?;
        handle_redirects(self, response).await

    })
    }

    async fn connect(&self) -> Result<TcpStream, Error> {
        #[cfg(feature = "proxy")]
        match self.request.config.proxy {
            Some(ref proxy) => {
                // do proxy things
                let mut tcp = self.tcp_connect(&proxy.server, proxy.port as u16).await?;
                let connect_payload = proxy.connect(&self.request);
                tcp.write_all(connect_payload.as_bytes())
                    .await
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
                // write!(tcp, "{}", proxy.connect(&self.request)).unwrap();
                tcp.flush().await?;

                let mut proxy_response = Vec::new();

                loop {
                    let mut buf = vec![0; 256];
                    let total = tcp.read(&mut buf).await?;
                    proxy_response.append(&mut buf);
                    if total < 256 {
                        break;
                    }
                }

                crate::Proxy::verify_response(&proxy_response)?;

                Ok(tcp)
            }
            None => {
                self.tcp_connect(&self.request.url.host, self.request.url.port.port())
                    .await
            }
        }

        #[cfg(not(feature = "proxy"))]
        self.tcp_connect(&self.request.url.host, self.request.url.port.port())
            .await
    }

    async fn tcp_connect(&self, host: &str, port: u16) -> Result<TcpStream, Error> {
        let addrs = (host, port).to_socket_addrs().map_err(Error::IoError)?;
        let addrs_count = addrs.len();

        // Try all resolved addresses. Return the first one to which we could connect. If all
        // failed return the last error encountered.
        for (i, addr) in addrs.enumerate() {
            let stream = if let Some(duration) = self.timeout()? {
                match timeout(duration, TcpStream::connect(&addr)).await {
                    Ok(result) => result,
                    Err(_) => Err(timeout_err()),
                }
            } else {
                TcpStream::connect(addr).await
            };
            if stream.is_ok() || i == addrs_count - 1 {
                return stream.map_err(Error::from);
            }
        }

        Err(Error::AddressNotFound)
    }
}

fn handle_redirects(
    connection: Connection,
    mut response: ResponseLazy,
) -> Pin<Box<dyn Future<Output = Result<ResponseLazy, Error>> + Send + 'static>> {
    Box::pin(async move {
        let status_code = response.status_code;
        let url = response.headers.get("location");
        match get_redirect(connection, status_code, url) {
            NextHop::Redirect(connection) => {
                let connection = connection?;
                if connection.request.url.https {
                    #[cfg(not(any(
                        feature = "rustls",
                        feature = "openssl",
                        feature = "native-tls"
                    )))]
                    return Err(Error::HttpsFeatureNotEnabled);
                    #[cfg(any(feature = "rustls", feature = "openssl", feature = "native-tls"))]
                    return connection.send_https().await;
                } else {
                    connection.send().await
                }
            }
            NextHop::Destination(connection) => {
                let dst_url = connection.request.url;
                dst_url.write_base_url_to(&mut response.url).unwrap();
                dst_url.write_resource_to(&mut response.url).unwrap();
                Ok(response)
            }
        }
    })
}

enum NextHop {
    Redirect(Result<Connection, Error>),
    Destination(Connection),
}

fn get_redirect(mut connection: Connection, status_code: i32, url: Option<&String>) -> NextHop {
    match status_code {
        301 | 302 | 303 | 307 => {
            let url = match url {
                Some(url) => url,
                None => return NextHop::Redirect(Err(Error::RedirectLocationMissing)),
            };
            log::debug!("Redirecting ({}) to: {}", status_code, url);

            match connection.request.redirect_to(url.as_str()) {
                Ok(()) => {
                    if status_code == 303 {
                        match connection.request.config.method {
                            Method::Post | Method::Put | Method::Delete => {
                                connection.request.config.method = Method::Get;
                            }
                            _ => {}
                        }
                    }

                    NextHop::Redirect(Ok(connection))
                }
                Err(err) => NextHop::Redirect(Err(err)),
            }
        }
        _ => NextHop::Destination(connection),
    }
}

fn ensure_ascii_host(host: String) -> Result<String, Error> {
    if host.is_ascii() {
        Ok(host)
    } else {
        #[cfg(not(feature = "punycode"))]
        {
            Err(Error::PunycodeFeatureNotEnabled)
        }

        #[cfg(feature = "punycode")]
        {
            let mut result = String::with_capacity(host.len() * 2);
            for s in host.split('.') {
                if s.is_ascii() {
                    result += s;
                } else {
                    match punycode::encode(s) {
                        Ok(s) => result = result + "xn--" + &s,
                        Err(_) => return Err(Error::PunycodeConversionFailed),
                    }
                }
                result += ".";
            }
            result.truncate(result.len() - 1); // Remove the trailing dot
            Ok(result)
        }
    }
}
