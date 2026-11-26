//! The lyrics pipeline.
//!
//! * Each visual line of text is rasterised once (cosmic-text + swash, at the
//!   physical scale) into an R8 atlas, with room around it for the blur.
//! * Right after a line is uploaded, three blurred copies of its slot are
//!   made on the GPU (separable Gaussian, at half resolution, in a texture
//!   array): light, medium and full blur. Nothing else is ever re-blurred.
//! * A frame draws one instanced quad per visible line; the fragment shader
//!   picks the blur by blending the sharp text and the neighbouring levels,
//!   so a line can sharpen continuously. Halos and dots are drawn as signed
//!   distance shapes by the same shader.
//!
//! Lines are keyed by their text, size and weight: while the layout does not
//! change, scrolling and the timing only move quads.

use std::collections::HashMap;

use iced::advanced::graphics::text::cosmic_text as ct;
use iced::widget::shader::{self, Viewport};
use iced::{Color, Point, Rectangle};

/// A visual row of text to rasterise.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LineKey {
    pub text: String,
    pub font_centi: u32,
    pub line_centi: u32,
    pub weight: u16,
}

#[derive(Debug, Clone, Copy)]
enum Shape {
    Text { line: usize, origin: Point, blur: f32 },
    Rounded { rect: Rectangle, radius: f32, soft: f32 },
    Glow { rect: Rectangle, radius: f32, blur: f32 },
    Dot { rect: Rectangle },
}

/// Something to draw, in logical px relative to the widget.
#[derive(Debug, Clone, Copy)]
pub struct Instance {
    shape: Shape,
    color: Color,
}

impl Instance {
    pub fn text(line: usize, origin: Point, color: Color, blur: f32) -> Self {
        Self { shape: Shape::Text { line, origin, blur: blur.clamp(0.0, 1.0) }, color }
    }
    pub fn dot(rect: Rectangle, color: Color, alpha: f32) -> Self {
        Self { shape: Shape::Dot { rect }, color: Color { a: color.a * alpha, ..color } }
    }
    pub fn rounded(rect: Rectangle, color: Color, alpha: f32, radius: f32, soft: f32) -> Self {
        Self { shape: Shape::Rounded { rect, radius, soft }, color: Color { a: color.a * alpha, ..color } }
    }
    pub fn glow(rect: Rectangle, color: Color, alpha: f32, radius: f32, blur: f32) -> Self {
        Self { shape: Shape::Glow { rect, radius, blur }, color: Color { a: color.a * alpha, ..color } }
    }
}

#[derive(Debug)]
pub struct LyricsPrimitive {
    lines: Vec<LineKey>,
    instances: Vec<Instance>,
    /// Which instance buffer (one per presentation, in case both are drawn).
    slot: usize,
    font: f32,
}

impl LyricsPrimitive {
    pub fn new(lines: Vec<LineKey>, instances: Vec<Instance>, now_playing: bool, font: f32) -> Self {
        Self { lines, instances, slot: usize::from(!now_playing), font }
    }
    pub fn empty() -> Self {
        Self { lines: Vec::new(), instances: Vec::new(), slot: 0, font: 14.0 }
    }
}

/// GPU vertex of an instance (physical px relative to the widget).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuInstance {
    rect: [f32; 4],
    uv: [f32; 4],
    color: [f32; 4],
    /// kind (0 text, 1 rounded, 2 dot), blur, radius, softness
    params: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    size: [f32; 2],
    _pad: [f32; 2],
}

/// One slot to blur: its rectangle in the half-resolution textures and the
/// full blur radius of its text (full-resolution px).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct BlurInstance {
    rect: [f32; 4],
    sigma: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct BlurPass {
    /// Texel step in the source (normalised), direction included.
    step: [f32; 2],
    target: [f32; 2],
    /// Fraction of the full sigma for this level, and the source scale (1 or 0.5).
    level: f32,
    src_scale: f32,
    _pad: [f32; 2],
}

