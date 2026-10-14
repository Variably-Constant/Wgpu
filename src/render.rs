//! Render pipelines and render passes: drawing offscreen into textures and
//! image files.

use std::path::Path;
use std::sync::Arc;

use pwrs::prelude::*;

use crate::binding::{self, BindGroup, Bound};
use crate::buffer::{self, Buffer};
use crate::device::{core_for, CoreRef, Device};
use crate::element::{self, ElementType};
use crate::encoder::{CommandEncoder, QuerySet};
use crate::error::{invalid, invalid_operation, not_found};
use crate::image_io;
use crate::pipeline::{self, Group};
use crate::shader::{self, Parsed, Reflected, ShaderBinding};
use crate::texfile::Said;
use crate::texture::{self, Texture, TextureView};

/// How vertices make primitives.
#[psenum(name = "Wgpu.PrimitiveTopology")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PrimitiveTopology {
    /// Each vertex is a point.
    PointList,
    /// Each pair of vertices is a line.
    LineList,
    /// Each vertex after the first ends a line from the one before.
    LineStrip,
    /// Each three vertices are a triangle.
    #[default]
    TriangleList,
    /// Each vertex after the second ends a triangle with the two before.
    TriangleStrip,
}

/// Which faces a render pipeline discards. A triangle whose corners turn
/// counterclockwise on the target faces the viewer.
#[psenum(name = "Wgpu.CullMode")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CullMode {
    /// Draws both faces.
    #[default]
    None,
    /// Discards faces wound counterclockwise, which face the viewer.
    Front,
    /// Discards faces wound clockwise, which face away.
    Back,
}

/// How a fragment's color combines with the color under it.
#[psenum(name = "Wgpu.BlendMode")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BlendMode {
    /// The fragment replaces the color.
    #[default]
    Replace,
    /// Alpha blending: the fragment over the color by its alpha.
    Alpha,
    /// The fragment adds to the color.
    Additive,
    /// Alpha blending of a fragment whose color is multiplied by its alpha.
    Premultiplied,
    /// The fragment and the color mixed by the blend constant a draw sets,
    /// by -BlendConstant or SetBlendConstant: the fragment times the
    /// constant plus the color times one minus it.
    Constant,
    /// The factors and operations a custom blend names: -BlendSource,
    /// -BlendDestination and -BlendOperation, and their alpha forms.
    Custom,
}

/// What a blend multiplies the fragment's color (source) or the color
/// under it (destination) by.
#[psenum(name = "Wgpu.BlendFactor")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BlendFactor {
    /// 0.
    Zero,
    /// 1.
    #[default]
    One,
    /// The fragment's color.
    Src,
    /// 1 minus the fragment's color.
    OneMinusSrc,
    /// The fragment's alpha.
    SrcAlpha,
    /// 1 minus the fragment's alpha.
    OneMinusSrcAlpha,
    /// The color under the fragment.
    Dst,
    /// 1 minus the color under the fragment.
    OneMinusDst,
    /// The alpha under the fragment.
    DstAlpha,
    /// 1 minus the alpha under the fragment.
    OneMinusDstAlpha,
    /// The smaller of the fragment's alpha and 1 minus the alpha under it;
    /// 1 for alpha.
    SrcAlphaSaturated,
    /// The blend constant a draw sets, by -BlendConstant or SetBlendConstant.
    Constant,
    /// 1 minus the blend constant.
    OneMinusConstant,
    /// The fragment's second color, `@blend_src(1)`; needs the
    /// DualSourceBlending feature.
    Src1,
    /// 1 minus the fragment's second color.
    OneMinusSrc1,
    /// The fragment's second color's alpha.
    Src1Alpha,
    /// 1 minus the fragment's second color's alpha.
    OneMinusSrc1Alpha,
}

impl BlendFactor {
    fn wgpu(self) -> wgpu::BlendFactor {
        match self {
            BlendFactor::Zero => wgpu::BlendFactor::Zero,
            BlendFactor::One => wgpu::BlendFactor::One,
            BlendFactor::Src => wgpu::BlendFactor::Src,
            BlendFactor::OneMinusSrc => wgpu::BlendFactor::OneMinusSrc,
            BlendFactor::SrcAlpha => wgpu::BlendFactor::SrcAlpha,
            BlendFactor::OneMinusSrcAlpha => wgpu::BlendFactor::OneMinusSrcAlpha,
            BlendFactor::Dst => wgpu::BlendFactor::Dst,
            BlendFactor::OneMinusDst => wgpu::BlendFactor::OneMinusDst,
            BlendFactor::DstAlpha => wgpu::BlendFactor::DstAlpha,
            BlendFactor::OneMinusDstAlpha => wgpu::BlendFactor::OneMinusDstAlpha,
            BlendFactor::SrcAlphaSaturated => wgpu::BlendFactor::SrcAlphaSaturated,
            BlendFactor::Constant => wgpu::BlendFactor::Constant,
            BlendFactor::OneMinusConstant => wgpu::BlendFactor::OneMinusConstant,
            BlendFactor::Src1 => wgpu::BlendFactor::Src1,
            BlendFactor::OneMinusSrc1 => wgpu::BlendFactor::OneMinusSrc1,
            BlendFactor::Src1Alpha => wgpu::BlendFactor::Src1Alpha,
            BlendFactor::OneMinusSrc1Alpha => wgpu::BlendFactor::OneMinusSrc1Alpha,
        }
    }
}

/// How a blend combines the fragment's weighted color with the weighted
/// color under it.
#[psenum(name = "Wgpu.BlendOperation")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BlendOperation {
    /// Source plus destination.
    #[default]
    Add,
    /// Source minus destination.
    Subtract,
    /// Destination minus source.
    ReverseSubtract,
    /// The smaller of source and destination; both factors must be One.
    Min,
    /// The larger of source and destination; both factors must be One.
    Max,
}

impl BlendOperation {
    fn wgpu(self) -> wgpu::BlendOperation {
        match self {
            BlendOperation::Add => wgpu::BlendOperation::Add,
            BlendOperation::Subtract => wgpu::BlendOperation::Subtract,
            BlendOperation::ReverseSubtract => wgpu::BlendOperation::ReverseSubtract,
            BlendOperation::Min => wgpu::BlendOperation::Min,
            BlendOperation::Max => wgpu::BlendOperation::Max,
        }
    }
}

/// The factors and operations of a Custom blend. The color ones are One,
/// Zero and Add when omitted, which replaces the color, and each alpha one
/// is its color one when omitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct CustomBlend {
    /// What the fragment's color is multiplied by.
    pub source: Option<BlendFactor>,
    /// What the color under the fragment is multiplied by.
    pub destination: Option<BlendFactor>,
    /// How the two products combine.
    pub operation: Option<BlendOperation>,
    /// What the fragment's alpha is multiplied by.
    pub alpha_source: Option<BlendFactor>,
    /// What the alpha under the fragment is multiplied by.
    pub alpha_destination: Option<BlendFactor>,
    /// How the two alpha products combine.
    pub alpha_operation: Option<BlendOperation>,
}

