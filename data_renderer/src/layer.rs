use crate::surface::decode_surface;
use eframe::egui::{self, Color32, TextureHandle, TextureOptions, Vec2};
use melis_data::structs::{DataFile, ResourceKind, TextAlign, UiObject, UiRect, UiScene};
use std::collections::HashMap;

pub struct ResourceTex {
    pub index: usize,
    pub handle: TextureHandle,
}

pub struct UiLayer {
    pub file: DataFile,
    pub scene: UiScene,
    pub resource_textures: Vec<ResourceTex>,
    pub translations: HashMap<String, String>,
}

impl UiLayer {
    pub fn from_file(
        ctx: &egui::Context,
        file: DataFile,
        translations: HashMap<String, String>,
    ) -> Self {
        let resource_textures: Vec<ResourceTex> = file
            .resources
            .iter()
            .enumerate()
            .filter_map(|(index, resource)| {
                let color_image = match resource.kind {
                    ResourceKind::Png | ResourceKind::Bmp => {
                        let image = image::load_from_memory(&resource.bytes).ok()?.to_rgba8();
                        let size = [image.width() as usize, image.height() as usize];
                        egui::ColorImage::from_rgba_unmultiplied(size, &image.into_raw())
                    }
                    ResourceKind::Utf16 => return None,
                    ResourceKind::Surface => {
                        let surface = decode_surface(resource)?;
                        egui::ColorImage::from_rgba_unmultiplied(
                            [surface.width, surface.height],
                            &surface.rgba,
                        )
                    }
                };
                let handle = ctx.load_texture(
                    format!("resource-{index}"),
                    color_image,
                    TextureOptions::NEAREST,
                );
                Some(ResourceTex { index, handle })
            })
            .collect();

        Self {
            scene: file.scene(),
            file,
            resource_textures,
            translations,
        }
    }

    pub fn screen_size(&self) -> Vec2 {
        self.file
            .section
            .screen_size()
            .map(|(width, height)| Vec2::new(width as f32, height as f32))
            .unwrap_or(Vec2::new(1024.0, 600.0))
    }

    pub fn draw_objects(
        &self,
        painter: &egui::Painter,
        canvas: egui::Rect,
        logical_size: Vec2,
        scale: f32,
    ) {
        for object in &self.scene.objects {
            self.draw_object(painter, canvas, logical_size, object, scale, (0, 0));
        }
    }

