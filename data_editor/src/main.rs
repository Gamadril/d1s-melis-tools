mod edit;
mod resource;

use clap::Parser;
use data_renderer::layer::{paint_canvas, UiLayer};
use eframe::egui::{self, Vec2};
use edit::RESOURCE_SLOT_UNUSED;
use melis_data::{parse, UiObject};
use std::{fs, path::PathBuf};

#[derive(Parser, Debug)]
#[command(
    version,
    about = "Small IDE-like editor for Melis declarative UI .data files"
)]
struct Args {
    /// .data file to open on start
    input: Option<PathBuf>,
}

/// Indices from the scene root down to a UiObject. Used instead of holding a
/// reference, because every edit re-parses the byte buffer and rebuilds the
/// scene from scratch.
type ObjectPath = Vec<usize>;

fn resolve<'a>(objects: &'a [UiObject], path: &[usize]) -> Option<&'a UiObject> {
    let (first, rest) = path.split_first()?;
    let object = objects.get(*first)?;
    if rest.is_empty() {
        Some(object)
    } else {
        resolve(&object.children, rest)
    }
}

fn type_name(serialized_type: u32) -> &'static str {
    match serialized_type {
        0 => "Text",
        1 => "Button",
        2 => "Slider",
        3 => "Grid/List",
        4 => "Image",
        5 => "View/Layer",
        _ => "Unknown",
    }
}

/// Convert an ObjectPath whose bounds are parent-relative into absolute
/// scene coordinates.
fn absolute_bounds(objects: &[UiObject], path: &[usize]) -> Option<(u32, u32, u32, u32)> {
    let (first, rest) = path.split_first()?;
    let mut object = objects.get(*first)?;
    let mut bounds = object.bounds?;

    let mut x = bounds.x;
    let mut y = bounds.y;

    for index in rest {
        object = object.children.get(*index)?;
        bounds = object.bounds?;

        x = x.checked_add(bounds.x)?;
        y = y.checked_add(bounds.y)?;
    }

    Some((x, y, bounds.width, bounds.height))
}

/// An edit produced while drawing the (immutably borrowed) tree/inspector,
/// applied to the byte buffer afterwards to keep the borrow checker happy.
enum Edit {
    Bounds {
        offset: usize,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
    Text {
        block_offset: usize,
        text: String,
    },
    ResourceSlot {
        block_offset: usize,
        slot: usize,
        value: u32,
    },
}

struct EditorApp {
    path: Option<PathBuf>,
    bytes: Vec<u8>,
    layer: Option<UiLayer>,
    selected: Option<ObjectPath>,
    status: String,
    text_scratch: String,
    text_scratch_for: Option<ObjectPath>,
}

impl EditorApp {
    fn new(input: Option<PathBuf>, ctx: &egui::Context) -> Self {
        let mut app = Self {
            path: None,
            bytes: Vec::new(),
            layer: None,
            selected: None,
            status: "Open a .data file to start".to_string(),
            text_scratch: String::new(),
            text_scratch_for: None,
        };
        if let Some(path) = input {
            app.load(ctx, path);
        }
        app
    }

    fn load(&mut self, ctx: &egui::Context, path: PathBuf) {
        match fs::read(&path) {
            Ok(bytes) => {
                self.bytes = bytes;
                self.path = Some(path.clone());
                self.selected = None;
                self.status = format!("Loaded {}", path.display());
                self.refresh(ctx);
            }
            Err(err) => self.status = format!("Failed to read {}: {err}", path.display()),
        }
    }

    fn refresh(&mut self, ctx: &egui::Context) {
        match parse(&self.bytes) {
            Ok(file) => self.layer = Some(UiLayer::from_file(ctx, file, Default::default())),
            Err(err) => {
                self.layer = None;
                self.status = format!("Parse error: {err}");
            }
        }
    }

