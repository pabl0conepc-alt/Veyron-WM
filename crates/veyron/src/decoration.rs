//! GPU window decorations: rounded corners, rounded borders and drop shadows.
//!
//! Two small GLSL ES 1.00 programs, cheap enough for old integrated GPUs:
//!
//! - a *texture* shader that replaces the default one while a window's own
//!   surfaces are drawn and fades out pixels outside a rounded rectangle, so
//!   client content gets rounded corners;
//! - a *pixel* shader drawn behind each window that paints the rounded border
//!   ring and a soft drop shadow from a signed distance field, with no
//!   textures and no extra passes.
//!
//! Both are compiled once, lazily, on the first frame. If a driver rejects
//! them the compositor logs it and falls back to square solid borders.

use smithay::{
    backend::renderer::{
        element::{
            surface::WaylandSurfaceRenderElement, Element, Id, Kind, RenderElement,
            UnderlyingStorage,
        },
        gles::{
            element::PixelShaderElement, GlesError, GlesFrame, GlesPixelProgram, GlesRenderer,
            GlesTexProgram, Uniform, UniformName, UniformType,
        },
        utils::{CommitCounter, DamageSet, OpaqueRegions},
    },
    utils::{Buffer, Logical, Physical, Point, Rectangle, Scale, Transform},
};

/// Rounded-rectangle signed distance: negative inside, positive outside.
const ROUNDED_BOX: &str = r#"
float rounded_box(vec2 p, vec2 half_size, float r) {
    vec2 q = abs(p) - half_size + vec2(r);
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
}
"#;

/// Default texture shader plus a rounded clip. The clip works in output pixels
/// from `gl_FragCoord`, so it is independent of buffer transforms, viewporter
/// crops and subsurface offsets. `frag_map` turns framebuffer rows into output
/// rows (`(height, -1)` when the output is drawn flipped).
const CLIP_SHADER: &str = r#"#version 100

//_DEFINES_

#if defined(EXTERNAL)
#extension GL_OES_EGL_image_external : require
#endif

#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif

#if defined(EXTERNAL)
uniform samplerExternalOES tex;
#else
uniform sampler2D tex;
#endif

uniform float alpha;
varying vec2 v_coords;

#if defined(DEBUG_FLAGS)
uniform float tint;
#endif

uniform vec4 clip_rect;
uniform float radius;
uniform vec2 frag_map;

ROUNDED_BOX

void main() {
    vec4 color = texture2D(tex, v_coords);

#if defined(NO_ALPHA)
    color = vec4(color.rgb, 1.0) * alpha;
#else
    color = color * alpha;
#endif

    vec2 p = vec2(gl_FragCoord.x, frag_map.x + frag_map.y * gl_FragCoord.y);
    vec2 half_size = clip_rect.zw * 0.5;
    float d = rounded_box(p - (clip_rect.xy + half_size), half_size, radius);
    color *= clamp(0.5 - d, 0.0, 1.0);

#if defined(DEBUG_FLAGS)
    if (tint == 1.0)
        color = vec4(0.0, 0.2, 0.0, 0.2) + color * 0.8;
#endif

    gl_FragColor = color;
}
"#;

/// Border ring and drop shadow around a window rectangle given in output
/// pixels. Like the clip shader it works from `gl_FragCoord`: interpolated
/// `v_coords` lose precision on mediump-only paths and showed as a dotted
/// grid on Intel HD Graphics. Colours are straight alpha; output is
/// premultiplied like every other Smithay element.
const DECORATION_SHADER: &str = r#"
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif

uniform float alpha;

#if defined(DEBUG_FLAGS)
uniform float tint;
#endif

uniform vec4 window_rect;
uniform vec2 frag_map;
uniform float radius;
uniform float border_width;
uniform vec4 border_color;
uniform vec4 shadow_color;
uniform float shadow_size;
uniform float shadow_offset;

ROUNDED_BOX

