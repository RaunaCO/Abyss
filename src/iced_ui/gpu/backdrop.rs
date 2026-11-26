//! The Now Playing background on the GPU: the cover, blurred and saturated
//! (one small texture made on a worker thread), blown up to about twice the
//! window's larger side and turned about its centre; soft sweeps of the
//! cover's colours over it (the "changing light") and a veil that keeps text
//! readable, with a little dithering so the slow gradients never band. The
//! GPUI reference drew 48 pre-rotated frames on the CPU; here one texture is
//! turned by the shader.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use iced::mouse;
use iced::widget::shader::{self, Action, Viewport};
use iced::{Color, Rectangle};

use super::super::Message;

/// What the background shows.
#[derive(Debug, Clone)]
pub struct Backdrop {
    /// The playing cover's blurred thumbnail (None while it is being made).
    pub cover: Option<(String, Arc<image::RgbaImage>)>,
    /// Blurred cover (true) or flat colour (the setting).
    pub blur: bool,
    /// Turning angle source.
    pub spin: crate::iced_ui::now_playing::BgSpin,
    pub drag_frac: f32,
    pub ambient: Color,
    pub palette: [Color; 2],
    pub shade: Color,
}

#[derive(Default)]
pub struct State {
    current: Option<(String, Arc<image::RgbaImage>)>,
    previous: Option<(String, Arc<image::RgbaImage>)>,
    changed_at: Option<Instant>,
}

const FADE_S: f32 = 0.585;
const LIGHT_PERIOD_S: f32 = 28.0;

fn epoch() -> Instant {
    static E: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    *E.get_or_init(Instant::now)
}

impl shader::Program<Message> for Backdrop {
    type State = State;
    type Primitive = BackdropPrimitive;

    fn update(&self, state: &mut State, event: &iced::Event, _bounds: Rectangle, _cursor: mouse::Cursor) -> Option<Action<Message>> {
        let iced::Event::Window(iced::window::Event::RedrawRequested(now)) = event else { return None };
        let key = self.cover.as_ref().map(|(k, _)| k.as_str());
        if key != state.current.as_ref().map(|(k, _)| k.as_str()) && self.cover.is_some() {
            state.previous = state.current.take();
            state.current = self.cover.clone();
            state.changed_at = Some(*now);
        }
        let reduced = crate::shared::lyrics::reduced_motion();
        let fading = state.changed_at.is_some_and(|at| at.elapsed().as_secs_f32() < FADE_S) && !reduced;
        if reduced && !fading {
            return None;
        }
        // The light sweeps and the turning cover move slowly: 30 frames a second.
        Some(Action::request_redraw_at(*now + crate::iced_ui::motion::ambient_interval()))
    }

    fn draw(&self, state: &State, _cursor: mouse::Cursor, _bounds: Rectangle) -> BackdropPrimitive {
        let reduced = crate::shared::lyrics::reduced_motion();
        let now = Instant::now();
        let angle = if reduced { 0.0 } else { self.spin.angle(now, self.drag_frac).rem_euclid(360.0) };
        let current = state.current.clone().or_else(|| self.cover.clone());
        let mix = match (state.changed_at, &state.previous) {
            (Some(at), Some(_)) if !reduced => {
                let t = (at.elapsed().as_secs_f32() / FADE_S).clamp(0.0, 1.0);
                1.0 - (1.0 - t).powi(3)
            }
            _ => 1.0,
        };
        let t = if reduced { 0.0 } else { (now.duration_since(epoch()).as_secs_f32() / LIGHT_PERIOD_S).fract() };
        let flow_t = if reduced { 0.0 } else { now.duration_since(epoch()).as_secs_f32() };
        BackdropPrimitive {
            flow_t,
            current: if self.blur { current } else { None },
            previous: if self.blur && mix < 1.0 { state.previous.clone() } else { None },
            mix,
            angle,
            light_t: t,
            blur: self.blur,
            ambient: self.ambient,
            palette: self.palette,
            shade: self.shade,
        }
    }
}

#[derive(Debug)]
pub struct BackdropPrimitive {
    current: Option<(String, Arc<image::RgbaImage>)>,
    previous: Option<(String, Arc<image::RgbaImage>)>,
    mix: f32,
    angle: f32,
    light_t: f32,
    flow_t: f32,
    blur: bool,
    ambient: Color,
    palette: [Color; 2],
    shade: Color,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    size: [f32; 2],
    angle: f32,
    mix: f32,
    /// 0: blurred cover, 1: ambient gradient (cover still loading), 2: flat colour.
    mode: f32,
    light_t: f32,
    srgb: f32,
    /// Seconds on the liquid flow's clock (0 with reduced motion).
    flow_t: f32,
    /// Where the widget starts in the frame (physical px).
    origin: [f32; 2],
    _pad2: [f32; 2],
    ambient: [f32; 4],
    c0: [f32; 4],
    c1: [f32; 4],
    shade: [f32; 4],
}

