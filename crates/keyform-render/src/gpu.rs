//! Headless wgpu renderer. Produces the same image as [`crate::CpuRenderer`]
//! up to rasterization and sampling precision.
//!
//! spec: 40-rendering/

use std::collections::HashMap;

use keyform_eval::Frame;
use keyform_model::{ColorComposition, MeshIndex, Model};
use wgpu::util::DeviceExt;

use crate::{blend, finish_image, RenderError, RenderOptions, RgbaImage};

const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const STENCIL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Stencil8;

fn gpu_err(e: impl std::fmt::Display) -> RenderError {
    RenderError::Gpu(e.to_string())
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct DrawUniform {
    base: [f32; 4],
    mul: [f32; 4],
    scr: [f32; 4],
    flags: [f32; 4],
    modes: [u32; 4],
    /// xy: UV scale onto the unpadded texture area (appearance.md §5).
    uvs: [f32; 4],
}

/// An uploaded texture: all mip levels, and how it is sampled.
struct GpuTex {
    view: wgpu::TextureView,
    uv_scale: [f32; 2],
    /// Index into `GpuRenderer::samplers`.
    sampler: usize,
}

/// Sampler slots: bilinear, trilinear, trilinear + anisotropic.
const SAMPLER_BILINEAR: usize = 0;
const SAMPLER_TRILINEAR: usize = 1;
const SAMPLER_ANISOTROPIC: usize = 2;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    pos: [f32; 2],
    uv: [f32; 2],
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Cull {
    None,
    Back,
    Front,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum PipeKind {
    Normal,
    Add,
    Multiply,
    Advanced,
    Mask,
}

/// GPU device, pipelines and uploaded textures for one model.
pub struct GpuRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    layout: wgpu::BindGroupLayout,
    pipelines: HashMap<(PipeKind, Cull), wgpu::RenderPipeline>,
    samplers: Vec<wgpu::Sampler>,
    textures: Vec<Option<GpuTex>>,
    /// 1×1 placeholder bound where a texture slot is unused.
    dummy: GpuTex,
    pub adapter_info: String,
    pub warnings: Vec<String>,
}

impl GpuRenderer {
    /// Number of texture slots (the model's texture count at creation).
    pub fn texture_count(&self) -> usize {
        self.textures.len()
    }

    /// Creates a headless device. Fails when no adapter is available.
    pub fn new(model: &Model) -> Result<Self, RenderError> {
        pollster::block_on(Self::new_async(model))
    }

    async fn new_async(model: &Model) -> Result<Self, RenderError> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: None,
                apply_limit_buckets: false,
            })
            .await
            .map_err(gpu_err)?;
        let info = adapter.get_info();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("keyform"),
                // Transparent texture borders when available (appearance.md §5).
                required_features: adapter.features()
                    & wgpu::Features::ADDRESS_MODE_CLAMP_TO_BORDER,
                required_limits: wgpu::Limits::downlevel_defaults()
                    .using_resolution(adapter.limits()),
                ..Default::default()
            })
            .await
            .map_err(gpu_err)?;
        let info = format!("{} ({:?})", info.name, info.backend);
        Ok(Self::with_device(device, queue, model, info))
    }

    /// Builds pipelines on an existing device (e.g. the UI's) and uploads
    /// `model`'s textures.
    pub fn with_device(
        device: wgpu::Device,
        queue: wgpu::Queue,
        model: &Model,
        adapter_info: String,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("keyform shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
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
            label: Some("draw"),
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
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                tex_entry(3),
                tex_entry(4),
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("draw"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });

        let comp = |src, dst, op| wgpu::BlendComponent {
            src_factor: src,
            dst_factor: dst,
            operation: op,
        };
        use wgpu::BlendFactor as F;
        use wgpu::BlendOperation::Add as Plus;
        let mut pipelines = HashMap::new();
        for kind in [
            PipeKind::Normal,
            PipeKind::Add,
            PipeKind::Multiply,
            PipeKind::Advanced,
            PipeKind::Mask,
        ] {
            for cull in [Cull::None, Cull::Back, Cull::Front] {
                // spec: blending.md §3 (fixed-function, premultiplied).
                let blend = match kind {
                    PipeKind::Normal | PipeKind::Mask => {
                        Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING)
                    }
                    PipeKind::Add => Some(wgpu::BlendState {
                        color: comp(F::One, F::One, Plus),
                        alpha: comp(F::Zero, F::One, Plus),
                    }),
                    PipeKind::Multiply => Some(wgpu::BlendState {
                        color: comp(F::Dst, F::OneMinusSrcAlpha, Plus),
                        alpha: comp(F::Zero, F::One, Plus),
                    }),
                    PipeKind::Advanced => None,
                };
                let fs = match kind {
                    PipeKind::Advanced => "fs_advanced",
                    PipeKind::Mask => "fs_mask",
                    _ => "fs_simple",
                };
                // One blend per pixel per mesh (blending.md §2): pass only where
                // the stencil differs from this mesh's reference, then write it.
                let stencil_face = wgpu::StencilFaceState {
                    compare: wgpu::CompareFunction::NotEqual,
                    fail_op: wgpu::StencilOperation::Keep,
                    depth_fail_op: wgpu::StencilOperation::Keep,
                    pass_op: wgpu::StencilOperation::Replace,
                };
                let depth_stencil = (kind != PipeKind::Mask).then(|| wgpu::DepthStencilState {
                    format: STENCIL_FORMAT,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::Always),
                    stencil: if kind == PipeKind::Advanced {
                        wgpu::StencilState {
                            front: stencil_face,
                            back: stencil_face,
                            read_mask: 0xff,
                            write_mask: 0xff,
                        }
                    } else {
                        wgpu::StencilState::default()
                    },
                    bias: wgpu::DepthBiasState::default(),
                });
                let p = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("drawable"),
                    layout: Some(&pl),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("vs_main"),
                        compilation_options: Default::default(),
                        buffers: &[Some(wgpu::VertexBufferLayout {
                            array_stride: std::mem::size_of::<Vertex>() as u64,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2],
                        })],
                    },
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::TriangleList,
                        // Front face: positive signed area in y-down canvas space,
                        // i.e. clockwise once y is flipped to NDC (blending.md §4).
                        front_face: wgpu::FrontFace::Cw,
                        cull_mode: match cull {
                            Cull::None => None,
                            Cull::Back => Some(wgpu::Face::Back),
                            Cull::Front => Some(wgpu::Face::Front),
                        },
                        ..Default::default()
                    },
                    depth_stencil,
                    multisample: wgpu::MultisampleState::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some(fs),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: TARGET_FORMAT,
                            blend,
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    multiview_mask: None,
                    cache: None,
                });
                pipelines.insert((kind, cull), p);
            }
        }

        // Texture sampling (appearance.md §5): bilinear, or trilinear with
        // mipmaps, optionally anisotropic at the usual device maximum (16).
        // Outside the texture is transparent where the device can do it.
        let border = device
            .features()
            .contains(wgpu::Features::ADDRESS_MODE_CLAMP_TO_BORDER);
        let address = if border {
            wgpu::AddressMode::ClampToBorder
        } else {
            wgpu::AddressMode::ClampToEdge
        };
        let sampler = |label: &str, mips: bool, aniso: u16| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some(label),
                address_mode_u: address,
                address_mode_v: address,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: if mips {
                    wgpu::MipmapFilterMode::Linear
                } else {
                    wgpu::MipmapFilterMode::Nearest
                },
                anisotropy_clamp: aniso,
                border_color: border.then_some(wgpu::SamplerBorderColor::TransparentBlack),
                ..Default::default()
            })
        };
        let samplers = vec![
            sampler("bilinear", false, 1),
            sampler("trilinear", true, 1),
            sampler("anisotropic", true, crate::image::MAX_ANISOTROPY as u16),
        ];

        let mut warnings = Vec::new();
        // Uploads levels (largest first, premultiplied RGBA8) as one texture.
        let upload = |w: u32, h: u32, levels: u32, rgba: &[u8], label: &str| -> wgpu::TextureView {
            let t = device.create_texture_with_data(
                &queue,
                &wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: w,
                        height: h,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: levels,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: TARGET_FORMAT,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                rgba,
            );
            t.create_view(&Default::default())
        };
        let dummy = GpuTex {
            view: upload(1, 1, 1, &[0, 0, 0, 0], "dummy"),
            uv_scale: [1.0, 1.0],
            sampler: SAMPLER_BILINEAR,
        };
        let max_side = device.limits().max_texture_dimension_2d;
        let textures = model
            .textures
            .iter()
            .map(|t| {
                let img = match t.png.as_deref().map(RgbaImage::decode_png) {
                    Some(Ok(img)) => img,
                    Some(Err(e)) => {
                        warnings.push(format!("texture {:?}: {e}", t.name));
                        return None;
                    }
                    None => {
                        warnings.push(format!("texture {:?} has no image data", t.name));
                        return None;
                    }
                };
                if img.width > max_side || img.height > max_side {
                    warnings.push(format!(
                        "texture {:?} exceeds the GPU limit {max_side}",
                        t.name
                    ));
                    return None;
                }
                // Premultiplied, padded mip chain built like the CPU path's
                // (cmo3.md §3, appearance.md §5), stored as RGBA8.
                let chain = crate::image::Texture::new(&img, t.sampling);
                let base = &chain.levels[0];
                if base.width as u32 > max_side || base.height as u32 > max_side {
                    warnings.push(format!(
                        "texture {:?} exceeds the GPU limit {max_side}",
                        t.name
                    ));
                    return None;
                }
                let bytes: Vec<u8> = chain
                    .levels
                    .iter()
                    .flat_map(|l| l.texels.iter())
                    .flat_map(|p| p.map(|c| (c * 255.0).round().clamp(0.0, 255.0) as u8))
                    .collect();
                let sampler = match (t.sampling.mipmaps, t.sampling.anisotropy) {
                    (false, _) => SAMPLER_BILINEAR,
                    (true, false) => SAMPLER_TRILINEAR,
                    (true, true) => SAMPLER_ANISOTROPIC,
                };
                Some(GpuTex {
                    view: upload(
                        base.width as u32,
                        base.height as u32,
                        chain.levels.len() as u32,
                        &bytes,
                        &t.name,
                    ),
                    uv_scale: chain.uv_scale,
                    sampler,
                })
            })
            .collect();

        Self {
            device,
            queue,
            layout,
            pipelines,
            samplers,
            textures,
            dummy,
            adapter_info,
            warnings,
        }
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// Format of render targets passed to [`Self::render_to`].
    pub const TARGET_FORMAT: wgpu::TextureFormat = TARGET_FORMAT;

    /// Usage flags a render target for [`Self::render_to`] needs.
    pub const TARGET_USAGE: wgpu::TextureUsages = wgpu::TextureUsages::RENDER_ATTACHMENT
        .union(wgpu::TextureUsages::COPY_SRC)
        .union(wgpu::TextureUsages::TEXTURE_BINDING);

    /// Renders into `target` (format [`Self::TARGET_FORMAT`], size = view
    /// size × supersample) and submits; no readback. The result is
    /// premultiplied.
    pub fn render_to(
        &self,
        model: &Model,
        frame: &Frame,
        opts: &RenderOptions,
        target: &wgpu::Texture,
    ) -> Result<(), RenderError> {
        let enc = self.encode(model, frame, opts, target)?;
        self.queue.submit([enc.finish()]);
        Ok(())
    }

    fn target(
        &self,
        w: u32,
        h: u32,
        usage: wgpu::TextureUsages,
        format: wgpu::TextureFormat,
        label: &str,
    ) -> wgpu::Texture {
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
    }

    /// Renders headlessly and reads the image back.
    pub fn render(
        &self,
        model: &Model,
        frame: &Frame,
        opts: &RenderOptions,
    ) -> Result<RgbaImage, RenderError> {
        let ss = opts.supersample.clamp(1, 8);
        let v = opts.view;
        crate::check_view(&v)?;
        let (w, h) = (v.width * ss, v.height * ss);
        let max = self.device.limits().max_texture_dimension_2d;
        if w > max || h > max {
            return Err(RenderError::Options(format!(
                "{w}×{h} exceeds the GPU limit {max}"
            )));
        }
        let color = self.target(w, h, Self::TARGET_USAGE, TARGET_FORMAT, "color");
        let mut enc = self.encode(model, frame, opts, &color)?;
        self.readback(
            &mut enc,
            &color,
            w,
            h,
            v.width as usize,
            v.height as usize,
            ss as usize,
        )
    }

    fn encode(
        &self,
        model: &Model,
        frame: &Frame,
        opts: &RenderOptions,
        color: &wgpu::Texture,
    ) -> Result<wgpu::CommandEncoder, RenderError> {
        let ss = opts.supersample.clamp(1, 8);
        let v = opts.view;
        crate::check_view(&v)?;
        let (w, h) = (v.width * ss, v.height * ss);
        if color.width() != w || color.height() != h || color.format() != TARGET_FORMAT {
            return Err(RenderError::Options(format!(
                "target must be {w}×{h} {TARGET_FORMAT:?}"
            )));
        }
        let ssf = ss as f32;
        let (fw, fh) = (w as f32, h as f32);
        let ndc = |p: &[f32]| -> [f32; 2] {
            let q = v.to_px([p[0], p[1]]);
            let (x, y) = (q[0] * ssf, q[1] * ssf);
            [x / fw * 2.0 - 1.0, 1.0 - y / fh * 2.0]
        };

        use wgpu::TextureUsages as U;
        let color_view = color.create_view(&Default::default());
        let dst_copy = self.target(
            w,
            h,
            U::COPY_DST | U::TEXTURE_BINDING,
            TARGET_FORMAT,
            "dst copy",
        );
        let dst_view = dst_copy.create_view(&Default::default());
        let stencil = self.target(w, h, U::RENDER_ATTACHMENT, STENCIL_FORMAT, "stencil");
        let stencil_view = stencil.create_view(&Default::default());

        let m = model;
        // Geometry buffers per mesh (for both drawing and mask building).
        let mut geo: HashMap<MeshIndex, (wgpu::Buffer, wgpu::Buffer, u32)> = HashMap::new();
        let geometry = |geo: &mut HashMap<MeshIndex, (wgpu::Buffer, wgpu::Buffer, u32)>,
                        mi: MeshIndex| {
            if geo.contains_key(&mi) {
                return;
            }
            let (Some(am), Some(ms)) = (m.art_meshes.get(mi), frame.meshes.get(mi)) else {
                return;
            };
            let verts: Vec<Vertex> = ms
                .positions
                .chunks_exact(2)
                .enumerate()
                .map(|(i, p)| Vertex {
                    pos: ndc(p),
                    uv: [
                        am.uvs.get(2 * i).copied().unwrap_or(0.0),
                        am.uvs.get(2 * i + 1).copied().unwrap_or(0.0),
                    ],
                })
                .collect();
            let nv = verts.len() as u32;
            let idx: Vec<u32> = am
                .indices
                .chunks_exact(3)
                .filter(|t| t.iter().all(|&i| i < nv))
                .flatten()
                .copied()
                .collect();
            if verts.is_empty() || idx.is_empty() {
                return;
            }
            let vb = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("vertices"),
                    contents: bytemuck::cast_slice(&verts),
                    usage: wgpu::BufferUsages::VERTEX,
                });
            let ib = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("indices"),
                    contents: bytemuck::cast_slice(&idx),
                    usage: wgpu::BufferUsages::INDEX,
                });
            geo.insert(mi, (vb, ib, idx.len() as u32));
        };

        let tex_of = |mi: MeshIndex| -> Option<&GpuTex> {
            m.art_meshes
                .get(mi)?
                .texture
                .and_then(|t| self.textures.get(t))
                .and_then(Option::as_ref)
        };

        // Draw list in render order.
        struct Item {
            mesh: MeshIndex,
            kind: PipeKind,
            cull: Cull,
            mask: Option<usize>,
            uniform: DrawUniform,
        }
        let mut items = Vec::new();
        let mut mask_keys: Vec<Vec<MeshIndex>> = Vec::new();
        for &mi in &frame.render_order {
            let (Some(am), Some(ms)) = (m.art_meshes.get(mi), frame.meshes.get(mi)) else {
                continue;
            };
            if !ms.is_drawn() || tex_of(mi).is_none() {
                continue;
            }
            geometry(&mut geo, mi);
            if !geo.contains_key(&mi) {
                continue;
            }
            let mask = (!am.clip.is_empty()).then(|| {
                for &c in &am.clip {
                    geometry(&mut geo, c);
                }
                match mask_keys.iter().position(|k| *k == am.clip) {
                    Some(i) => i,
                    None => {
                        mask_keys.push(am.clip.clone());
                        mask_keys.len() - 1
                    }
                }
            });
            let kind = if !blend::is_simple(am.color_composition, am.alpha_composition) {
                PipeKind::Advanced
            } else {
                match am.color_composition {
                    ColorComposition::Add => PipeKind::Add,
                    ColorComposition::Multiply => PipeKind::Multiply,
                    _ => PipeKind::Normal,
                }
            };
            let a = ms.opacity;
            items.push(Item {
                mesh: mi,
                kind,
                // A mirrored view reverses on-screen winding: cull the other
                // face so the same triangles stay visible (blending.md §4).
                cull: match (am.culling, v.mirror) {
                    (false, _) => Cull::None,
                    (true, false) => Cull::Back,
                    (true, true) => Cull::Front,
                },
                mask,
                uniform: DrawUniform {
                    base: [a, a, a, a],
                    mul: [ms.multiply[0], ms.multiply[1], ms.multiply[2], 1.0],
                    scr: [ms.screen[0], ms.screen[1], ms.screen[2], 1.0],
                    flags: [
                        f32::from(u8::from(mask.is_some())),
                        f32::from(u8::from(am.invert_clip)),
                        a,
                        0.0,
                    ],
                    modes: [
                        u32::from(am.color_composition.moc3_code()),
                        u32::from(am.alpha_composition.moc3_code()),
                        0,
                        0,
                    ],
                    uvs: {
                        let s = tex_of(mi).map_or([1.0, 1.0], |t| t.uv_scale);
                        [s[0], s[1], 0.0, 0.0]
                    },
                },
            });
        }

        let bind = |uniform: &DrawUniform,
                    tex: &GpuTex,
                    mask: &wgpu::TextureView,
                    dst: &wgpu::TextureView| {
            let ub = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("draw uniform"),
                    contents: bytemuck::bytes_of(uniform),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
            self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("draw"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: ub.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&tex.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.samplers[tex.sampler]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(mask),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::TextureView(dst),
                    },
                ],
            })
        };

        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });

        // Mask textures (clipping.md §1): clip sources' texture alpha, over-accumulated.
        let zero_uniform = DrawUniform {
            base: [1.0; 4],
            mul: [1.0; 4],
            scr: [0.0; 4],
            flags: [0.0; 4],
            modes: [0; 4],
            uvs: [1.0, 1.0, 0.0, 0.0],
        };
        let mut mask_views = Vec::new();
        for key in &mask_keys {
            let mt = self.target(
                w,
                h,
                U::RENDER_ATTACHMENT | U::TEXTURE_BINDING,
                TARGET_FORMAT,
                "mask",
            );
            let view = mt.create_view(&Default::default());
            let groups: Vec<(MeshIndex, wgpu::BindGroup)> = key
                .iter()
                .filter_map(|&c| {
                    let t = tex_of(c)?;
                    let u = DrawUniform {
                        uvs: [t.uv_scale[0], t.uv_scale[1], 0.0, 0.0],
                        ..zero_uniform
                    };
                    Some((c, bind(&u, t, &self.dummy.view, &self.dummy.view)))
                })
                .collect();
            {
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("mask"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&self.pipelines[&(PipeKind::Mask, Cull::None)]);
                for (c, g) in &groups {
                    let Some((vb, ib, n)) = geo.get(c) else {
                        continue;
                    };
                    pass.set_bind_group(0, g, &[]);
                    pass.set_vertex_buffer(0, vb.slice(..));
                    pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..*n, 0, 0..1);
                }
            }
            mask_views.push(view);
        }

        let bg = opts.background.unwrap_or([0.0; 4]);
        let groups: Vec<wgpu::BindGroup> = items
            .iter()
            .map(|it| {
                let mask = it.mask.map_or(&self.dummy.view, |i| &mask_views[i]);
                let dst = if it.kind == PipeKind::Advanced {
                    &dst_view
                } else {
                    &self.dummy.view
                };
                bind(
                    &it.uniform,
                    tex_of(it.mesh).unwrap_or(&self.dummy),
                    mask,
                    dst,
                )
            })
            .collect();

        // Clear, then draw in passes: each advanced draw first copies the target.
        drop(enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &color_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: bg[0].into(),
                        g: bg[1].into(),
                        b: bg[2].into(),
                        a: bg[3].into(),
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &stencil_view,
                depth_ops: None,
                stencil_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0),
                    store: wgpu::StoreOp::Store,
                }),
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        }));
        let mut start = 0;
        let mut stencil_ref = 0u32;
        while start < items.len() {
            let advanced = items[start].kind == PipeKind::Advanced;
            let end = if advanced {
                enc.copy_texture_to_texture(
                    color.as_image_copy(),
                    dst_copy.as_image_copy(),
                    color.size(),
                );
                start + 1
            } else {
                (start..items.len())
                    .find(|&i| items[i].kind == PipeKind::Advanced)
                    .unwrap_or(items.len())
            };
            // 8-bit stencil: references cycle 1…255, clearing on wrap.
            let clear_stencil = advanced && stencil_ref >= 255;
            if clear_stencil {
                stencil_ref = 0;
            }
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("draw"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &stencil_view,
                    depth_ops: None,
                    stencil_ops: Some(wgpu::Operations {
                        load: if clear_stencil {
                            wgpu::LoadOp::Clear(0)
                        } else {
                            wgpu::LoadOp::Load
                        },
                        store: wgpu::StoreOp::Store,
                    }),
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            for i in start..end {
                let it = &items[i];
                let Some((vb, ib, n)) = geo.get(&it.mesh) else {
                    continue;
                };
                pass.set_pipeline(&self.pipelines[&(it.kind, it.cull)]);
                if it.kind == PipeKind::Advanced {
                    stencil_ref += 1;
                    pass.set_stencil_reference(stencil_ref);
                }
                pass.set_bind_group(0, &groups[i], &[]);
                pass.set_vertex_buffer(0, vb.slice(..));
                pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..*n, 0, 0..1);
            }
            drop(pass);
            start = end;
        }

        Ok(enc)
    }

    #[allow(clippy::too_many_arguments)]
    fn readback(
        &self,
        enc: &mut wgpu::CommandEncoder,
        color: &wgpu::Texture,
        w: u32,
        h: u32,
        ow: usize,
        oh: usize,
        ss: usize,
    ) -> Result<RgbaImage, RenderError> {
        // Rows padded to 256 bytes.
        let row = (w * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: u64::from(row) * u64::from(h),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        enc.copy_texture_to_buffer(
            color.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(h),
                },
            },
            color.size(),
        );
        let enc = std::mem::replace(enc, self.device.create_command_encoder(&Default::default()));
        self.queue.submit([enc.finish()]);
        let slice = readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(gpu_err)?;
        rx.recv().map_err(gpu_err)?.map_err(gpu_err)?;
        let data = slice.get_mapped_range().map_err(gpu_err)?;
        let mut px = Vec::with_capacity((w * h) as usize);
        for y in 0..h as usize {
            let r = &data[y * row as usize..][..(w * 4) as usize];
            px.extend(
                r.chunks_exact(4)
                    .map(|p| [p[0], p[1], p[2], p[3]].map(|c| f32::from(c) / 255.0)),
            );
        }
        drop(data);
        readback.unmap();
        Ok(finish_image(&px, w as usize, ow, oh, ss))
    }
}