void main() {
    vec2 p = vec2(gl_FragCoord.x, frag_map.x + frag_map.y * gl_FragCoord.y);
    vec2 half_size = window_rect.zw * 0.5;
    vec2 center = window_rect.xy + half_size;
    vec2 outer_half = half_size + vec2(border_width);
    float outer_radius = radius + border_width;

    float inside = clamp(0.5 - rounded_box(p - center, half_size, radius), 0.0, 1.0);
    float ring = clamp(0.5 - rounded_box(p - center, outer_half, outer_radius), 0.0, 1.0)
        * (1.0 - inside) * step(0.001, border_width);

    float shadow = 0.0;
    if (shadow_size > 0.0) {
        float d = rounded_box(p - center - vec2(0.0, shadow_offset), outer_half, outer_radius);
        shadow = (1.0 - smoothstep(-1.0, shadow_size, d)) * shadow_color.a * (1.0 - inside);
        // The faint tail is dithered into a dotted grid by some drivers
        // (seen on Intel HD Graphics); below 2% it is invisible anyway.
        shadow *= step(0.02, shadow);
    }

    float border_alpha = ring * border_color.a;
    vec3 rgb = border_color.rgb * border_alpha + shadow_color.rgb * shadow * (1.0 - border_alpha);
    vec4 color = vec4(rgb, border_alpha + shadow * (1.0 - border_alpha)) * alpha;

#if defined(DEBUG_FLAGS)
    if (tint == 1.0)
        color = vec4(0.0, 0.2, 0.0, 0.2) + color * 0.8;
#endif

    gl_FragColor = color;
}
"#;

/// The compiled programs.
#[derive(Debug, Clone)]
pub struct Shaders {
    clip: GlesTexProgram,
    decoration: GlesPixelProgram,
}

impl Shaders {
    pub fn compile(renderer: &mut GlesRenderer) -> Result<Self, GlesError> {
        let clip = renderer.compile_custom_texture_shader(
            CLIP_SHADER.replace("ROUNDED_BOX", ROUNDED_BOX),
            &[
                UniformName::new("clip_rect", UniformType::_4f),
                UniformName::new("radius", UniformType::_1f),
                UniformName::new("frag_map", UniformType::_2f),
            ],
        )?;
        let decoration = renderer.compile_custom_pixel_shader(
            DECORATION_SHADER.replace("ROUNDED_BOX", ROUNDED_BOX),
            &[
                UniformName::new("window_rect", UniformType::_4f),
                UniformName::new("frag_map", UniformType::_2f),
                UniformName::new("radius", UniformType::_1f),
                UniformName::new("border_width", UniformType::_1f),
                UniformName::new("border_color", UniformType::_4f),
                UniformName::new("shadow_color", UniformType::_4f),
                UniformName::new("shadow_size", UniformType::_1f),
                UniformName::new("shadow_offset", UniformType::_1f),
            ],
        )?;
        Ok(Self { clip, decoration })
    }

    /// Wrap one of a window's surface elements so it is clipped to the
    /// window's rounded rectangle (`window` in output pixels).
    pub fn rounded(
        &self,
        inner: WaylandSurfaceRenderElement<GlesRenderer>,
        window: Rectangle<i32, Physical>,
        radius: f32,
        frag_map: [f32; 2],
    ) -> RoundedSurface {
        RoundedSurface {
            inner,
            program: self.clip.clone(),
            uniforms: vec![
                Uniform::new(
                    "clip_rect",
                    [
                        window.loc.x as f32,
                        window.loc.y as f32,
                        window.size.w as f32,
                        window.size.h as f32,
                    ],
                ),
                Uniform::new("radius", radius),
                Uniform::new("frag_map", frag_map),
            ],
        }
    }

    pub fn decoration_program(&self) -> GlesPixelProgram {
        self.decoration.clone()
    }
}

/// Everything that determines how a window's decoration looks. Elements are
/// only updated when this changes, so a still window causes no damage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecorationParams {
    /// Window geometry in logical output coordinates.
    pub window: Rectangle<i32, Logical>,
    pub radius: i32,
    pub border_width: i32,
    pub border_color: [f32; 4],
    pub shadow_color: [f32; 4],
    pub shadow_size: i32,
    pub shadow_offset: i32,
    /// Framebuffer-row to output-row mapping, as for the clip shader.
    pub frag_map: [f32; 2],
}

impl DecorationParams {
    /// Area the decoration element covers: the window plus border and shadow.
    fn area(&self) -> Rectangle<i32, Logical> {
        let margin = self.border_width + self.shadow_size + self.shadow_offset.abs();
        Rectangle::new(
            (self.window.loc.x - margin, self.window.loc.y - margin).into(),
            (
                self.window.size.w + 2 * margin,
                self.window.size.h + 2 * margin,
            )
                .into(),
        )
    }

    fn uniforms(&self) -> Vec<Uniform<'static>> {
        vec![
            Uniform::new(
                "window_rect",
                [
                    self.window.loc.x as f32,
                    self.window.loc.y as f32,
                    self.window.size.w as f32,
                    self.window.size.h as f32,
                ],
            ),
            Uniform::new("radius", self.radius as f32),
            Uniform::new("border_width", self.border_width as f32),
            Uniform::new("border_color", self.border_color),
            Uniform::new("shadow_color", self.shadow_color),
            Uniform::new("shadow_size", self.shadow_size as f32),
            Uniform::new("shadow_offset", self.shadow_offset as f32),
            Uniform::new("frag_map", self.frag_map),
        ]
    }
}

