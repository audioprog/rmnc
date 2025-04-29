// Prevent console window in addition to Slint window in Windows release builds when, e.g., starting the app via file manager. Ignored on other platforms.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleRate, Stream};
use slint::SharedVector;
use num_traits::ToPrimitive;
use std::cell::RefCell;

mod render_plot;

use render_plot::render_plot;

// Importieren Sie die generierten Module
slint::include_modules!();

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ui = MainApp::new()?; // Verwenden Sie die Hauptkomponente MainApp
    let waveform_data = Arc::new(Mutex::new(SharedVector::default()));

    // Starten des Audio-Streams
    let waveform_data_clone = waveform_data.clone();
    let stream = start_audio_stream(waveform_data_clone).expect("Failed to start audio stream");

    // Timer für regelmäßiges Rendern
    let ui_weak = ui.as_weak();
    ui.global::<crate::WaveformImage>().on_render_plot(render_plot);
    ui.set_waveform_data(waveform_data.clone());
    thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(50));
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_dummy_property(ui.get_dummy_property() + 1);
            }
        }
    });

    ui.run()?;
    drop(stream);
    Ok(())
}

fn start_audio_stream(waveform_data: Arc<Mutex<SharedVector<f32>>>) -> Result<Stream, Box<dyn std::error::Error>> {
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
    // Statischer Buffer für überstehende Daten zwischen den Aufrufen
    thread_local! {
        static REMAINDER: RefCell<Vec<f32>> = RefCell::new(Vec::new());
    }

    // Konvertiere eingehende Daten in f32
    let mut samples: Vec<f32> = data.iter().filter_map(|&s| s.to_f32()).collect();

    // Füge evtl. übrig gebliebene Samples vom letzten Aufruf vorne an
    REMAINDER.with(|rem| {
        let mut rem = rem.borrow_mut();
        if !rem.is_empty() {
            let mut new_samples = Vec::with_capacity(rem.len() + samples.len());
            new_samples.extend_from_slice(&rem);
            new_samples.extend_from_slice(&samples);
            samples.clear();
            samples.extend(new_samples);
            rem.clear();
        }
    });

    // Verarbeite nur vollständige Chunks
    let chunk_size = 2048;
    let full_chunks = samples.len() / chunk_size;
    for chunk in samples.chunks(chunk_size).take(full_chunks) {
        let left_channel = chunk.iter().step_by(2); // Linker Kanal
        let right_channel = chunk.iter().skip(1).step_by(2); // Rechter Kanal

        let min_left = left_channel.clone().fold(f32::INFINITY, |a, &b| f32::min(a, b));
        let max_left = left_channel.clone().fold(f32::NEG_INFINITY, |a, &b| f32::max(a, b));

        let min_right = right_channel.clone().fold(f32::INFINITY, |a, &b| f32::min(a, b));
        let max_right = right_channel.clone().fold(f32::NEG_INFINITY, |a, &b| f32::max(a, b));

        // Berechne die größte Abweichung von 0 für den linken Kanal
        let max_deviation_left = if min_left.abs() > max_left.abs() { min_left.abs() } else { max_left.abs() };
        min_max_data.push(-max_deviation_left); // Linker Kanal (nach oben)
        // Berechne die größte Abweichung von 0 für den rechten Kanal
        let max_deviation_right = if min_right.abs() > max_right.abs() { min_right.abs() } else { max_right.abs() };
        min_max_data.push(max_deviation_right); // Linker Kanal (nach oben)
    }

    // Überstehende Samples für den nächsten Aufruf zwischenspeichern
    let remainder = samples.len() % chunk_size;
    if remainder > 0 {
        REMAINDER.with(|rem| {
            rem.borrow_mut().extend_from_slice(&samples[samples.len() - remainder..]);
        });
    }

    // Aktualisiere die SharedVector-Daten
    let mut waveform = waveform_data.lock().unwrap();
    for value in min_max_data {
        waveform.push(value);
    }

    // Begrenze die Länge des Verlaufs (z. B. 1000 Punkte)
    if waveform.len() > 2000 {
        let excess = waveform.len() - 2000;
        let new_waveform: SharedVector<f32> = waveform[excess..].into(); // Kopiere nur die letzten 1000 Elemente
        *waveform = new_waveform; // Ersetze den alten Vektor
    }
}

fn err_fn(err: cpal::StreamError) {
    eprintln!("Stream error: {}", err);
}
