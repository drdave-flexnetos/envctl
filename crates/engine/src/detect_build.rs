//! The build-system detector table: signal file → (default build cmd, artifact
//! globs). `detect()` honors spec.build_system / spec.build_cmd / spec.artifacts
//! overrides, else sniffs the tree top-down by signal-file presence. Re-runnable
//! AFTER a strategy transform, so an AI-to-Rust port of a Go/C repo re-detects as
//! cargo. `build_cmd` is a `String` — "" means "use the detector default".
use crate::model::{AddRepoSpec, BuildSystem};
use std::path::Path;

#[derive(Clone, Debug)]
pub struct BuildPlan {
    pub system: BuildSystem,
    /// Command run via `bash -lc` in the clone dir.
    pub build_cmd: String,
    /// Artifact globs relative to the clone dir.
    pub artifact_globs: Vec<String>,
}

struct Detector {
    system: BuildSystem,
    signals: &'static [&'static str],
    build_cmd: &'static str,
    artifacts: &'static [&'static str],
}

/// Top-down priority: cargo first (a Rust port lands here), then native build
/// systems, then language ecosystems. flake.nix is LAST so a repo that merely
/// ships a flake but is really cargo/go still builds natively.
const TABLE: &[Detector] = &[
    Detector {
        system: BuildSystem::Cargo,
        signals: &["Cargo.toml"],
        build_cmd: "cargo build --release --locked || cargo build --release",
        artifacts: &["target/release/*"],
    },
    Detector {
        system: BuildSystem::Cmake,
        signals: &["CMakeLists.txt"],
        build_cmd: "cmake -S . -B build -DCMAKE_BUILD_TYPE=Release && cmake --build build -j",
        artifacts: &["build/*", "build/bin/*"],
    },
    Detector {
        system: BuildSystem::Meson,
        signals: &["meson.build"],
        build_cmd: "meson setup build --buildtype=release && meson compile -C build",
        artifacts: &["build/*"],
    },
    Detector {
        system: BuildSystem::Autotools,
        signals: &["configure.ac", "configure.in", "Makefile.am", "configure"],
        build_cmd: "[ -x ./configure ] || autoreconf -fi; ./configure && make -j",
        artifacts: &["src/*", "*"],
    },
    Detector {
        system: BuildSystem::Make,
        signals: &["Makefile", "makefile", "GNUmakefile"],
        build_cmd: "make -j",
        artifacts: &["*", "bin/*", "build/*"],
    },
    Detector {
        system: BuildSystem::Go,
        signals: &["go.mod"],
        build_cmd: "go build ./...",
        artifacts: &["*"],
    },
    Detector {
        system: BuildSystem::Zig,
        signals: &["build.zig"],
        build_cmd: "zig build -Doptimize=ReleaseSafe",
        artifacts: &["zig-out/bin/*"],
    },
    Detector {
        system: BuildSystem::Node,
        signals: &["package.json"],
        build_cmd: "if command -v bun >/dev/null; then bun install && (bun run build || true); else npm ci || npm install; npm run build --if-present; fi",
        // Audit fix: dropped node_modules/.bin/* — those are symlinks to third-party
        // dep CLIs (eslint/tsc/vite), not the repo's own artifacts.
        artifacts: &["dist/*", "build/*", "bin/*"],
    },
    Detector {
        system: BuildSystem::Python,
        signals: &["pyproject.toml", "setup.py"],
        build_cmd: "if command -v uv >/dev/null; then uv build; else python3 -m pip install --user .; fi",
        artifacts: &["dist/*", ".venv/bin/*"],
    },
    Detector {
        system: BuildSystem::NixFlake,
        signals: &["flake.nix"],
        build_cmd: "nix build --no-link --print-out-paths .",
        artifacts: &["result/bin/*"],
    },
];

/// Resolve the build recipe for `clone_dir`, honoring spec overrides. An empty
/// `spec.build_cmd` means "use the detector default".
pub fn detect(clone_dir: &Path, spec: &AddRepoSpec) -> anyhow::Result<BuildPlan> {
    let row = match spec.build_system {
        Some(BuildSystem::Auto) | None => sniff(clone_dir),
        Some(forced) => TABLE.iter().find(|d| d.system == forced),
    };

    let (system, default_cmd, default_globs): (BuildSystem, String, Vec<String>) = match row {
        Some(d) => (
            d.system,
            d.build_cmd.to_string(),
            d.artifacts.iter().map(|s| s.to_string()).collect(),
        ),
        None => {
            if spec.build_cmd.trim().is_empty() {
                anyhow::bail!(
                    "could not detect a build system in {} — pass --build-system or --build-cmd",
                    clone_dir.display()
                );
            }
            (BuildSystem::Auto, String::new(), vec!["*".into()])
        }
    };

    Ok(BuildPlan {
        system,
        build_cmd: if spec.build_cmd.trim().is_empty() {
            default_cmd
        } else {
            spec.build_cmd.clone()
        },
        artifact_globs: if spec.artifacts.is_empty() {
            default_globs
        } else {
            spec.artifacts.clone()
        },
    })
}

fn sniff(clone_dir: &Path) -> Option<&'static Detector> {
    TABLE
        .iter()
        .find(|d| d.signals.iter().any(|s| clone_dir.join(s).exists()))
}

/// String tag for provenance / the registered drop-in.
pub fn system_tag(s: BuildSystem) -> &'static str {
    match s {
        BuildSystem::Auto => "auto",
        BuildSystem::Cargo => "cargo",
        BuildSystem::Cmake => "cmake",
        BuildSystem::Meson => "meson",
        BuildSystem::Autotools => "autotools",
        BuildSystem::Make => "make",
        BuildSystem::Node => "node",
        BuildSystem::Python => "python",
        BuildSystem::NixFlake => "nix_flake",
        BuildSystem::Go => "go",
        BuildSystem::Zig => "zig",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn spec() -> AddRepoSpec {
        AddRepoSpec::default()
    }

    #[test]
    fn cargo_beats_flake() {
        let d = tempdir();
        fs::write(d.join("Cargo.toml"), "[package]").unwrap();
        fs::write(d.join("flake.nix"), "{}").unwrap();
        assert_eq!(detect(&d, &spec()).unwrap().system, BuildSystem::Cargo);
    }
    #[test]
    fn go_mod_detected() {
        let d = tempdir();
        fs::write(d.join("go.mod"), "module x").unwrap();
        assert_eq!(detect(&d, &spec()).unwrap().system, BuildSystem::Go);
    }
    #[test]
    fn override_forces_system_and_empty_cmd_uses_default() {
        let d = tempdir();
        fs::write(d.join("Cargo.toml"), "[package]").unwrap();
        let mut s = spec();
        s.build_system = Some(BuildSystem::Cmake);
        let p = detect(&d, &s).unwrap();
        assert_eq!(p.system, BuildSystem::Cmake);
        assert!(p.build_cmd.contains("cmake"));
    }
    #[test]
    fn no_signal_no_cmd_bails() {
        let d = tempdir();
        assert!(detect(&d, &spec()).is_err());
    }

    fn tempdir() -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("envctl-detect-{}", unique()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }
    fn unique() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }
}
