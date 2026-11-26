//! Compile WGSL shaders to SPIR-V at build time using `naga`. No system
//! toolchain (no shaderc, no Vulkan SDK) is required.
//!
//! The crate only really *uses* the SPIR-V when the `vulkano-renderer`
//! feature is on, but compiling unconditionally keeps the build script
//! cheap and avoids feature-conditional build logic.

use std::path::{Path, PathBuf};

fn main() {
    let shader_dir = Path::new("src/backend/shaders");
    println!("cargo:rerun-if-changed={}", shader_dir.display());

    let out_dir = std::env::var_os("OUT_DIR").expect("OUT_DIR not set");
    let out_dir = PathBuf::from(out_dir);

    let shaders = [("ui.wgsl", "ui.spv"), ("blur.wgsl", "blur.spv")];
    for (src_name, out_name) in shaders {
        let src_path = shader_dir.join(src_name);
        println!("cargo:rerun-if-changed={}", src_path.display());
        let source = std::fs::read_to_string(&src_path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", src_path.display()));
        let spv = compile_wgsl(&source, &src_path);
        let out_path = out_dir.join(out_name);
        // SPIR-V is naturally a sequence of u32 words; write as little-endian bytes.
        let mut bytes = Vec::with_capacity(spv.len() * 4);
        for w in spv {
            bytes.extend_from_slice(&w.to_le_bytes());
        }
        std::fs::write(&out_path, &bytes)
            .unwrap_or_else(|e| panic!("failed to write {}: {e}", out_path.display()));
    }
}

fn compile_wgsl(source: &str, path: &Path) -> Vec<u32> {
    let module = naga::front::wgsl::parse_str(source).unwrap_or_else(|e| {
        panic!(
            "WGSL parse failed for {}: {}",
            path.display(),
            e.emit_to_string(source)
        )
    });

    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::PUSH_CONSTANT,
    )
    .validate(&module)
    .unwrap_or_else(|e| panic!("WGSL validation failed for {}: {e:?}", path.display()));

    // ROOT CAUSE OF THE Y-FLIP BUG: naga's `Options::default()` enables
    // `WriterFlags::ADJUST_COORDINATE_SPACE`, which negates `clip_pos.y`
    // in every vertex output. The flag exists to bridge WGSL targeting
    // backends with a different clip-space Y convention (e.g. OpenGL's
    // Y-up), but Vulkan's clip space is Y-down — same as WGSL — so the
    // adjustment is wrong here and implicitly V-flips every rendered
    // primitive. Take the default flag set and strip just that flag.
    let default_flags = naga::back::spv::Options::default().flags;
    let flags = default_flags - naga::back::spv::WriterFlags::ADJUST_COORDINATE_SPACE;
    let options = naga::back::spv::Options {
        lang_version: (1, 3),
        flags,
        ..Default::default()
    };

    naga::back::spv::write_vec(&module, &info, &options, None)
        .unwrap_or_else(|e| panic!("SPIR-V emit failed for {}: {e:?}", path.display()))
}