pub struct Pipeline {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniforms: wgpu::Buffer,
    sampler: wgpu::Sampler,
    textures: HashMap<String, (wgpu::TextureView, u64)>,
    blank: wgpu::TextureView,
    group: Option<wgpu::BindGroup>,
    srgb: bool,
    frame: u64,
}

const WGSL: &str = r#"
struct U {
    size: vec2<f32>, angle: f32, mix_: f32,
    mode: f32, light_t: f32, srgb: f32, flow_t: f32,
    origin: vec2<f32>, pad2: vec2<f32>,
    ambient: vec4<f32>, c0: vec4<f32>, c1: vec4<f32>, shade: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var current: texture_2d<f32>;
@group(0) @binding(2) var previous: texture_2d<f32>;
@group(0) @binding(3) var samp: sampler;

@vertex
fn vs(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let x = f32((vi << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(vi & 2u) * 2.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

// CSS linear-gradient position (0 at the start edge, 1 at the end) for `deg`.
fn css_t(p: vec2<f32>, size: vec2<f32>, deg: f32) -> f32 {
    let a = radians(deg);
    let d = vec2<f32>(sin(a), -cos(a));
    let len = abs(size.x * sin(a)) + abs(size.y * cos(a));
    return dot(p - size * 0.5, d) / len + 0.5;
}

// The liquid flow shared with the album banner and the player bar: a slow
// current of uneven pace, folded onto itself so the colour curls like ink in
// water. `uv` is any 0..1-ish field position; `amount` the largest shift.
fn flow(uv: vec2<f32>, t: f32, amount: vec2<f32>) -> vec2<f32> {
    let s = 1.45 * t + 2.8 * sin(t * 0.137) + 1.9 * sin(t * 0.089 + 1.7) + 1.1 * sin(t * 0.229 + 4.1);
    let q = vec2<f32>(
        sin(uv.y * 6.0 + s * 0.50 + 1.3 * sin(uv.x * 3.1 - s * 0.37)),
        sin(uv.x * 5.0 - s * 0.43 + 1.2 * sin(uv.y * 2.6 + s * 0.31)),
    );
    let w = uv + 0.06 * q;
    let r = vec2<f32>(sin(w.y * 4.3 - s * 0.29 + 1.4 * q.x), sin(w.x * 3.7 + s * 0.33 + 1.4 * q.y));
    return uv + amount * (0.6 * q + 0.6 * r);
}

fn over(base: vec3<f32>, c: vec3<f32>, a: f32) -> vec3<f32> {
    return mix(base, c, clamp(a, 0.0, 1.0));
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let raw = pos.xy - u.origin;
    // The whole field flows: the blurred cover and the light over it are read
    // through the same slow current (a few dozen px at most).
    let m = max(u.size.x, u.size.y);
    let p = flow(raw / m, u.flow_t * 0.8, vec2<f32>(0.035, 0.035)) * m;
    var col: vec3<f32>;
    if (u.mode < 0.5) {
        let q = (p - u.size * 0.5) / m;
        let a = radians(u.angle);
        let r = vec2<f32>(q.x * cos(a) + q.y * sin(a), -q.x * sin(a) + q.y * cos(a));
        let uv = r / 2.4 + vec2<f32>(0.5, 0.5);
        let now = textureSampleLevel(current, samp, uv, 0.0).rgb;
        let before = textureSampleLevel(previous, samp, uv, 0.0).rgb;
        col = mix(before, now, u.mix_);
    } else if (u.mode < 1.5) {
        let t = clamp(p.y / u.size.y, 0.0, 1.0);
        col = mix(u.ambient.rgb * 0.78, vec3<f32>(0.0), t);
    } else {
        col = u.ambient.rgb * 0.9;
    }
    // Changing light: two soft sweeps of the cover's colours.
    let tau = 6.2831853;
    let a1 = 0.5 + 0.5 * sin(u.light_t * tau);
    let a2 = 0.5 + 0.5 * sin(u.light_t * tau + 2.4);
    let g1 = clamp(1.0 - css_t(p, u.size, 60.0 + 90.0 * cos(u.light_t * tau)) / 0.75, 0.0, 1.0);
    let g2 = clamp(1.0 - css_t(p, u.size, 240.0 + 90.0 * sin(u.light_t * tau + 1.0)) / 0.75, 0.0, 1.0);
    col = over(col, u.c0.rgb, 0.34 * a1 * g1);
    col = over(col, u.c1.rgb, 0.30 * a2 * g2);
    // The veil: the cover's own colour multiplied down to near black.
    let v = clamp(raw.y / u.size.y, 0.0, 1.0);
    col = over(col, u.shade.rgb, mix(0.34, 0.78, v));
    // Dithering against banding.
    let n = fract(sin(dot(floor(raw), vec2<f32>(12.9898, 78.233))) * 43758.5453) - 0.5;
    col = col + vec3<f32>(n * 2.2 / 255.0);
    if (u.srgb > 0.5) {
        col = to_linear(clamp(col, vec3<f32>(0.0), vec3<f32>(1.0)));
    }
    return vec4<f32>(col, 1.0);
}
"#;

impl shader::Pipeline for Pipeline {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let tex_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("backdrop"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                tex_entry(1),
                tex_entry(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let module = device
            .create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("backdrop"), source: wgpu::ShaderSource::Wgsl(WGSL.into()) });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("backdrop"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("backdrop"),
            layout: Some(&pl),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("backdrop uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("backdrop"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let srgb = format.is_srgb();
        let blank = upload(device, queue, &image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 0, 255])), srgb);
        Pipeline { pipeline, layout, uniforms, sampler, textures: HashMap::new(), blank, group: None, srgb, frame: 0 }
    }

    fn trim(&mut self) {
        self.frame += 1;
    }
}

/// Upload a cover thumbnail. Sampled as sRGB when the target is sRGB (the
/// GPU then returns linear values); the shader linearises the rest itself.
fn upload(device: &wgpu::Device, queue: &wgpu::Queue, img: &image::RgbaImage, srgb: bool) -> wgpu::TextureView {
    let (w, h) = img.dimensions();
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("backdrop cover"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: if srgb { wgpu::TextureFormat::Rgba8UnormSrgb } else { wgpu::TextureFormat::Rgba8Unorm },
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        img.as_raw(),
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * w), rows_per_image: Some(h) },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    texture.create_view(&Default::default())
}

fn rgba(c: Color) -> [f32; 4] {
    [c.r, c.g, c.b, c.a]
}

impl shader::Primitive for BackdropPrimitive {
    type Pipeline = Pipeline;

