#[cfg(not(target_family = "wasm"))]
use crate::utils::BufferDimensions;
use std::fmt::Debug;

pub trait RenderTargetFrame: Debug {
    fn view(&self) -> &wgpu::TextureView;
}

pub trait RenderTarget: Debug + 'static {
    type Frame: RenderTargetFrame;

    fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32);

    fn format(&self) -> wgpu::TextureFormat;

    fn width(&self) -> u32;

    fn height(&self) -> u32;

    fn get_next_texture(&mut self) -> Result<Self::Frame, wgpu::SurfaceError>;

    fn submit<I: IntoIterator<Item = wgpu::CommandBuffer>>(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        command_buffers: I,
        frame: Self::Frame,
    );
}

#[derive(Debug)]
pub struct SwapChainTarget {
    window_surface: wgpu::Surface,
    surface_config: wgpu::SurfaceConfiguration,
}

#[derive(Debug)]
pub struct SwapChainTargetFrame {
    texture: wgpu::SurfaceTexture,
    view: wgpu::TextureView,
}

impl RenderTargetFrame for SwapChainTargetFrame {
    fn view(&self) -> &wgpu::TextureView {
        &self.view
    }
}

impl SwapChainTarget {
    pub fn new(
        surface: wgpu::Surface,
        format: wgpu::TextureFormat,
        size: (u32, u32),
        device: &wgpu::Device,
    ) -> Self {
        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.0,
            height: size.1,
            present_mode: wgpu::PresentMode::Fifo,
        };
        surface.configure(device, &surface_config);
        Self {
            surface_config,
            window_surface: surface,
        }
    }
}

impl RenderTarget for SwapChainTarget {
    type Frame = SwapChainTargetFrame;

    fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        self.surface_config.width = width;
        self.surface_config.height = height;
        self.window_surface.configure(device, &self.surface_config);
    }

    fn format(&self) -> wgpu::TextureFormat {
        self.surface_config.format
    }

    fn width(&self) -> u32 {
        self.surface_config.width
    }

    fn height(&self) -> u32 {
        self.surface_config.height
    }

    fn get_next_texture(&mut self) -> Result<Self::Frame, wgpu::SurfaceError> {
        let texture = self.window_surface.get_current_texture()?;
        let view = texture.texture.create_view(&Default::default());
        Ok(SwapChainTargetFrame { texture, view })
    }

    fn submit<I: IntoIterator<Item = wgpu::CommandBuffer>>(
        &self,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        command_buffers: I,
        frame: Self::Frame,
    ) {
        queue.submit(command_buffers);
        frame.texture.present();
    }
}

#[cfg(not(target_family = "wasm"))]
#[derive(Debug)]
pub struct TextureTarget {
    size: wgpu::Extent3d,
    texture: wgpu::Texture,
    format: wgpu::TextureFormat,
    buffer: wgpu::Buffer,
    buffer_dimensions: BufferDimensions,
}

#[cfg(not(target_family = "wasm"))]
#[derive(Debug)]
pub struct TextureTargetFrame(wgpu::TextureView);

#[cfg(not(target_family = "wasm"))]
impl RenderTargetFrame for TextureTargetFrame {
    fn view(&self) -> &wgpu::TextureView {
        &self.0
    }
}

#[cfg(not(target_family = "wasm"))]
impl TextureTarget {
    pub fn new(device: &wgpu::Device, size: (u32, u32)) -> Self {
        let buffer_dimensions = BufferDimensions::new(size.0 as usize, size.1 as usize);
        let size = wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        };
        let texture_label = create_debug_label!("Render target texture");
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: texture_label.as_deref(),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        });
        let buffer_label = create_debug_label!("Render target buffer");
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: buffer_label.as_deref(),
            size: (buffer_dimensions.padded_bytes_per_row.get() as u64
                * buffer_dimensions.height as u64),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        Self {
            size,
            texture,
            format,
            buffer,
            buffer_dimensions,
        }
    }

    pub fn capture(&self, device: &wgpu::Device) -> Option<image::RgbaImage> {
        let (sender, receiver) = std::sync::mpsc::channel();
        let buffer_slice = self.buffer.slice(..);
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            sender.send(result).unwrap();
        });
        device.poll(wgpu::Maintain::Wait);
        let result = receiver.recv().unwrap();
        match result {
            Ok(()) => {
                let map = buffer_slice.get_mapped_range();
                let mut buffer = Vec::with_capacity(
                    self.buffer_dimensions.height * self.buffer_dimensions.unpadded_bytes_per_row,
                );

                for chunk in map.chunks(self.buffer_dimensions.padded_bytes_per_row.get() as usize)
                {
                    buffer
                        .extend_from_slice(&chunk[..self.buffer_dimensions.unpadded_bytes_per_row]);
                }

                let image = image::RgbaImage::from_raw(self.size.width, self.size.height, buffer);
                drop(map);
                self.buffer.unmap();
                image
            }
            Err(e) => {
                log::error!("Unknown error reading capture buffer: {:?}", e);
                None
            }
        }
    }
}

#[cfg(not(target_family = "wasm"))]
impl RenderTarget for TextureTarget {
    type Frame = TextureTargetFrame;

    fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        self.size.width = width;
        self.size.height = height;

        let label = create_debug_label!("Render target texture");
        self.texture = device.create_texture(&wgpu::TextureDescriptor {
            label: label.as_deref(),
            size: self.size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        });

        let buffer_label = create_debug_label!("Render target buffer");
        self.buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: buffer_label.as_deref(),
            size: width as u64 * height as u64 * 4,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
    }

    fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    fn width(&self) -> u32 {
        self.size.width
    }

    fn height(&self) -> u32 {
        self.size.height
    }

    fn get_next_texture(&mut self) -> Result<Self::Frame, wgpu::SurfaceError> {
        Ok(TextureTargetFrame(
            self.texture.create_view(&Default::default()),
        ))
    }

    fn submit<I: IntoIterator<Item = wgpu::CommandBuffer>>(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        command_buffers: I,
        _frame: Self::Frame,
    ) {
        let label = create_debug_label!("Render target transfer encoder");
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: label.as_deref(),
        });
        encoder.copy_texture_to_buffer(
            wgpu::ImageCopyTexture {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyBuffer {
                buffer: &self.buffer,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(self.buffer_dimensions.padded_bytes_per_row),
                    rows_per_image: None,
                },
            },
            self.size,
        );
        queue.submit(command_buffers.into_iter().chain(Some(encoder.finish())));
    }
}