#[derive(Debug, Clone, Copy)]
struct Slot {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    pad: u32,
    used: u64,
}

/// Shelf packing of the atlas.
#[derive(Default)]
struct Shelves {
    width: u32,
    height: u32,
    shelves: Vec<(u32, u32, u32)>,
    next_y: u32,
}

impl Shelves {
    fn alloc(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        let (w, h) = (w.next_multiple_of(2), h.next_multiple_of(2));
        if w > self.width {
            return None;
        }
        for shelf in &mut self.shelves {
            let (y, sh, x) = *shelf;
            if h <= sh && h * 5 >= sh * 4 && x + w <= self.width {
                shelf.2 += w;
                return Some((x, y));
            }
        }
        if self.next_y + h > self.height {
            return None;
        }
        let y = self.next_y;
        self.shelves.push((y, h, w));
        self.next_y += h;
        Some((0, y))
    }
    fn clear(&mut self) {
        self.shelves.clear();
        self.next_y = 0;
    }
}

pub struct Pipeline {
    draw: wgpu::RenderPipeline,
    blur: wgpu::RenderPipeline,
    globals: Vec<wgpu::Buffer>,
    draw_groups: Vec<wgpu::BindGroup>,
    instances: Vec<(wgpu::Buffer, u32, usize)>,
    sharp: wgpu::Texture,
    _blurred: wgpu::Texture,
    temp_view: wgpu::TextureView,
    level_views: Vec<wgpu::TextureView>,
    /// (bind group for the pass, its target view index: None = temp)
    passes: Vec<(wgpu::BindGroup, Option<usize>)>,
    blur_instances: wgpu::Buffer,
    blur_capacity: usize,
    atlas: Shelves,
    slots: HashMap<(LineKey, u32), Slot>,
    swash: ct::SwashCache,
    srgb: bool,
    frame: u64,
}

const LEVELS: u32 = 3;
/// Full blur radius (Gaussian sigma) as a fraction of the font size.
/// (GPUI softens with copies 0.7 px away: about 1.5 px at the fullscreen size.)
const SIGMA_OF_FONT: f32 = 0.028;

const DRAW_WGSL: &str = r#"
struct Globals { size: vec2<f32>, pad: vec2<f32> };
@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var sharp: texture_2d<f32>;
@group(0) @binding(2) var blurred: texture_2d_array<f32>;
@group(0) @binding(3) var samp: sampler;

struct Inst {
    @location(0) rect: vec4<f32>,
    @location(1) uv: vec4<f32>,
    @location(2) color: vec4<f32>,
    @location(3) params: vec4<f32>,
};

struct Out {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) params: vec4<f32>,
    @location(3) local: vec2<f32>,
    @location(4) size: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) vi: u32, inst: Inst) -> Out {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0));
    let c = corners[vi];
    let p = inst.rect.xy + c * inst.rect.zw;
    var out: Out;
    out.pos = vec4<f32>(p.x / g.size.x * 2.0 - 1.0, 1.0 - p.y / g.size.y * 2.0, 0.0, 1.0);
    out.uv = mix(inst.uv.xy, inst.uv.zw, c);
    out.color = inst.color;
    out.params = inst.params;
    out.local = c * inst.rect.zw;
    out.size = inst.rect.zw;
    return out;
}

fn erf2(x: vec2<f32>) -> vec2<f32> {
    let m = abs(x);
    var r = 1.0 + (0.278393 + (0.230389 + (0.000972 + 0.078108 * m) * m) * m) * m;
    r = r * r;
    return sign(x) * (1.0 - 1.0 / (r * r));
}

