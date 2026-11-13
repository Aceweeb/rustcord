//! Sandboxed WebAssembly plugins (priority-roadmap item 8).
//!
//! The old `load_wasm_plugin` stub refused to load anything, on the grounds that
//! pretending to sandbox arbitrary code is worse than not running it (`D21`).
//! This module makes that promise real with `wasmi`, a pure-Rust interpreter:
//!
//! * **One folder per plugin.** `plugins/<name>/plugin.json` points at a `.wasm`
//!   file inside the same folder. The path is resolved and canonically checked,
//!   so a manifest cannot escape its directory.
//! * **No imports.** A module that asks the host for anything is rejected before
//!   instantiation. There is no filesystem, network, clock or clipboard host
//!   function to abuse, because there are no host functions at all.
//! * **Fuel, memory and output ceilings.** Every call runs in a fresh `Store`
//!   with a fuel budget (deterministic instruction metering), a linear-memory
//!   limit and a hard cap on input and output size. Out of fuel, a trap, a bad
//!   pointer or invalid UTF-8 all fail the call and leave the message untouched.
//! * **Declared permissions.** The only capability today is
//!   `transform_messages`, which lets a plugin rewrite message text *after*
//!   Discord has sent it. A plugin without it is refused, and nothing else in
//!   the app is reachable from the sandbox.
//!
//! The ABI is deliberately tiny. A module exports:
//!
//! ```text
//! memory                       linear memory
//! alloc(len: i32) -> i32       reserve `len` bytes, return the offset
//! run(ptr: i32, len: i32) -> i32   transform UTF-8 input, return output offset
//! result_len() -> i32          byte length of the output
//! ```
//!
//! Anything a guest does not understand it can simply not export; loading fails
//! closed with a message naming the missing export.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::{Component, Path, PathBuf};
use wasmi::{Config, Engine, Linker, Memory, Module, Store, StoreLimits, StoreLimitsBuilder};

use super::Plugin;
use crate::models::Message;

/// Refuse absurd module files before handing them to the compiler.
pub const MAX_MODULE_BYTES: u64 = 1024 * 1024;
/// Refuse absurd manifests; a real one is a few hundred bytes.
pub const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
/// Linear memory a plugin may allocate (16 MiB).
pub const MAX_MEMORY_BYTES: usize = 16 * 1024 * 1024;
/// Fuel per transform call. An empty `loop` burns this in well under a second.
pub const DEFAULT_FUEL: u64 = 5_000_000;
/// Upper bound a manifest may request, so a plugin cannot buy unlimited time.
pub const MAX_FUEL: u64 = 20_000_000;
/// Longest message body handed to a plugin.
pub const MAX_INPUT_BYTES: usize = 8 * 1024;
/// Longest replacement a plugin may return.
pub const MAX_OUTPUT_BYTES: usize = 8 * 1024;
/// Nesting limit for guest calls.
const MAX_RECURSION: usize = 64;

/// What a plugin is allowed to do. Unknown names fail deserialization, so a
/// manifest cannot smuggle in a permission the host does not implement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    /// Rewrite outgoing message text through `transform`.
    TransformMessages,
}

/// `plugin.json`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub name: String,
    #[serde(default)]
    pub version: String,
    /// File name of the module, relative to the plugin folder.
    pub module: String,
    #[serde(default)]
    pub permissions: Vec<Permission>,
    /// Instructions allowed per call; clamped to `MAX_FUEL`.
    #[serde(default)]
    pub fuel: Option<u64>,
}

impl Manifest {
    /// Validate everything that does not need the wasm file.
    pub fn validate(&self) -> Result<()> {
        let name = self.name.trim();
        if name.is_empty() || name.len() > 64 {
            bail!("plugin name must be 1-64 characters");
        }
        if !name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || " -_.".contains(character))
        {
            bail!("plugin name may only contain letters, digits, spaces, '.', '-' and '_'");
        }
        if !self.permissions.contains(&Permission::TransformMessages) {
            bail!(
                "plugin '{}' declares no usable permission (expected 'transform_messages')",
                name
            );
        }
        Ok(())
    }

    /// Fuel for one call, clamped so a manifest cannot exceed `MAX_FUEL`.
    pub fn fuel(&self) -> u64 {
        self.fuel.unwrap_or(DEFAULT_FUEL).clamp(1, MAX_FUEL)
    }
}

