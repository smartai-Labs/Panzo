use crate::platform::background_image::DecodedBackgroundImage;
use crate::platform::d3d11::D3d11Device;
use crate::platform::editor::{
    EditorSessionError, load_effective_background, load_effective_edit_state,
};
use crate::platform::media_decoder::{DecodedBgraFrame, MediaDecoderError, MfBgraDecoder};
use panzo_core::{
    CursorEvent, CursorTimeline, FrameEvaluation, FrameEvaluator, OutputDescriptor,
    OutputPixelFormat, ProjectIoError, ProjectLayout, RenderEvaluationError, SourceFrameTimeline,
    TimeTick, WorkbenchSettings, WorkbenchValidationError,
};
use sha2::{Digest, Sha256};
use std::cell::{Cell, RefCell};
use std::fmt;
use std::fs;
use std::io::{BufRead, BufReader};
use std::mem;
use std::path::{Path, PathBuf};
use std::ptr;
use std::time::Instant;
use thiserror::Error;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::{
    D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST, ID3DBlob, ID3DInclude,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_CONSTANT_BUFFER, D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE,
    D3D11_BUFFER_DESC, D3D11_COMPARISON_NEVER, D3D11_CPU_ACCESS_READ,
    D3D11_FILTER_MIN_MAG_MIP_LINEAR, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_SAMPLER_DESC,
    D3D11_SUBRESOURCE_DATA, D3D11_TEXTURE_ADDRESS_CLAMP, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
    D3D11_USAGE_IMMUTABLE, D3D11_USAGE_STAGING, D3D11_VIEWPORT, ID3D11Buffer, ID3D11ClassLinkage,
    ID3D11DepthStencilView, ID3D11InputLayout, ID3D11PixelShader, ID3D11RenderTargetView,
    ID3D11SamplerState, ID3D11ShaderResourceView, ID3D11Texture2D, ID3D11VertexShader,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_IGNORE, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    DXGI_MWA_NO_ALT_ENTER, DXGI_PRESENT, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1,
    DXGI_SWAP_EFFECT_FLIP_DISCARD, DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIDevice, IDXGIFactory2,
    IDXGIOutput, IDXGISwapChain1,
};
use windows::core::{Interface, PCSTR};

const COMPOSITOR_SHADER: &str = r"
Texture2D SourceTexture : register(t0);
Texture2D BackgroundTexture : register(t1);
SamplerState SourceSampler : register(s0);

cbuffer RenderParameters : register(b0) {
    float4 SourceRect;
    float4 CursorParameters;
    float4 CanvasRect;
    float4 BackgroundColor;
    float4 StyleParameters;
    float4 BackgroundRect;
    float4 OverlayParameters;
    float4 CropRect;
};

struct VertexOutput {
    float4 position : SV_POSITION;
    float2 uv : TEXCOORD0;
};

VertexOutput vertex_main(uint vertex_id : SV_VertexID) {
    VertexOutput output;
    float2 uv = float2((vertex_id << 1) & 2, vertex_id & 2);
    output.position = float4(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    output.uv = uv;
    return output;
}

float cursor_distance(float2 p) {
    // A complete arrow silhouette, including the stem. The tip is the hotspot.
    const float2 vertices[7] = {
        float2(0, 0), float2(0, 23), float2(6, 18), float2(11, 29),
        float2(16, 27), float2(11, 16), float2(21, 16)
    };
    float distance_squared = 10000.0;
    float sign = 1.0;
    for (int i = 0, j = 6; i < 7; j = i, i++) {
        float2 edge = vertices[j] - vertices[i];
        float2 v = p - vertices[i];
        float2 nearest = v - edge * saturate(dot(v, edge) / dot(edge, edge));
        distance_squared = min(distance_squared, dot(nearest, nearest));
        bool3 crosses = bool3(p.y >= vertices[i].y, p.y < vertices[j].y,
            edge.x * v.y > edge.y * v.x);
        if (all(crosses) || all(!crosses)) sign = -sign;
    }
    return sign * sqrt(distance_squared);
}

float4 pixel_main(VertexOutput input) : SV_Target {
    float2 pixel = input.position.xy;
    float2 canvas_min = CanvasRect.xy;
    float2 canvas_size = CanvasRect.zw;
    float2 canvas_max = canvas_min + canvas_size;
    float2 center = canvas_min + canvas_size * 0.5;
    float radius = StyleParameters.x;
    float2 q = abs(pixel - center) - (canvas_size * 0.5 - radius);
    float distance_to_canvas = length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - radius;
    float4 background = BackgroundColor;
    if (StyleParameters.w > 0.5) {
        float2 background_uv = BackgroundRect.xy + input.uv * BackgroundRect.zw;
        background = BackgroundTexture.Sample(SourceSampler, background_uv);
    }
    float edge_aa = max(fwidth(distance_to_canvas) * 0.5, 0.5);
    float coverage = radius > 0.0
        ? 1.0 - smoothstep(-edge_aa, edge_aa, distance_to_canvas)
        : step(distance_to_canvas, 0.0);
    float spread = max(StyleParameters.z, 1.0);
    float shadow = StyleParameters.y * exp(-max(distance_to_canvas, 0.0) / spread) * 0.35;
    float4 outside = lerp(background, float4(0.0, 0.0, 0.0, 1.0), shadow);
    if (coverage <= 0.0 && OverlayParameters.z <= 0.5 && CropRect.z <= CropRect.x) return outside;

    float2 canvas_uv = saturate((pixel - canvas_min) / canvas_size);
    float2 source_uv = SourceRect.xy + canvas_uv * SourceRect.zw;
    float4 color = SourceTexture.Sample(SourceSampler, source_uv);

    if (CursorParameters.w > 0.5) {
        float2 cursor_point = input.position.xy - CursorParameters.xy;
        float size_scale = CursorParameters.z / 32.0;
        cursor_point /= size_scale;
        float distance = cursor_distance(cursor_point);
        float aa = max(fwidth(distance), 0.65);
        float outer = 1.0 - smoothstep(-aa, aa, distance - 0.9);
        float inner = 1.0 - smoothstep(-aa, aa, distance + 1.0);
        color = lerp(color, float4(0.08, 0.09, 0.12, 1.0), outer);
        color = lerp(color, float4(1.0, 1.0, 1.0, 1.0), inner);
    }
    // Editor handles belong above the finished canvas, including its rounded corners.
    color = lerp(outside, color, coverage);
    if (OverlayParameters.z > 0.5) {
        float2 delta = abs(pixel - OverlayParameters.xy);
        float cross_mask = max(
            step(delta.x, 1.5) * step(delta.y, 14.5),
            step(delta.y, 1.5) * step(delta.x, 14.5));
        color = lerp(color, float4(1.0, 0.62, 0.12, 1.0), cross_mask);
    }
    if (CropRect.z > CropRect.x) {
        float inside = step(CropRect.x, pixel.x) * step(pixel.x, CropRect.z) * step(CropRect.y, pixel.y) * step(pixel.y, CropRect.w);
        color.rgb *= lerp(0.38, 1.0, inside);
        float2 mid = (CropRect.xy + CropRect.zw) * 0.5;
        float dx = min(abs(pixel.x-CropRect.x), abs(pixel.x-CropRect.z));
        float dy = min(abs(pixel.y-CropRect.y), abs(pixel.y-CropRect.w));
        float horizontal = step(dy, 1.5) * step(CropRect.x, pixel.x) * step(pixel.x, CropRect.z);
        float vertical = step(dx, 1.5) * step(CropRect.y, pixel.y) * step(pixel.y, CropRect.w);
        float handle = max(step(dx, 5.5)*step(min(dy,abs(pixel.y-mid.y)),5.5), step(dy,5.5)*step(abs(pixel.x-mid.x),5.5));
        float2 cell = (pixel - CropRect.xy) / max((CropRect.zw-CropRect.xy)/3.0, 1.0);
        float grid = max((1.0-smoothstep(0.0,0.01,abs(cell.x-1.0)))+(1.0-smoothstep(0.0,0.01,abs(cell.x-2.0))), (1.0-smoothstep(0.0,0.01,abs(cell.y-1.0)))+(1.0-smoothstep(0.0,0.01,abs(cell.y-2.0)))) * inside * 0.22;
        color = lerp(color,float4(1,1,1,1),saturate(max(max(horizontal,vertical),max(handle,grid))));
    }
    return color;
}
";

#[repr(C)]
#[derive(Clone, Copy)]
struct ShaderParameters {
    source_rect: [f32; 4],
    cursor: [f32; 4],
    canvas_rect: [f32; 4],
    background: [f32; 4],
    style: [f32; 4],
    background_rect: [f32; 4],
    overlay: [f32; 4],
    crop_rect: [f32; 4],
}

const _: () = assert!(mem::size_of::<ShaderParameters>().is_multiple_of(16));

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompositedFrame {
    pub width: u32,
    pub height: u32,
    /// Tightly packed, top-down BGRA8 pixels.
    pub pixels: Vec<u8>,
}

impl CompositedFrame {
    pub fn sha256_hex(&self) -> String {
        format!("{:x}", Sha256::digest(&self.pixels))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderHashProbeReport {
    pub project_root: PathBuf,
    pub project_tick: TimeTick,
    pub source_frame_tick: TimeTick,
    pub output_width: u32,
    pub output_height: u32,
    pub preview_hash: String,
    pub export_hash: String,
    pub hashes_match: bool,
}

impl fmt::Display for RenderHashProbeReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "Panzo M3 shared compositor hash probe")?;
        writeln!(formatter, "  Project: {}", self.project_root.display())?;
        writeln!(formatter, "  Project tick: {}", self.project_tick.as_i64())?;
        writeln!(
            formatter,
            "  Selected source PTS: {}",
            self.source_frame_tick.as_i64()
        )?;
        writeln!(
            formatter,
            "  Output: {}x{} BGRA8",
            self.output_width, self.output_height
        )?;
        writeln!(formatter, "  Preview hash: {}", self.preview_hash)?;
        writeln!(formatter, "  Export hash:  {}", self.export_hash)?;
        write!(formatter, "  Hashes match: {}", self.hashes_match)
    }
}