fn rounded_box(p: vec2<f32>, half: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - (half - vec2<f32>(r, r));
    return length(max(q, vec2<f32>(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - r;
}

@fragment
fn fs(in: Out) -> @location(0) vec4<f32> {
    let kind = in.params.x;
    var a = 0.0;
    if (kind < 0.5) {
        let s = textureSampleLevel(sharp, samp, in.uv, 0.0).r;
        let level = clamp(in.params.y, 0.0, 1.0) * 3.0;
        let b0 = textureSampleLevel(blurred, samp, in.uv, 0, 0.0).r;
        let b1 = textureSampleLevel(blurred, samp, in.uv, 1, 0.0).r;
        let b2 = textureSampleLevel(blurred, samp, in.uv, 2, 0.0).r;
        if (level <= 1.0) {
            a = mix(s, b0, level);
        } else if (level <= 2.0) {
            a = mix(b0, b1, level - 1.0);
        } else {
            a = mix(b1, b2, level - 2.0);
        }
    } else if (kind < 1.5) {
        // Rounded rectangle; params.w is the edge softness (a glow's blur).
        let half = in.size * 0.5;
        let soft = max(in.params.w, 0.5);
        let inset = half - vec2<f32>(soft, soft);
        let d = rounded_box(in.local - half, max(inset, vec2<f32>(0.5, 0.5)), min(in.params.z, min(inset.x, inset.y)));
        a = 1.0 - smoothstep(-soft, soft, d);
    } else if (kind < 2.5) {
        let half = in.size * 0.5;
        let d = length(in.local - half) - half.x + 0.5;
        a = clamp(0.5 - d, 0.0, 1.0);
    } else {
        // A rectangle blurred by a Gaussian: the product of both axes.
        let half = in.size * 0.5;
        let box_half = half - vec2<f32>(in.params.z, in.params.z);
        let k = 1.0 / (1.4142136 * max(in.params.w, 0.5));
        let at = in.local - half;
        let side = 0.5 * (erf2((at + box_half) * k) - erf2((at - box_half) * k));
        a = side.x * side.y;
    }
    return in.color * a;
}
"#;

const BLUR_WGSL: &str = r#"
struct Pass { step: vec2<f32>, target_size: vec2<f32>, level: f32, src_scale: f32, pad: vec2<f32> };
@group(0) @binding(0) var<uniform> pass_: Pass;
@group(0) @binding(1) var src: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

struct Inst { @location(0) rect: vec4<f32>, @location(1) sigma: vec4<f32> };
struct Out { @builtin(position) pos: vec4<f32>, @location(0) sigma: f32, @location(1) @interpolate(flat) bounds: vec4<f32> };

@vertex
fn vs(@builtin(vertex_index) vi: u32, inst: Inst) -> Out {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0));
    let p = inst.rect.xy + corners[vi] * inst.rect.zw;
    var out: Out;
    out.pos = vec4<f32>(p.x / pass_.target_size.x * 2.0 - 1.0, 1.0 - p.y / pass_.target_size.y * 2.0, 0.0, 1.0);
    // The full sigma is in full-resolution px; the source may be half resolution.
    out.sigma = inst.sigma.x * pass_.level * pass_.src_scale;
    // Samples stay inside the slot (its edges are empty padding), so nothing
    // from a neighbouring line or an old blur leaks in.
    let lo = (inst.rect.xy + vec2<f32>(0.5, 0.5)) / pass_.target_size;
    let hi = (inst.rect.xy + inst.rect.zw - vec2<f32>(0.5, 0.5)) / pass_.target_size;
    out.bounds = vec4<f32>(lo, hi);
    return out;
}

@fragment
fn fs(in: Out) -> @location(0) vec4<f32> {
    let uv = in.pos.xy / pass_.target_size;
    let sigma = in.sigma;
    if (sigma < 0.3) {
        return vec4<f32>(textureSampleLevel(src, samp, clamp(uv, in.bounds.xy, in.bounds.zw), 0.0).r, 0.0, 0.0, 1.0);
    }
    let radius = min(i32(ceil(sigma * 3.0)), 24);
    var sum = 0.0;
    var total = 0.0;
    for (var i = -radius; i <= radius; i = i + 1) {
        let x = f32(i);
        let w = exp(-(x * x) / (2.0 * sigma * sigma));
        sum = sum + w * textureSampleLevel(src, samp, clamp(uv + pass_.step * x, in.bounds.xy, in.bounds.zw), 0.0).r;
        total = total + w;
    }
    return vec4<f32>(sum / total, 0.0, 0.0, 1.0);
}
"#;

fn attr(location: u32, offset: u64) -> wgpu::VertexAttribute {
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset, shader_location: location }
}

