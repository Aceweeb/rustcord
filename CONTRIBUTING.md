# Contributing to Rustcord

Rustcord is a scaffold. Contributions that make the architecture clearer and
the build reproducible are welcome. Please keep the following in mind.

## Ground rules

- No browser engine dependencies (Electron, WebView2, CEF, Chromium).
- No user-token authentication clients. Use bot tokens or approved OAuth2.
- Plugins must not execute untrusted code outside a sandbox.
- Keep the release profile size-conscious.
- Add comments for non-obvious state transitions, especially around plugin
  hooks and theme parsing.

## Plugin contributions

1. Create `src/plugins/my_plugin.rs`.
2. Implement the `Plugin` trait from `src/plugins/mod.rs`.
3. Register the plugin in `PluginManager::new` and call it from the relevant
   manager hook (`on_message`, `on_settings`, etc.).
4. Add a short note to `README.md` if the plugin has user-visible behavior.
5. If the plugin needs network access, put the client in `src/api/` and keep
   credentials out of source control.

The WASM loader in `src/plugins/mod.rs` is intentionally unimplemented. A real
implementation should include:

- a `plugin.json` manifest with name, version, entrypoint, and permissions;
- a permission list (`read_messages`, `modify_embed`, `theme_injection`, ...);
- validation before instantiation;
- a sandboxed interpreter (`wasmi` or `wasmtime`);
- resource limits (fuel/epochs, memory caps);
- a clear error path that disables the plugin instead of crashing the UI.

## Code style

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo check
```

Keep module boundaries honest: UI code should not know how Discord/Spotify
payloads are decoded, and API code should not know about `egui`.

## Pull requests

- Describe the user-visible change and any new dependencies.
- Include the Arch build command you used.
- Note whether the change affects the size-optimized release profile.
- Do not commit `settings.json` with real tokens, `target/`, or theme files
  containing personal data.