pub struct RenderHashProbe;

impl RenderHashProbe {
    pub fn project_frame(
        project_root: impl AsRef<Path>,
        project_tick: TimeTick,
        output: OutputDescriptor,
    ) -> Result<RenderHashProbeReport, RenderHashProbeError> {
        if project_tick < TimeTick::ZERO {
            return Err(RenderHashProbeError::NegativeProjectTime(project_tick));
        }
        let layout = ProjectLayout::open(project_root)?;
        let manifest = layout.load_manifest()?;
        let (camera, settings) = load_effective_edit_state(layout.root())?;
        let background = load_effective_background(layout.root(), &settings)?;
        let cursor_path = layout.root().join(&manifest.tracks.cursor);
        let cursor = read_cursor_events(&cursor_path)?;
        let media_path = layout.root().join(&manifest.media.screen);
        let video = settings
            .video_edit(TimeTick(manifest.media.duration_tick))
            .map_err(EditorSessionError::from)?;
        let source_tick = panzo_core::TimeMapping::source_time(&video, project_tick)
            .map_err(panzo_core::RenderEvaluationError::from)?;
        let (selected_frame, timeline) = decode_selected_frame(&media_path, source_tick)?;
        let evaluator = FrameEvaluator::new(&timeline, &camera, &cursor)?;
        let evaluation = evaluator.evaluate_edit(project_tick, output, &settings)?;
        if evaluation.source_frame.presentation_tick != selected_frame.pts {
            return Err(RenderHashProbeError::SelectionMismatch {
                evaluator_tick: evaluation.source_frame.presentation_tick,
                decoded_tick: selected_frame.pts,
            });
        }

        let compositor = D3d11Compositor::create()?;
        // Exercise the real preview decode/cache/selection path independently,
        // then the same pre-encoder function used by the export frame loop.
        let mut preview_session =
            crate::platform::preview::ProjectPreviewSession::open(layout.root(), output)
                .map_err(|error| RenderHashProbeError::Pipeline(error.to_string()))?;
        preview_session
            .seek(project_tick)
            .map_err(|error| RenderHashProbeError::Pipeline(error.to_string()))?;
        let preview = preview_session
            .render_current()
            .map_err(|error| RenderHashProbeError::Pipeline(error.to_string()))?
            .composited;
        let export = crate::platform::export::compose_export_frame(
            &compositor,
            &selected_frame,
            &evaluation,
            &settings,
            background.as_ref(),
        )
        .map_err(|error| RenderHashProbeError::Pipeline(error.to_string()))?;
        let preview_hash = preview.sha256_hex();
        let export_hash = export.sha256_hex();
        let hashes_match = preview_hash == export_hash;

        Ok(RenderHashProbeReport {
            project_root: layout.root().into(),
            project_tick,
            source_frame_tick: selected_frame.pts,
            output_width: output.width,
            output_height: output.height,
            preview_hash,
            export_hash,
            hashes_match,
        })
    }
}

fn read_cursor_events(path: &Path) -> Result<CursorTimeline, RenderHashProbeError> {
    let file = fs::File::open(path).map_err(|source| RenderHashProbeError::Read {
        path: path.into(),
        source,
    })?;
    let mut events = Vec::new();
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line = line.map_err(|source| RenderHashProbeError::Read {
            path: path.into(),
            source,
        })?;
        if line.trim().is_empty() {
            continue;
        }
        let event = serde_json::from_str::<CursorEvent>(&line).map_err(|source| {
            RenderHashProbeError::InvalidCursorRecord {
                path: path.into(),
                line: index + 1,
                source,
            }
        })?;
        events.push(event);
    }
    CursorTimeline::new(events).map_err(RenderHashProbeError::Evaluation)
}

fn decode_selected_frame(
    path: &Path,
    source_tick: TimeTick,
) -> Result<(DecodedBgraFrame, SourceFrameTimeline), RenderHashProbeError> {
    let mut decoder = MfBgraDecoder::open(path)?;
    let mut presentation_ticks = Vec::new();
    let mut selected = None;
    while let Some(frame) = decoder.read_next()? {
        let first = presentation_ticks.is_empty();
        presentation_ticks.push(frame.pts);
        if first || frame.pts <= source_tick {
            selected = Some(frame);
        } else {
            break;
        }
    }
    let selected = selected.ok_or(RenderHashProbeError::NoSourceFrame)?;
    let timeline = SourceFrameTimeline::new(presentation_ticks)?;
    Ok((selected, timeline))
}

