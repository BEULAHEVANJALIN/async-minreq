alias b:= build
alias r:= release
alias t:= test
alias f:= fmt

default:
    just -l

#Building async-minreq (debug)
[group('dev')]
build:
    cargo build

#Building release
[group('dev')]
release:
    cargo build -r

#Formatting source code
[group('dev')]
fmt:
    cargo fmt

#Checking code style and lints
[group('dev')]
lint: fmt
    cargo clippy -- -D warnings

#Running tests for async-minreq
[group('test')]
test:
    cargo test

#Running https tests for async-minreq
[group('test')]
test-https:
    cargo test --no-default-features --features https

#Running https-rustls tests for async-minreq
[group('test')]
test-https-rustls:
    cargo test --no-default-features --features https-rustls

#Running https-rustls-probe tests for async-minreq
[group('test')]
test-https-rustls-probe:
    cargo test --no-default-features --features https-rustls-probe

#Running https-bundled tests for async-minreq
[group('test')]
test-https-bundled:
    cargo test --no-default-features --features https-bundled

#Running https-bundled-probe  tests for async-minreq
[group('test')]
test-https-bundled-probe:
    cargo test --no-default-features --features https-bundled-probe 

#Running https-native tests for async-minreq
[group('test')]
test-https-native:
    cargo test --no-default-features --features https-native

#Running json-using-serde tests for async-minreq
[group('test')]
test-json-using-serde:
    cargo test --no-default-features --features json-using-serde

#Running proxy tests for async-minreq
[group('test')]
test-proxy:
    cargo test --no-default-features --features proxy

#Running all tests for async-minreq
[group('test')]
test-all: test test-https test-https-rustls test-https-rustls-probe test-https-bundled test-https-bundled-probe test-https-native test-json-using-serde test-proxy