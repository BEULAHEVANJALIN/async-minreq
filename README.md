# async-minreq
[![Crates.io](https://img.shields.io/crates/d/async-minreq.svg)](https://crates.io/crates/async-minreq)
[![Documentation](https://img.shields.io/docsrs/async_minreq)](https://docs.rs/async_minreq/latest/async_minreq/)
![Unit tests](https://github.com/BEULAHEVANJALIN/async-minreq/actions/workflows/unit-tests.yml/badge.svg)
![MSRV](https://github.com/BEULAHEVANJALIN/async-minreq/actions/workflows/msrv.yml/badge.svg)

async-minreq is a lightweight, minimal-dependency asynchronous HTTP client for Rust. It is a fork of the [minreq](https://github.com/neonmoe/minreq) crate, enhanced with async capabilities using Rust's async/await pattern and the Tokio runtime.

###  Features
- Simple API for sending HTTP requests asynchronously.
- Optional JSON response handling using serde (json-using-serde).
- Unicode domain support (punycode).
- HTTP proxy support (proxy).
- HTTPS support with flexible TLS implementations:
    - https-rustls (default)
    - https-rustls-probe
    - https-bundled
    - https-bundled-probe
    - https-native

Note: Enabling features such as serde or various HTTPS options may significantly increase executable size due to their complexity and dependency footprint.

### Documentation
Detailed documentation is available on [docs.rs](https://docs.rs/async_minreq/latest/async_minreq/)

### Minimum Supported Rust Version (MSRV)

If you don't care about the MSRV, you can ignore this section
entirely, including the commands instructed.

We use an MSRV per major release, i.e., with a new major release we
reserve the right to change the MSRV.

The current version of this library should always compile with any
combination of features excluding the TLS and urlencoding features on **Rust
1.71**. This is because those dependencies themselves have a higher MSRV.

That said, the crate does still require forcing some dependencies to
lower-than-latest versions to actually compile with the older
compiler, as these dependencies have upped their MSRV in a patch
version. This can be achieved with the following (these just update
your Cargo.lock):

```sh
cargo update --package=log --precise=0.4.18
cargo update --package=httpdate --precise=1.0.2
cargo update --package=serde_json --precise=1.0.100
cargo update --package=chrono --precise=0.4.23
cargo update --package=num-traits --precise=0.2.18
cargo update --package=libc --precise=0.2.163
```

### Contributing
Contributions are welcome! Feel free to open issues, submit pull requests, or suggest improvements.

### License
`async-minreq` crate is distributed under the terms of the [ISC license](COPYING.md).