    fn save_to(&mut self, path: &PathBuf) {
        match fs::write(path, &self.bytes) {
            Ok(()) => {
                self.path = Some(path.clone());
                self.status = format!("Saved {}", path.display());
            }
            Err(err) => self.status = format!("Failed to save {}: {err}", path.display()),
        }
    }
}

impl eframe::App for EditorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Open…").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Melis data", &["data"])
                        .pick_file()
                    {
                        self.load(ui, path);
                    }
                }
                let has_file = !self.bytes.is_empty();
                if ui
                    .add_enabled(has_file, egui::Button::new("Save"))
                    .clicked()
                {
                    if let Some(path) = self.path.clone() {
                        self.save_to(&path);
                    }
                }
                if ui
                    .add_enabled(has_file, egui::Button::new("Save As…"))
                    .clicked()
                {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Melis data", &["data"])
                        .save_file()
                    {
                        self.save_to(&path);
                    }
                }
                ui.separator();
                if ui
                    .add_enabled(has_file, egui::Button::new("Add PNG/BMP resource…"))
                    .clicked()
                {
                    self.add_resource(ui);
                }
                ui.separator();
                ui.label(&self.status);
            });
        });

        egui::Panel::left("tree").default_size(260.0).show(ui, |ui| {
            ui.heading("Objects");
            egui::ScrollArea::vertical().show(ui, |ui| {
                if let Some(layer) = &self.layer {
                    let objects = &layer.scene.objects;
                    let mut new_selection = None;
                    for (index, object) in objects.iter().enumerate() {
                        draw_tree(ui, object, vec![index], 0, &self.selected, &mut new_selection);
                    }
                    if let Some(path) = new_selection {
                        self.selected = Some(path);
                    }
                } else {
                    ui.label("(nothing loaded)");
                }
            });
        });

        let mut edits: Vec<Edit> = Vec::new();
        let mut resource_count = 0usize;

        egui::Panel::right("inspector")
            .default_size(300.0)
            .show(ui, |ui| {
                ui.heading("Inspector");
                if let Some(layer) = &self.layer {
                    resource_count = layer.file.resources.len();
                    let selected = self
                        .selected
                        .as_ref()
                        .and_then(|path| resolve(&layer.scene.objects, path).map(|o| (path.clone(), o)));
                    if let Some((path, object)) = selected {
                        draw_inspector(
                            ui,
                            object,
                            &path,
                            &self.bytes,
                            resource_count,
                            &mut self.text_scratch,
                            &mut self.text_scratch_for,
                            &mut edits,
                        );
                    } else {
                        ui.label("Select an object in the tree.");
                    }
                    ui.separator();
                    ui.label(format!("Resources: {}", layer.file.resources.len()));
                    egui::ScrollArea::vertical()
                        .max_height(200.0)
                        .show(ui, |ui| {
                            for (index, res) in layer.file.resources.iter().enumerate() {
                                let dims = match (res.width, res.height) {
                                    (Some(w), Some(h)) => format!(" {w}x{h}"),
                                    _ => String::new(),
                                };
                                ui.label(format!("[{index}] {:?}{dims}", res.kind));
                            }
                        });
                } else {
                    ui.label("(nothing loaded)");
                }
            });

        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
            if let Some(layer) = &self.layer {
                let size = layer.screen_size();

                // paint_canvas renders the scene starting at the central
                // panel's current UI origin. Keep that origin so the
                // selected object's scene-space bounds can be mapped onto
                // the rendered canvas.
                let canvas_origin = ui.min_rect().min;

                paint_canvas(ui, layer, size);

                if let Some(path) = &self.selected {
                    if let Some((x, y, width, height)) =
                        absolute_bounds(&layer.scene.objects, path)
                    {
                        let selection_rect = egui::Rect::from_min_size(
                            canvas_origin + egui::vec2(x as f32, y as f32),
                            egui::vec2(width as f32, height as f32),
                        );

                        ui.painter().rect_stroke(
                            selection_rect.expand(1.0),
                            0.0,
                            egui::Stroke::new(
                                2.0,
                                egui::Color32::from_rgb(255, 190, 0),
                            ),
                            egui::StrokeKind::Outside,
                        );
                    }
                }
            } else {
                ui.centered_and_justified(|ui| ui.label("Open a .data file (Open… above)"));
            }
        });

        if !edits.is_empty() {
            for edit in edits {
                match edit {
                    Edit::Bounds {
                        offset,
                        x,
                        y,
                        width,
                        height,
                    } => {
                        self.bytes[offset..offset + 4].copy_from_slice(&x.to_le_bytes());
                        self.bytes[offset + 4..offset + 8].copy_from_slice(&y.to_le_bytes());
                        self.bytes[offset + 8..offset + 12].copy_from_slice(&width.to_le_bytes());
                        self.bytes[offset + 12..offset + 16]
                            .copy_from_slice(&height.to_le_bytes());
                    }
                    Edit::Text { block_offset, text } => {
                        // Reconstruct a lightweight stand-in object just to reuse
                        // edit::set_text's offset math (it only needs block_offset
                        // and serialized_type == 0).
                        let mut object = blank_object();
                        object.block_offset = block_offset;
                        edit::set_text(&mut self.bytes, &object, &text);
                    }
                    Edit::ResourceSlot {
                        block_offset,
                        slot,
                        value,
                    } => {
                        let mut object = blank_object();
                        object.block_offset = block_offset;
                        edit::set_resource_slot(&mut self.bytes, &object, slot, value);
                    }
                }
            }
            self.refresh(ui);
        }
    }
}

