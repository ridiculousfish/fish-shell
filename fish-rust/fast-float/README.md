fast-float
==========

[![Build](https://github.com/aldanor/fast-float-rust/workflows/CI/badge.svg)](https://github.com/aldanor/fast-float-rust/actions?query=branch%3Amaster)
[![Latest Version](https://img.shields.io/crates/v/fast-float.svg)](https://crates.io/crates/fast-float)
[![Documentation](https://docs.rs/fast-float/badge.svg)](https://docs.rs/fast-float)
[![Apache 2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)
[![MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)
[![Rustc 1.37+](https://img.shields.io/badge/rustc-1.37+-lightgray.svg)](https://blog.rust-lang.org/2019/08/15/Rust-1.37.0.html)

This crate provides a super-fast decimal number parser from strings into floats.

```toml
[dependencies]
fast-float = "0.2"
```

There are no dependencies and the crate can be used in a no_std context by disabling the "std" feature.

*Compiler support: rustc 1.37+.*

#### References

- Daniel Lemire, [Number Parsing at a Gigabyte per Second](https://arxiv.org/abs/2101.11408), Software: Practice and Experience 51 (8), 2021.

#### License

<sup>
Licensed under either of <a href="LICENSE-APACHE">Apache License, Version
2.0</a> or <a href="LICENSE-MIT">MIT license</a> at your option.
</sup>

<br>

<sub>
Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this crate by you, as defined in the Apache-2.0 license, shall
be dual licensed as above, without any additional terms or conditions.
</sub>