impl shader::Pipeline for Pipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let max = device.limits().max_texture_dimension_2d;
        let (aw, ah) = (4096.min(max), 2048.min(max));
        let tex = |label, w, h, layers| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: layers },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
        };
        let sharp = tex("lyrics sharp", aw, ah, 1);
        let blurred = tex("lyrics blurred", aw / 2, ah / 2, LEVELS);
        let temp = tex("lyrics blur temp", aw / 2, ah / 2, 1);
        let sharp_view = sharp.create_view(&Default::default());
        let temp_view = temp.create_view(&Default::default());
        let array_view = blurred
            .create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
        let level_views: Vec<_> = (0..LEVELS)
            .map(|k| {
                blurred.create_view(&wgpu::TextureViewDescriptor {
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    base_array_layer: k,
                    array_layer_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("lyrics sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });

        // ---- draw pipeline
        let tex_entry = |binding, dim| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: dim,
                multisampled: false,
            },
            count: None,
        };
        let uniform_entry = |binding, vis| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: vis,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let sampler_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let draw_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("lyrics draw"),
            entries: &[
                uniform_entry(0, wgpu::ShaderStages::VERTEX),
                tex_entry(1, wgpu::TextureViewDimension::D2),
                tex_entry(2, wgpu::TextureViewDimension::D2Array),
                sampler_entry(3),
            ],
        });
        let globals: Vec<wgpu::Buffer> = (0..2)
            .map(|_| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("lyrics globals"),
                    size: std::mem::size_of::<Globals>() as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            })
            .collect();
        let draw_groups = globals
            .iter()
            .map(|buffer| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("lyrics draw"),
                    layout: &draw_layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&sharp_view) },
                        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&array_view) },
                        wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&sampler) },
                    ],
                })
            })
            .collect();
        let draw_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("lyrics draw"),
            source: wgpu::ShaderSource::Wgsl(DRAW_WGSL.into()),
        });
        let draw_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("lyrics draw"),
            bind_group_layouts: &[&draw_layout],
            push_constant_ranges: &[],
        });
        let draw = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("lyrics draw"),
            layout: Some(&draw_pl),
            vertex: wgpu::VertexState {
                module: &draw_module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<GpuInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &[attr(0, 0), attr(1, 16), attr(2, 32), attr(3, 48)],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &draw_module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // ---- blur pipeline (writes R8)
        let blur_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("lyrics blur"),
            entries: &[
                uniform_entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT),
                tex_entry(1, wgpu::TextureViewDimension::D2),
                sampler_entry(2),
            ],
        });
        let blur_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("lyrics blur"),
            source: wgpu::ShaderSource::Wgsl(BLUR_WGSL.into()),
        });
        let blur_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("lyrics blur"),
            bind_group_layouts: &[&blur_layout],
            push_constant_ranges: &[],
        });
        let blur = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("lyrics blur"),
            layout: Some(&blur_pl),
            vertex: wgpu::VertexState {
                module: &blur_module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<BlurInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &[attr(0, 0), attr(1, 16)],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &blur_module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::R8Unorm,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        // Two passes per level: horizontal (sharp, full res -> temp, half res)
        // then vertical (temp -> the level's layer). Their constants never change.
        let half = [(aw / 2) as f32, (ah / 2) as f32];
        let mut passes = Vec::new();
        for k in 0..LEVELS {
            let level = (k + 1) as f32 / LEVELS as f32;
            for vertical in [false, true] {
                let pass = if vertical {
                    BlurPass { step: [0.0, 1.0 / half[1]], target: half, level, src_scale: 0.5, _pad: [0.0; 2] }
                } else {
                    BlurPass { step: [1.0 / aw as f32, 0.0], target: half, level, src_scale: 1.0, _pad: [0.0; 2] }
                };
                let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("lyrics blur pass"),
                    size: std::mem::size_of::<BlurPass>() as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: true,
                });
                buffer.slice(..).get_mapped_range_mut().copy_from_slice(bytemuck::bytes_of(&pass));
                buffer.unmap();
                let source = if vertical { &temp_view } else { &sharp_view };
                let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("lyrics blur pass"),
                    layout: &blur_layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(source) },
                        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&sampler) },
                    ],
                });
                passes.push((group, vertical.then_some(k as usize)));
            }
        }
        let blur_capacity = 64;
        let blur_instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("lyrics blur instances"),
            size: (blur_capacity * std::mem::size_of::<BlurInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Pipeline {
            draw,
            blur,
            globals,
            draw_groups,
            instances: Vec::new(),
            sharp,
            _blurred: blurred,
            temp_view,
            level_views,
            passes,
            blur_instances,
            blur_capacity,
            atlas: Shelves { width: aw, height: ah, ..Default::default() },
            slots: HashMap::new(),
            swash: ct::SwashCache::new(),
            srgb: format.is_srgb(),
            frame: 0,
        }
    }

    fn trim(&mut self) {
        self.frame += 1;
    }
}