/// Host state for a single sandboxed call.
struct Sandbox {
    limits: StoreLimits,
}

/// A loaded, compiled plugin. Compilation happens once; every call gets a fresh
/// instance and store, so guest state never survives between messages.
pub struct WasmPlugin {
    manifest: Manifest,
    engine: Engine,
    module: Module,
    source: PathBuf,
}

impl WasmPlugin {
    /// Load `dir/plugin.json` and compile the module it names.
    pub fn load(dir: &Path) -> Result<Self> {
        let manifest_path = dir.join("plugin.json");
        ensure_regular_file(&manifest_path, MAX_MANIFEST_BYTES)?;
        let raw = std::fs::read_to_string(&manifest_path)
            .with_context(|| format!("reading {}", manifest_path.display()))?;
        let manifest: Manifest = serde_json::from_str(&raw)
            .with_context(|| format!("parsing {}", manifest_path.display()))?;
        manifest.validate()?;

        let module_path = resolve_module_path(dir, &manifest.module)?;
        ensure_regular_file(&module_path, MAX_MODULE_BYTES)?;
        let bytes = std::fs::read(&module_path)
            .with_context(|| format!("reading {}", module_path.display()))?;

        let plugin = Self::compile(manifest, &bytes, module_path)?;
        log::info!(
            "wasm plugin '{}' loaded from {} (fuel {}, max {} MiB memory)",
            plugin.name(),
            plugin.source.display(),
            plugin.manifest.fuel(),
            MAX_MEMORY_BYTES / (1024 * 1024)
        );
        Ok(plugin)
    }

    fn compile(manifest: Manifest, bytes: &[u8], source: PathBuf) -> Result<Self> {
        if bytes.len() as u64 > MAX_MODULE_BYTES {
            bail!(
                "module is {} bytes, above the {} byte limit",
                bytes.len(),
                MAX_MODULE_BYTES
            );
        }
        if bytes.len() < 8 || &bytes[..4] != b"\0asm" {
            bail!("module is not a WebAssembly binary (missing \\0asm magic)");
        }

        let engine = sandbox_engine();
        let module = Module::new(&engine, bytes).context("compiling wasm module")?;
        Self::from_module(manifest, engine, module, source)
    }

    fn from_module(
        manifest: Manifest,
        engine: Engine,
        module: Module,
        source: PathBuf,
    ) -> Result<Self> {
        // No host imports means no host surface: reject at load, not at call.
        if let Some(import) = module.imports().next() {
            bail!(
                "module imports '{}::{}'; wasm plugins may not import host functions",
                import.module(),
                import.name()
            );
        }
        for export in ["memory", "alloc", "run", "result_len"] {
            if !module.exports().any(|item| item.name() == export) {
                bail!("module must export '{export}'");
            }
        }

        Ok(Self {
            manifest,
            engine,
            module,
            source,
        })
    }

