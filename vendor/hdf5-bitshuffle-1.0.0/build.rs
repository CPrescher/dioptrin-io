use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

static CMAKE_SEPARATOR: &[u8] = b"\n";
static CMAKE_FILE: &str = "cmake_cryiorust_hdf5";
static BITSHUFFLE_PATCH: &str = "0001-bshuf_h5_filter-guard-comp_lvl-read.patch";

fn main() {
    println!("cargo:rerun-if-changed=ext/bitshuffle");
    println!("cargo:rerun-if-changed=patches");
    println!("cargo:rerun-if-changed=dioptrin_unshuffle.c");
    let hdf5_include = PathBuf::from(
        env::var("DEP_HDF5_INCLUDE").expect("hdf5-sys did not export its include directory"),
    );
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let ext = manifest.join("ext").join("bitshuffle");
    let lz4_include = ext.join("lz4");
    let src_include = ext.join("src");
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let h5filter = patch_h5filter(&ext, &manifest, &out_dir);
    let core = patch_core(&ext, &manifest, &out_dir);
    write_cmake_file(&[&hdf5_include, &lz4_include, &src_include]);
    compile(
        &ext,
        &h5filter,
        &core,
        &hdf5_include,
        &lz4_include,
        &src_include,
    );
}

// Upstream 0.5.2 reads cd_values[5] without checking cd_nelmts
// (kiyo-masui/bitshuffle#180); patch a copy in OUT_DIR and compile that.
fn patch_h5filter(ext: &Path, manifest: &Path, out_dir: &Path) -> PathBuf {
    let patch = manifest.join("patches").join(BITSHUFFLE_PATCH);
    let dir = out_dir.join("src");
    fs::create_dir_all(&dir).unwrap();
    let dest = dir.join("bshuf_h5filter.c");
    fs::copy(ext.join("src").join("bshuf_h5filter.c"), &dest).unwrap();
    apply_patch(out_dir, &patch, &dest);
    dest
}

// Backport the upstream NEON movemask optimization and its MSVC guard.
// Originals stay byte-for-byte intact; apply pinned patches only in OUT_DIR.
fn patch_core(ext: &Path, manifest: &Path, out_dir: &Path) -> PathBuf {
    let dest = out_dir.join("src").join("bitshuffle_core.c");
    fs::copy(ext.join("src").join("bitshuffle_core.c"), &dest).unwrap();
    for name in [
        "0002-upstream-neon-movemask.patch",
        "0003-upstream-msvc-alias.patch",
    ] {
        let patch = manifest.join("patches").join(name);
        println!("cargo:rerun-if-changed={}", patch.display());
        apply_patch(out_dir, &patch, &dest);
    }
    dest
}

// OUT_DIR may be inside the consumer's Git worktree. Without an explicit
// git-dir, git apply can silently skip paths outside that worktree's prefix.
// This non-repository directory forces standalone patch application.
fn apply_patch(out_dir: &Path, patch: &Path, destination: &Path) {
    let before = fs::read(destination).unwrap();
    let output = Command::new("git")
        .arg("--git-dir")
        .arg(out_dir)
        .arg("-C")
        .arg(out_dir)
        .args(["apply", "-p1"])
        .arg(patch)
        .output()
        .expect("failed to run git");
    assert!(
        output.status.success(),
        "git apply failed for {patch:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_ne!(
        before,
        fs::read(destination).unwrap(),
        "git apply skipped {patch:?} without modifying {destination:?}"
    );
}

fn compile(
    ext: &Path,
    h5filter: &Path,
    core: &Path,
    hdf5_include: &Path,
    lz4_include: &Path,
    src_include: &Path,
) {
    let src = [
        "lz4/lz4.c",
        "src/bitshuffle.c",
        "src/bshuf_h5plugin.c",
        "src/iochain.c",
    ];
    cc::Build::new()
        .files(src.iter().map(|s| ext.join(s)))
        .file(h5filter)
        .file(core)
        .file(Path::new(env!("CARGO_MANIFEST_DIR")).join("dioptrin_unshuffle.c"))
        .flag("-w")
        .include(hdf5_include)
        .include(lz4_include)
        .include(src_include)
        .compile("bitshuffle");
}

fn write_cmake_file(includes: &[&Path]) {
    let mut file = File::create(profile_dir().join(CMAKE_FILE)).unwrap();
    for (n, include) in includes.iter().enumerate() {
        if n > 0 {
            file.write_all(CMAKE_SEPARATOR).unwrap();
        }
        file.write_all(include.to_str().unwrap().as_bytes())
            .unwrap();
    }
}

// OUT_DIR is <target>/<profile>/build/<pkg>-<hash>/out on cargo before 1.99 and
// <target>/<profile>/build/<pkg>/<hash>/out after it, so anchor on the nearest
// "build" ancestor instead of counting levels.
fn profile_dir() -> PathBuf {
    let out = env::var("OUT_DIR").unwrap();
    let mut dir = Path::new(&out);
    while let Some(parent) = dir.parent() {
        if dir.file_name().is_some_and(|name| name == "build") {
            return parent.to_path_buf();
        }
        dir = parent;
    }
    panic!("cannot locate the cargo profile directory from OUT_DIR {out}");
}