/// A line rasterised at the physical size: coverage, width, height.
fn rasterize(swash: &mut ct::SwashCache, key: &LineKey, scale: f32, pad: u32) -> (Vec<u8>, u32, u32) {
    let font = key.font_centi as f32 / 100.0 * scale;
    let line_h = key.line_centi as f32 / 100.0 * scale;
    let mut guard = iced::advanced::graphics::text::font_system().write().expect("font system");
    let fs = guard.raw();
    let mut buffer = ct::Buffer::new(fs, ct::Metrics::new(font, line_h));
    buffer.set_size(fs, None, None);
    let attrs = ct::Attrs::new().family(ct::Family::Name("Inter")).weight(ct::Weight(key.weight));
    buffer.set_text(fs, &key.text, &attrs, ct::Shaping::Advanced, None);
    buffer.shape_until_scroll(fs, false);
    let line_w = buffer.layout_runs().map(|r| r.line_w).fold(0.0_f32, f32::max);
    let w = (line_w.ceil() as u32 + 2 * pad).max(2);
    let h = (line_h.ceil() as u32 + 2 * pad).max(2);
    let mut px = vec![0u8; (w * h) as usize];
    for run in buffer.layout_runs() {
        for glyph in run.glyphs {
            let physical = glyph.physical((pad as f32, pad as f32 + run.line_y), 1.0);
            let Some(image) = swash.get_image_uncached(fs, physical.cache_key) else { continue };
            let p = image.placement;
            let (gx, gy) = (physical.x + p.left, physical.y - p.top);
            let (gw, gh) = (p.width as i32, p.height as i32);
            for yy in 0..gh {
                let ty = gy + yy;
                if ty < 0 || ty >= h as i32 {
                    continue;
                }
                for xx in 0..gw {
                    let tx = gx + xx;
                    if tx < 0 || tx >= w as i32 {
                        continue;
                    }
                    let i = (yy * gw + xx) as usize;
                    let v = match image.content {
                        ct::SwashContent::Mask => image.data.get(i).copied().unwrap_or(0),
                        ct::SwashContent::SubpixelMask => image.data.get(i * 4 + 1).copied().unwrap_or(0),
                        // Colour glyphs (emoji): their luminance times alpha.
                        ct::SwashContent::Color => {
                            let c = image.data.get(i * 4..i * 4 + 4).unwrap_or(&[0, 0, 0, 0]);
                            ((c[0] as u32 + c[1] as u32 + c[2] as u32) / 3 * c[3] as u32 / 255) as u8
                        }
                    };
                    let dst = &mut px[(ty as u32 * w + tx as u32) as usize];
                    *dst = (*dst).max(v);
                }
            }
        }
    }
    (px, w, h)
}