    /// Run the plugin over `input`. The caller keeps the original text on error.
    pub fn transform(&self, input: &str) -> Result<String> {
        if input.len() > MAX_INPUT_BYTES {
            bail!(
                "input is {} bytes, above the {} byte limit",
                input.len(),
                MAX_INPUT_BYTES
            );
        }

        let limits = StoreLimitsBuilder::new()
            .memory_size(MAX_MEMORY_BYTES)
            .build();
        let mut store = Store::new(&self.engine, Sandbox { limits });
        store.limiter(|sandbox| &mut sandbox.limits);
        store.set_fuel(self.manifest.fuel())?;

        let linker = Linker::<Sandbox>::new(&self.engine);
        // `_and_start` only runs a `start` function, and those are refused in
        // `compile` by `allow_start_fn(false)`, so this is a plain instantiate.
        let instance = linker
            .instantiate_and_start(&mut store, &self.module)
            .context("instantiating wasm module")?;
        let memory = instance
            .get_memory(&store, "memory")
            .context("module has no 'memory' export")?;
        let alloc = instance
            .get_typed_func::<i32, i32>(&store, "alloc")
            .context("module has no alloc(i32) -> i32 export")?;
        let run = instance
            .get_typed_func::<(i32, i32), i32>(&store, "run")
            .context("module has no run(i32, i32) -> i32 export")?;
        let result_len = instance
            .get_typed_func::<(), i32>(&store, "result_len")
            .context("module has no result_len() -> i32 export")?;

        let input_ptr = alloc
            .call(&mut store, input.len() as i32)
            .context("alloc failed")?;
        write_guest(&memory, &mut store, input_ptr, input.as_bytes())?;

        let output_ptr = run
            .call(&mut store, (input_ptr, input.len() as i32))
            .context("plugin trapped while transforming")?;
        let output_len = result_len
            .call(&mut store, ())
            .context("result_len failed")?;
        if output_len < 0 {
            bail!("plugin returned a negative length ({output_len})");
        }
        let output_len = output_len as usize;
        if output_len > MAX_OUTPUT_BYTES {
            bail!("plugin returned {output_len} bytes, above the {MAX_OUTPUT_BYTES} byte limit");
        }
        let bytes = read_guest(&memory, &store, output_ptr, output_len)?;
        String::from_utf8(bytes).context("plugin output is not valid UTF-8")
    }
}

impl std::fmt::Debug for WasmPlugin {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WasmPlugin")
            .field("name", &self.manifest.name)
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

impl Plugin for WasmPlugin {
    fn name(&self) -> &str {
        &self.manifest.name
    }

    fn on_message(&mut self, message: &mut Message) {
        // Text only: no content means nothing to rewrite.
        if message.content.is_empty() {
            return;
        }
        match self.transform(&message.content) {
            Ok(transformed) if !transformed.is_empty() && transformed != message.content => {
                message.content = transformed;
            }
            Ok(_) => {}
            Err(err) => {
                // Fail closed: the message is sent exactly as typed.
                log::debug!("wasm plugin '{}' skipped a transform: {err:#}", self.name());
            }
        }
    }
}

fn sandbox_engine() -> Engine {
    let mut config = Config::default();
    config.consume_fuel(true);
    // A `start` function would run before any limit is in place and before the
    // plugin is known to be sane, so modules that have one are refused.
    config.allow_start_fn(false);
    config.set_max_recursion_depth(MAX_RECURSION);
    Engine::new(&config)
}

fn ensure_regular_file(path: &Path, limit: u64) -> Result<()> {
    let metadata =
        std::fs::metadata(path).with_context(|| format!("reading {}", path.display()))?;
    if !metadata.is_file() {
        bail!("{} is not a regular file", path.display());
    }
    if metadata.len() > limit {
        bail!(
            "{} is {} bytes, above the {limit} byte limit",
            path.display(),
            metadata.len()
        );
    }
    Ok(())
}

/// Resolve `module` inside `dir`, refusing absolute paths, `..` and anything
/// that canonicalizes outside the plugin folder.
fn resolve_module_path(dir: &Path, module: &str) -> Result<PathBuf> {
    let candidate = Path::new(module);
    if candidate.is_absolute()
        || candidate.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        bail!("module path '{module}' must stay inside the plugin folder");
    }
    let base = dir
        .canonicalize()
        .with_context(|| format!("resolving {}", dir.display()))?;
    let resolved = base
        .join(candidate)
        .canonicalize()
        .with_context(|| format!("resolving module '{module}'"))?;
    if !resolved.starts_with(&base) {
        bail!("module path '{module}' escapes the plugin folder");
    }
    Ok(resolved)
}

fn write_guest(
    memory: &Memory,
    store: &mut Store<Sandbox>,
    offset: i32,
    bytes: &[u8],
) -> Result<()> {
    let offset = usize::try_from(offset).context("negative guest pointer")?;
    let end = offset
        .checked_add(bytes.len())
        .context("guest pointer overflow")?;
    let data = memory.data_mut(store);
    if end > data.len() {
        bail!("guest pointer {offset}..{end} is outside linear memory");
    }
    data[offset..end].copy_from_slice(bytes);
    Ok(())
}

