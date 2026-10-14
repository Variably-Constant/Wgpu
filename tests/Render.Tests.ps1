# Pester 5 syntax; runs in pwsh and Windows PowerShell. PWRS_MODULE points
# at the built module folder. GPU cases are skipped on a host with no
# adapter, and say so.
BeforeDiscovery {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Wgpu.psd1') -ErrorAction Stop
    $script:noGpu = @(Get-WgpuAdapter).Count -eq 0
}

BeforeAll {
    Import-Module (Join-Path $env:PWRS_MODULE 'Wgpu.psd1') -ErrorAction Stop
    $script:fullscreen = @'
@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    var corners = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
    return vec4<f32>(corners[i], 0.0, 1.0);
}
@fragment
fn fs() -> @location(0) vec4<f32> { return vec4<f32>(1.0, 0.0, 0.0, 1.0); }
'@
    $script:colored = @'
struct Out { @builtin(position) position: vec4<f32>, @location(0) color: vec3<f32> }
@vertex
fn vs(@location(0) position: vec2<f32>, @location(1) color: vec3<f32>) -> Out {
    var out: Out;
    out.position = vec4<f32>(position, 0.0, 1.0);
    out.color = color;
    return out;
}
@fragment
fn fs(input: Out) -> @location(0) vec4<f32> { return vec4<f32>(input.color, 1.0); }
'@
    $script:everyPixel = { param($bytes, $rgba) for ($i = 0; $i -lt $bytes.Length; $i += 4) { if (($bytes[$i..($i + 3)] -join ',') -ne $rgba) { return $false } }; $true }
    # Every pixel within one step of each channel of $rgba: a GPU may blend
    # an 8-bit target at its own precision.
    $script:nearEveryPixel = {
        param($bytes, $rgba)
        $want = $rgba -split ','
        for ($i = 0; $i -lt $bytes.Length; $i++) { if ([Math]::Abs([int]$bytes[$i] - [int]$want[$i % 4]) -gt 1) { return $false } }
        $true
    }
}

Describe 'Invoke-WgpuRender' {
    It 'draws a full-screen triangle from vertex_index alone' -Skip:$noGpu {
        $texture = Invoke-WgpuRender $fullscreen 8 8 -VertexCount 3
        $texture.Format | Should -Be 'Rgba8Unorm'
        & $everyPixel (Read-WgpuTexture $texture) '255,0,0,255' | Should -BeTrue
    }

    It 'feeds vertex inputs by name and counts the vertices from them' -Skip:$noGpu {
        $vertices = @{ position = [float[]](-1, -1, 3, -1, -1, 3); color = [float[]](0, 1, 0, 0, 1, 0, 0, 1, 0) }
        $texture = Invoke-WgpuRender $colored 8 8 -Vertex $vertices
        & $everyPixel (Read-WgpuTexture $texture) '0,255,0,255' | Should -BeTrue
    }

    It 'draws by indices' -Skip:$noGpu {
        $vertices = @{ position = [float[]](-1, -1, 1, -1, -1, 1, 1, 1); color = [float[]](0, 0, 1, 0, 0, 1, 0, 0, 1, 0, 0, 1) }
        $texture = Invoke-WgpuRender $colored 8 8 -Vertex $vertices -Index ([uint32[]](0, 1, 2, 2, 1, 3))
        & $everyPixel (Read-WgpuTexture $texture) '0,0,255,255' | Should -BeTrue
    }

    It 'draws a shader read from SPIR-V the way up its WGSL draws' -Skip:$noGpu {
        $corner = @'
@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    var corners = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(0.0, -1.0), vec2<f32>(-1.0, 0.0));
    return vec4<f32>(corners[i], 0.0, 1.0);
}
@fragment
fn fs() -> @location(0) vec4<f32> { return vec4<f32>(1.0, 1.0, 1.0, 1.0); }
'@
        $spirv = New-WgpuShader -SpirV (ConvertTo-WgpuShader -Source $corner -To SpirV)
        $fromSpirV = Read-WgpuTexture (Invoke-WgpuRender $spirv 8 8 -VertexCount 3)
        $fromWgsl = Read-WgpuTexture (Invoke-WgpuRender $corner 8 8 -VertexCount 3)
        $fromSpirV[224..227] -join ',' | Should -Be '255,255,255,255'
        $fromSpirV[0..3] -join ',' | Should -Be '0,0,0,255'
        ($fromSpirV -join ',') | Should -Be ($fromWgsl -join ',')
    }

    It 'clears to the color given and leaves undrawn pixels that color' -Skip:$noGpu {
        $vertices = @{ position = [float[]](-1, -1, 0, -1, -1, 0); color = [float[]](1, 1, 1, 1, 1, 1, 1, 1, 1) }
        $texels = Read-WgpuTexture (Invoke-WgpuRender $colored 8 8 -Vertex $vertices -ClearColor 0, 0, 1, 1)
        $last = $texels.Length - 4
        $texels[$last..($last + 3)] -join ',' | Should -Be '0,0,255,255'
    }

    It 'discards the counterclockwise triangle with -CullMode Front and keeps it with Back' -Skip:$noGpu {
        & $everyPixel (Read-WgpuTexture (Invoke-WgpuRender $fullscreen 4 4 -VertexCount 3 -CullMode Front)) '0,0,0,255' | Should -BeTrue
        & $everyPixel (Read-WgpuTexture (Invoke-WgpuRender $fullscreen 4 4 -VertexCount 3 -CullMode Back)) '255,0,0,255' | Should -BeTrue
    }

    It 'draws with multisampling and depth' -Skip:$noGpu {
        $texture = Invoke-WgpuRender $fullscreen 8 8 -VertexCount 3 -SampleCount 4 -Depth
        & $everyPixel (Read-WgpuTexture $texture) '255,0,0,255' | Should -BeTrue
    }

    It 'saves the drawing as an image file' -Skip:$noGpu {
        $file = Join-Path ([IO.Path]::GetTempPath()) "wgpu-render-$PID.png"
        try {
            Invoke-WgpuRender $fullscreen 16 16 -VertexCount 3 -OutFile $file | Should -BeNullOrEmpty
            (Get-Item $file).Length | Should -BeGreaterThan 0
            & $everyPixel (Read-WgpuTexture (Import-WgpuImage $file)) '255,0,0,255' | Should -BeTrue
        } finally {
            Remove-Item $file -ErrorAction Ignore
        }
    }

    It 'asks for vertices a shader reads' -Skip:$noGpu {
        { Invoke-WgpuRender $colored 8 8 } | Should -Throw '*give them with -Vertex*'
    }

    It 'blends with the factors and operations a Custom blend names, alpha taking the color''s unless given' -Skip:$noGpu {
        $custom = @{ Blend = 'Custom'; BlendSource = 'One'; BlendDestination = 'One'; BlendOperation = 'ReverseSubtract' }
        & $everyPixel (Read-WgpuTexture (Invoke-WgpuRender $fullscreen 4 4 -VertexCount 3 -ClearColor 1, 1, 1, 1 @custom)) '0,255,255,0' | Should -BeTrue
        & $everyPixel (Read-WgpuTexture (Invoke-WgpuRender $fullscreen 4 4 -VertexCount 3 -ClearColor 1, 1, 1, 1 @custom -AlphaBlendOperation Max)) '0,255,255,255' | Should -BeTrue
    }

    It 'refuses a blend factor or operation beside a blend that is not Custom' -Skip:$noGpu {
        { Invoke-WgpuRender $fullscreen 4 4 -VertexCount 3 -Blend Alpha -BlendSource Constant } | Should -Throw '*the blend is Alpha: give -Blend Custom'
        { New-WgpuRenderPipeline $fullscreen -AlphaBlendOperation Max } | Should -Throw '*the blend is Replace: give -Blend Custom'
    }

    It 'mixes the fragment over the target by -BlendConstant' -Skip:$noGpu {
        $mixed = Invoke-WgpuRender $fullscreen 4 4 -VertexCount 3 -ClearColor 0, 0, 1, 1 -Blend Constant -BlendConstant 0.25, 0.25, 0.25, 0.25
        & $nearEveryPixel (Read-WgpuTexture $mixed) '64,0,191,255' | Should -BeTrue
    }

    It 'refuses a blend that reads a constant without -BlendConstant, and -BlendConstant beside a blend that reads none' -Skip:$noGpu {
        { Invoke-WgpuRender $fullscreen 4 4 -VertexCount 3 -Blend Constant } | Should -Throw '*the blend reads a constant: give -BlendConstant'
        { Invoke-WgpuRender $fullscreen 4 4 -VertexCount 3 -Blend Custom -AlphaBlendDestination OneMinusConstant } | Should -Throw '*the blend reads a constant: give -BlendConstant'
        { Invoke-WgpuRender $fullscreen 4 4 -VertexCount 3 -BlendConstant 1, 1, 1, 1 } | Should -Throw '*-BlendConstant sets the constant a blend reads, and this blend reads none'
        { Invoke-WgpuRender $fullscreen 4 4 -VertexCount 3 -Blend Constant -BlendConstant 0.5, 0.5, 0.5 } | Should -Throw '*a blend constant is four numbers, red, green, blue and alpha; 3 were given'
    }
}