/// A window's cached decoration element.
#[derive(Debug)]
pub struct Decoration {
    params: DecorationParams,
    element: PixelShaderElement,
}

impl Decoration {
    pub fn new(program: GlesPixelProgram, params: DecorationParams) -> Self {
        Self {
            element: PixelShaderElement::new(
                program,
                params.area(),
                None,
                1.0,
                params.uniforms(),
                Kind::Unspecified,
            ),
            params,
        }
    }

    /// Bring the element up to date; only real changes bump its commit.
    pub fn update(&mut self, params: DecorationParams) {
        if params == self.params {
            return;
        }
        if params.area() != self.params.area() {
            self.element.resize(params.area(), None);
        }
        self.element.update_uniforms(params.uniforms());
        self.params = params;
    }

    pub fn element(&self) -> PixelShaderElement {
        self.element.clone()
    }
}

/// A client surface drawn through the rounded clip shader.
#[derive(Debug)]
pub struct RoundedSurface {
    inner: WaylandSurfaceRenderElement<GlesRenderer>,
    program: GlesTexProgram,
    uniforms: Vec<Uniform<'static>>,
}

impl Element for RoundedSurface {
    fn id(&self) -> &Id {
        self.inner.id()
    }

    fn current_commit(&self) -> CommitCounter {
        self.inner.current_commit()
    }

    fn location(&self, scale: Scale<f64>) -> Point<i32, Physical> {
        self.inner.location(scale)
    }

    fn src(&self) -> Rectangle<f64, Buffer> {
        self.inner.src()
    }

    fn transform(&self) -> Transform {
        self.inner.transform()
    }

    fn geometry(&self, scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.inner.geometry(scale)
    }

    fn damage_since(
        &self,
        scale: Scale<f64>,
        commit: Option<CommitCounter>,
    ) -> DamageSet<i32, Physical> {
        self.inner.damage_since(scale, commit)
    }

    /// Rounded corners are transparent, so nothing may be treated as opaque.
    fn opaque_regions(&self, _scale: Scale<f64>) -> OpaqueRegions<i32, Physical> {
        OpaqueRegions::default()
    }

    fn alpha(&self) -> f32 {
        self.inner.alpha()
    }

    fn kind(&self) -> Kind {
        self.inner.kind()
    }
}

impl RenderElement<GlesRenderer> for RoundedSurface {
    fn draw(
        &self,
        frame: &mut GlesFrame<'_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque_regions: &[Rectangle<i32, Physical>],
    ) -> Result<(), GlesError> {
        frame.override_default_tex_program(self.program.clone(), self.uniforms.clone());
        let result = RenderElement::<GlesRenderer>::draw(
            &self.inner,
            frame,
            src,
            dst,
            damage,
            opaque_regions,
        );
        frame.clear_tex_program_override();
        result
    }

    fn underlying_storage(&self, renderer: &mut GlesRenderer) -> Option<UnderlyingStorage<'_>> {
        self.inner.underlying_storage(renderer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> DecorationParams {
        DecorationParams {
            window: Rectangle::new((100, 50).into(), (400, 300).into()),
            radius: 12,
            border_width: 2,
            border_color: [1.0, 0.0, 0.0, 1.0],
            shadow_color: [0.0, 0.0, 0.0, 0.35],
            shadow_size: 18,
            shadow_offset: 4,
            frag_map: [800.0, -1.0],
        }
    }

    #[test]
    fn decoration_area_covers_border_and_shadow_on_every_side() {
        let params = params();
        let area = params.area();
        let margin = 2 + 18 + 4;
        assert_eq!(area.loc, (100 - margin, 50 - margin).into());
        assert_eq!(area.size, (400 + 2 * margin, 300 + 2 * margin).into());
    }

    #[test]
    fn shaders_splice_the_distance_function() {
        for source in [CLIP_SHADER, DECORATION_SHADER] {
            let spliced = source.replace("ROUNDED_BOX", ROUNDED_BOX);
            assert!(spliced.contains("float rounded_box(vec2 p"));
            assert!(!spliced.contains("\nROUNDED_BOX\n"));
        }
        // Texture shaders carry their own version line and the define marker;
        // Smithay prepends the version to pixel shaders itself.
        assert!(CLIP_SHADER.starts_with("#version 100\n"));
        assert!(CLIP_SHADER.contains("//_DEFINES_"));
        assert!(!DECORATION_SHADER.contains("#version"));
    }
}