impl CustomBlend {
    fn is_empty(&self) -> bool {
        *self == CustomBlend::default()
    }
}

/// The blend state `mode` stands for, with `custom` naming a Custom
/// blend's factors and operations. A factor or operation given with any
/// other mode is refused.
pub(crate) fn blend_state(mode: BlendMode, custom: &CustomBlend) -> PsResult<Option<wgpu::BlendState>> {
    if mode != BlendMode::Custom && !custom.is_empty() {
        return Err(invalid(
            "WgpuBlend",
            format!("-BlendSource, -BlendDestination, -BlendOperation and their alpha forms name a Custom blend, and the blend is {mode:?}: give -Blend Custom"),
        ));
    }
    let both = |source: wgpu::BlendFactor, destination: wgpu::BlendFactor| {
        let component = wgpu::BlendComponent { src_factor: source, dst_factor: destination, operation: wgpu::BlendOperation::Add };
        Some(wgpu::BlendState { color: component, alpha: component })
    };
    Ok(match mode {
        BlendMode::Replace => None,
        BlendMode::Alpha => Some(wgpu::BlendState::ALPHA_BLENDING),
        BlendMode::Premultiplied => Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
        BlendMode::Additive => both(wgpu::BlendFactor::One, wgpu::BlendFactor::One),
        BlendMode::Constant => both(wgpu::BlendFactor::Constant, wgpu::BlendFactor::OneMinusConstant),
        BlendMode::Custom => {
            let source = custom.source.unwrap_or(BlendFactor::One);
            let destination = custom.destination.unwrap_or(BlendFactor::Zero);
            let operation = custom.operation.unwrap_or(BlendOperation::Add);
            let component = |s: BlendFactor, d: BlendFactor, o: BlendOperation| wgpu::BlendComponent { src_factor: s.wgpu(), dst_factor: d.wgpu(), operation: o.wgpu() };
            Some(wgpu::BlendState {
                color: component(source, destination, operation),
                alpha: component(custom.alpha_source.unwrap_or(source), custom.alpha_destination.unwrap_or(destination), custom.alpha_operation.unwrap_or(operation)),
            })
        }
    })
}

/// Whether `blend` reads the blend constant: a Constant or
/// OneMinusConstant factor on its color or its alpha. A draw through a
/// pipeline whose blend reads it must set it.
pub(crate) fn reads_constant(blend: Option<wgpu::BlendState>) -> bool {
    let constant = |f: wgpu::BlendFactor| matches!(f, wgpu::BlendFactor::Constant | wgpu::BlendFactor::OneMinusConstant);
    blend.is_some_and(|b| [b.color, b.alpha].iter().any(|c| constant(c.src_factor) || constant(c.dst_factor)))
}

/// One `@location` input of a vertex shader, fed from a buffer of its own.
#[derive(Clone)]
pub(crate) struct VertexInput {
    pub(crate) location: u32,
    pub(crate) name: String,
    pub(crate) format: wgpu::VertexFormat,
    pub(crate) element: ElementType,
    pub(crate) components: u32,
}

fn vertex_format(scalar: naga::Scalar, components: u32) -> Option<(wgpu::VertexFormat, ElementType)> {
    use naga::ScalarKind as K;
    use wgpu::VertexFormat as V;
    Some(match (scalar.kind, scalar.width, components) {
        (K::Float, 4, 1) => (V::Float32, ElementType::Float32),
        (K::Float, 4, 2) => (V::Float32x2, ElementType::Float32),
        (K::Float, 4, 3) => (V::Float32x3, ElementType::Float32),
        (K::Float, 4, 4) => (V::Float32x4, ElementType::Float32),
        (K::Float, 8, 1) => (V::Float64, ElementType::Float64),
        (K::Float, 8, 2) => (V::Float64x2, ElementType::Float64),
        (K::Float, 8, 3) => (V::Float64x3, ElementType::Float64),
        (K::Float, 8, 4) => (V::Float64x4, ElementType::Float64),
        (K::Float, 2, 2) => (V::Float16x2, ElementType::Float16),
        (K::Float, 2, 4) => (V::Float16x4, ElementType::Float16),
        (K::Uint, 4, 1) => (V::Uint32, ElementType::UInt32),
        (K::Uint, 4, 2) => (V::Uint32x2, ElementType::UInt32),
        (K::Uint, 4, 3) => (V::Uint32x3, ElementType::UInt32),
        (K::Uint, 4, 4) => (V::Uint32x4, ElementType::UInt32),
        (K::Sint, 4, 1) => (V::Sint32, ElementType::Int32),
        (K::Sint, 4, 2) => (V::Sint32x2, ElementType::Int32),
        (K::Sint, 4, 3) => (V::Sint32x3, ElementType::Int32),
        (K::Sint, 4, 4) => (V::Sint32x4, ElementType::Int32),
        _ => return None,
    })
}

/// The `@location` inputs of a vertex entry point, in location order.
pub(crate) fn vertex_inputs(module: &naga::Module, entry: usize) -> PsResult<Vec<VertexInput>> {
    let function = &module.entry_points[entry].function;
    let mut inputs = Vec::new();
    let mut take = |name: String, ty: naga::Handle<naga::Type>, binding: &Option<naga::Binding>| -> PsResult<()> {
        let Some(naga::Binding::Location { location, .. }) = binding else { return Ok(()) };
        let (scalar, components) = match &module.types[ty].inner {
            naga::TypeInner::Scalar(s) => (*s, 1),
            naga::TypeInner::Vector { size, scalar } => (*scalar, *size as u32),
            other => return Err(invalid("WgpuVertexInput", format!("vertex input {name} is {other:?}, which no vertex format holds"))),
        };
        let (format, element) = vertex_format(scalar, components).ok_or_else(|| invalid("WgpuVertexInput", format!("vertex input {name} has no vertex format")))?;
        inputs.push(VertexInput { location: *location, name, format, element, components });
        Ok(())
    };
    for (i, argument) in function.arguments.iter().enumerate() {
        let arg_name = match &argument.name {
            Some(n) => n.clone(),
            None => format!("input{i}"),
        };
        match &module.types[argument.ty].inner {
            naga::TypeInner::Struct { members, .. } if argument.binding.is_none() => {
                for (j, member) in members.iter().enumerate() {
                    let member_name = match &member.name {
                        Some(n) => n.clone(),
                        None => format!("{arg_name}{j}"),
                    };
                    take(member_name, member.ty, &member.binding)?;
                }
            }
            _ => take(arg_name, argument.ty, &argument.binding)?,
        }
    }
    inputs.sort_by_key(|i| i.location);
    Ok(inputs)
}