pub struct D3d11Compositor {
    graphics: D3d11Device,
    vertex_shader: ID3D11VertexShader,
    pixel_shader: ID3D11PixelShader,
    sampler: ID3D11SamplerState,
    source_cache: RefCell<Option<CachedInputTexture>>,
    background_cache: RefCell<Option<CachedInputTexture>>,
    output_cache: RefCell<Option<CachedOutputTextures>>,
    source_uploads: Cell<u64>,
    background_uploads: Cell<u64>,
    output_allocations: Cell<u64>,
    readbacks: Cell<u64>,
    presentations: Cell<u64>,
    slow_presentations_logged: Cell<u8>,
    crop_overlay: Cell<Option<[f64; 4]>>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CompositorCacheStats {
    pub source_uploads: u64,
    pub background_uploads: u64,
    pub output_allocations: u64,
    pub readbacks: u64,
    pub presentations: u64,
}

/// Flip-model swap chain bound to the Editor's preview child window. The compositor renders
/// directly into this target, so interactive preview never has to map a GPU texture into CPU
/// memory merely to hand the same pixels back to the desktop compositor.
pub struct GpuPreviewPresenter {
    swap_chain: IDXGISwapChain1,
    render_target: ID3D11RenderTargetView,
    width: u32,
    height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct InputTextureKey {
    identity: i64,
    width: u32,
    height: u32,
    data_address: usize,
}

struct CachedInputTexture {
    key: InputTextureKey,
    _texture: ID3D11Texture2D,
    view: ID3D11ShaderResourceView,
}

struct CachedOutputTextures {
    width: u32,
    height: u32,
    output: ID3D11Texture2D,
    staging: ID3D11Texture2D,
    view: ID3D11RenderTargetView,
}

impl D3d11Compositor {
    pub fn set_crop_overlay(&self, rect: Option<[f64; 4]>) {
        self.crop_overlay.set(rect);
    }
    pub fn create() -> Result<Self, CompositorError> {
        let graphics = stage("D3D11 hardware device", D3d11Device::create_hardware())?;
        let vertex_bytecode = compile_shader(b"vertex_main\0", b"vs_5_0\0")?;
        let pixel_bytecode = compile_shader(b"pixel_main\0", b"ps_5_0\0")?;
        let mut vertex_shader = None;
        stage("ID3D11Device::CreateVertexShader", unsafe {
            graphics.native_device().CreateVertexShader(
                &vertex_bytecode,
                None::<&ID3D11ClassLinkage>,
                Some(&raw mut vertex_shader),
            )
        })?;
        let vertex_shader = vertex_shader.ok_or(CompositorError::NullInterface("vertex shader"))?;
        let mut pixel_shader = None;
        stage("ID3D11Device::CreatePixelShader", unsafe {
            graphics.native_device().CreatePixelShader(
                &pixel_bytecode,
                None::<&ID3D11ClassLinkage>,
                Some(&raw mut pixel_shader),
            )
        })?;
        let pixel_shader = pixel_shader.ok_or(CompositorError::NullInterface("pixel shader"))?;
        let sampler_description = D3D11_SAMPLER_DESC {
            Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
            AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
            AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
            AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
            MipLODBias: 0.0,
            MaxAnisotropy: 1,
            ComparisonFunc: D3D11_COMPARISON_NEVER,
            BorderColor: [0.0; 4],
            MinLOD: 0.0,
            MaxLOD: f32::MAX,
        };
        let mut sampler = None;
        stage("ID3D11Device::CreateSamplerState", unsafe {
            graphics
                .native_device()
                .CreateSamplerState(&raw const sampler_description, Some(&raw mut sampler))
        })?;
        let sampler = sampler.ok_or(CompositorError::NullInterface("sampler state"))?;

        Ok(Self {
            graphics,
            vertex_shader,
            pixel_shader,
            sampler,
            source_cache: RefCell::new(None),
            background_cache: RefCell::new(None),
            output_cache: RefCell::new(None),
            source_uploads: Cell::new(0),
            background_uploads: Cell::new(0),
            output_allocations: Cell::new(0),
            readbacks: Cell::new(0),
            presentations: Cell::new(0),
            slow_presentations_logged: Cell::new(0),
            crop_overlay: Cell::new(None),
        })
    }

    pub fn invalidate_background_cache(&self) {
        self.background_cache.borrow_mut().take();
    }

    pub fn cache_stats(&self) -> CompositorCacheStats {
        CompositorCacheStats {
            source_uploads: self.source_uploads.get(),
            background_uploads: self.background_uploads.get(),
            output_allocations: self.output_allocations.get(),
            readbacks: self.readbacks.get(),
            presentations: self.presentations.get(),
        }
    }

    pub fn create_presenter(
        &self,
        hwnd: HWND,
        width: u32,
        height: u32,
    ) -> Result<GpuPreviewPresenter, CompositorError> {
        pixel_length(width, height)?;
        let dxgi_device: IDXGIDevice = stage(
            "ID3D11Device::cast(IDXGIDevice)",
            self.graphics.native_device().cast(),
        )?;
        let adapter = stage("IDXGIDevice::GetAdapter", unsafe {
            dxgi_device.GetAdapter()
        })?;
        let factory: IDXGIFactory2 = stage("IDXGIAdapter::GetParent(IDXGIFactory2)", unsafe {
            adapter.GetParent()
        })?;
        let description = DXGI_SWAP_CHAIN_DESC1 {
            Width: width,
            Height: height,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            Stereo: false.into(),
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            Scaling: DXGI_SCALING_STRETCH,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
            AlphaMode: DXGI_ALPHA_MODE_IGNORE,
            Flags: 0,
        };
        let swap_chain = stage("IDXGIFactory2::CreateSwapChainForHwnd", unsafe {
            factory.CreateSwapChainForHwnd(
                self.graphics.native_device(),
                hwnd,
                &raw const description,
                None,
                None::<&IDXGIOutput>,
            )
        })?;
        stage("IDXGIFactory::MakeWindowAssociation", unsafe {
            factory.MakeWindowAssociation(hwnd, DXGI_MWA_NO_ALT_ENTER)
        })?;
        let back_buffer: ID3D11Texture2D = stage("IDXGISwapChain::GetBuffer", unsafe {
            swap_chain.GetBuffer(0)
        })?;
        let mut render_target = None;
        stage("ID3D11Device::CreateRenderTargetView(swap chain)", unsafe {
            self.graphics.native_device().CreateRenderTargetView(
                &back_buffer,
                None,
                Some(&raw mut render_target),
            )
        })?;
        Ok(GpuPreviewPresenter {
            swap_chain,
            render_target: render_target
                .ok_or(CompositorError::NullInterface("swap-chain render target"))?,
            width,
            height,
        })
    }

    pub fn compose(
        &self,
        source: &DecodedBgraFrame,
        evaluation: &FrameEvaluation,
    ) -> Result<CompositedFrame, CompositorError> {
        self.compose_with_settings(source, evaluation, &WorkbenchSettings::default())
    }

    pub fn compose_with_settings(
        &self,
        source: &DecodedBgraFrame,
        evaluation: &FrameEvaluation,
        settings: &WorkbenchSettings,
    ) -> Result<CompositedFrame, CompositorError> {
        self.compose_with_background(source, evaluation, settings, None)
    }

    #[allow(clippy::too_many_lines)]
    pub fn compose_with_background(
        &self,
        source: &DecodedBgraFrame,
        evaluation: &FrameEvaluation,
        settings: &WorkbenchSettings,
        background: Option<&DecodedBackgroundImage>,
    ) -> Result<CompositedFrame, CompositorError> {
        validate_source(source)?;
        settings.validate()?;
        if settings.background.kind == panzo_core::BackgroundKind::Image && background.is_none() {
            return Err(CompositorError::MissingBackgroundImageData);
        }
        if evaluation.output.pixel_format != OutputPixelFormat::Bgra8 {
            return Err(CompositorError::UnsupportedPixelFormat);
        }
        let output_width = evaluation.output.width;
        let output_height = evaluation.output.height;
        let output_length = pixel_length(output_width, output_height)?;
        let (output_texture, staging_texture, output_view, output_created) =
            cached_output_textures(
                &self.output_cache,
                self.graphics.native_device(),
                output_width,
                output_height,
            )?;
        self.output_allocations
            .set(self.output_allocations.get() + u64::from(output_created));
        self.render_to_target(source, evaluation, settings, background, &output_view, None)?;

        let context = self.graphics.immediate_context();
        unsafe {
            context.CopyResource(&staging_texture, &output_texture);
        }
        let pixels = read_staging_texture(
            context,
            &staging_texture,
            output_width,
            output_height,
            output_length,
        )?;
        self.readbacks.set(self.readbacks.get().saturating_add(1));
        Ok(CompositedFrame {
            width: output_width,
            height: output_height,
            pixels,
        })
    }