Describe 'Render pipelines and passes' {
    It 'reflects the vertex inputs of a pipeline' -Skip:$noGpu {
        $pipeline = New-WgpuRenderPipeline $colored
        $pipeline.VertexInputs -join ',' | Should -Be 'position,color'
        $pipeline.TargetFormats | Should -Be @('Rgba8Unorm')
    }

    It 'draws through a pipeline built with multisampling and depth as it was built' -Skip:$noGpu {
        $pipeline = New-WgpuRenderPipeline $fullscreen -SampleCount 4 -Depth
        $texture = Invoke-WgpuRender $pipeline 8 8 -VertexCount 3
        & $everyPixel (Read-WgpuTexture $texture) '255,0,0,255' | Should -BeTrue
    }

    It 'records a render pass by hand' -Skip:$noGpu {
        $pipeline = New-WgpuRenderPipeline $fullscreen
        $target = New-WgpuTexture 4 4
        $encoder = New-WgpuCommandEncoder
        $pass = $encoder.BeginRenderPass($target, @(0, 0, 0, 1), $null, $null, $null, $null)
        $pass.SetPipeline($pipeline)
        $pass.Draw(3, $null, $null, $null)
        $pass.End()
        $encoder.Finish() | Submit-WgpuCommand
        & $everyPixel (Read-WgpuTexture $target) '255,0,0,255' | Should -BeTrue
    }

    It 'mixes the fragment over the target by the blend constant a pass sets' -Skip:$noGpu {
        $pipeline = New-WgpuRenderPipeline $fullscreen -Blend Constant
        $target = New-WgpuTexture 4 4
        $encoder = New-WgpuCommandEncoder
        $pass = $encoder.BeginRenderPass($target, @(0, 0, 1, 1), $null, $null, $null, $null)
        $pass.SetPipeline($pipeline)
        $pass.SetBlendConstant(0.25, 0.25, 0.25, 0.25)
        $pass.Draw(3, $null, $null, $null)
        $pass.End()
        $encoder.Finish() | Submit-WgpuCommand
        & $nearEveryPixel (Read-WgpuTexture $target) '64,0,191,255' | Should -BeTrue
    }
}