/// What a render pipeline is built with.
#[derive(Clone)]
pub(crate) struct RenderSpec {
    pub(crate) vertex_entry: Option<String>,
    pub(crate) fragment_entry: Option<String>,
    pub(crate) formats: Vec<wgpu::TextureFormat>,
    pub(crate) depth: Option<wgpu::TextureFormat>,
    pub(crate) topology: PrimitiveTopology,
    pub(crate) cull: CullMode,
    pub(crate) blend: Option<wgpu::BlendState>,
    pub(crate) sample_count: u32,
    pub(crate) constants: Vec<(String, f64)>,
}

/// A render pipeline and what it was built from.
pub(crate) struct RenderInner {
    pub(crate) core: CoreRef,
    pub(crate) pipeline: wgpu::RenderPipeline,
    pub(crate) groups: Vec<Group>,
    pub(crate) bindings: Vec<Reflected>,
    pub(crate) inputs: Vec<VertexInput>,
    pub(crate) spec: RenderSpec,
    pub(crate) vertex_entry: String,
    pub(crate) fragment_entry: String,
    pub(crate) label: String,
}

/// The bindings a vertex and a fragment entry point use between them, in
/// group and binding order, each once.
pub(crate) fn render_bindings(parsed: &Parsed, vertex: usize, fragment: usize) -> Vec<Reflected> {
    let mut bindings = shader::reflect(&parsed.module, &parsed.info, Some(vertex));
    for b in shader::reflect(&parsed.module, &parsed.info, Some(fragment)) {
        if !bindings.iter().any(|x| x.group == b.group && x.binding == b.binding) {
            bindings.push(b);
        }
    }
    bindings.sort_by_key(|b| (b.group, b.binding));
    bindings
}

pub(crate) fn build_render(core: &CoreRef, parsed: &Arc<Parsed>, spec: RenderSpec, label: Option<String>) -> PsResult<Arc<RenderInner>> {
    let module = &parsed.module;
    let vertex = shader::entry_index(module, spec.vertex_entry.as_deref(), naga::ShaderStage::Vertex)?;
    let fragment = shader::entry_index(module, spec.fragment_entry.as_deref(), naga::ShaderStage::Fragment)?;
    let vertex_entry = module.entry_points[vertex].name.clone();
    let fragment_entry = module.entry_points[fragment].name.clone();
    let label = match label {
        Some(l) => l,
        None => format!("{} {vertex_entry}/{fragment_entry}", parsed.label),
    };
    let bindings = render_bindings(parsed, vertex, fragment);
    let inputs = vertex_inputs(module, vertex)?;
    let groups = pipeline::layouts(core, &bindings, wgpu::ShaderStages::VERTEX_FRAGMENT, &label)?;
    let shader_module = shader::module_on(core, parsed)?;
    let constants: Vec<(&str, f64)> = spec.constants.iter().map(|(k, v)| (k.as_str(), *v)).collect();
    let attributes: Vec<[wgpu::VertexAttribute; 1]> = inputs.iter().map(|i| [wgpu::VertexAttribute { format: i.format, offset: 0, shader_location: i.location }]).collect();
    let buffers: Vec<Option<wgpu::VertexBufferLayout>> = inputs
        .iter()
        .zip(&attributes)
        .map(|(i, a)| Some(wgpu::VertexBufferLayout { array_stride: i.format.size(), step_mode: wgpu::VertexStepMode::Vertex, attributes: a }))
        .collect();
    let targets: Vec<Option<wgpu::ColorTargetState>> = spec
        .formats
        .iter()
        .map(|f| Some(wgpu::ColorTargetState { format: *f, blend: spec.blend, write_mask: wgpu::ColorWrites::ALL }))
        .collect();
    let topology = match spec.topology {
        PrimitiveTopology::PointList => wgpu::PrimitiveTopology::PointList,
        PrimitiveTopology::LineList => wgpu::PrimitiveTopology::LineList,
        PrimitiveTopology::LineStrip => wgpu::PrimitiveTopology::LineStrip,
        PrimitiveTopology::TriangleList => wgpu::PrimitiveTopology::TriangleList,
        PrimitiveTopology::TriangleStrip => wgpu::PrimitiveTopology::TriangleStrip,
    };
    let cull_mode = match spec.cull {
        CullMode::None => None,
        CullMode::Front => Some(wgpu::Face::Front),
        CullMode::Back => Some(wgpu::Face::Back),
    };
    let pipeline = core.scoped("WgpuRenderPipeline", "the render pipeline cannot be built", || {
        let group_layouts: Vec<Option<&wgpu::BindGroupLayout>> = groups.iter().map(|g| Some(&g.layout)).collect();
        let layout = core.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some(&label), bind_group_layouts: &group_layouts, immediate_size: 0 });
        core.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(&label),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader_module,
                entry_point: Some(&vertex_entry),
                compilation_options: wgpu::PipelineCompilationOptions { constants: &constants, zero_initialize_workgroup_memory: true },
                buffers: &buffers,
            },
            primitive: wgpu::PrimitiveState { topology, cull_mode, ..Default::default() },
            depth_stencil: spec.depth.map(|format| wgpu::DepthStencilState {
                format,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState { count: spec.sample_count, mask: !0, alpha_to_coverage_enabled: false },
            fragment: Some(wgpu::FragmentState {
                module: &shader_module,
                entry_point: Some(&fragment_entry),
                compilation_options: wgpu::PipelineCompilationOptions { constants: &constants, zero_initialize_workgroup_memory: true },
                targets: &targets,
            }),
            multiview_mask: None,
            cache: None,
        })
    })?;
    Ok(Arc::new(RenderInner { core: core.clone(), pipeline, groups, bindings, inputs, spec, vertex_entry, fragment_entry, label }))
}

/// A render pipeline, built for one device: a vertex and a fragment
/// entry point with their vertex inputs, bindings and target formats.
#[psclass(name = "Wgpu.RenderPipeline", mode = proxy, columns = ["Label", "VertexEntryPoint", "FragmentEntryPoint", "TargetFormats"])]
pub struct RenderPipeline {
    /// The pipeline's label.
    pub label: String,
    /// The vertex entry point.
    pub vertex_entry_point: String,
    /// The fragment entry point.
    pub fragment_entry_point: String,
    /// The vertex inputs, in location order, each fed from a buffer of its own.
    pub vertex_inputs: Vec<String>,
    /// The bindings both stages use, in group and binding order.
    pub bindings: Vec<ShaderBinding>,
    /// The formats of its color targets.
    pub target_formats: Vec<String>,
    /// The format of its depth target, or `$null`.
    pub depth_format: Option<String>,
    /// Samples per pixel.
    pub sample_count: u32,
    /// The adapter of the device it was built on.
    pub adapter: String,
    #[psfield(skip)]
    pub(crate) inner: Arc<RenderInner>,
}

