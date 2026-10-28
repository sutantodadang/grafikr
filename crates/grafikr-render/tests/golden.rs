//! Headless renderer golden-image tests.
//!
//! Set `GRAFIKR_UPDATE_GOLDEN=1` to regenerate the reference PNGs. Environments
//! without a GPU adapter skip the tests instead of failing.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use grafikr_core::{Color, DisplayList, ImageData, Rect};
use grafikr_render::Renderer;
use wgpu::util::DeviceExt;

#[derive(Clone, Copy)]
struct Scene {
    name: &'static str,
    light: bool,
    scale: f32,
    /// Physical pixels, because the golden is captured from the swapchain.
    width: u32,
    height: u32,
}

const SCENES: [Scene; 2] = [
    Scene {
        name: "basic",
        light: false,
        scale: 1.0,
        width: 240,
        height: 160,
    },
    Scene {
        name: "scale15",
        light: true,
        scale: 1.5,
        width: 360,
        height: 240,
    },
];

fn render_scene(scene: Scene) -> Option<Vec<u8>> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let request = |fallback| {
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: None,
            force_fallback_adapter: fallback,
            ..Default::default()
        }))
    };
    let adapter = request(false).or_else(|_| request(true)).ok()?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("grafikr golden"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        ..Default::default()
    }))
    .ok()?;

    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("golden target"),
        size: wgpu::Extent3d {
            width: scene.width,
            height: scene.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    let (clear, card, border) = if scene.light {
        (
            Color::hex(0xf4f5f7),
            Color::hex(0xffffff),
            Color::hex(0x6b7280),
        )
    } else {
        (
            Color::hex(0x121417),
            Color::hex(0x1d2025),
            Color::hex(0x9aa0a6),
        )
    };

    let mut list = DisplayList::new();
    list.push_shadow(
        Rect::new(18.0, 18.0, 204.0, 124.0),
        14.0,
        20.0,
        Color::BLACK.with_alpha(150),
        None,
    );
    list.push_bordered_rect(
        Rect::new(20.0, 20.0, 200.0, 120.0),
        card,
        12.0,
        Some((1.0, border)),
        None,
    );
    list.push_rect(
        Rect::new(40.0, 40.0, 70.0, 40.0),
        Color::hex(0x4c8dff),
        8.0,
        None,
    );
    list.push_rect(
        Rect::new(120.0, 40.0, 70.0, 40.0),
        Color::hex(0xe5484d),
        20.0,
        None,
    );
    let image = ImageData::new_rgba(
        2,
        2,
        vec![
            255, 255, 255, 255, 76, 141, 255, 255, 76, 141, 255, 255, 255, 255, 255, 255,
        ],
    );
    list.push_image(Arc::clone(&image), Rect::new(40.0, 100.0, 32.0, 24.0), None);
    list.push_image(image, Rect::new(88.0, 100.0, 24.0, 24.0), None);

    let mut renderer = Renderer::new(&device, format);
    renderer.render(
        &device,
        &queue,
        &view,
        (scene.width, scene.height),
        scene.scale,
        clear,
        &list,
    );

    let padded = (scene.width * 4).div_ceil(256) * 256;
    let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("golden readback"),
        contents: &vec![0; (padded * scene.height) as usize],
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(scene.height),
            },
        },
        wgpu::Extent3d {
            width: scene.width,
            height: scene.height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));

    let (sender, receiver) = std::sync::mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("poll");
    receiver.recv().expect("map callback").expect("map");

    let mapped = buffer.slice(..).get_mapped_range().expect("mapped range");
    let mut pixels = Vec::with_capacity((scene.width * scene.height * 4) as usize);
    for row in mapped.chunks(padded as usize) {
        pixels.extend_from_slice(&row[..(scene.width * 4) as usize]);
    }
    Some(pixels)
}

#[test]
fn golden_scenes() {
    if std::env::var("GRAFIKR_SKIP_GOLDEN").as_deref() == Ok("1") {
        return;
    }
    for scene in SCENES {
        let Some(pixels) = render_scene(scene) else {
            eprintln!("golden: no GPU adapter, skipping {}", scene.name);
            return;
        };
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden")
            .join(format!("{}.png", scene.name));

        if std::env::var("GRAFIKR_UPDATE_GOLDEN").is_ok() {
            write_png(&path, scene.width, scene.height, &pixels);
            continue;
        }

        let Some(reference) = read_png(&path) else {
            panic!("missing golden image at {}", path.display());
        };
        assert_eq!(reference.len(), pixels.len(), "golden size changed");
        let mut differing = 0usize;
        let mut max_delta = 0u8;
        for (expected, actual) in reference.chunks_exact(4).zip(pixels.chunks_exact(4)) {
            let delta = expected
                .iter()
                .zip(actual)
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap_or(0);
            max_delta = max_delta.max(delta);
            if delta > 8 {
                differing += 1;
            }
        }
        let total = (scene.width * scene.height) as usize;
        assert!(
            differing * 200 <= total,
            "{}: {differing}/{total} pixels differ, max channel delta {max_delta}",
            scene.name
        );
    }
}

fn write_png(path: &Path, width: u32, height: u32, pixels: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("golden dir");
    }
    let file = std::fs::File::create(path).expect("create golden");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("png header");
    writer.write_image_data(pixels).expect("png data");
}

fn read_png(path: &Path) -> Option<Vec<u8>> {
    let file = std::fs::File::open(path).ok()?;
    let decoder = png::Decoder::new(std::io::BufReader::new(file));
    let mut reader = decoder.read_info().ok()?;
    let mut bytes = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut bytes).ok()?;
    bytes.truncate(info.buffer_size());
    Some(bytes)
}