impl EditorApp {
    fn add_resource(&mut self, ctx: &egui::Context) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("image", &["png", "bmp"])
            .pick_file()
        else {
            return;
        };
        // Clone the parsed file out so we can mutate self.bytes right after
        // without holding a borrow of self.layer.
        let Some(file) = self.layer.as_ref().map(|layer| layer.file.clone()) else {
            self.status = "Open a .data file first".into();
            return;
        };
        match image::open(&path) {
            Ok(img) => {
                let rgba = img.to_rgba8();
                let (width, height) = (rgba.width(), rgba.height());
                match resource::insert_surface_resource(
                    &mut self.bytes,
                    &file,
                    width,
                    height,
                    rgba.as_raw(),
                ) {
                    Ok(index) => {
                        self.status = format!(
                            "Added resource #{index} ({width}x{height}) from {}. Set an object's resource slot to {index} to use it.",
                            path.display()
                        );
                        self.refresh(ctx);
                    }
                    Err(err) => self.status = format!("Could not add resource: {err}"),
                }
            }
            Err(err) => self.status = format!("Could not decode {}: {err}", path.display()),
        }
    }
}

/// A UiObject carrying only the fields edit::set_text/set_resource_slot
/// actually read (block_offset, serialized_type); the rest are throwaway
/// defaults so we don't need melis_data to expose a constructor.
fn blank_object() -> UiObject {
    UiObject {
        index: 0,
        serialized_type: 0,
        block_offset: 0,
        metadata: Vec::new(),
        bounds: None,
        elements: Vec::new(),
        name: None,
        text: None,
        text_color: None,
        text_align: None,
        string_id: None,
        resource_indices: Vec::new(),
        grid: None,
        children: Vec::new(),
        raw: Vec::new(),
    }
}

/// Flat, indented tree (click a row to select it). Kept deliberately simple
/// -- no expand/collapse or drag handling, just labels and clicks.
fn draw_tree(
    ui: &mut egui::Ui,
    object: &UiObject,
    path: ObjectPath,
    depth: usize,
    selected: &Option<ObjectPath>,
    new_selection: &mut Option<ObjectPath>,
) {
let label = format!(
    "{}#{} {}{}",
    "  ".repeat(depth),
    object.index,
    type_name(object.serialized_type),
    object
        .name
        .as_ref()
        .map(|n| format!(" {n}"))
        .unwrap_or_default()
);
    let is_selected = selected.as_ref() == Some(&path);
    if ui.selectable_label(is_selected, label).clicked() {
        *new_selection = Some(path.clone());
    }
    for (index, child) in object.children.iter().enumerate() {
        let mut child_path = path.clone();
        child_path.push(index);
        draw_tree(ui, child, child_path, depth + 1, selected, new_selection);
    }
}