impl RenderPipeline {
    pub(crate) fn from_inner(inner: Arc<RenderInner>) -> RenderPipeline {
        RenderPipeline {
            label: inner.label.clone(),
            vertex_entry_point: inner.vertex_entry.clone(),
            fragment_entry_point: inner.fragment_entry.clone(),
            vertex_inputs: inner.inputs.iter().map(|i| i.name.clone()).collect(),
            bindings: inner.bindings.iter().map(Reflected::describe).collect(),
            target_formats: inner.spec.formats.iter().map(|f| texture::format_name(*f)).collect(),
            depth_format: inner.spec.depth.map(texture::format_name),
            sample_count: inner.spec.sample_count,
            adapter: inner.core.info.name.clone(),
            inner,
        }
    }
}

#[psmethods]
impl RenderPipeline {
    /// The bind group `group` of this pipeline, binding `resources`.
    pub fn bind_group(&self, group: u32, resources: PsObject) -> PsResult<BindGroup> {
        binding::bind_group_from(&self.inner.core, &self.inner.groups, group, &resources, &self.inner.label)
    }
}

/// The vertex buffers of a draw, from arrays or buffers by input name or
/// in location order; and the vertex count the first array gives.
fn vertex_buffers(core: &CoreRef, inputs: &[VertexInput], vertices: Option<&PsObject>) -> PsResult<(Vec<wgpu::Buffer>, Option<u32>)> {
    let Some(vertices) = vertices else {
        if inputs.is_empty() {
            return Ok((Vec::new(), None));
        }
        return Err(invalid("WgpuNoVertices", format!("the vertex shader reads {}; give them with -Vertex", inputs.iter().map(|i| i.name.as_str()).collect::<Vec<_>>().join(", "))));
    };
    let type_name = vertices.type_name()?;
    let values: Vec<PsObject> = if type_name == "System.Collections.Hashtable" || type_name == "System.Collections.Specialized.OrderedDictionary" {
        let table = PsHashtable(vertices.clone());
        let mut out = Vec::with_capacity(inputs.len());
        for input in inputs {
            let key = table.keys()?.into_iter().find(|k| k.eq_ignore_ascii_case(&input.name)).ok_or_else(|| not_found("WgpuNoVertices", format!("-Vertex gives no values for {}", input.name)))?;
            out.push(table.get(&key)?);
        }
        out
    } else if inputs.len() == 1 {
        vec![vertices.clone()]
    } else {
        Vec::<PsObject>::from_ps(vertices)?
    };
    if values.len() != inputs.len() {
        return Err(invalid("WgpuVertexCount", format!("the vertex shader reads {} inputs and {} were given", inputs.len(), values.len())));
    }
    let mut count = None;
    let mut buffers = Vec::with_capacity(inputs.len());
    for (input, value) in inputs.iter().zip(&values) {
        if value.type_name()? == "Wgpu.Buffer" {
            let (raw, length) = PsProxy::<Buffer>::from_ps(value)?.with(|b| (b.buffer.clone(), b.length))?;
            count.get_or_insert((length / input.components as u64) as u32);
            buffers.push(raw);
            continue;
        }
        let raw = element::with_host_bytes(value, Some(input.element), |ty, bytes| {
            count.get_or_insert((bytes.len() as u64 / ty.size() / input.components as u64) as u32);
            buffer::create_raw(core, bytes.len() as u64, wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST, &input.name, Some(bytes))
        })?;
        buffers.push(raw);
    }
    Ok((buffers, count))
}

/// What one draw sets on a render pass: its bind groups, one vertex buffer
/// per vertex input in location order, its indices, the vertices and
/// instances it draws, and the color a blend factor of Constant reads,
/// zero when `None`.
pub(crate) struct DrawCall<'a> {
    pub(crate) groups: &'a [(u32, wgpu::BindGroup)],
    pub(crate) vertex_buffers: &'a [wgpu::Buffer],
    pub(crate) index: Option<(&'a wgpu::Buffer, wgpu::IndexFormat, u32)>,
    pub(crate) vertex_count: u32,
    pub(crate) instances: u32,
    pub(crate) blend_constant: Option<wgpu::Color>,
}

/// Sets `pipeline` and what `call` reads on `pass`, and draws.
pub(crate) fn draw_on(pass: &mut wgpu::RenderPass<'_>, pipeline: &wgpu::RenderPipeline, call: &DrawCall<'_>) {
    pass.set_pipeline(pipeline);
    if let Some(constant) = call.blend_constant {
        pass.set_blend_constant(constant);
    }
    for (index, group) in call.groups {
        pass.set_bind_group(*index, group, &[]);
    }
    for (slot, buffer) in call.vertex_buffers.iter().enumerate() {
        pass.set_vertex_buffer(slot as u32, buffer.slice(..));
    }
    match call.index {
        Some((raw, format, count)) => {
            pass.set_index_buffer(raw.slice(..), format);
            pass.draw_indexed(0..count, 0, 0..call.instances);
        }
        None => pass.draw(0..call.vertex_count, 0..call.instances),
    }
}

