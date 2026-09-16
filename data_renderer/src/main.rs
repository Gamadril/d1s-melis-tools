mod layer;
mod surface;

use clap::Parser;
use eframe::egui::{self, Vec2};
use layer::{paint_canvas, UiLayer};
use melis_data::{load_language_dir, parse, utf16_text, DataFile, ResourceKind, UiObject};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Parser, Debug)]
#[command(
    version,
    about = "Render and inspect a Melis declarative UI .data file"
)]
struct Args {
    /// Input .data file
    input: PathBuf,
    /// Language column from apps/Language (default: en / 英文)
    #[arg(long, default_value = "en")]
    lang: String,
}

struct RendererApp {
    layer: UiLayer,
}

impl RendererApp {
    fn screen_size(&self) -> Vec2 {
        self.layer.screen_size()
    }
}

impl eframe::App for RendererApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                paint_canvas(ui, &self.layer, self.screen_size());
            });
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let bytes = fs::read(&args.input)?;
    let file = parse(&bytes)?;
    print!("{}", describe(&file, &args.input.display().to_string()));
    let translations = load_translations(&args.input, &args.lang);
    let title = format!("Melis DATA renderer — {}", args.input.display());
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(&title)
            .with_inner_size([1024.0, 600.0])
            .with_min_inner_size([1024.0, 600.0])
            .with_max_inner_size([1024.0, 600.0])
            .with_resizable(false),
        ..Default::default()
    };
    eframe::run_native(
        "Melis DATA renderer",
        options,
        Box::new(move |cc| {
            Ok(Box::new(RendererApp {
                layer: UiLayer::from_file(&cc.egui_ctx, file, translations),
            }))
        }),
    )?;
    Ok(())
}

fn load_translations(input: &Path, lang: &str) -> HashMap<String, String> {
    input
        .parent()
        .and_then(|data_dir| data_dir.parent())
        .map(|apps| apps.join("Language"))
        .filter(|path| path.is_dir())
        .map(|dir| load_language_dir(dir, lang))
        .unwrap_or_default()
}

fn describe(file: &DataFile, path: &str) -> String {
    use std::fmt::Write as _;
    let mut output = String::new();
    let header = &file.header;
    writeln!(output, "file: {path}").unwrap();
    writeln!(output, "size: {} bytes", file.bytes.len()).unwrap();
    writeln!(output, "magic: {}", utf16_text(&header.magic)).unwrap();
    writeln!(output, "version: {}", utf16_text(&header.version)).unwrap();
    if let Some((width, height)) = file.section.screen_size() {
        writeln!(output, "screen: {width}x{height}").unwrap();
    }
    let scene = file.scene();
    writeln!(output, "objects: {}", scene.objects.len()).unwrap();
    for object in &scene.objects {
        write_object(&mut output, object, 0);
    }
    writeln!(output, "resources: {}", file.resources.len()).unwrap();
    for (index, resource) in file.resources.iter().enumerate() {
        let extra = match (
            resource.width,
            resource.height,
            resource.format,
            resource.decode_method,
        ) {
            (Some(width), Some(height), Some(format), Some(method)) => {
                format!(" {width}x{height} format={format} {method:?}")
            }
            _ if resource.kind == ResourceKind::Utf16 => {
                format!(" {:?}", utf16_text(&resource.bytes))
            }
            _ => String::new(),
        };
        writeln!(
            output,
            "  [{index}] {}: {} bytes{extra}",
            resource_name(resource.kind),
            resource.bytes.len(),
        )
        .unwrap();
    }
    output
}

fn write_object(output: &mut String, object: &UiObject, depth: usize) {
    use std::fmt::Write as _;
    let pad = "  ".repeat(depth + 1);
    write!(
        output,
        "{pad}type={} offset=0x{:x}",
        object.serialized_type, object.block_offset
    )
    .unwrap();
    if let Some(name) = &object.name {
        write!(output, " name={name}").unwrap();
    }
    if let Some(bounds) = object.bounds {
        write!(
            output,
            " bounds={{{},{},{},{}}}",
            bounds.x, bounds.y, bounds.width, bounds.height
        )
        .unwrap();
    }
    if let Some(text) = &object.text {
        write!(output, " text={text:?}").unwrap();
    }
    if let Some(align) = object.text_align {
        write!(output, " align={align:?}").unwrap();
    }
    if let Some(id) = object.string_id {
        write!(output, " string_id=0x{id:x}").unwrap();
    }
    if !object.resource_indices.is_empty() {
        write!(output, " resources={:?}", object.resource_indices).unwrap();
    }
    if let Some(grid) = object.grid {
        write!(
            output,
            " grid={}x{} cell={}x{}",
            grid.columns, grid.rows, grid.cell_width, grid.cell_height
        )
        .unwrap();
    }
    if !object.children.is_empty() {
        write!(output, " children={}", object.children.len()).unwrap();
    }
    writeln!(output).unwrap();
    for child in &object.children {
        write_object(output, child, depth + 1);
    }
}

fn resource_name(kind: ResourceKind) -> &'static str {
    match kind {
        ResourceKind::Png => "PNG",
        ResourceKind::Bmp => "BMP",
        ResourceKind::Utf16 => "UTF-16",
        ResourceKind::Surface => "surface",
    }
}