fn read_guest(memory: &Memory, store: &Store<Sandbox>, offset: i32, len: usize) -> Result<Vec<u8>> {
    let offset = usize::try_from(offset).context("negative guest pointer")?;
    let end = offset.checked_add(len).context("guest pointer overflow")?;
    let data = memory.data(store);
    if end > data.len() {
        bail!("guest pointer {offset}..{end} is outside linear memory");
    }
    Ok(data[offset..end].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(permissions: &[Permission]) -> Manifest {
        Manifest {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            module: "plugin.wasm".to_string(),
            permissions: permissions.to_vec(),
            fuel: None,
        }
    }

    // `wasmi` only auto-detects WAT when it is handed text, so keep a small
    // test-only path that compiles the fixture directly.
    fn compile_wat(wat: &str) -> Result<WasmPlugin> {
        let engine = sandbox_engine();
        let module = Module::new(&engine, wat).context("compiling WAT fixture")?;
        WasmPlugin::from_module(
            manifest(&[Permission::TransformMessages]),
            engine,
            module,
            PathBuf::from("plugin.wat"),
        )
    }

    fn plugin(wat: &str) -> WasmPlugin {
        compile_wat(wat).expect("module compiles")
    }

    const TRANSFORM: &str = r#"
        (module
            (memory (export "memory") 1)
            (data (i32.const 128) "hello from wasm")
            (func (export "alloc") (param i32) (result i32) (i32.const 2048))
            (func (export "run") (param i32 i32) (result i32) (i32.const 128))
            (func (export "result_len") (result i32) (i32.const 15)))
    "#;

    #[test]
    fn a_manifest_must_declare_the_capability_it_uses() {
        assert!(manifest(&[Permission::TransformMessages])
            .validate()
            .is_ok());
        let err = manifest(&[]).validate().unwrap_err().to_string();
        assert!(err.contains("permission"), "{err}");
    }

    #[test]
    fn unknown_permissions_fail_to_parse() {
        let json = r#"{
            "name": "x",
            "module": "x.wasm",
            "permissions": ["read_everything"]
        }"#;
        assert!(serde_json::from_str::<Manifest>(json).is_err());
    }

    #[test]
    fn unknown_manifest_keys_fail_to_parse() {
        // A typo must not silently disable a security-relevant field.
        let json =
            r#"{"name":"x","module":"x.wasm","permissions":["transform_messages"],"network":true}"#;
        assert!(serde_json::from_str::<Manifest>(json).is_err());
    }

    #[test]
    fn fuel_is_clamped_to_the_host_ceiling() {
        let mut manifest = manifest(&[Permission::TransformMessages]);
        manifest.fuel = Some(u64::MAX);
        assert_eq!(manifest.fuel(), MAX_FUEL);
        manifest.fuel = Some(0);
        assert_eq!(manifest.fuel(), 1);
    }

    #[test]
    fn a_plugin_rewrites_the_message() {
        let mut plugin = plugin(TRANSFORM);
        assert_eq!(plugin.transform("original").unwrap(), "hello from wasm");

        let mut message = Message::new(
            crate::models::User {
                id: "1".to_string(),
                username: "alice".to_string(),
                avatar: None,
                status: crate::models::Presence::default(),
                ..Default::default()
            },
            "original",
        );
        plugin.on_message(&mut message);
        assert_eq!(message.content, "hello from wasm");
    }

    #[test]
    fn imports_are_refused_at_load_time() {
        let wat = r#"
            (module
                (import "host" "read_file" (func $read))
                (memory (export "memory") 1)
                (func (export "alloc") (param i32) (result i32) (i32.const 1024))
                (func (export "run") (param i32 i32) (result i32) (i32.const 0))
                (func (export "result_len") (result i32) (i32.const 0)))
        "#;
        let err = compile_wat(wat).unwrap_err().to_string();
        assert!(err.contains("imports"), "{err}");
    }

    #[test]
    fn start_functions_are_refused() {
        let wat = r#"
            (module
                (func $start)
                (start $start)
                (memory (export "memory") 1)
                (func (export "alloc") (param i32) (result i32) (i32.const 1024))
                (func (export "run") (param i32 i32) (result i32) (i32.const 0))
                (func (export "result_len") (result i32) (i32.const 0)))
        "#;
        assert!(compile_wat(wat).is_err());
    }

    #[test]
    fn missing_exports_are_refused() {
        let wat = r#"
            (module
                (memory (export "memory") 1)
                (func (export "alloc") (param i32) (result i32) (i32.const 1024)))
        "#;
        let err = compile_wat(wat).unwrap_err().to_string();
        assert!(err.contains("export"), "{err}");
    }

    #[test]
    fn non_wasm_bytes_are_refused() {
        let err = WasmPlugin::compile(
            manifest(&[Permission::TransformMessages]),
            b"#!/bin/sh\nrm -rf ~\n",
            PathBuf::from("p.sh"),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("\\0asm"), "{err}");
    }

    #[test]
    fn an_endless_loop_runs_out_of_fuel() {
        let wat = r#"
            (module
                (memory (export "memory") 1)
                (func (export "alloc") (param i32) (result i32) (i32.const 1024))
                (func (export "run") (param i32 i32) (result i32)
                    (loop $l (br $l))
                    (i32.const 0))
                (func (export "result_len") (result i32) (i32.const 0)))
        "#;
        let plugin = plugin(wat);
        let err = plugin.transform("hi").unwrap_err().to_string();
        assert!(
            err.to_lowercase().contains("fuel") || err.contains("trapped"),
            "{err}"
        );
    }

    #[test]
    fn a_trapping_plugin_leaves_the_message_untouched() {
        let wat = r#"
            (module
                (memory (export "memory") 1)
                (func (export "alloc") (param i32) (result i32) (i32.const 1024))
                (func (export "run") (param i32 i32) (result i32) unreachable)
                (func (export "result_len") (result i32) (i32.const 0)))
        "#;
        let mut plugin = plugin(wat);
        assert!(plugin.transform("hi").is_err());

        let mut message = Message::new(
            crate::models::User {
                id: "1".to_string(),
                username: "alice".to_string(),
                avatar: None,
                status: crate::models::Presence::default(),
                ..Default::default()
            },
            "original",
        );
        plugin.on_message(&mut message);
        assert_eq!(message.content, "original");
    }

    #[test]
    fn an_oversized_reply_is_refused() {
        let wat = r#"
            (module
                (memory (export "memory") 1)
                (func (export "alloc") (param i32) (result i32) (i32.const 1024))
                (func (export "run") (param i32 i32) (result i32) (i32.const 0))
                (func (export "result_len") (result i32) (i32.const 999999)))
        "#;
        let plugin = plugin(wat);
        assert!(plugin.transform("hi").is_err());
    }

    #[test]
    fn a_pointer_outside_memory_is_refused() {
        let wat = r#"
            (module
                (memory (export "memory") 1)
                (func (export "alloc") (param i32) (result i32) (i32.const 65530))
                (func (export "run") (param i32 i32) (result i32) (i32.const 0))
                (func (export "result_len") (result i32) (i32.const 0)))
        "#;
        let plugin = plugin(wat);
        // 65530 + 8 bytes of input runs past a one-page (64 KiB) memory.
        let err = plugin.transform("12345678").unwrap_err().to_string();
        assert!(err.contains("outside linear memory"), "{err}");
    }

    #[test]
    fn a_plugin_folder_loads_end_to_end() {
        let dir = std::env::temp_dir().join(format!(
            "rustcord-wasm-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("plugin.json"),
            r#"{
                "name": "Uppercase",
                "version": "1.0.0",
                "module": "plugin.wasm",
                "permissions": ["transform_messages"],
                "fuel": 100000
            }"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("plugin.wasm"),
            wat::parse_str(TRANSFORM).expect("fixture compiles"),
        )
        .unwrap();

        let plugin = WasmPlugin::load(&dir).expect("loads from disk");
        assert_eq!(plugin.name(), "Uppercase");
        assert_eq!(plugin.transform("original").unwrap(), "hello from wasm");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_manifest_cannot_escape_its_folder() {
        let dir = std::env::temp_dir();
        assert!(resolve_module_path(&dir, "../evil.wasm").is_err());
        assert!(resolve_module_path(&dir, "/etc/passwd").is_err());
        assert!(resolve_module_path(&dir, "nested/../../evil.wasm").is_err());
    }
}
