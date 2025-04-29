use slint::{SharedPixelBuffer, Image};
use image::{ImageBuffer, Rgba};

pub fn render_plot(waveform_data: &[f32], width: i32, height: i32) -> Image {
    // Erstelle ein leeres Bild
    let mut img = ImageBuffer::from_pixel(width as u32, height as u32, Rgba([0, 0, 0, 255]));

    let center_y = height as f32 / 2.0;
    let max_amplitude = waveform_data.iter().cloned().fold(0.0_f32, |a, b| a.max(b.abs()));
    let scale = if max_amplitude > 0.0 { 1.0 / max_amplitude } else { 1.0 };

    for (i, chunk) in waveform_data.chunks(2).enumerate() {
        if i >= width as usize {
            break;
        }

        let x = i as u32;
        let y_min = center_y + chunk[0] * center_y * scale;
        let y_max = center_y + chunk[1] * center_y * scale;

        if y_min >= 0.0 && y_min < height as f32 {
            img.put_pixel(x, y_min as u32, Rgba([0, 255, 0, 255])); // Grün
        }
        if y_max >= 0.0 && y_max < height as f32 {
            img.put_pixel(x, y_max as u32, Rgba([255, 0, 0, 255])); // Rot
        }
    }

    let buffer = SharedPixelBuffer::clone_from_slice(&img.into_raw(), width as u32, height as u32);
    Image::from_rgba8_premultiplied(buffer)
}