/// A size-matched attachment for `target`, of the pipeline's sample count.
fn attachment(core: &CoreRef, target: &wgpu::Texture, format: wgpu::TextureFormat, samples: u32, what: &str) -> PsResult<wgpu::Texture> {
    core.scoped("WgpuTexture", &format!("the {what} target cannot be created"), || {
        core.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(what),
            size: wgpu::Extent3d { width: target.width(), height: target.height(), depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: samples,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
    })
}

/// Records one draw of `inner` into `target`, which holds what `load`
/// says before it. A pipeline built with more than one sample draws into a
/// multisampled target resolved into `target`, and one built with depth
/// tests against a Depth32Float target.
pub(crate) fn record_draw(inner: &RenderInner, target: &wgpu::Texture, call: &DrawCall<'_>, load: wgpu::LoadOp<wgpu::Color>) -> PsResult<wgpu::CommandBuffer> {
    let core = &inner.core;
    let samples = inner.spec.sample_count;
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let multisampled = if samples > 1 { Some(attachment(core, target, target.format(), samples, "multisampled")?) } else { None };
    let depth = match inner.spec.depth {
        Some(format) => Some(attachment(core, target, format, samples, "depth")?),
        None => None,
    };
    core.scoped("WgpuDraw", "the draw is invalid", || {
        let ms_view = multisampled.as_ref().map(|t| t.create_view(&wgpu::TextureViewDescriptor::default()));
        let depth_view = depth.as_ref().map(|t| t.create_view(&wgpu::TextureViewDescriptor::default()));
        let mut encoder = core.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some(&inner.label) });
        {
            let (color, resolve) = match &ms_view {
                Some(ms) => (ms, Some(&view)),
                None => (&view, None),
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(&inner.label),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color,
                    depth_slice: None,
                    resolve_target: resolve,
                    ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: depth_view.as_ref().map(|v| wgpu::RenderPassDepthStencilAttachment {
                    view: v,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            draw_on(&mut pass, &inner.pipeline, call);
        }
        encoder.finish()
    })
}

fn color_of(values: &Option<Vec<f64>>) -> PsResult<wgpu::Color> {
    match values.as_deref() {
        None => Ok(wgpu::Color::BLACK),
        Some([r, g, b]) => Ok(wgpu::Color { r: *r, g: *g, b: *b, a: 1.0 }),
        Some([r, g, b, a]) => Ok(wgpu::Color { r: *r, g: *g, b: *b, a: *a }),
        Some(other) => Err(invalid("WgpuClearColor", format!("a clear color is three or four numbers, red, green, blue and alpha; {} were given", other.len()))),
    }
}

/// The color -BlendConstant names: four numbers, red, green, blue and
/// alpha.
fn constant_of(values: &Option<Vec<f64>>) -> PsResult<Option<wgpu::Color>> {
    match values.as_deref() {
        None => Ok(None),
        Some([r, g, b, a]) => Ok(Some(wgpu::Color { r: *r, g: *g, b: *b, a: *a })),
        Some(other) => Err(invalid("WgpuBlendConstant", format!("a blend constant is four numbers, red, green, blue and alpha; {} were given", other.len()))),
    }
}

/// Draws offscreen with a vertex and a fragment shader into a texture, or
/// into an image file.
///
/// The shader is WGSL text or a Wgpu.Shader holding a vertex and a
/// fragment entry point, or a pipeline from New-WgpuRenderPipeline; GLSL
/// holds one stage, so it cannot draw. Each `@location` input of the
/// vertex shader reads an array or buffer of its own from -Vertex, by
/// name or in location order; a shader with no inputs draws -VertexCount
/// vertices from vertex_index alone. -Binding gives the resources both
/// stages use.
///
/// # Examples
/// Invoke-WgpuRender $wgsl -Width 256 -Height 256 -VertexCount 3 -OutFile .\triangle.png
/// Invoke-WgpuRender $wgsl 512 512 -Vertex @{ position = $xyz; color = $rgb } -Depth -SampleCount 4
/// $tex = Invoke-WgpuRender $pipeline -Target $tex -Vertex $positions -Index ([uint32[]](0, 1, 2, 2, 1, 3))
#[cmdlet(verb = "Invoke", noun = "WgpuRender", alias = ["Invoke-GpuRender"], output = ["Wgpu.Texture"])]
#[derive(Default)]
pub struct InvokeWgpuRender {
    /// WGSL text or a Wgpu.Shader with a vertex and a fragment entry point, or a Wgpu.RenderPipeline.
    #[param(mandatory, position = 0, raw)]
    pub shader: Option<PsObject>,
    /// The width of the texture drawn into, when -Target is not given.
    #[param(position = 1)]
    pub width: Option<u32>,
    /// The height of the texture drawn into, when -Target is not given.
    #[param(position = 2)]
    pub height: Option<u32>,
    /// A texture to draw into instead of a new one.
    #[param]
    pub target: Option<PsProxy<Texture>>,
    /// The format of a new target texture; Rgba8Unorm when omitted.
    #[param]
    pub format: Option<String>,
    /// The vertex inputs: a hashtable by input name, or arrays or buffers in location order.
    #[param(raw)]
    pub vertex: Option<PsObject>,
    /// Vertices to draw; from the first vertex input when omitted.
    #[param]
    pub vertex_count: Option<u32>,
    /// Instances to draw; 1 when omitted.
    #[param]
    pub instance_count: Option<u32>,
    /// Indices to draw by: uint[] or ushort[], or a Wgpu.Buffer of either.
    #[param(raw)]
    pub index: Option<PsObject>,
    /// The resources for the shaders' bindings.
    #[param(raw)]
    pub binding: Option<PsObject>,
    /// The color the target is cleared to, as red, green, blue and alpha; opaque black when omitted.
    #[param]
    pub clear_color: Option<Vec<f64>>,
    /// How vertices make primitives; TriangleList when omitted.
    #[param]
    pub topology: Option<PrimitiveTopology>,
    /// Which faces to discard; none when omitted.
    #[param]
    pub cull_mode: Option<CullMode>,
    /// How fragments combine with the target; Replace when omitted. Constant
    /// mixes by -BlendConstant, and Custom takes -BlendSource,
    /// -BlendDestination and -BlendOperation.
    #[param]
    pub blend: Option<BlendMode>,
    /// With -Blend Custom, what the fragment's color is multiplied by; One when omitted.
    #[param]
    pub blend_source: Option<BlendFactor>,
    /// With -Blend Custom, what the color under the fragment is multiplied by; Zero when omitted.
    #[param]
    pub blend_destination: Option<BlendFactor>,
    /// With -Blend Custom, how the two products combine; Add when omitted.
    #[param]
    pub blend_operation: Option<BlendOperation>,
    /// With -Blend Custom, what the fragment's alpha is multiplied by; -BlendSource when omitted.
    #[param]
    pub alpha_blend_source: Option<BlendFactor>,
    /// With -Blend Custom, what the alpha under the fragment is multiplied by; -BlendDestination when omitted.
    #[param]
    pub alpha_blend_destination: Option<BlendFactor>,
    /// With -Blend Custom, how the two alpha products combine; -BlendOperation when omitted.
    #[param]
    pub alpha_blend_operation: Option<BlendOperation>,
    /// The color a blend factor of Constant reads, as red, green, blue and alpha; zero when omitted.
    #[param]
    pub blend_constant: Option<Vec<f64>>,
    /// Test and write depth in a Depth32Float target; a pipeline from New-WgpuRenderPipeline draws as it was built.
    #[param]
    pub depth: bool,
    /// Samples per pixel, resolved into the target; 1 when omitted. A pipeline from New-WgpuRenderPipeline draws with its own.
    #[param]
    pub sample_count: Option<u32>,
    /// The vertex entry point; the shader's only one when omitted.
    #[param]
    pub vertex_entry: Option<String>,
    /// The fragment entry point; the shader's only one when omitted.
    #[param]
    pub fragment_entry: Option<String>,
    /// Override constant values, by name or `@id`.
    #[param]
    pub constant: Option<PsHashtable>,
    /// Save the result as an image file.
    #[param]
    pub out_file: Option<String>,
    /// Write the texture even when -OutFile is given.
    #[param]
    pub pass_thru: bool,
    /// The device to draw on; the default device when omitted.
    #[param]
    pub device: Option<PsProxy<Device>>,
}

impl Cmdlet for InvokeWgpuRender {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let Some(shader) = &self.shader else {
            return Err(invalid("WgpuNoShader", "no shader was given"));
        };
        let core = core_for(&self.device, ps)?;
        let target = match &self.target {
            Some(t) => t.with(|t| Texture::wrap(&t.core, t.texture.clone(), t.label.clone()))?,
            None => {
                let (Some(width), Some(height)) = (self.width, self.height) else {
                    return Err(invalid("WgpuNoTarget", "give -Width and -Height, or a -Target texture"));
                };
                let format = match &self.format {
                    Some(name) => texture::parse_format(name)?,
                    None => wgpu::TextureFormat::Rgba8Unorm,
                };
                let spec = texture::TextureSpec {
                    width,
                    height,
                    depth_or_layers: 1,
                    format,
                    mip_levels: 1,
                    sample_count: 1,
                    dimension: texture::TextureDimension::D2,
                    usage: None,
                    view_formats: Vec::new(),
                    label: Some("Invoke-WgpuRender".to_string()),
                };
                texture::create(&core, &spec)?
            }
        };
        let samples = self.sample_count.unwrap_or(1);
        let inner = if shader.type_name()? == "Wgpu.RenderPipeline" {
            PsProxy::<RenderPipeline>::from_ps(shader)?.with(|p| p.inner.clone())?
        } else {
            let parsed = pipeline::parsed_of(shader, None, None)?;
            let spec = RenderSpec {
                vertex_entry: self.vertex_entry.clone(),
                fragment_entry: self.fragment_entry.clone(),
                formats: vec![target.texture.format()],
                depth: if self.depth { Some(wgpu::TextureFormat::Depth32Float) } else { None },
                topology: self.topology.unwrap_or(PrimitiveTopology::TriangleList),
                cull: self.cull_mode.unwrap_or(CullMode::None),
                blend: blend_state(
                    self.blend.unwrap_or(BlendMode::Replace),
                    &CustomBlend {
                        source: self.blend_source,
                        destination: self.blend_destination,
                        operation: self.blend_operation,
                        alpha_source: self.alpha_blend_source,
                        alpha_destination: self.alpha_blend_destination,
                        alpha_operation: self.alpha_blend_operation,
                    },
                )?,
                sample_count: samples,
                constants: pipeline::constants_of(self.constant.as_ref())?,
            };
            build_render(&core, &parsed, spec, None)?
        };
        let blend_constant = constant_of(&self.blend_constant)?;
        let reads = reads_constant(inner.spec.blend);
        if reads && blend_constant.is_none() {
            return Err(invalid("WgpuBlendConstant", "the blend reads a constant: give -BlendConstant"));
        }
        if !reads && blend_constant.is_some() {
            return Err(invalid("WgpuBlendConstant", "-BlendConstant sets the constant a blend reads, and this blend reads none"));
        }
        let (vertex_buffers, counted) = vertex_buffers(&core, &inner.inputs, self.vertex.as_ref())?;
        let bound: Vec<Bound> = binding::resolve_all(&core, &inner.bindings, self.binding.as_ref())?;
        let groups = binding::make_groups(&core, &inner.groups, &bound, &inner.label)?;
        let index = match &self.index {
            Some(value) if value.type_name()? == "Wgpu.Buffer" => {
                let (raw, ty, length) = PsProxy::<Buffer>::from_ps(value)?.with(|b| (b.buffer.clone(), b.element_type, b.length))?;
                Some((raw, ty, length as u32))
            }
            Some(value) => Some(element::with_host_bytes(value, None, |ty, bytes| {
                let ty = if ty == ElementType::UInt16 { ty } else { ElementType::UInt32 };
                let converted = element::convert(ElementType::UInt32, bytes, ty);
                let source = if ty == ElementType::UInt16 { bytes.to_vec() } else { converted };
                let raw = buffer::create_raw(&core, source.len() as u64, wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST, "indices", Some(&source))?;
                Ok((raw, ty, (source.len() as u64 / ty.size()) as u32))
            })?),
            None => None,
        };
        let vertex_count = match (self.vertex_count, counted) {
            (Some(n), _) => n,
            (None, Some(n)) => n,
            (None, None) => return Err(invalid("WgpuVertexCount", "the shader reads no vertex inputs; give -VertexCount")),
        };
        let index = index.as_ref().map(|(raw, ty, count)| (raw, if *ty == ElementType::UInt16 { wgpu::IndexFormat::Uint16 } else { wgpu::IndexFormat::Uint32 }, *count));
        let call = DrawCall {
            groups: &groups,
            vertex_buffers: &vertex_buffers,
            index,
            vertex_count,
            instances: self.instance_count.unwrap_or(1),
            blend_constant,
        };
        let commands = record_draw(&inner, &target.texture, &call, wgpu::LoadOp::Clear(color_of(&self.clear_color)?))?;
        core.submit_and_wait(vec![commands], Some(ps))?;
        if let Some(path) = &self.out_file {
            let resolved = ps.resolve_path(path, true)?;
            let file = resolved.into_iter().next().ok_or_else(|| invalid("WgpuImage", format!("{path} names no file")))?;
            let mut said = Said::default();
            let saved = image_io::save(&core, &target.texture, target.is_cube, Path::new(&file), None, Some(ps), &mut said);
            texture::tell(ps, &said)?;
            saved?;
            if !self.pass_thru {
                return Ok(());
            }
        }
        ps.write(target)
    }
}

/// Builds a render pipeline from a shader with a vertex and a fragment
/// entry point, for one device and one target format.
///
/// # Examples
/// $pipeline = New-WgpuRenderPipeline $wgsl -Format Rgba8Unorm
/// New-WgpuRenderPipeline $wgsl -Depth -SampleCount 4 -Blend Alpha
#[cmdlet(verb = "New", noun = "WgpuRenderPipeline", alias = ["New-GpuRenderPipeline"], output = ["Wgpu.RenderPipeline"])]
#[derive(Default)]
pub struct NewWgpuRenderPipeline {
    /// WGSL text or a Wgpu.Shader with a vertex and a fragment entry point.
    #[param(mandatory, position = 0, value_from_pipeline, raw)]
    pub shader: Option<PsObject>,
    /// The formats of the color targets; Rgba8Unorm when omitted.
    #[param]
    pub format: Option<Vec<String>>,
    /// Test and write depth in a Depth32Float target.
    #[param]
    pub depth: bool,
    /// How vertices make primitives; TriangleList when omitted.
    #[param]
    pub topology: Option<PrimitiveTopology>,
    /// Which faces to discard; none when omitted.
    #[param]
    pub cull_mode: Option<CullMode>,
    /// How fragments combine with the target; Replace when omitted. Constant
    /// mixes by the constant a draw sets, and Custom takes -BlendSource,
    /// -BlendDestination and -BlendOperation.
    #[param]
    pub blend: Option<BlendMode>,
    /// With -Blend Custom, what the fragment's color is multiplied by; One when omitted.
    #[param]
    pub blend_source: Option<BlendFactor>,
    /// With -Blend Custom, what the color under the fragment is multiplied by; Zero when omitted.
    #[param]
    pub blend_destination: Option<BlendFactor>,
    /// With -Blend Custom, how the two products combine; Add when omitted.
    #[param]
    pub blend_operation: Option<BlendOperation>,
    /// With -Blend Custom, what the fragment's alpha is multiplied by; -BlendSource when omitted.
    #[param]
    pub alpha_blend_source: Option<BlendFactor>,
    /// With -Blend Custom, what the alpha under the fragment is multiplied by; -BlendDestination when omitted.
    #[param]
    pub alpha_blend_destination: Option<BlendFactor>,
    /// With -Blend Custom, how the two alpha products combine; -BlendOperation when omitted.
    #[param]
    pub alpha_blend_operation: Option<BlendOperation>,
    /// Samples per pixel; 1 when omitted.
    #[param]
    pub sample_count: Option<u32>,
    /// The vertex entry point; the shader's only one when omitted.
    #[param]
    pub vertex_entry: Option<String>,
    /// The fragment entry point; the shader's only one when omitted.
    #[param]
    pub fragment_entry: Option<String>,
    /// Override constant values, by name or `@id`.
    #[param]
    pub constant: Option<PsHashtable>,
    /// The pipeline's label.
    #[param]
    pub label: Option<String>,
    /// The device to build it on; the default device when omitted.
    #[param]
    pub device: Option<PsProxy<Device>>,
}

impl Cmdlet for NewWgpuRenderPipeline {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let Some(shader) = &self.shader else {
            return Err(invalid("WgpuNoShader", "no shader was given"));
        };
        let core = core_for(&self.device, ps)?;
        let parsed = pipeline::parsed_of(shader, None, None)?;
        let formats = match &self.format {
            Some(list) => list.iter().map(|n| texture::parse_format(n)).collect::<PsResult<Vec<_>>>()?,
            None => vec![wgpu::TextureFormat::Rgba8Unorm],
        };
        let spec = RenderSpec {
            vertex_entry: self.vertex_entry.clone(),
            fragment_entry: self.fragment_entry.clone(),
            formats,
            depth: if self.depth { Some(wgpu::TextureFormat::Depth32Float) } else { None },
            topology: self.topology.unwrap_or(PrimitiveTopology::TriangleList),
            cull: self.cull_mode.unwrap_or(CullMode::None),
            blend: blend_state(
                self.blend.unwrap_or(BlendMode::Replace),
                &CustomBlend {
                    source: self.blend_source,
                    destination: self.blend_destination,
                    operation: self.blend_operation,
                    alpha_source: self.alpha_blend_source,
                    alpha_destination: self.alpha_blend_destination,
                    alpha_operation: self.alpha_blend_operation,
                },
            )?,
            sample_count: self.sample_count.unwrap_or(1),
            constants: pipeline::constants_of(self.constant.as_ref())?,
        };
        ps.write(RenderPipeline::from_inner(build_render(&core, &parsed, spec, self.label.clone())?))
    }
}

