//! Compile a WAT plugin into the `.wasm` file the loader expects.
//!
//! `wat` is a dev-dependency, so this ships with the repository rather than the
//! binary. Run it from the repo root:
//!
//! ```text
//! cargo run --example build_plugin -- examples/wasm-uppercase/plugin.wat \
//!     examples/wasm-uppercase/plugin.wasm
//! ```

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let input = args
        .next()
        .unwrap_or_else(|| "examples/wasm-uppercase/plugin.wat".to_string());
    let output = args
        .next()
        .unwrap_or_else(|| "examples/wasm-uppercase/plugin.wasm".to_string());

    let bytes =
        wat::parse_file(&input).map_err(|err| anyhow::anyhow!("compiling {input}: {err}"))?;
    std::fs::write(&output, &bytes)?;
    println!("wrote {output} ({} bytes)", bytes.len());
    Ok(())
}