    fn draw_object(
        &self,
        painter: &egui::Painter,
        canvas: egui::Rect,
        logical_size: Vec2,
        object: &UiObject,
        scale: f32,
        parent_origin: (u32, u32),
    ) {
        let Some(bounds) = object.bounds else {
            for child in &object.children {
                self.draw_object(painter, canvas, logical_size, child, scale, parent_origin);
            }
            return;
        };
        let absolute = UiRect {
            x: parent_origin.0.saturating_add(bounds.x),
            y: parent_origin.1.saturating_add(bounds.y),
            ..bounds
        };
        let object_rect = scene_rect(canvas, logical_size, absolute);
        // Type 2: every listed surface (viewer, no slider/clock compositor).
        // Type 1/4/5: idle first.
        let resource_indices = if object.serialized_type == 2 {
            object.resource_indices.as_slice()
        } else {
            object.resource_indices.get(..1).unwrap_or(&[])
        };
        for resource_index in resource_indices {
            let Some(texture) = self
                .resource_textures
                .iter()
                .find(|tex| tex.index == *resource_index as usize)
            else {
                continue;
            };
            let draw_rect = self.texture_rect(canvas, logical_size, absolute, texture.index);
            painter.image(
                texture.handle.id(),
                draw_rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }

        if let Some(text) = self.resolve_text(object) {
            let color = object
                .text_color
                .map(|(r, g, b)| Color32::from_rgb(r, g, b))
                .unwrap_or(Color32::WHITE);
            if object.serialized_type == 0 {
                let font_size = object
                    .bounds
                    .map(|bounds| {
                        let ratio = match object.text_align {
                            Some(TextAlign::Left) => 0.40,
                            Some(TextAlign::Right) => 0.55,
                            _ if bounds.height > 40 => 0.36,
                            _ => 0.62,
                        };
                        (bounds.height as f32 * ratio * scale).clamp(12.0, 22.0)
                    })
                    .unwrap_or((16.0 * scale).max(12.0));
                let (pos, anchor) = match object.text_align {
                    Some(TextAlign::Left) => (
                        egui::pos2(
                            object_rect.left() + 4.0 * scale.max(0.1),
                            object_rect.center().y,
                        ),
                        egui::Align2::LEFT_CENTER,
                    ),
                    Some(TextAlign::Right) => (
                        egui::pos2(
                            object_rect.right() - 4.0 * scale.max(0.1),
                            object_rect.center().y,
                        ),
                        egui::Align2::RIGHT_CENTER,
                    ),
                    _ => (object_rect.center(), egui::Align2::CENTER_CENTER),
                };
                painter.text(
                    pos,
                    anchor,
                    text,
                    egui::FontId::proportional(font_size),
                    color,
                );
            } else {
                let is_top_widget = absolute.y < 350;
                let (font_size, y_offset) = if is_top_widget {
                    ((20.0 * scale).max(14.0), 14.0 * scale)
                } else {
                    ((18.5 * scale).max(13.0), 2.0 * scale)
                };
                painter.text(
                    egui::pos2(
                        object_rect.center().x,
                        object_rect.max.y - y_offset.max(1.0),
                    ),
                    egui::Align2::CENTER_BOTTOM,
                    text,
                    egui::FontId::proportional(font_size),
                    color,
                );
            }
        }
        if object.serialized_type == 3 {
            if let (Some(grid), Some(template)) = (object.grid.as_ref(), object.children.first()) {
                let template_origin = template.bounds.unwrap_or(UiRect {
                    x: 0,
                    y: 0,
                    width: grid.cell_width,
                    height: grid.cell_height,
                });
                for row in 0..grid.rows {
                    for col in 0..grid.columns {
                        self.draw_object(
                            painter,
                            canvas,
                            logical_size,
                            template,
                            scale,
                            (
                                absolute
                                    .x
                                    .saturating_add(col.saturating_mul(grid.cell_width))
                                    .saturating_sub(template_origin.x),
                                absolute
                                    .y
                                    .saturating_add(row.saturating_mul(grid.cell_height))
                                    .saturating_sub(template_origin.y),
                            ),
                        );
                    }
                }
                return;
            }
        }
        for child in &object.children {
            self.draw_object(
                painter,
                canvas,
                logical_size,
                child,
                scale,
                (absolute.x, absolute.y),
            );
        }
    }

    fn texture_rect(
        &self,
        canvas: egui::Rect,
        logical_size: Vec2,
        widget: UiRect,
        resource_index: usize,
    ) -> egui::Rect {
        // 1:1 copy of src.w×src.h at widget origin (widget is hit/clip, not dest size).
        // BtBook panel is 531x432 in a 535x432 view; stretching desyncs baked chrome.
        let (width, height) = self
            .file
            .resources
            .get(resource_index)
            .and_then(|resource| Some((resource.width?, resource.height?)))
            .unwrap_or((widget.width, widget.height));
        scene_rect(
            canvas,
            logical_size,
            UiRect {
                x: widget.x,
                y: widget.y,
                width: width.min(widget.width),
                height: height.min(widget.height),
            },
        )
    }

    fn resolve_text(&self, object: &UiObject) -> Option<String> {
        if object.serialized_type == 1
            || object.serialized_type == 2
            || object.string_id == Some(0x2a1)
        {
            return None;
        }
        if let Some(ref text) = object.text {
            if let Some(translated) = self.lookup_translation(text) {
                return Some(translated);
            }
            if !text.is_empty() {
                return Some(text.clone());
            }
        }
        if let Some(ref key) = object.name {
            if let Some(translated) = self.lookup_translation(key) {
                return Some(translated);
            }
        }
        if let Some(id) = object.string_id {
            let direct_key = match id {
                0x207 => Some("手机互联"),
                0x27 => Some("蓝牙音乐"),
                0xa07 => Some("收音机"),
                0x14 => Some("蓝牙"),
                0x202 => Some("应用"),
                0x1d => Some("USB"),
                0x2a0 => Some("设置"),
                _ => None,
            };
            if let Some(key) = direct_key {
                if let Some(translated) = self.lookup_translation(key) {
                    return Some(translated);
                }
            }
            let fallback_name = match id {
                0x207 => Some("PhoneLink"),
                0x27 => Some("BT Music"),
                0xa07 => Some("FM Transmit"),
                0x14 => Some("Bluetooth"),
                0x202 => Some("Apps"),
                0x1d => Some("USB"),
                0x2a0 => Some("Setup"),
                _ => None,
            };
            if let Some(name) = fallback_name {
                return Some(name.to_string());
            }
        }
        None
    }

    fn lookup_translation(&self, key: &str) -> Option<String> {
        let key = key.trim().trim_matches('"').trim();
        if key.is_empty() {
            return None;
        }
        self.translations
            .get(key)
            .map(|value| value.trim().trim_matches('"').trim())
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    }
}

pub fn paint_canvas(ui: &mut egui::Ui, layer: &UiLayer, logical_size: Vec2) {
    let available = ui.available_size();
    let scale = (available.x / logical_size.x)
        .min(available.y / logical_size.y)
        .min(1.0);
    let canvas_size = logical_size * scale.max(0.1);
    let (response, painter) = ui.allocate_painter(canvas_size, egui::Sense::hover());
    painter.rect_filled(response.rect, 0.0, Color32::from_rgb(8, 10, 14));
    layer.draw_objects(&painter, response.rect, logical_size, scale.max(0.1));
}

fn scene_rect(canvas: egui::Rect, logical_size: Vec2, bounds: UiRect) -> egui::Rect {
    let scale_x = canvas.width() / logical_size.x;
    let scale_y = canvas.height() / logical_size.y;
    egui::Rect::from_min_size(
        canvas.left_top() + Vec2::new(bounds.x as f32 * scale_x, bounds.y as f32 * scale_y),
        Vec2::new(
            bounds.width as f32 * scale_x,
            bounds.height as f32 * scale_y,
        ),
    )
}