fn draw_inspector(
    ui: &mut egui::Ui,
    object: &UiObject,
    path: &ObjectPath,
    bytes: &[u8],
    resource_count: usize,
    text_scratch: &mut String,
    text_scratch_for: &mut Option<ObjectPath>,
    edits: &mut Vec<Edit>,
) {
    ui.label(format!(
        "type: {} ({})",
        type_name(object.serialized_type),
        object.serialized_type
    ));
    if let Some(name) = &object.name {
        ui.label(format!("name: {name}"));
    }
    if let Some(id) = object.string_id {
        ui.label(format!("string_id: 0x{id:x}"));
    }

    if let Some(bounds) = object.bounds {
        ui.separator();
        ui.label("Bounds");
        let mut x = bounds.x;
        let mut y = bounds.y;
        let mut width = bounds.width;
        let mut height = bounds.height;
        let mut changed = false;
        egui::Grid::new(("bounds-grid", path)).num_columns(2).show(ui, |ui| {
            ui.label("x");
            changed |= ui.add(egui::DragValue::new(&mut x)).changed();
            ui.end_row();
            ui.label("y");
            changed |= ui.add(egui::DragValue::new(&mut y)).changed();
            ui.end_row();
            ui.label("width");
            changed |= ui.add(egui::DragValue::new(&mut width)).changed();
            ui.end_row();
            ui.label("height");
            changed |= ui.add(egui::DragValue::new(&mut height)).changed();
            ui.end_row();
        });
        if changed {
            edits.push(Edit::Bounds {
                offset: edit::rect_byte_offset(object),
                x,
                y,
                width,
                height,
            });
        }
    }

    if edit::has_editable_text(object) {
        ui.separator();
        ui.label(format!(
            "Text (max {} chars)",
            edit::text_capacity_chars()
        ));
        if text_scratch_for.as_ref() != Some(path) {
            *text_scratch_for = Some(path.clone());
            *text_scratch = object.text.clone().unwrap_or_default();
        }
        let response = ui.text_edit_singleline(text_scratch);
        if response.lost_focus() && response.ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            edits.push(Edit::Text {
                block_offset: object.block_offset,
                text: text_scratch.clone(),
            });
        }
        if ui.button("Apply text").clicked() {
            edits.push(Edit::Text {
                block_offset: object.block_offset,
                text: text_scratch.clone(),
            });
        }
    }

    let slot_count = edit::resource_slot_count(object);
    if slot_count > 0 {
        ui.separator();
        ui.label("Resource slots (index into the Resources list below; leave at -1/none to clear)");
        for slot in 0..slot_count {
            let current = edit::resource_slot_value(bytes, object, slot);
            let mut as_i64: i64 = if current == RESOURCE_SLOT_UNUSED {
                -1
            } else {
                current as i64
            };
            ui.horizontal(|ui| {
                ui.label(format!("slot {slot}"));
                let response = ui.add(
                    egui::DragValue::new(&mut as_i64)
                        .range(-1..=(resource_count.max(1) as i64 - 1)),
                );
                if response.changed() {
                    let value = if as_i64 < 0 {
                        RESOURCE_SLOT_UNUSED
                    } else {
                        as_i64 as u32
                    };
                    edits.push(Edit::ResourceSlot {
                        block_offset: object.block_offset,
                        slot,
                        value,
                    });
                }
            });
        }
    } else if object.serialized_type == 4 {
        ui.separator();
        ui.label("Type-4 resource bindings are nested/derived and not editable here yet.");
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Melis DATA editor")
            .with_inner_size(Vec2::new(1400.0, 700.0))
            .with_min_inner_size(Vec2::new(900.0, 500.0)),
        ..Default::default()
    };
    eframe::run_native(
        "Melis DATA editor",
        options,
        Box::new(move |cc| Ok(Box::new(EditorApp::new(args.input.clone(), &cc.egui_ctx)))),
    )?;
    Ok(())
}