/// A render pass being recorded into an encoder.
#[psclass(name = "Wgpu.RenderPass", mode = proxy, columns = ["Label", "IsEnded"])]
pub struct RenderPass {
    /// The pass's label.
    pub label: String,
    /// Whether End was called; the encoder takes no other command until it is.
    pub is_ended: bool,
    #[psfield(skip)]
    pub(crate) pass: Option<wgpu::RenderPass<'static>>,
}

impl RenderPass {
    fn pass(&mut self) -> PsResult<&mut wgpu::RenderPass<'static>> {
        self.pass.as_mut().ok_or_else(|| invalid_operation("WgpuPassEnded", "the pass has ended"))
    }
}

#[psmethods]
impl RenderPass {
    /// Sets the pipeline the next draws use.
    pub fn set_pipeline(&mut self, pipeline: PsProxy<RenderPipeline>) -> PsResult<()> {
        let inner = pipeline.with(|p| p.inner.clone())?;
        self.pass()?.set_pipeline(&inner.pipeline);
        Ok(())
    }

    /// Sets bind group `index`.
    pub fn set_bind_group(&mut self, index: u32, bind_group: PsProxy<BindGroup>) -> PsResult<()> {
        let group = bind_group.with(|g| g.group_object.clone())?;
        self.pass()?.set_bind_group(index, &group, &[]);
        Ok(())
    }

