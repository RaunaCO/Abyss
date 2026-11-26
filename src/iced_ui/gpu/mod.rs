//! Custom `wgpu` pipelines, used only where the stock Iced widgets cannot
//! draw an effect: the blurred synchronised lyrics and the turning, blurred
//! cover behind Now Playing, and the liquid glass surfaces (`glass`). They
//! run inside Iced's render pass as `shader` primitives.

pub mod backdrop;
pub mod glass;
pub mod lyrics;

/// sRGB-encoded component to linear light.
pub fn to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// A colour as the target expects it, premultiplied: linear when the surface
/// is sRGB (Iced converts its own colours the same way), as is otherwise.
pub fn premultiplied(c: iced::Color, srgb_target: bool) -> [f32; 4] {
    let (r, g, b) = if srgb_target { (to_linear(c.r), to_linear(c.g), to_linear(c.b)) } else { (c.r, c.g, c.b) };
    [r * c.a, g * c.a, b * c.a, c.a]
}
