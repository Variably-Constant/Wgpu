---
title: Where It Has Been Run
weight: 120
---

What has been built and run, on what, and what it showed. The table is the Windows 11 machine every example on these pages ran on: each row is a run of the release module `cargo pwrs build --release` wrote from the repository's committed files, imported from its folder. [Linux](#linux) and [FreeBSD](#freebsd) are runs of the suites on virtual machines, [GitHub's runners](#githubs-runners) build and test the four platforms no owned machine covers, and the list at the end names what has not been run.

| | |
|---|---|
| Machine | AMD Ryzen 9 7900X, Windows 11 Pro 10.0.26200, with the Visual Studio 2022 Build Tools 17.14 and their ATL component |
| GPUs | NVIDIA GeForce RTX 5070, driver 32.0.16.1742 (617.42), and the processor's AMD Radeon Graphics, driver 32.0.21042.62; and WARP, the Microsoft Basic Render Driver 10.0.26100.9278 |
| Adapters | Get-WgpuAdapter lists each GPU through Vulkan and through Direct3D 12, and WARP through Direct3D 12; the default device is the RTX 5070 through Vulkan |
| Hosts | PowerShell 7.6.6 on .NET 10.0.12, with Pester 5.7.1, and Windows PowerShell 5.1.26100.9444 on .NET Framework 4.8.9345, with Pester 6.2.0; for the Pester suites, also PowerShell 7.4.20 on .NET 8.0.31 and 7.5.11 on .NET 9.0.20 |
| Built with | Rust 1.99.0, cargo-pwrs 0.7.0 and PoWerRuSt 0.7.0 from crates.io, wgpu and naga 30.0.1, wgpu-core 30.0.1 with the changes `vendor/wgpu-core/CHANGES.md` describes, DXC from mach-dxcompiler 2026.09.16 (48d5a66), the image crate 0.25.10, and Basis Universal 2.50 from `vendor/basis_universal` through the Build Tools' MSVC 14.44.35207; `.cargo/config.toml`'s x86-64 baseline and static C runtime, and no `RUSTFLAGS` |
| Result | clippy and rustdoc with warnings denied, with and without the module feature, both clean; the 143 Rust unit tests passed, and the 247 Pester tests passed in PowerShell 7.6.6, 7.5.11 and 7.4.20 and in Windows PowerShell 5.1, with the surface check finding every cmdlet run, every pipeline parameter fed and every ShouldProcess asked |
| On WARP | the 143 Rust unit tests passed with `WGPU_PWSH_TEST_ADAPTER` set to `Basic Render`, as [How To Run The Tests](How-To-Run-The-Tests.md#run-them-on-another-adapter) shows, and the 247 Pester tests passed in PowerShell 7.6.6 and in Windows PowerShell 5.1 with WARP made the default device before they ran: the compute, render, shader and texture suites ran on WARP, and the rest from the test that opens a default device of its own on, which moves them to the RTX 5070 |
| Repeated | the Rust unit tests 40 times in a row with none failing, at the earlier commit that fixed the three faults of NVIDIA's driver [Devices And Instances](Devices-And-Instances.md#nvidias-driver-on-windows) describes, which running them over and over had found |
| Rust library | `examples/library.rs`, built without the module feature, ran on the RTX 5070 through Vulkan and printed what [How To Use The Rust Library](How-To-Use-The-Rust-Library.md) shows |
| Examples | the 1770 examples of the README and of 40 pages of this wiki, each page's in order in a process of its own, in PowerShell 7.6.6: every one printed the output its page shows. In Windows PowerShell 5.1 every one ran but those marked for PowerShell 7, and none ended in an error its page does not show. Most ran on the RTX 5070, the default device; others opened devices on the Radeon and on WARP and gave them buffers, and ran shaders on WARP |
| Dependents | `dumpbin /dependents` on the native library lists only DLLs that are part of Windows |
| Timed | the copies and runs [How Data Moves](How-Data-Moves.md#what-that-costs) measures, at an earlier commit, nine rounds after one to warm up, in PowerShell 7.6.6 and Windows PowerShell 5.1 |

## Linux

An Ubuntu virtual machine with no GPU, on Linux 6.8.0-146, running the code these pages describe, built there for glibc 2.35 through zig 0.16.0 and cargo-zigbuild 0.23.4 built at the commit [Building From Source](Building-From-Source.md#install-what-the-build-needs) names, with Rust 1.99.0 and cargo-pwrs 0.7.0.

| | |
|---|---|
| Host | PowerShell 7.6.6 on .NET 10.0.12, with Pester 6.2.0 |
| Result | clippy with warnings denied, with and without the module feature, clean; the 143 Rust unit tests passed; the newest glibc symbol the library asks for is 2.34; and the 247 Pester tests passed on the machine's software adapter, with the surface check finding every cmdlet run, every pipeline parameter fed and every ShouldProcess asked. The notices beside the library quote every text the module's own build adds, and none of DXC's. The module's build printed no warning |

## FreeBSD

A FreeBSD 15.0 virtual machine with no GPU, running the code these pages describe, built there with Rust 1.99.0, cargo-pwrs 0.7.0 and the base system's clang, with `PWRS_TOOLSET` set to 5.3.0, the newest C# compiler built for the .NET 9 its PowerShell runs on.

| | |
|---|---|
| Host | PowerShell 7.5.5 on .NET 9.0.14, with Pester 6.2.0, imported through a module that sets `$IsLinux`, which Pester's platform check asks for there |
| Result | clippy with warnings denied, with and without the module feature, clean; the 143 Rust unit tests passed; and the 247 Pester tests passed on the machine's software adapter, with the surface check finding every cmdlet run, every pipeline parameter fed and every ShouldProcess asked. The notices beside the library quote every text the module's own build adds, and none of DXC's. The module's build printed no warning |

## GitHub's runners

The workflow `.github/workflows/ci.yml` builds and tests the four platforms no owned machine covers, on GitHub's hosted runners, none of which has a GPU of its own. Each job lints the crate with and without the module feature, runs `cargo pwrs test --release`, and lists the adapters. Its last run was of the code these pages describe, with Rust 1.99.0, cargo-pwrs 0.7.0 and Pester 6.2.0.

| Platform | Runner | What ran |
|---|---|---|
| macOS on Apple silicon | `macos-latest`, macOS 26, PowerShell 7.6.5 | clippy clean both ways; the 143 Rust unit tests passed; 246 Pester tests passed and one skipped, on the Apple Paravirtual device through Metal, which has no timestamp queries; the surface check found every cmdlet run, every pipeline parameter fed and every ShouldProcess asked |
| macOS on Intel | `macos-15-intel`, macOS 15, PowerShell 7.6.4 | the same as on Apple silicon: the 143 unit tests and 246 Pester tests passed, the timestamp test skipped, on the Apple Paravirtual device through Metal |
| Windows on Arm64 | `windows-11-arm`, PowerShell 7.6.6 and Windows PowerShell 5.1.26100.9457 | clippy clean both ways; the 143 Rust unit tests passed on WARP, the runner's only adapter, and the 247 Pester tests passed in both hosts, with Direct3D 12 compiling every shader with the `dxcompiler.dll` the module ships there; the surface check found every cmdlet run, every pipeline parameter fed and every ShouldProcess asked |
| Linux on Arm64 | `ubuntu-24.04-arm`, glibc 2.35 through cargo-zigbuild and zig 0.16.0 | clippy clean both ways; the 143 Rust unit tests passed, and the 247 Pester tests passed on Mesa's software Vulkan, which lists as llvmpipe (LLVM 20.1.2); the newest glibc symbol the library asks for is 2.34 |

## Not run

- Other GPUs and drivers: Intel's, AMD's discrete GPUs, and NVIDIA drivers other than 617.42.
- Other Windows versions, and Windows Server.