    /// Feeds vertex input `slot` from a buffer.
    pub fn set_vertex_buffer(&mut self, slot: u32, buffer: PsProxy<Buffer>) -> PsResult<()> {
        let raw = buffer.with(|b| b.buffer.clone())?;
        self.pass()?.set_vertex_buffer(slot, raw.slice(..));
        Ok(())
    }

    /// Sets the index buffer: UInt16 or UInt32 elements.
    pub fn set_index_buffer(&mut self, buffer: PsProxy<Buffer>) -> PsResult<()> {
        let (raw, ty) = buffer.with(|b| (b.buffer.clone(), b.element_type))?;
        let format = match ty {
            ElementType::UInt16 => wgpu::IndexFormat::Uint16,
            ElementType::UInt32 => wgpu::IndexFormat::Uint32,
            other => return Err(invalid("WgpuIndexType", format!("an index buffer holds UInt16 or UInt32, not {other:?}"))),
        };
        self.pass()?.set_index_buffer(raw.slice(..), format);
        Ok(())
    }

    /// Draws `vertexCount` vertices of `instanceCount` instances (1 when omitted).
    pub fn draw(&mut self, vertex_count: u32, instance_count: Option<u32>, first_vertex: Option<u32>, first_instance: Option<u32>) -> PsResult<()> {
        let (fv, fi) = (first_vertex.unwrap_or(0), first_instance.unwrap_or(0));
        self.pass()?.draw(fv..fv + vertex_count, fi..fi + instance_count.unwrap_or(1));
        Ok(())
    }

    /// Draws `indexCount` indices of `instanceCount` instances (1 when omitted).
    pub fn draw_indexed(&mut self, index_count: u32, instance_count: Option<u32>, first_index: Option<u32>, base_vertex: Option<i32>, first_instance: Option<u32>) -> PsResult<()> {
        let (fx, fi) = (first_index.unwrap_or(0), first_instance.unwrap_or(0));
        self.pass()?.draw_indexed(fx..fx + index_count, base_vertex.unwrap_or(0), fi..fi + instance_count.unwrap_or(1));
        Ok(())
    }

    /// Draws with the arguments a buffer holds at byte `offset`.
    pub fn draw_indirect(&mut self, buffer: PsProxy<Buffer>, offset: Option<u64>) -> PsResult<()> {
        let raw = buffer.with(|b| b.buffer.clone())?;
        self.pass()?.draw_indirect(&raw, offset.unwrap_or(0));
        Ok(())
    }

    /// Draws indexed with the arguments a buffer holds at byte `offset`.
    pub fn draw_indexed_indirect(&mut self, buffer: PsProxy<Buffer>, offset: Option<u64>) -> PsResult<()> {
        let raw = buffer.with(|b| b.buffer.clone())?;
        self.pass()?.draw_indexed_indirect(&raw, offset.unwrap_or(0));
        Ok(())
    }

