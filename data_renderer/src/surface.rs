use melis_data::{decode::decode_resource, structs::EmbeddedResource, surface_bytes_per_pixel};

pub struct DecodedSurface {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

pub fn decode_surface(resource: &EmbeddedResource) -> Option<DecodedSurface> {
    let decoded = decode_resource(resource).ok()?;
    let width = resource.width? as usize;
    let height = resource.height? as usize;
    let format = resource.format?;
    let bpp = surface_bytes_per_pixel(format)?;
    let stride = (width * bpp + 3) & !3;
    if decoded.len() < stride.saturating_mul(height) {
        return None;
    }
    let mut rgba = vec![0; width * height * 4];
    for y in 0..height {
        for x in 0..width {
            let source = y * stride + x * bpp;
            let target = (y * width + x) * 4;
            if source + bpp > decoded.len() {
                return None;
            }
            match bpp {
                2 => {
                    let pixel = u16::from_le_bytes(decoded[source..source + 2].try_into().ok()?);
                    expand_rgb565(pixel, &mut rgba[target..target + 4]);
                    rgba[target + 3] = 255;
                }
                3 => {
                    // Format 3: [RGB565 le][a5]. a=0 skip; a=0x1f copy (0→0x0841); else blend.
                    let mut pixel =
                        u16::from_le_bytes(decoded[source..source + 2].try_into().ok()?);
                    let alpha5 = decoded[source + 2];
                    if alpha5 == 0 {
                        continue;
                    }
                    if alpha5 == 0x1f && pixel == 0 {
                        pixel = 0x0841;
                    }
                    expand_rgb565(pixel, &mut rgba[target..target + 4]);
                    rgba[target + 3] = if alpha5 >= 0x1f {
                        255
                    } else {
                        ((u32::from(alpha5) * 255) / 31) as u8
                    };
                }
                _ => {
                    let pixel = &decoded[source..source + 4];
                    rgba[target] = pixel[2];
                    rgba[target + 1] = pixel[1];
                    rgba[target + 2] = pixel[0];
                    rgba[target + 3] = pixel[3];
                }
            }
        }
    }
    Some(DecodedSurface {
        width,
        height,
        rgba,
    })
}

fn expand_rgb565(pixel: u16, rgba: &mut [u8]) {
    rgba[0] = ((u32::from(pixel >> 11) * 255) / 31) as u8;
    rgba[1] = ((u32::from((pixel >> 5) & 0x3f) * 255) / 63) as u8;
    rgba[2] = ((u32::from(pixel & 0x1f) * 255) / 31) as u8;
}
