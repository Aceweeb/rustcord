//! Load a plugin folder and run one message through it, printing the result.
//!
//! This is the runtime path the app uses — `WasmPlugin::load` plus the `Plugin`
//! hook — without needing a Discord session:
//!
//! ```text
//! cargo run --example check_plugin -- ~/.config/rustcord/plugins/uppercase
//! ```

use rustcord::models::{Message, Presence, User};
use rustcord::plugins::Plugin;

fn main() -> anyhow::Result<()> {
    let dir = std::env::args().nth(1).unwrap_or_else(|| {
        let config = rustcord::settings::Settings::config_dir();
        config.join("plugins/uppercase").display().to_string()
    });

    let mut plugin = rustcord::plugins::wasm::WasmPlugin::load(std::path::Path::new(&dir))?;
    let mut message = Message::new(
        User {
            id: "0".to_string(),
            username: "example".to_string(),
            avatar: None,
            status: Presence::default(),
            ..User::default()
        },
        "Hello from Rustcord — this is a plugin test.",
    );

    let before = message.content.clone();
    plugin.on_message(&mut message);
    println!("plugin:  {}", plugin.name());
    println!("before:  {before}");
    println!("after:   {}", message.content);
    if message.content == before {
        anyhow::bail!("plugin did not change the message");
    }
    Ok(())
}
