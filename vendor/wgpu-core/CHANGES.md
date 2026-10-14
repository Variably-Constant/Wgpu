# Changes from wgpu-core 30.0.1

This is the wgpu-core crate, version 30.0.1, as crates.io publishes it,
under the licenses in `LICENSE.APACHE` and `LICENSE.MIT`, with two
changes:

- `Device::maintain` in `src/device/resource.rs` keeps whether its wait for
  a submission timed out, and reports the queue empty only when it did
  not: the change wgpu's pull request 9958 made on its trunk
  (https://github.com/gfx-rs/wgpu/pull/9958). In 30.0.1 a wait that timed
  out could read the fence before another thread polling the same device
  retired the submission, find the queue empty, and fail its assertion
  that the fence had reached the submission waited for ("If the queue is
  empty, the current submission index (106) should be at least the wait
  submission index (107)").
- `Cargo.toml` allows the `unfulfilled_lint_expectations` lint. Cargo
  shows the warnings of a crate it builds from a path, and Rust 1.99 does
  not report the field `src/lock/ranked.rs` expects to be unused, which
  would otherwise print a warning in every build.
