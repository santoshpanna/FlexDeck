//! The soundboard plugin: play clips to your headphones and, through a virtual
//! audio cable, to your microphone — with your real microphone mixed in.
//!
//! A native plugin, because a JavaScript runtime cannot choose which sound card
//! it plays to. It speaks the same protocol as every other plugin, through
//! `flexdeck-plugin-sdk`.

mod cpal_devices;
mod decode;
mod engine;
mod mix;
mod plugin;

fn main() {
    let null_audio = std::env::var_os("FLEXDECK_SOUNDBOARD_NULL_AUDIO").is_some();
    let engine = if null_audio {
        // For the host's integration test: everything but the sound cards.
        engine::Engine::new(|| Box::new(engine::NullDevices {
            outputs: vec!["Null speakers".into()],
            ..engine::NullDevices::default()
        }))
    } else {
        engine::Engine::new(|| Box::new(cpal_devices::CpalDevices::new()))
    };
    flexdeck_plugin_sdk::serve(plugin::Soundboard::new(engine));
}
