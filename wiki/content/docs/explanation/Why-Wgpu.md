---
title: Why Wgpu
weight: 1
---

PowerShell and .NET have no API for the GPU of their own. A script that wants one has reached for a vendor's toolkit, which serves that vendor's GPUs and has to be installed first; for bindings to a graphics API, which leave every buffer, every synchronization and every shader compile to the script; or for a library written for C#. The module puts the GPU behind cmdlets instead, with nothing installed but the module. This squares an array on whatever GPU the machine has:

```powershell
PS> (Invoke-WgpuMap 'x * x' ([float[]](1..8))) -join ','
1,4,9,16,25,36,49,64
```

## Why wgpu

[wgpu](https://wgpu.rs/) is the Rust implementation of WebGPU, the API browsers give web pages for the GPU; Firefox's WebGPU runs on it. It is one API over Direct3D 12, Vulkan, Metal and OpenGL, so a shader and the calls around it run unchanged on any vendor's GPU, on Windows, Linux, macOS and FreeBSD. Its shaders are written in WGSL, WebGPU's shading language, and naga, its shader compiler, reads GLSL and SPIR-V too and translates between all of them and HLSL and MSL.

wgpu validates every call, so a mistake comes back as an error with a message rather than as a driver fault. Reading a buffer after freeing it, which Vulkan and Direct3D 12 leave undefined, is refused, and the message names the buffer:

```powershell
PS> $buffer = New-WgpuBuffer ([float[]](1..4))
PS> Remove-WgpuBuffer $buffer
PS> Read-WgpuBuffer $buffer
Read-WgpuBuffer: the buffer cannot be read back: Validation Error

Caused by:
  In Queue::submit
    Buffer with '4 x Float32' label has been destroyed
```

## What the module adds

- wgpu and naga compiled into the module, with Direct3D 12's shader compiler on Windows x64, so nothing else is installed: no SDK, no runtime, nothing beyond the GPU's own driver.
- Objects that stay on the GPU between calls. Buffers, textures, shaders and pipelines are objects a script holds, and data moves only when it is read or written; [How Data Moves](How-Data-Moves.md) says where and what that costs.
- Arrays of any numeric type in and out: an array binds to a shader's buffer as the element type the shader declares.
- Every feature and limit the adapter has, on every device the module opens.
- Shader tools with no GPU: Test-WgpuShader, Get-WgpuShaderInfo and ConvertTo-WgpuShader run on any machine.
- Each way PowerShell is used: cmdlets in pipelines, methods on the objects, tasks, both hosts, runspaces and threads.
- The Rust library under the cmdlets, for Rust programs.

## Where it stops

- It draws offscreen, into textures and image files. It opens no window and presents nothing on a screen.
- It reaches what wgpu reaches. The native features wgpu has beyond WebGPU's are there when the adapter supports them, the experimental ones with `-Experimental`, but a vendor's own libraries, such as CUDA's, are not part of it.
- A GPU is fast at work over many elements and slow to reach: every call crosses to the GPU and back, so small arrays are often quicker on the CPU. [How Data Moves](How-Data-Moves.md) measures where the time goes.