    pub fn present_with_background(
        &self,
        presenter: &GpuPreviewPresenter,
        source: &DecodedBgraFrame,
        evaluation: &FrameEvaluation,
        settings: &WorkbenchSettings,
        background: Option<&DecodedBackgroundImage>,
        focus_overlay: Option<[f64; 2]>,
    ) -> Result<(), CompositorError> {
        if evaluation.output.width != presenter.width
            || evaluation.output.height != presenter.height
        {
            return Err(CompositorError::PresenterGeometryMismatch {
                presenter_width: presenter.width,
                presenter_height: presenter.height,
                output_width: evaluation.output.width,
                output_height: evaluation.output.height,
            });
        }
        let render_started = Instant::now();
        self.render_to_target(
            source,
            evaluation,
            settings,
            background,
            &presenter.render_target,
            focus_overlay,
        )?;
        let render_us = render_started.elapsed().as_micros();
        let present_started = Instant::now();
        stage("IDXGISwapChain::Present", unsafe {
            // The 16 ms Editor clock already paces playback. Waiting for another v-blank here
            // double-throttles the message loop on 60 Hz displays; windowed flip presentation is
            // still composed by DWM, so submitting immediately remains tear-free.
            presenter.swap_chain.Present(0, DXGI_PRESENT(0)).ok()
        })?;
        let present_us = present_started.elapsed().as_micros();
        if render_us + present_us > 100_000 && self.slow_presentations_logged.get() < 8 {
            self.slow_presentations_logged
                .set(self.slow_presentations_logged.get() + 1);
            eprintln!(
                "Panzo slow GPU preview: sourceTick={} renderUs={render_us} presentUs={present_us}",
                source.pts.as_i64()
            );
        }
        self.presentations
            .set(self.presentations.get().saturating_add(1));
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn render_to_target(
        &self,
        source: &DecodedBgraFrame,
        evaluation: &FrameEvaluation,
        settings: &WorkbenchSettings,
        background: Option<&DecodedBackgroundImage>,
        output_view: &ID3D11RenderTargetView,
        focus_overlay: Option<[f64; 2]>,
    ) -> Result<(), CompositorError> {
        validate_source(source)?;
        settings.validate()?;
        if settings.background.kind == panzo_core::BackgroundKind::Image && background.is_none() {
            return Err(CompositorError::MissingBackgroundImageData);
        }
        if evaluation.output.pixel_format != OutputPixelFormat::Bgra8 {
            return Err(CompositorError::UnsupportedPixelFormat);
        }
        let output_width = evaluation.output.width;
        let output_height = evaluation.output.height;
        pixel_length(output_width, output_height)?;
        let (source_view, source_created) = cached_input_view(
            &self.source_cache,
            self.graphics.native_device(),
            InputTextureKey {
                identity: source.pts.as_i64(),
                width: source.width,
                height: source.height,
                data_address: source.pixels.as_ptr() as usize,
            },
            &source.pixels,
            "source",
        )?;
        self.source_uploads
            .set(self.source_uploads.get() + u64::from(source_created));
        let background_view = background
            .map(|background| {
                validate_bgra(
                    background.width,
                    background.height,
                    &background.pixels,
                    "background",
                )?;
                cached_input_view(
                    &self.background_cache,
                    self.graphics.native_device(),
                    InputTextureKey {
                        identity: 0,
                        width: background.width,
                        height: background.height,
                        data_address: background.pixels.as_ptr() as usize,
                    },
                    &background.pixels,
                    "background",
                )
            })
            .transpose()?;
        let background_view = background_view.map(|(view, created)| {
            self.background_uploads
                .set(self.background_uploads.get() + u64::from(created));
            view
        });
        let mut parameters = shader_parameters_for_source(
            evaluation,
            settings,
            background,
            focus_overlay,
            source.native_size.unwrap_or((source.width, source.height)),
        )?;
        parameters.crop_rect = self.crop_overlay.get().map_or([0.0; 4], |r| {
            [
                r[0] as f32 * output_width as f32,
                r[1] as f32 * output_height as f32,
                r[2] as f32 * output_width as f32,
                r[3] as f32 * output_height as f32,
            ]
        });
        let parameter_buffer = create_parameter_buffer(self.graphics.native_device(), &parameters)?;

        let context = self.graphics.immediate_context();
        let viewport = D3D11_VIEWPORT {
            TopLeftX: 0.0,
            TopLeftY: 0.0,
            Width: output_width as f32,
            Height: output_height as f32,
            MinDepth: 0.0,
            MaxDepth: 1.0,
        };
        unsafe {
            context.IASetInputLayout(None::<&ID3D11InputLayout>);
            context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            context.VSSetShader(&self.vertex_shader, None);
            context.PSSetShader(&self.pixel_shader, None);
            context.PSSetShaderResources(
                0,
                Some(&[Some(source_view.clone()), background_view.clone()]),
            );
            context.PSSetSamplers(0, Some(&[Some(self.sampler.clone())]));
            context.PSSetConstantBuffers(0, Some(&[Some(parameter_buffer)]));
            context.RSSetViewports(Some(&[viewport]));
            context.OMSetRenderTargets(
                Some(&[Some(output_view.clone())]),
                None::<&ID3D11DepthStencilView>,
            );
            context.Draw(3, 0);
            context.PSSetShaderResources(0, Some(&[None, None]));
        }
        Ok(())
    }
}

fn cached_input_view(
    cache: &RefCell<Option<CachedInputTexture>>,
    device: &windows::Win32::Graphics::Direct3D11::ID3D11Device,
    key: InputTextureKey,
    pixels: &[u8],
    label: &'static str,
) -> Result<(ID3D11ShaderResourceView, bool), CompositorError> {
    let mut cache = cache.borrow_mut();
    if cache.as_ref().is_some_and(|cached| cached.key == key) {
        return Ok((
            cache.as_ref().expect("cache was checked").view.clone(),
            false,
        ));
    }
    let texture = create_bgra_texture(device, key.width, key.height, pixels, label)?;
    let mut view = None;
    stage("ID3D11Device::CreateShaderResourceView(cached)", unsafe {
        device.CreateShaderResourceView(&texture, None, Some(&raw mut view))
    })?;
    let view = view.ok_or(CompositorError::NullInterface(label))?;
    *cache = Some(CachedInputTexture {
        key,
        _texture: texture,
        view: view.clone(),
    });
    Ok((view, true))
}

fn cached_output_textures(
    cache: &RefCell<Option<CachedOutputTextures>>,
    device: &windows::Win32::Graphics::Direct3D11::ID3D11Device,
    width: u32,
    height: u32,
) -> Result<
    (
        ID3D11Texture2D,
        ID3D11Texture2D,
        ID3D11RenderTargetView,
        bool,
    ),
    CompositorError,
> {
    let mut cache = cache.borrow_mut();
    let created = !cache
        .as_ref()
        .is_some_and(|cached| cached.width == width && cached.height == height);
    if created {
        let output = create_output_texture(device, width, height)?;
        let staging = create_staging_texture(device, width, height)?;
        let mut view = None;
        stage("ID3D11Device::CreateRenderTargetView(cached)", unsafe {
            device.CreateRenderTargetView(&output, None, Some(&raw mut view))
        })?;
        let view = view.ok_or(CompositorError::NullInterface("output view"))?;
        *cache = Some(CachedOutputTextures {
            width,
            height,
            output,
            staging,
            view,
        });
    }
    let cached = cache.as_ref().expect("output cache was populated");
    Ok((
        cached.output.clone(),
        cached.staging.clone(),
        cached.view.clone(),
        created,
    ))
}

#[cfg(test)]
fn shader_parameters(
    evaluation: &FrameEvaluation,
    settings: &WorkbenchSettings,
    background: Option<&DecodedBackgroundImage>,
    focus_overlay: Option<[f64; 2]>,
) -> Result<ShaderParameters, CompositorError> {
    shader_parameters_for_source(
        evaluation,
        settings,
        background,
        focus_overlay,
        (evaluation.output.width, evaluation.output.height),
    )
}

fn shader_parameters_for_source(
    evaluation: &FrameEvaluation,
    settings: &WorkbenchSettings,
    background: Option<&DecodedBackgroundImage>,
    focus_overlay: Option<[f64; 2]>,
    source_size: (u32, u32),
) -> Result<ShaderParameters, CompositorError> {
    let rect = evaluation.camera_transform.source_rect();
    let output_width = evaluation.output.width as f32;
    let output_height = evaluation.output.height as f32;
    let content = evaluation.content_rect(source_size, settings.canvas.inset);
    let canvas_width = (content.right - content.left) as f32 * output_width;
    let canvas_height = (content.bottom - content.top) as f32 * output_height;
    let canvas_left = content.left as f32 * output_width;
    let canvas_top = content.top as f32 * output_height;
    let cursor = evaluation
        .cursor
        .filter(|state| state.visible && settings.cursor.visible)
        .map_or([0.0, 0.0, 32.0, 0.0], |state| {
            [
                canvas_left + (state.output_x as f32) * canvas_width,
                canvas_top + (state.output_y as f32) * canvas_height,
                (f64::from(state.size_px) * settings.cursor.scale) as f32,
                1.0,
            ]
        });
    Ok(ShaderParameters {
        source_rect: [
            rect.left as f32,
            rect.top as f32,
            (rect.right - rect.left) as f32,
            (rect.bottom - rect.top) as f32,
        ],
        cursor,
        canvas_rect: [canvas_left, canvas_top, canvas_width, canvas_height],
        background: settings.background_rgba()?,
        style: [
            settings.canvas.corner_radius as f32 * output_width.min(output_height),
            settings.canvas.shadow as f32,
            output_width.min(output_height) * 0.025,
            f32::from(background.is_some()),
        ],
        background_rect: background.map_or([0.0, 0.0, 1.0, 1.0], |background| {
            cover_source_rect(
                background.width,
                background.height,
                evaluation.output.width,
                evaluation.output.height,
            )
        }),
        crop_rect: [0.0; 4],
        overlay: focus_overlay.map_or([0.0; 4], |point| {
            [
                (point[0].clamp(0.0, 1.0) * f64::from(evaluation.output.width)) as f32,
                (point[1].clamp(0.0, 1.0) * f64::from(evaluation.output.height)) as f32,
                1.0,
                0.0,
            ]
        }),
    })
}

fn cover_source_rect(
    source_width: u32,
    source_height: u32,
    output_width: u32,
    output_height: u32,
) -> [f32; 4] {
    let source_aspect = source_width as f32 / source_height as f32;
    let output_aspect = output_width as f32 / output_height as f32;
    if source_aspect > output_aspect {
        let width = output_aspect / source_aspect;
        [(1.0 - width) * 0.5, 0.0, width, 1.0]
    } else {
        let height = source_aspect / output_aspect;
        [0.0, (1.0 - height) * 0.5, 1.0, height]
    }
}

fn compile_shader(entry: &'static [u8], target: &'static [u8]) -> Result<Vec<u8>, CompositorError> {
    let mut bytecode = None;
    let mut errors = None;
    let result = unsafe {
        D3DCompile(
            COMPOSITOR_SHADER.as_ptr().cast(),
            COMPOSITOR_SHADER.len(),
            PCSTR::null(),
            None,
            None::<&ID3DInclude>,
            PCSTR(entry.as_ptr()),
            PCSTR(target.as_ptr()),
            0,
            0,
            &raw mut bytecode,
            Some(&raw mut errors),
        )
    };
    if let Err(source) = result {
        return Err(CompositorError::ShaderCompile {
            message: errors
                .as_ref()
                .map_or_else(|| "D3DCompile returned no diagnostic".into(), blob_string),
            source,
        });
    }
    let bytecode = bytecode.ok_or(CompositorError::NullInterface("compiled shader blob"))?;
    Ok(blob_bytes(&bytecode).to_vec())
}

fn blob_bytes(blob: &ID3DBlob) -> &[u8] {
    unsafe { std::slice::from_raw_parts(blob.GetBufferPointer().cast(), blob.GetBufferSize()) }
}

fn blob_string(blob: &ID3DBlob) -> String {
    String::from_utf8_lossy(blob_bytes(blob))
        .trim_end_matches('\0')
        .to_owned()
}

fn validate_source(source: &DecodedBgraFrame) -> Result<(), CompositorError> {
    validate_bgra(source.width, source.height, &source.pixels, "source")
}

fn validate_bgra(
    width: u32,
    height: u32,
    pixels: &[u8],
    label: &'static str,
) -> Result<(), CompositorError> {
    let required = pixel_length(width, height)?;
    if pixels.len() != required {
        return Err(CompositorError::InvalidSourceLength {
            label,
            actual: pixels.len(),
            required,
        });
    }
    Ok(())
}

fn pixel_length(width: u32, height: u32) -> Result<usize, CompositorError> {
    width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .and_then(|bytes| usize::try_from(bytes).ok())
        .ok_or(CompositorError::InvalidGeometry { width, height })
}

fn create_bgra_texture(
    device: &windows::Win32::Graphics::Direct3D11::ID3D11Device,
    width: u32,
    height: u32,
    pixels: &[u8],
    label: &'static str,
) -> Result<ID3D11Texture2D, CompositorError> {
    let description = texture_description(
        width,
        height,
        D3D11_USAGE_IMMUTABLE,
        D3D11_BIND_SHADER_RESOURCE.0 as u32,
        0,
    );
    let initial = D3D11_SUBRESOURCE_DATA {
        pSysMem: pixels.as_ptr().cast(),
        SysMemPitch: width.saturating_mul(4),
        SysMemSlicePitch: 0,
    };
    let mut texture = None;
    stage("ID3D11Device::CreateTexture2D(BGRA)", unsafe {
        device.CreateTexture2D(
            &raw const description,
            Some(&raw const initial),
            Some(&raw mut texture),
        )
    })?;
    texture.ok_or(CompositorError::NullInterface(label))
}

fn create_output_texture(
    device: &windows::Win32::Graphics::Direct3D11::ID3D11Device,
    width: u32,
    height: u32,
) -> Result<ID3D11Texture2D, CompositorError> {
    let description = texture_description(
        width,
        height,
        D3D11_USAGE_DEFAULT,
        D3D11_BIND_RENDER_TARGET.0 as u32,
        0,
    );
    let mut texture = None;
    stage("ID3D11Device::CreateTexture2D(output)", unsafe {
        device.CreateTexture2D(&raw const description, None, Some(&raw mut texture))
    })?;
    texture.ok_or(CompositorError::NullInterface("output texture"))
}

fn create_staging_texture(
    device: &windows::Win32::Graphics::Direct3D11::ID3D11Device,
    width: u32,
    height: u32,
) -> Result<ID3D11Texture2D, CompositorError> {
    let description = texture_description(
        width,
        height,
        D3D11_USAGE_STAGING,
        0,
        D3D11_CPU_ACCESS_READ.0 as u32,
    );
    let mut texture = None;
    stage("ID3D11Device::CreateTexture2D(staging)", unsafe {
        device.CreateTexture2D(&raw const description, None, Some(&raw mut texture))
    })?;
    texture.ok_or(CompositorError::NullInterface("staging texture"))
}

fn texture_description(
    width: u32,
    height: u32,
    usage: windows::Win32::Graphics::Direct3D11::D3D11_USAGE,
    bind_flags: u32,
    cpu_access_flags: u32,
) -> D3D11_TEXTURE2D_DESC {
    D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: usage,
        BindFlags: bind_flags,
        CPUAccessFlags: cpu_access_flags,
        MiscFlags: 0,
    }
}

fn create_parameter_buffer(
    device: &windows::Win32::Graphics::Direct3D11::ID3D11Device,
    parameters: &ShaderParameters,
) -> Result<ID3D11Buffer, CompositorError> {
    let description = D3D11_BUFFER_DESC {
        ByteWidth: mem::size_of::<ShaderParameters>() as u32,
        Usage: D3D11_USAGE_IMMUTABLE,
        BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
        StructureByteStride: 0,
    };
    let initial = D3D11_SUBRESOURCE_DATA {
        pSysMem: ptr::from_ref(parameters).cast(),
        SysMemPitch: 0,
        SysMemSlicePitch: 0,
    };
    let mut buffer = None;
    stage("ID3D11Device::CreateBuffer(parameters)", unsafe {
        device.CreateBuffer(
            &raw const description,
            Some(&raw const initial),
            Some(&raw mut buffer),
        )
    })?;
    buffer.ok_or(CompositorError::NullInterface("parameter buffer"))
}

fn read_staging_texture(
    context: &windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext,
    texture: &ID3D11Texture2D,
    width: u32,
    height: u32,
    output_length: usize,
) -> Result<Vec<u8>, CompositorError> {
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    stage("ID3D11DeviceContext::Map(staging)", unsafe {
        context.Map(texture, 0, D3D11_MAP_READ, 0, Some(&raw mut mapped))
    })?;
    let row_bytes = width.saturating_mul(4);
    if mapped.pData.is_null() || mapped.RowPitch < row_bytes {
        unsafe { context.Unmap(texture, 0) };
        return Err(CompositorError::InvalidMappedOutput {
            row_pitch: mapped.RowPitch,
            row_bytes,
        });
    }
    let mut pixels = vec![0_u8; output_length];
    for row in 0..height as usize {
        unsafe {
            ptr::copy_nonoverlapping(
                mapped
                    .pData
                    .cast::<u8>()
                    .add(row * mapped.RowPitch as usize),
                pixels.as_mut_ptr().add(row * row_bytes as usize),
                row_bytes as usize,
            );
        }
    }
    unsafe { context.Unmap(texture, 0) };
    Ok(pixels)
}

fn stage<T>(
    stage_name: &'static str,
    result: windows::core::Result<T>,
) -> Result<T, CompositorError> {
    result.map_err(|source| CompositorError::WindowsStage {
        stage: stage_name,
        source,
    })
}

#[derive(Debug, Error)]
pub enum CompositorError {
    #[error("D3D11 compositor stage '{stage}' failed: {source}")]
    WindowsStage {
        stage: &'static str,
        #[source]
        source: windows::core::Error,
    },
    #[error("D3D shader compilation failed: {message} ({source})")]
    ShaderCompile {
        message: String,
        #[source]
        source: windows::core::Error,
    },
    #[error("D3D11 returned a null {0} interface")]
    NullInterface(&'static str),
    #[error("invalid compositor geometry {width}x{height}")]
    InvalidGeometry { width: u32, height: u32 },
    #[error(
        "preview presenter is {presenter_width}x{presenter_height}, but frame output is {output_width}x{output_height}"
    )]
    PresenterGeometryMismatch {
        presenter_width: u32,
        presenter_height: u32,
        output_width: u32,
        output_height: u32,
    },
    #[error("{label} BGRA length {actual} does not equal required {required}")]
    InvalidSourceLength {
        label: &'static str,
        actual: usize,
        required: usize,
    },
    #[error("compositor only supports BGRA8 output")]
    UnsupportedPixelFormat,
    #[error("image background settings require decoded Project background data")]
    MissingBackgroundImageData,
    #[error(transparent)]
    InvalidWorkbenchSettings(#[from] WorkbenchValidationError),
    #[error("mapped output row pitch {row_pitch} is smaller than row bytes {row_bytes}")]
    InvalidMappedOutput { row_pitch: u32, row_bytes: u32 },
}

#[derive(Debug, Error)]
pub enum RenderHashProbeError {
    #[error("preview/export pipeline: {0}")]
    Pipeline(String),
    #[error(transparent)]
    Project(#[from] ProjectIoError),
    #[error(transparent)]
    Editor(#[from] EditorSessionError),
    #[error("failed to read project file {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid cursor JSONL record {path}:{line}: {source}")]
    InvalidCursorRecord {
        path: PathBuf,
        line: usize,
        #[source]
        source: serde_json::Error,
    },
    #[error(transparent)]
    Decoder(#[from] MediaDecoderError),
    #[error(transparent)]
    Evaluation(#[from] RenderEvaluationError),
    #[error(transparent)]
    Compositor(#[from] CompositorError),
    #[error("project time cannot be negative: {0:?}")]
    NegativeProjectTime(TimeTick),
    #[error("source video has no decoded frame")]
    NoSourceFrame,
    #[error("evaluator selected PTS {evaluator_tick:?}, but decoder retained {decoded_tick:?}")]
    SelectionMismatch {
        evaluator_tick: TimeTick,
        decoded_tick: TimeTick,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use panzo_core::{CameraState, CameraTransform, SourceFrameSelection};

    #[test]
    fn rounded_video_corners_blend_edges_without_softening_the_center() {
        let compositor = D3d11Compositor::create().unwrap();
        let source = DecodedBgraFrame {
            native_size: None,
            pts: TimeTick::ZERO,
            duration: None,
            width: 320,
            height: 180,
            pixels: [230, 230, 230, 255].repeat(320 * 180).into(),
        };
        let evaluation = FrameEvaluation {
            project_tick: TimeTick::ZERO,
            source_tick: TimeTick::ZERO,
            source_frame: SourceFrameSelection {
                frame_index: 0,
                presentation_tick: TimeTick::ZERO,
            },
            camera_state: CameraState::BASE,
            camera_transform: CameraState::BASE.into(),
            cursor: None,
            output: OutputDescriptor::bgra8(320, 180).unwrap(),
        };
        let mut settings = WorkbenchSettings::default();
        settings.background.color = "#101010".into();
        settings.canvas.inset = 0.1;
        settings.canvas.corner_radius = 0.1;
        settings.canvas.shadow = 0.0;
        let frame = compositor
            .compose_with_settings(&source, &evaluation, &settings)
            .unwrap();
        assert_eq!(&frame.pixels[..4], &[16, 16, 16, 255]);
        assert_eq!(
            &frame.pixels[(90 * 320 + 160) * 4..(90 * 320 + 160) * 4 + 4],
            &[230, 230, 230, 255]
        );
        let blended = frame
            .pixels
            .chunks_exact(4)
            .filter(|p| p[0] > 16 && p[0] < 230)
            .count();
        assert!(blended >= 20, "rounded edges need partial pixel coverage");
        assert!(
            frame
                .pixels
                .chunks_exact(4)
                .all(|p| p[3] == 255 && (16..=230).contains(&p[0]))
        );
        if let Some(dir) = std::env::var_os("PANZO_UI_REVIEW_DIR") {
            image::save_buffer(
                std::path::Path::new(&dir).join("canvas-rounded.png"),
                &frame.pixels,
                320,
                180,
                image::ColorType::Rgba8,
            )
            .unwrap();
        }
    }

    #[test]
    fn cursor_has_a_stem_border_and_antialiased_edges_at_multiple_sizes() {
        let compositor = D3d11Compositor::create().unwrap();
        let source = DecodedBgraFrame {
            native_size: None,
            pts: TimeTick::ZERO,
            duration: None,
            width: 160,
            height: 120,
            pixels: [100, 100, 100, 255].repeat(160 * 120).into(),
        };
        for size in [32, 48, 64] {
            let evaluation = FrameEvaluation {
                project_tick: TimeTick::ZERO,
                source_tick: TimeTick::ZERO,
                source_frame: SourceFrameSelection {
                    frame_index: 0,
                    presentation_tick: TimeTick::ZERO,
                },
                camera_state: CameraState::BASE,
                camera_transform: CameraTransform::from(CameraState::BASE),
                cursor: Some(panzo_core::CursorRenderState {
                    source_x: 0.25,
                    source_y: 0.25,
                    output_x: 0.25,
                    output_y: 0.25,
                    visible: true,
                    size_px: size,
                }),
                output: OutputDescriptor::bgra8(160, 120).unwrap(),
            };
            let frame = compositor.compose(&source, &evaluation).unwrap();
            let pixel = |x: u32, y: u32| frame.pixels[((y * 160 + x) * 4) as usize];
            assert!(
                pixel(40 + 12 * size / 32, 30 + 24 * size / 32) > 230,
                "missing cursor stem at {size}px"
            );
            assert_eq!(pixel(34, 24), 100, "cursor moved above its hotspot");
            assert_eq!(pixel(40 + 25 * size / 32, 30 + 25 * size / 32), 100);
            assert!(
                frame.pixels.chunks_exact(4).any(|p| p[0] < 40),
                "missing outline"
            );
            assert!(
                frame
                    .pixels
                    .chunks_exact(4)
                    .any(|p| p[0] > 110 && p[0] < 220),
                "missing antialiasing"
            );
            if let Some(output) = std::env::var_os("PANZO_UI_REVIEW_DIR") {
                image::save_buffer(
                    std::path::Path::new(&output).join(format!("cursor-{size}.png")),
                    &frame.pixels,
                    160,
                    120,
                    image::ColorType::Rgba8,
                )
                .unwrap();
            }
        }
    }

    #[test]
    fn non_widescreen_and_small_sources_are_contained_without_upscaling() {
        let compositor = D3d11Compositor::create().unwrap();
        let output = OutputDescriptor::bgra8(320, 180).unwrap();
        let evaluation = FrameEvaluation {
            project_tick: TimeTick::ZERO,
            source_tick: TimeTick::ZERO,
            source_frame: SourceFrameSelection {
                frame_index: 0,
                presentation_tick: TimeTick::ZERO,
            },
            camera_state: CameraState::BASE,
            camera_transform: CameraTransform::from(CameraState::BASE),
            cursor: None,
            output,
        };
        let mut settings = WorkbenchSettings::default();
        settings.background.color = "#112233".into();
        for (size, expected) in [
            ((240, 180), [40.0, 0.0, 240.0, 180.0]),
            ((80, 60), [120.0, 60.0, 80.0, 60.0]),
            ((1200, 900), [40.0, 0.0, 240.0, 180.0]),
        ] {
            let parameters =
                shader_parameters_for_source(&evaluation, &settings, None, None, size).unwrap();
            for (actual, expected) in parameters.canvas_rect.iter().zip(expected) {
                assert!((actual - expected).abs() < 0.01);
            }
            let source = DecodedBgraFrame {
                native_size: Some(size),
                pts: TimeTick::ZERO,
                duration: None,
                width: size.0,
                height: size.1,
                pixels: [20, 80, 180, 255].repeat((size.0 * size.1) as usize).into(),
            };
            let frame = compositor
                .compose_with_settings(&source, &evaluation, &settings)
                .unwrap();
            assert_eq!(&frame.pixels[..4], &[0x33, 0x22, 0x11, 255]);
            let center = ((90 * 320 + 160) * 4) as usize;
            assert_eq!(&frame.pixels[center..center + 4], &[20, 80, 180, 255]);
        }
    }

    #[test]
    fn four_corner_camera_states_never_expose_black_border() {
        const WIDTH: u32 = 320;
        const HEIGHT: u32 = 180;
        const SOURCE_COLOR: [u8; 4] = [40, 80, 120, 255];
        let compositor = D3d11Compositor::create().unwrap();
        let mut source_pixels = vec![0_u8; (WIDTH * HEIGHT * 4) as usize];
        for pixel in source_pixels.chunks_exact_mut(4) {
            pixel.copy_from_slice(&SOURCE_COLOR);
        }
        let source = DecodedBgraFrame {
            native_size: None,
            pts: TimeTick::ZERO,
            duration: None,
            width: WIDTH,
            height: HEIGHT,
            pixels: source_pixels.into(),
        };
        let output = OutputDescriptor::bgra8(WIDTH, HEIGHT).unwrap();
        for (center_x, center_y) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
            let camera_state = CameraState {
                center_x,
                center_y,
                scale: 2.0,
            };
            camera_state.validate().unwrap();
            let evaluation = FrameEvaluation {
                project_tick: TimeTick::ZERO,
                source_tick: TimeTick::ZERO,
                source_frame: SourceFrameSelection {
                    frame_index: 0,
                    presentation_tick: TimeTick::ZERO,
                },
                camera_state,
                camera_transform: CameraTransform::from(camera_state),
                cursor: None,
                output,
            };
            let frame = compositor.compose(&source, &evaluation).unwrap();
            assert!(
                frame
                    .pixels
                    .chunks_exact(4)
                    .all(|pixel| pixel == SOURCE_COLOR),
                "camera center ({center_x}, {center_y}) exposed pixels outside the source"
            );
        }
    }

    #[test]
    fn workbench_inset_reveals_solid_background_and_keeps_source_center() {
        const WIDTH: u32 = 320;
        const HEIGHT: u32 = 180;
        const SOURCE_COLOR: [u8; 4] = [20, 80, 180, 255];
        let compositor = D3d11Compositor::create().unwrap();
        let mut source_pixels = vec![0_u8; (WIDTH * HEIGHT * 4) as usize];
        for pixel in source_pixels.chunks_exact_mut(4) {
            pixel.copy_from_slice(&SOURCE_COLOR);
        }
        let source = DecodedBgraFrame {
            native_size: None,
            pts: TimeTick::ZERO,
            duration: None,
            width: WIDTH,
            height: HEIGHT,
            pixels: source_pixels.into(),
        };
        let output = OutputDescriptor::bgra8(WIDTH, HEIGHT).unwrap();
        let evaluation = FrameEvaluation {
            project_tick: TimeTick::ZERO,
            source_tick: TimeTick::ZERO,
            source_frame: SourceFrameSelection {
                frame_index: 0,
                presentation_tick: TimeTick::ZERO,
            },
            camera_state: CameraState::BASE,
            camera_transform: CameraTransform::from(CameraState::BASE),
            cursor: None,
            output,
        };
        let mut settings = WorkbenchSettings::default();
        settings.background.color = "#112233".into();
        settings.canvas.inset = 0.10;
        let frame = compositor
            .compose_with_settings(&source, &evaluation, &settings)
            .unwrap();
        assert_eq!(&frame.pixels[0..4], &[0x33, 0x22, 0x11, 0xff]);
        let center = ((HEIGHT / 2 * WIDTH + WIDTH / 2) * 4) as usize;
        assert_eq!(&frame.pixels[center..center + 4], &SOURCE_COLOR);
    }

    #[test]
    fn crop_removes_source_borders_without_stretching_canvas_or_zoom() {
        let pixels: Vec<u8> = (0..180)
            .flat_map(|_| {
                (0..320).flat_map(|x| {
                    if x < 80 {
                        [0, 0, 255, 255]
                    } else if x >= 240 {
                        [255, 0, 0, 255]
                    } else {
                        [0, 255, 0, 255]
                    }
                })
            })
            .collect();
        let source = DecodedBgraFrame {
            native_size: None,
            pts: TimeTick::ZERO,
            duration: None,
            width: 320,
            height: 180,
            pixels: pixels.into(),
        };
        let compositor = D3d11Compositor::create().unwrap();
        let mut settings = WorkbenchSettings::default();
        settings.background.color = "#202020".into();
        settings.canvas.inset = 0.0;
        settings.canvas.corner_radius = 0.0;
        settings.canvas.shadow = 0.0;
        let crop = panzo_core::VideoCrop {
            left: 250,
            right: 250,
            ..panzo_core::VideoCrop::default()
        };
        for scale in [1.0, 2.0] {
            let camera = CameraState {
                scale,
                ..CameraState::BASE
            };
            let eval = FrameEvaluation {
                project_tick: TimeTick::ZERO,
                source_tick: TimeTick::ZERO,
                source_frame: SourceFrameSelection {
                    frame_index: 0,
                    presentation_tick: TimeTick::ZERO,
                },
                camera_state: camera,
                camera_transform: CameraTransform::from(camera).with_crop(crop),
                cursor: None,
                output: OutputDescriptor::bgra8(320, 180).unwrap(),
            };
            let frame = compositor
                .compose_with_settings(&source, &eval, &settings)
                .unwrap();
            for (x, color) in [
                (20, [32, 32, 32, 255]),
                (100, [0, 255, 0, 255]),
                (220, [0, 255, 0, 255]),
                (300, [32, 32, 32, 255]),
            ] {
                let start = (90 * 320 + x) * 4;
                assert_eq!(&frame.pixels[start..start + 4], &color);
            }
        }
    }

    #[test]
    fn workbench_image_background_is_sampled_behind_inset_canvas() {
        const WIDTH: u32 = 320;
        const HEIGHT: u32 = 180;
        const SOURCE_COLOR: [u8; 4] = [20, 80, 180, 255];
        const BACKGROUND_COLOR: [u8; 4] = [9, 80, 210, 255];
        let compositor = D3d11Compositor::create().unwrap();
        let source = DecodedBgraFrame {
            native_size: None,
            pts: TimeTick::ZERO,
            duration: None,
            width: WIDTH,
            height: HEIGHT,
            pixels: SOURCE_COLOR.repeat((WIDTH * HEIGHT) as usize).into(),
        };
        let background = DecodedBackgroundImage::new(4, 2, BACKGROUND_COLOR.repeat(8));
        let output = OutputDescriptor::bgra8(WIDTH, HEIGHT).unwrap();
        let evaluation = FrameEvaluation {
            project_tick: TimeTick::ZERO,
            source_tick: TimeTick::ZERO,
            source_frame: SourceFrameSelection {
                frame_index: 0,
                presentation_tick: TimeTick::ZERO,
            },
            camera_state: CameraState::BASE,
            camera_transform: CameraTransform::from(CameraState::BASE),
            cursor: None,
            output,
        };
        let mut settings = WorkbenchSettings::default();
        settings.background.kind = panzo_core::BackgroundKind::Image;
        settings.background.image = Some("assets/background.png".into());
        settings.canvas.inset = 0.10;
        let frame = compositor
            .compose_with_background(&source, &evaluation, &settings, Some(&background))
            .unwrap();
        assert_eq!(&frame.pixels[0..4], &BACKGROUND_COLOR);
        let center = ((HEIGHT / 2 * WIDTH + WIDTH / 2) * 4) as usize;
        assert_eq!(&frame.pixels[center..center + 4], &SOURCE_COLOR);
    }

    #[test]
    fn compositor_reuses_textures_for_repeated_interactive_render() {
        const WIDTH: u32 = 320;
        const HEIGHT: u32 = 180;
        let compositor = D3d11Compositor::create().unwrap();
        let source = DecodedBgraFrame {
            native_size: None,
            pts: TimeTick::ZERO,
            duration: None,
            width: WIDTH,
            height: HEIGHT,
            pixels: [20, 80, 180, 255].repeat((WIDTH * HEIGHT) as usize).into(),
        };
        let background = DecodedBackgroundImage::new(4, 2, [9, 80, 210, 255].repeat(8));
        let output = OutputDescriptor::bgra8(WIDTH, HEIGHT).unwrap();
        let evaluation = FrameEvaluation {
            project_tick: TimeTick::ZERO,
            source_tick: TimeTick::ZERO,
            source_frame: SourceFrameSelection {
                frame_index: 0,
                presentation_tick: TimeTick::ZERO,
            },
            camera_state: CameraState::BASE,
            camera_transform: CameraTransform::from(CameraState::BASE),
            cursor: None,
            output,
        };
        let mut settings = WorkbenchSettings::default();
        settings.background.kind = panzo_core::BackgroundKind::Image;
        settings.background.image = Some("assets/background.png".into());
        for _ in 0..3 {
            compositor
                .compose_with_background(&source, &evaluation, &settings, Some(&background))
                .unwrap();
        }
        assert_eq!(
            compositor.cache_stats(),
            CompositorCacheStats {
                source_uploads: 1,
                background_uploads: 1,
                output_allocations: 1,
                readbacks: 3,
                presentations: 0,
            }
        );
    }

    #[test]
    fn focus_overlay_is_scaled_into_output_pixels_and_clamped() {
        let output = OutputDescriptor::bgra8(320, 180).unwrap();
        let evaluation = FrameEvaluation {
            project_tick: TimeTick::ZERO,
            source_tick: TimeTick::ZERO,
            source_frame: SourceFrameSelection {
                frame_index: 0,
                presentation_tick: TimeTick::ZERO,
            },
            camera_state: CameraState::BASE,
            camera_transform: CameraTransform::from(CameraState::BASE),
            cursor: None,
            output,
        };
        let parameters = shader_parameters(
            &evaluation,
            &WorkbenchSettings::default(),
            None,
            Some([1.2, -0.5]),
        )
        .unwrap();
        assert_eq!(parameters.overlay, [320.0, 0.0, 1.0, 0.0]);
    }

    #[test]
    fn focus_cross_remains_visible_over_rounded_corners_and_background() {
        let compositor = D3d11Compositor::create().unwrap();
        let source = DecodedBgraFrame {
            native_size: None,
            pts: TimeTick::ZERO,
            duration: None,
            width: 320,
            height: 180,
            pixels: [20, 80, 180, 255].repeat(320 * 180).into(),
        };
        let evaluation = FrameEvaluation {
            project_tick: TimeTick::ZERO,
            source_tick: TimeTick::ZERO,
            source_frame: SourceFrameSelection {
                frame_index: 0,
                presentation_tick: TimeTick::ZERO,
            },
            camera_state: CameraState::BASE,
            camera_transform: CameraTransform::from(CameraState::BASE),
            cursor: None,
            output: OutputDescriptor::bgra8(320, 180).unwrap(),
        };
        let mut settings = WorkbenchSettings::default();
        settings.canvas.inset = 0.2;
        settings.canvas.corner_radius = 0.1;
        let (texture, staging, view, _) = cached_output_textures(
            &compositor.output_cache,
            compositor.graphics.native_device(),
            320,
            180,
        )
        .unwrap();
        for (point, (x, y)) in [
            ([0.2, 0.2], (64, 36)),
            ([0.8, 0.2], (256, 36)),
            ([0.2, 0.8], (64, 144)),
            ([0.8, 0.8], (256, 144)),
            ([0.0, 0.0], (0, 0)),
            ([1.0, 1.0], (319, 179)),
        ] {
            compositor
                .render_to_target(&source, &evaluation, &settings, None, &view, Some(point))
                .unwrap();
            let context = compositor.graphics.immediate_context();
            unsafe {
                context.CopyResource(&staging, &texture);
            }
            let pixels = read_staging_texture(context, &staging, 320, 180, 320 * 180 * 4).unwrap();
            let index = (y * 320 + x) * 4;
            assert!(
                pixels[index] < 40
                    && (150..=165).contains(&pixels[index + 1])
                    && pixels[index + 2] > 250,
                "focus cross clipped at {point:?}: {:?}",
                &pixels[index..index + 4]
            );
        }
        let export = compositor
            .compose_with_settings(&source, &evaluation, &settings)
            .unwrap();
        assert_eq!(
            &export.pixels[..4],
            &[0x27, 0x18, 0x11, 255],
            "editor cross must not appear in export"
        );
    }
}
