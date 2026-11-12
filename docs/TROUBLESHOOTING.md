# Troubleshooting

Symptoms first, then the fix. Commands assume the repository is checked out at
`/mnt/storage/Default Project/rustcord`; adjust the path if you moved it.

## The app does not start / no window appears

1. Confirm you are on a graphical session:

   ```bash
   echo "WAYLAND_DISPLAY=$WAYLAND_DISPLAY DISPLAY=$DISPLAY"
   ```

   Both empty means the binary has no display to draw on. Run it from a
   terminal inside your desktop session, not over a bare SSH connection.

2. Check the log. Rustcord logs to stderr and to an in-memory ring (visible in
   **Settings → Diagnostics**):

   ```bash
   ./target/release/rustcord 2>&1 | head -30
   ```

3. On NVIDIA + Wayland, try the X11 backend:

   ```bash
   WAYLAND_DISPLAY= ./target/release/rustcord
   ```

## "Could not save token" / keyring errors

Rustcord stores the bot token in the OS Secret Service keyring and never in
`settings.json`.

- The session must have a Secret Service provider running (`gnome-keyring`,
  KWallet with the Secret Service bridge, or `keepassxc` with its Secret Service
  integration).
- Check the bus exists: `echo $DBUS_SESSION_BUS_ADDRESS`.
- Without a keyring the token stays in memory for the current session only, and
  **Settings → Diagnostics** reports `WARN  Secret Service`.

## Gateway stays at "Reconnecting"

- A bot token is required. A user token is rejected by design and is not
  supported.
- Verify the token independently:

  ```bash
  curl -s -H "Authorization: Bot $TOKEN" https://discord.com/api/v10/users/@me
  ```

- `401` means the token is wrong or was regenerated; paste it again in
  **Settings → Privacy → Discord connection**.
- `429` means rate limited; wait and reconnect.

## Theme file is ignored

- Only CSS custom properties are read. Selectors, `@import`, `url()` and
  arbitrary CSS are intentionally dropped.
- The properties must be spelled exactly, e.g. `--background-primary`,
  `--brand-experiment`, `--text-normal`.
- Check **Settings → Themes**: the resolved path is shown, along with
  "file not found" when the path does not exist.
- Reload without restarting: press the **Reload theme file** button in
  **Settings → Themes**, or use the `refresh_view` shortcut (default `F5`).

## Build fails with "No space left on device"

The project lives on `/mnt/storage` but Cargo's registry is in `$HOME`.

```bash
du -sh ~/.cargo ~/.rustup
export CARGO_HOME=/mnt/storage/cargo    # persist this in your shell profile
cargo build --release
```

Also remove stale build output: `cargo clean` (deletes `target/`, several GB).

## `cargo build` fails on a missing system library

```bash
sudo pacman -S --needed base-devel pkgconf \
  wayland wayland-protocols libxkbcommon \
  libx11 libxcursor libxi libxrandr libxinerama \
  mesa vulkan-icd-loader dbus gnome-keyring
```

## Memory is higher than the README number

Measure it instead of guessing:

```bash
scripts/measure-memory.sh 30
```

Notes:
- `VmHWM` is the kernel high-water mark and is the honest "peak" figure.
- The number covers this process only; there is no WebView or renderer child.
- Window size, UI scale and font atlas size all affect RSS. Emoji glyphs in the
  sidebar pull in a larger font atlas than plain text.
- A steady RSS with a flat `VmHWM` over minutes means no leak.

## Settings window will not open

`Ctrl+,` toggles it. If the shortcut was rebound, check
**Settings → Shortcuts** — the stored chord is shown per action and can be
reset. `Esc` cancels a pending rebind.

## Reset everything

```bash
rm -rf ~/.config/rustcord ~/.local/share/rustcord
```

This removes settings, diagnostics exports and the theme path reference. The
bot token lives in the keyring: clear it inside the app with **Clear token**.
