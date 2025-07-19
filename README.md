# async-minreq
[![Crates.io](https://img.shields.io/crates/d/async-minreq.svg)](https://crates.io/crates/async-minreq)
<!-- [![Documentation](https://docs.rs/minreq/badge.svg)](https://docs.rs/minreq) -->
![Unit tests](https://github.com/BEULAHEVANJALIN/async-minreq/actions/workflows/unit-tests.yml/badge.svg)
![MSRV](https://github.com/BEULAHEVANJALIN/async-minreq/actions/workflows/msrv.yml/badge.svg)

This crate is fork of the crate [minreq](https://github.com/neonmoe/minreq), with async capabilities.
Simple, async minimal-dependency HTTP client. Optional features for json
responses (`json-using-serde`), unicode domains (`punycode`), http
proxies (`proxy`), and https with various TLS implementations
(`https-rustls`, `https-rustls-probe`, `https-bundled`,
`https-bundled-probe`,`https-native`, and `https` which is an alias
for `https-rustls`).

Note: some of the dependencies of this crate (especially `serde` and
the various `https` libraries) are a lot more complicated than this
library, and their impact on executable size reflects that.

## [Documentation](https://docs.rs/async-minreq)

## Minimum Supported Rust Version (MSRV)

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

## License
This crate is distributed under the terms of the [ISC license](COPYING.md).
