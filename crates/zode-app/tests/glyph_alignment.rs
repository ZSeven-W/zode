use jian_widgets::{Color, Rect, TextBox, VerticalAlign};
use zode_app::render::{FramePainter, NativeBackend, RasterSurface};

const SNAPSHOT_REGULAR: &[u8] = include_bytes!("fonts/NotoSansSC-Regular.subset.ttf");
const SNAPSHOT_SEMIBOLD: &[u8] = include_bytes!("fonts/NotoSansSC-SemiBold.subset.ttf");
const SNAPSHOT_FAMILY: &str = "Zode Snapshot Sans SC";

#[derive(Debug, Clone, Copy)]
struct ControlSample {
    surface: &'static str,
    text: &'static str,
    font_size: f32,
    font_weight: u16,
    control_height: f32,
}

const CONTROL_SAMPLES: [ControlSample; 5] = [
    ControlSample {
        surface: "sidebar",
        text: "新建任务",
        font_size: 13.0,
        font_weight: 400,
        control_height: 32.0,
    },
    ControlSample {
        surface: "header",
        text: "新任务",
        font_size: 13.0,
        font_weight: 600,
        control_height: 46.0,
    },
    ControlSample {
        surface: "settings",
        text: "主机连接",
        font_size: 13.0,
        font_weight: 500,
        control_height: 52.0,
    },
    ControlSample {
        surface: "composer",
        text: "工作区写入",
        font_size: 11.0,
        font_weight: 400,
        control_height: 28.0,
    },
    ControlSample {
        surface: "environment",
        text: "查看变更",
        font_size: 13.0,
        font_weight: 600,
        control_height: 34.0,
    },
];

#[derive(Debug, Clone, Copy)]
struct PixelBounds {
    top: u32,
    bottom: u32,
}

impl PixelBounds {
    fn center_y(self) -> f32 {
        (self.top + self.bottom) as f32 / 2.0
    }
}

#[test]
fn centered_controls_use_visible_glyph_bounds_at_supported_dpi_scales() {
    jian_skia::register_bundled_fonts(vec![SNAPSHOT_REGULAR.to_vec(), SNAPSHOT_SEMIBOLD.to_vec()]);

    for scale in [1.0, 1.25, 2.0] {
        let mut backend = NativeBackend::new(scale);
        for sample in CONTROL_SAMPLES {
            let target = Rect::xywh(20.0, 20.0, 180.0, sample.control_height);
            let bounds = render_centered_control_text(&mut backend, scale, target, sample);
            let target_center = (target.origin.y + target.size.y / 2.0) * scale;
            let error = (bounds.center_y() - target_center).abs();
            assert!(
                error <= 1.5,
                "{} scale={scale}: visible glyph center {} differs from target center {target_center} by {error}px; bounds={bounds:?}",
                sample.surface,
                bounds.center_y(),
            );
        }
    }
}

fn render_centered_control_text(
    backend: &mut NativeBackend,
    scale: f32,
    target: Rect,
    sample: ControlSample,
) -> PixelBounds {
    let logical_width = 220.0_f32;
    let logical_height = target.origin.y + target.size.y + 20.0;
    let physical_width = (logical_width * scale).round() as u32;
    let physical_height = (logical_height * scale).round() as u32;
    let mut surface = RasterSurface::new(physical_width, physical_height).unwrap();
    let canvas = surface.canvas();
    canvas.clear(skia_safe::Color::WHITE);
    canvas.scale((scale, scale));

    {
        let mut painter = FramePainter::new(backend, canvas);
        TextBox::new(sample.text)
            .with_font_family(SNAPSHOT_FAMILY)
            .with_font_size(sample.font_size)
            .with_font_weight(sample.font_weight)
            .with_color(Color::BLACK)
            .with_vertical_align(VerticalAlign::Center)
            .paint(&mut painter, target);
    }

    let mut rgba = vec![0_u8; physical_width as usize * physical_height as usize * 4];
    assert!(surface.read_rgba8(&mut rgba));
    visible_ink_bounds(&rgba, physical_width, physical_height)
}

fn visible_ink_bounds(rgba: &[u8], width: u32, height: u32) -> PixelBounds {
    let mut top = height;
    let mut bottom = 0;
    let mut found = false;
    for y in 0..height {
        for x in 0..width {
            let offset = ((y * width + x) * 4) as usize;
            let pixel = &rgba[offset..offset + 4];
            if pixel[0] < 224 || pixel[1] < 224 || pixel[2] < 224 {
                top = top.min(y);
                bottom = bottom.max(y);
                found = true;
            }
        }
    }
    assert!(found, "text render did not produce visible ink");
    PixelBounds { top, bottom }
}
