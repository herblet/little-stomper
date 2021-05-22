[![Crates.io](https://img.shields.io/crates/v/stomper.svg)](https://crates.io/crates/stomper)
![CI](https://github.com/herblet/stomper/actions/workflows/build_with_coverage.yml/badge.svg)
[![codecov](https://codecov.io/gh/herblet/stomper/branch/main/graph/badge.svg?token=2DKFJT5UZJ)](https://codecov.io/gh/herblet/stomper)

Stomper is a rust library which implements the STOMP protocol. It also includes a sample server using the library.

It uses two other libraries developed as part of the same project:

- [aysnc-map](https://github.com/herblet/async-map), which provides datastructures for concurrent use in an asynchronous context.
- [stomp-parser](https://github.com/herblet/stomp-parser), which provides a parser and serialiser for STOMP 1.2 frames.

This project was started as an exercise to learn both Rust and STOMP. The code is not in production anywhere.