    /// Sets the viewport, with depths 0 to 1 when omitted.
    pub fn set_viewport(&mut self, x: f32, y: f32, width: f32, height: f32, min_depth: Option<f32>, max_depth: Option<f32>) -> PsResult<()> {
        self.pass()?.set_viewport(x, y, width, height, min_depth.unwrap_or(0.0), max_depth.unwrap_or(1.0));
        Ok(())
    }

    /// Limits drawing to a rectangle.
    pub fn set_scissor_rect(&mut self, x: u32, y: u32, width: u32, height: u32) -> PsResult<()> {
        self.pass()?.set_scissor_rect(x, y, width, height);
        Ok(())
    }

    /// Sets the constant color blend factors read.
    pub fn set_blend_constant(&mut self, r: f64, g: f64, b: f64, a: f64) -> PsResult<()> {
        self.pass()?.set_blend_constant(wgpu::Color { r, g, b, a });
        Ok(())
    }

    /// Sets the stencil reference value.
    pub fn set_stencil_reference(&mut self, reference: u32) -> PsResult<()> {
        self.pass()?.set_stencil_reference(reference);
        Ok(())
    }

    /// Starts counting samples into occlusion query `index` of the pass's query set.
    pub fn begin_occlusion_query(&mut self, index: u32) -> PsResult<()> {
        self.pass()?.begin_occlusion_query(index);
        Ok(())
    }

    /// Stops counting samples.
    pub fn end_occlusion_query(&mut self) -> PsResult<()> {
        self.pass()?.end_occlusion_query();
        Ok(())
    }

    /// Opens a named group of commands for graphics debuggers.
    pub fn push_debug_group(&mut self, label: String) -> PsResult<()> {
        self.pass()?.push_debug_group(&label);
        Ok(())
    }

    /// Closes the innermost debug group.
    pub fn pop_debug_group(&mut self) -> PsResult<()> {
        self.pass()?.pop_debug_group();
        Ok(())
    }

    /// Marks a point in the pass for graphics debuggers.
    pub fn insert_debug_marker(&mut self, label: String) -> PsResult<()> {
        self.pass()?.insert_debug_marker(&label);
        Ok(())
    }

    /// Ends the pass, so its encoder takes commands again.
    pub fn end(&mut self) -> PsResult<()> {
        self.pass = None;
        self.is_ended = true;
        Ok(())
    }
}

/// The view a render attachment value names: a texture, or a view.
fn attachment_view(value: &PsObject) -> PsResult<wgpu::TextureView> {
    match value.type_name()?.as_str() {
        "Wgpu.Texture" => PsProxy::<Texture>::from_ps(value)?.with(|t| t.texture.create_view(&wgpu::TextureViewDescriptor::default())),
        "Wgpu.TextureView" => PsProxy::<TextureView>::from_ps(value)?.with(|v| v.view.clone()),
        other => Err(invalid("WgpuAttachment", format!("a render attachment is a Wgpu.Texture or a Wgpu.TextureView, not {other}"))),
    }
}

/// Begins a render pass on an encoder: implemented here and called from
/// the encoder's BeginRenderPass method.
pub(crate) fn begin_render_pass(
    encoder: &mut CommandEncoder,
    color: &PsObject,
    clear_color: Option<Vec<f64>>,
    depth: Option<&PsObject>,
    resolve_target: Option<&PsObject>,
    occlusion: Option<&PsProxy<QuerySet>>,
    label: Option<String>,
) -> PsResult<RenderPass> {
    let view = attachment_view(color)?;
    let resolve = match resolve_target {
        Some(r) => Some(attachment_view(r)?),
        None => None,
    };
    let depth_view = match depth {
        Some(d) => Some(attachment_view(d)?),
        None => None,
    };
    let query = match occlusion {
        Some(q) => Some(q.with(|q| q.set.clone())?),
        None => None,
    };
    let clear = color_of(&clear_color)?;
    let label = match label {
        Some(l) => l,
        None => format!("{} render pass", encoder.label),
    };
    let pass = encoder
        .encoder()?
        .begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(&label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: resolve.as_ref(),
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(clear), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: depth_view.as_ref().map(|v| wgpu::RenderPassDepthStencilAttachment {
                view: v,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: query.as_ref(),
            multiview_mask: None,
        })
        .forget_lifetime();
    Ok(RenderPass { label, is_ended: false, pass: Some(pass) })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each mode builds wgpu's blend state for it; a Custom blend's alpha
    /// takes what its color names unless it names its own; a factor or an
    /// operation beside any mode but Custom is refused.
    #[test]
    fn builds_the_blend_each_mode_names_and_refuses_a_custom_part_without_custom() {
        let none = CustomBlend::default();
        assert_eq!(blend_state(BlendMode::Replace, &none).expect("replace"), None);
        let constant = blend_state(BlendMode::Constant, &none).expect("constant").expect("a state");
        let mixed = wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Constant, dst_factor: wgpu::BlendFactor::OneMinusConstant, operation: wgpu::BlendOperation::Add };
        assert_eq!((constant.color, constant.alpha), (mixed, mixed));
        let plain = blend_state(BlendMode::Custom, &none).expect("custom").expect("a state");
        assert_eq!(plain, wgpu::BlendState::REPLACE);
        let custom = CustomBlend { destination: Some(BlendFactor::One), operation: Some(BlendOperation::ReverseSubtract), alpha_operation: Some(BlendOperation::Max), ..CustomBlend::default() };
        let state = blend_state(BlendMode::Custom, &custom).expect("custom").expect("a state");
        assert_eq!(state.color, wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::ReverseSubtract });
        assert_eq!(state.alpha, wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Max });
        let refused = blend_state(BlendMode::Alpha, &custom).expect_err("refused");
        assert_eq!(refused.error_id, "WgpuBlend");
        assert!(refused.message.ends_with("the blend is Alpha: give -Blend Custom"), "{}", refused.message);
    }

    /// A blend reads the constant when a factor of its color or of its
    /// alpha is Constant or OneMinusConstant, and not otherwise.
    #[test]
    fn tells_a_blend_that_reads_the_constant() {
        let none = CustomBlend::default();
        let blend = |mode: BlendMode, custom: &CustomBlend| blend_state(mode, custom).expect("a blend");
        assert!(reads_constant(blend(BlendMode::Constant, &none)));
        assert!(reads_constant(blend(BlendMode::Custom, &CustomBlend { alpha_destination: Some(BlendFactor::OneMinusConstant), ..CustomBlend::default() })));
        assert!(!reads_constant(blend(BlendMode::Replace, &none)));
        assert!(!reads_constant(blend(BlendMode::Alpha, &none)));
        assert!(!reads_constant(blend(BlendMode::Custom, &CustomBlend { source: Some(BlendFactor::SrcAlpha), ..CustomBlend::default() })));
    }
}
