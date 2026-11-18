
use std::io::Cursor;
use std::sync::OnceLock;
use std::sync::mpsc::{Sender, channel};
use std::time::Duration;

use rodio::buffer::SamplesBuffer;
use rodio::Source;

const EXPORT_SOUND: &[u8] = include_bytes!("../assets/export.mp3");

const PRE_ROLL: Duration = Duration::from_millis(150);

static PLAYER: OnceLock<Sender<()>> = OnceLock::new();

pub fn start() {
    PLAYER.get_or_init(|| {
        let (requests, played) = channel();
        std::thread::spawn(move || {
            let Ok(sink) = rodio::DeviceSinkBuilder::open_default_sink() else {
                return;
            };
            let Some(sound) = decode_export_sound() else {
                return;
            };
            for () in played {
                sink.mixer().add(sound.clone().delay(PRE_ROLL));
            }
        });
        requests
    });
}

pub fn play_export() {
    if let Some(requests) = PLAYER.get() {
        let _ = requests.send(());
    }
}

fn decode_export_sound() -> Option<SamplesBuffer> {
    let decoder = rodio::Decoder::try_from(Cursor::new(EXPORT_SOUND)).ok()?;
    let (channels, sample_rate) = (decoder.channels(), decoder.sample_rate());
    Some(SamplesBuffer::new(channels, sample_rate, decoder.collect::<Vec<_>>()))
}