impl shader::Primitive for LyricsPrimitive {
    type Pipeline = Pipeline;

    fn prepare(&self, pipeline: &mut Pipeline, device: &wgpu::Device, queue: &wgpu::Queue, bounds: &Rectangle, viewport: &Viewport) {
        let scale = viewport.scale_factor();
        let scale_key = (scale * 100.0).round() as u32;
        let frame = pipeline.frame;
        // 1. Make sure every visible line is in the atlas.
        let mut fresh: Vec<BlurInstance> = Vec::new();
        let mut resolved: Vec<Option<Slot>> = Vec::with_capacity(self.lines.len());
        for attempt in 0..2 {
            resolved.clear();
            fresh.clear();
            let mut full = false;
            for key in &self.lines {
                let map_key = (key.clone(), scale_key);
                if let Some(slot) = pipeline.slots.get_mut(&map_key) {
                    slot.used = frame;
                    resolved.push(Some(*slot));
                    continue;
                }
                let font_px = key.font_centi as f32 / 100.0 * scale;
                let sigma = (font_px * SIGMA_OF_FONT).max(1.0);
                let pad = (sigma * 3.0).ceil() as u32 + 2;
                let (pixels, w, h) = rasterize(&mut pipeline.swash, key, scale, pad);
                let Some((x, y)) = pipeline.atlas.alloc(w, h) else {
                    full = true;
                    resolved.push(None);
                    continue;
                };
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &pipeline.sharp,
                        mip_level: 0,
                        origin: wgpu::Origin3d { x, y, z: 0 },
                        aspect: wgpu::TextureAspect::All,
                    },
                    &pixels,
                    wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w), rows_per_image: Some(h) },
                    wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                );
                let slot = Slot { x, y, w, h, pad, used: frame };
                pipeline.slots.insert(map_key, slot);
                resolved.push(Some(slot));
                fresh.push(BlurInstance {
                    rect: [(x / 2) as f32, (y / 2) as f32, w.div_ceil(2) as f32, h.div_ceil(2) as f32],
                    sigma: [sigma, 0.0, 0.0, 0.0],
                });
            }
            if !full || attempt == 1 {
                break;
            }
            // The atlas is full: start it again with only what is visible now.
            pipeline.atlas.clear();
            pipeline.slots.clear();
        }

        // 2. Blur what was just uploaded (three levels, two passes each).
        if !fresh.is_empty() {
            if fresh.len() > pipeline.blur_capacity {
                pipeline.blur_capacity = fresh.len().next_power_of_two();
                pipeline.blur_instances = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("lyrics blur instances"),
                    size: (pipeline.blur_capacity * std::mem::size_of::<BlurInstance>()) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
            }
            queue.write_buffer(&pipeline.blur_instances, 0, bytemuck::cast_slice(&fresh));
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("lyrics blur") });
            for (group, target) in &pipeline.passes {
                let view = match target {
                    Some(k) => &pipeline.level_views[*k],
                    None => &pipeline.temp_view,
                };
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("lyrics blur"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                pass.set_pipeline(&pipeline.blur);
                pass.set_bind_group(0, group, &[]);
                pass.set_vertex_buffer(0, pipeline.blur_instances.slice(..));
                pass.draw(0..6, 0..fresh.len() as u32);
            }
            queue.submit(Some(encoder.finish()));
        }

        // 3. The instances of this frame, in physical px.
        let (aw, ah) = (pipeline.atlas.width as f32, pipeline.atlas.height as f32);
        let srgb = pipeline.srgb;
        let mut out: Vec<GpuInstance> = Vec::with_capacity(self.instances.len());
        let px = |v: f32| v * scale;
        for inst in &self.instances {
            let color = super::premultiplied(inst.color, srgb);
            if color[3] <= 0.001 {
                continue;
            }
            match inst.shape {
                Shape::Text { line, origin, blur } => {
                    let Some(Some(slot)) = resolved.get(line) else { continue };
                    let x = px(origin.x).round() - slot.pad as f32;
                    let y = px(origin.y).round() - slot.pad as f32;
                    out.push(GpuInstance {
                        rect: [x, y, slot.w as f32, slot.h as f32],
                        uv: [slot.x as f32 / aw, slot.y as f32 / ah, (slot.x + slot.w) as f32 / aw, (slot.y + slot.h) as f32 / ah],
                        color,
                        params: [0.0, blur, 0.0, 0.0],
                    });
                }
                Shape::Rounded { rect, radius, soft } => out.push(GpuInstance {
                    rect: [px(rect.x), px(rect.y), px(rect.width), px(rect.height)],
                    uv: [0.0; 4],
                    color,
                    params: [1.0, 0.0, px(radius), px(soft)],
                }),
                Shape::Glow { rect, radius, blur } => {
                    // A box shadow as GPUI draws it: `blur` is the Gaussian's sigma.
                    let grow = 2.5 * blur;
                    out.push(GpuInstance {
                        rect: [px(rect.x - grow), px(rect.y - grow), px(rect.width + 2.0 * grow), px(rect.height + 2.0 * grow)],
                        uv: [0.0; 4],
                        color,
                        params: [3.0, px(radius), px(grow), px(blur)],
                    });
                }
                Shape::Dot { rect } => out.push(GpuInstance {
                    rect: [px(rect.x), px(rect.y), px(rect.width), px(rect.height)],
                    uv: [0.0; 4],
                    color,
                    params: [2.0, 0.0, 0.0, 0.0],
                }),
            }
        }
        let _ = self.font;
        let slot = self.slot.min(1);
        while pipeline.instances.len() <= slot {
            pipeline.instances.push((
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("lyrics instances"),
                    size: 256 * std::mem::size_of::<GpuInstance>() as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                0,
                256,
            ));
        }
        if out.len() > pipeline.instances[slot].2 {
            let capacity = out.len().next_power_of_two();
            pipeline.instances[slot].0 = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("lyrics instances"),
                size: (capacity * std::mem::size_of::<GpuInstance>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            pipeline.instances[slot].2 = capacity;
        }
        if !out.is_empty() {
            queue.write_buffer(&pipeline.instances[slot].0, 0, bytemuck::cast_slice(&out));
        }
        pipeline.instances[slot].1 = out.len() as u32;
        let globals = Globals { size: [px(bounds.width).max(1.0), px(bounds.height).max(1.0)], _pad: [0.0; 2] };
        queue.write_buffer(&pipeline.globals[slot], 0, bytemuck::bytes_of(&globals));
    }

    fn draw(&self, pipeline: &Pipeline, pass: &mut wgpu::RenderPass<'_>) -> bool {
        let slot = self.slot.min(1);
        let Some((buffer, count, _)) = pipeline.instances.get(slot) else { return true };
        if *count == 0 {
            return true;
        }
        pass.set_pipeline(&pipeline.draw);
        pass.set_bind_group(0, &pipeline.draw_groups[slot], &[]);
        pass.set_vertex_buffer(0, buffer.slice(..));
        pass.draw(0..6, 0..*count);
        true
    }
}

impl std::fmt::Debug for Pipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LyricsPipeline").field("slots", &self.slots.len()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::Shelves;

    #[test]
    fn shelves_pack_rows_and_report_when_full() {
        let mut s = Shelves { width: 100, height: 40, ..Default::default() };
        assert_eq!(s.alloc(50, 20), Some((0, 0)));
        assert_eq!(s.alloc(50, 19), Some((50, 0)));
        assert_eq!(s.alloc(10, 20), Some((0, 20)));
        assert_eq!(s.alloc(10, 20), Some((10, 20)));
        assert_eq!(s.alloc(10, 30), None);
        s.clear();
        assert_eq!(s.alloc(100, 40), Some((0, 0)));
    }
}