    fn prepare(&self, pipeline: &mut Pipeline, device: &wgpu::Device, queue: &wgpu::Queue, bounds: &Rectangle, viewport: &Viewport) {
        let frame = pipeline.frame;
        for (key, img) in [&self.current, &self.previous].into_iter().flatten() {
            if let Some(entry) = pipeline.textures.get_mut(key) {
                entry.1 = frame;
            } else {
                let view = upload(device, queue, img, pipeline.srgb);
                pipeline.textures.insert(key.clone(), (view, frame));
            }
        }
        // Keep the playing cover and the previous one.
        if pipeline.textures.len() > 3 {
            let mut keys: Vec<_> = pipeline.textures.iter().map(|(k, (_, used))| (k.clone(), *used)).collect();
            keys.sort_by_key(|(_, used)| *used);
            for (k, _) in keys.iter().take(pipeline.textures.len() - 3) {
                pipeline.textures.remove(k);
            }
        }
        let view_of =
            |k: &Option<(String, Arc<image::RgbaImage>)>| k.as_ref().and_then(|(key, _)| pipeline.textures.get(key)).map(|(v, _)| v);
        let current = view_of(&self.current);
        let previous = view_of(&self.previous).or(current);
        let mode = if !self.blur {
            2.0
        } else if current.is_some() {
            0.0
        } else {
            1.0
        };
        let current = current.unwrap_or(&pipeline.blank);
        let previous = previous.unwrap_or(current);
        pipeline.group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("backdrop"),
            layout: &pipeline.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: pipeline.uniforms.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(current) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(previous) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&pipeline.sampler) },
            ],
        }));
        let scale = viewport.scale_factor();
        let u = Uniforms {
            size: [(bounds.width * scale).max(1.0), (bounds.height * scale).max(1.0)],
            angle: self.angle,
            mix: if self.previous.is_some() { self.mix } else { 1.0 },
            mode,
            light_t: self.light_t,
            srgb: if pipeline.srgb { 1.0 } else { 0.0 },
            flow_t: self.flow_t,
            origin: [bounds.x * scale, bounds.y * scale],
            _pad2: [0.0; 2],
            ambient: rgba(self.ambient),
            c0: rgba(self.palette[0]),
            c1: rgba(self.palette[1]),
            shade: rgba(self.shade),
        };
        queue.write_buffer(&pipeline.uniforms, 0, bytemuck::bytes_of(&u));
    }

    fn draw(&self, pipeline: &Pipeline, pass: &mut wgpu::RenderPass<'_>) -> bool {
        let Some(group) = &pipeline.group else { return true };
        pass.set_pipeline(&pipeline.pipeline);
        pass.set_bind_group(0, group, &[]);
        pass.draw(0..3, 0..1);
        true
    }
}
