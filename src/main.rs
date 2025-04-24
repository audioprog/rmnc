// Prevent console window in addition to Slint window in Windows release builds when, e.g., starting the app via file manager. Ignored on other platforms.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::error::Error;
use std::sync::{Arc, Mutex};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleRate, Stream};
use slint::{SharedVector, SharedPixelBuffer};
use num_traits::ToPrimitive;

slint::include_modules!();

fn main() -> Result<(), Box<dyn Error>> {
    let ui = AppWindow::new()?;
    let waveform_data = Arc::new(Mutex::new(SharedVector::default()));

    // Starten des Audio-Streams
    let waveform_data_clone = waveform_data.clone();
    // Der Stream muss bis zum Programmende erhalten bleiben, daher außerhalb des Threads speichern
    let stream = start_audio_stream(waveform_data_clone).expect("Failed to start audio stream");
    // stream wird im Scope gehalten, damit es nicht gedroppt wird

    // Verbinden Sie die render_plot-Callback-Funktion
    let waveform_data_clone = waveform_data.clone();
    ui.on_render_plot(move |_width, _height, _pitch, _yaw, _amplitude| {
        let data = waveform_data_clone.lock().unwrap();
        render_plot(&data, _width, _height)
    });

    ui.run()?;
    drop(stream); // Stream wird hier gedroppt, wenn das UI geschlossen wird
    Ok(())
}

fn start_audio_stream(waveform_data: Arc<Mutex<SharedVector<f32>>>) -> Result<(Stream), Box<dyn Error>> {
    let host = cpal::default_host();
    let device = host.default_input_device().expect("No input device available");
    println!("Using input device: {}", device.name()?);

    let config = device.default_input_config().expect("Error retrieving default configuration");
    println!("StreamConfig: {:?}", config);
    let sample_format = config.sample_format();
    println!("Sample format: {:?}", sample_format);

    let supported_config = cpal::StreamConfig {
        channels: config.channels(),
        sample_rate: SampleRate(48000),
        buffer_size: match config.buffer_size() {
            cpal::SupportedBufferSize::Range { min, max } => {
                println!("Buffer Size Range: min = {}, max = {}", min, max);
                let size = (*max).min(1024 * 4 * 1024);
                println!("Buffer Size: {}", size);
                cpal::BufferSize::Fixed(size)
            }
            cpal::SupportedBufferSize::Unknown => {
                println!("Buffer Size: Unknown");
                cpal::BufferSize::Default
            }
        },
    };

    let stream = match sample_format {
        cpal::SampleFormat::I16 => {
            println!("Using I16 sample format");
            device.build_input_stream(
                &supported_config,
                move |data: &[i16], _| process_audio(data, &waveform_data),
                err_fn,
                None,
            )?
        }
        cpal::SampleFormat::F32 => {
            println!("Using F32 sample format");
            device.build_input_stream(
                &supported_config,
                move |data: &[f32], _| process_audio(data, &waveform_data),
                err_fn,
                None,
            )?
        }
        _ => return Err("Unsupported sample format".into()),
    };

    stream.play()?;
    println!("Audio stream started and playing.");
    Ok(stream)
}

fn process_audio<T: cpal::Sample + ToPrimitive>(data: &[T], waveform_data: &Arc<Mutex<SharedVector<f32>>>) {
    let mut min_max_data = vec![];

    // Gruppiere alle 128 Samples und berechne Min/Max
    for chunk in data.chunks(128) {
        let min = chunk.iter().filter_map(|&s| s.to_f32()).fold(f32::INFINITY, f32::min);
        let max = chunk.iter().filter_map(|&s| s.to_f32()).fold(f32::NEG_INFINITY, f32::max);
        min_max_data.push(min);
        min_max_data.push(max);
    }

    // Aktualisiere die SharedVector-Daten
    let mut waveform = waveform_data.lock().unwrap();
    for value in min_max_data {
        waveform.push(value);
    }

    // Begrenze die Länge des Verlaufs (z. B. 1000 Punkte)
    if waveform.len() > 1000 {
        let excess = waveform.len() - 1000;
        let new_waveform: SharedVector<f32> = waveform[excess..].into(); // Kopiere nur die letzten 1000 Elemente
        *waveform = new_waveform; // Ersetze den alten Vektor
    }
}

fn render_plot(waveform_data: &[f32], width: i32, height: i32) -> slint::Image {
    use image::{ImageBuffer, Rgba};

    // Erstelle ein leeres Bild
    let mut img = ImageBuffer::from_pixel(width.try_into().unwrap(), height.try_into().unwrap(), Rgba([0, 0, 0, 255]));

    // Zeichne die Wellenform
    let center_y = height as f32 / 2.0;
    let step = width as usize / (waveform_data.len() / 2).max(1);

    for (i, chunk) in waveform_data.chunks(2).enumerate() {
        if i >= width as usize {
            break;
        }

        let x = i as u32;
        let y_min = center_y - chunk[0] * center_y;
        let y_max = center_y - chunk[1] * center_y;

        if y_min >= 0.0 && y_min < height as f32 {
            img.put_pixel(x, y_min as u32, Rgba([0, 255, 0, 255])); // Grün für Min
        }
        if y_max >= 0.0 && y_max < height as f32 {
            img.put_pixel(x, y_max as u32, Rgba([255, 0, 0, 255])); // Rot für Max
        }
    }

    // Konvertiere das Bild in ein SharedPixelBuffer
    let buffer = SharedPixelBuffer::clone_from_slice(&img.into_raw(), height.try_into().unwrap(), width.try_into().unwrap());
    slint::Image::from_rgba8_premultiplied(buffer)
}

fn err_fn(err: cpal::StreamError) {
    eprintln!("Stream error: {}", err);
}
