---
title: How Data Moves
weight: 3
---

A script's arrays live in .NET's memory, and a GPU computes on buffers in memory it can reach, its own on a discrete GPU. Data crosses between them by copying, and every cmdlet and method says by its shape whether it copies: one that takes or returns an array copies, one that takes or returns a Wgpu.Buffer does not.

## What each step copies

- New-WgpuBuffer, Write-WgpuBuffer, a device's `Upload` and an array bound to Invoke-WgpuCompute copy the array's elements into a GPU buffer, converted to the element type asked for. An array already of that type is read in place, with no copy on the .NET side.
- Read-WgpuBuffer and a buffer's `Read` have the GPU copy the buffer into a staging buffer the CPU can map, wait for the GPU to finish everything given to it before, then copy the staging buffer's bytes into a new array.
- Invoke-WgpuCompute over arrays copies each one into a buffer, runs, and copies back every array the shader binds `read_write`. Over buffers it copies nothing: the shader reads and writes the buffers where they are.
- Textures and images move the same way: Write-WgpuTexture and Import-WgpuImage copy in, and Read-WgpuTexture and Export-WgpuImage copy out through a staging buffer.

## What that costs

One shader doubles 8,388,608 floats, 32 MiB, in place, run nine times after one to warm up, on the machine [Where It Has Been Run](Where-It-Has-Been-Run.md) describes, through the RTX 5070's Vulkan driver. The medians:

| Step | PowerShell 7.6.6 | Windows PowerShell 5.1 |
|---|---|---|
| Invoke-WgpuCompute over the array: copy in, run, copy back | 25.1 ms | 23.1 ms |
| Invoke-WgpuCompute over a buffer already on the GPU | 0.4 ms | 0.4 ms |
| New-WgpuBuffer from the array | 4.1 ms | 3.9 ms |
| Read-WgpuBuffer into a new array | 22.0 ms | 9.2 ms |

The GPU's own pass, by its timestamps, took between 0.04 and 7.3 ms of each run. So a run over arrays spends nearly all its time copying, and above all copying back: reading into a new array is the slowest step, and slower in PowerShell 7 than in Windows PowerShell on the same machine. A run over a buffer costs a third to a half of a millisecond, whatever the buffer holds.

## Keeping data on the GPU

The way to fast work is to copy in once, run as often as needed on buffers, and copy out once. Here three runs double four numbers in place, and only the last command copies anything back:

```powershell
PS> Get-Content ./double.wgsl
@group(0) @binding(0) var<storage, read_write> v: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x < arrayLength(&v)) { v[id.x] = v[id.x] * 2.0; }
}
PS> $double = New-WgpuComputePipeline (Get-Content ./double.wgsl -Raw)
PS> $values = New-WgpuBuffer ([float[]](1..4))
PS> 1..3 | ForEach-Object { Invoke-WgpuCompute $double $values }
PS> Read-WgpuBuffer $values
8
16
24
32
```

[How To Work With Buffers](How-To-Work-With-Buffers.md) shows buffers in use, [How To Map And Reduce Arrays](How-To-Map-And-Reduce-Arrays.md) leaves results on the GPU with `-AsBuffer`, and [How To Record Commands By Hand](How-To-Record-Commands-By-Hand.md) puts many runs in one submission.

## An array returned from a script block

PowerShell unrolls an array that a script block or a function returns, element by element, and collects the elements again as an `Object[]`. For a method that returns a large array, that costs far more than the read did. With the 32 MiB buffer above, the medians of nine runs after one to warm up, in PowerShell 7.6.6:

| | Time | Result |
|---|---|---|
| `$x = $buffer.Read()` | 11 ms | `Single[]` |
| `$x = & { $buffer.Read() }` | 461 ms | `Object[]` |
| `$x = & { , $buffer.Read() }` | 19 ms | `Single[]` |

A cmdlet writes its array as one object, and the comma in front of an array returned from a script block does the same.
