[![Crates.io](https://img.shields.io/crates/v/little-stomper.svg)](https://crates.io/crates/little-stomper)
![CI](https://github.com/herblet/little-stomper/actions/workflows/build_with_coverage.yml/badge.svg)
[![codecov](https://codecov.io/gh/herblet/little-stomper/branch/main/graph/badge.svg?token=2DKFJT5UZJ)](https://codecov.io/gh/herblet/little-stomper)

Little Stomper is a rust library which implements the STOMP protocol. It also includes a [sample server](src/bin/sample_server.rs) using the library.

It uses two other libraries developed as part of the same project:

- [aysnc-map](https://github.com/herblet/async-map), which provides datastructures for concurrent use in an asynchronous context.
- [stomp-parser](https://github.com/herblet/stomp-parser), which provides a parser and serialiser for STOMP 1.2 frames.

This project was started as an exercise to learn both Rust and STOMP. The code is not in production anywhere.

Originally named Stomper, but a crate with that name already exists; a toddler, ever-present during initial development, provided the rest.
