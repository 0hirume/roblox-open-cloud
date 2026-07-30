mod generate;
mod rust;
mod schema;

use std::{
    ffi::{OsStr, OsString},
    fs,
    io::Write as _,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use generate::{Generated, Source, generate};
use thiserror::Error;

const UPSTREAM_DOCUMENT: &str = "content/en-us/reference/cloud/openapi.json";
const SPEC_PATH: &str = "spec/openapi.json";
const SOURCE_PATH: &str = "spec/source.json";

fn main() -> Result<()> {
    run(std::env::args_os().skip(1))
}

/// A code-generation failure.
#[derive(Debug, Error)]
enum CodegenError {
    /// A filesystem operation failed.
    #[error("failed to {action} `{path}`")]
    Io {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// The OpenAPI document or generated JSON could not be decoded.
    #[error("failed to process JSON at `{path}`")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    /// A subprocess could not be started.
    #[error("failed to run `{command}`")]
    CommandIo {
        command: String,
        #[source]
        source: std::io::Error,
    },
    /// A subprocess returned a failure status.
    #[error("`{command}` failed: {stderr}")]
    Command { command: String, stderr: String },
    /// A subprocess returned text that was not UTF-8.
    #[error("`{command}` returned non-UTF-8 output")]
    CommandUtf8 {
        command: String,
        #[source]
        source: std::string::FromUtf8Error,
    },
    /// The supplied command-line arguments were invalid.
    #[error("{0}")]
    Arguments(String),
    /// The OpenAPI document contains an unsupported or inconsistent value.
    #[error("{0}")]
    Specification(String),
    /// Generated Rust tokens could not be parsed as a complete source file.
    #[error("generated invalid Rust syntax")]
    Rust(#[source] syn::Error),
    /// Checked-in generated output differs from a fresh generation.
    #[error("generated output is stale: {0}")]
    Drift(String),
}

/// The result type used by the code generator.
type Result<T> = std::result::Result<T, Box<CodegenError>>;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    Sync,
    Check,
    Update(PathBuf),
}

fn run(arguments: impl IntoIterator<Item = OsString>) -> Result<()> {
    let mode = parse_arguments(arguments)?;
    let root = workspace_root()?;

    match mode {
        Mode::Sync => {
            let generated = generate_vendored(&root)?;
            write_generated(&root, &generated)?;
            cargo_fmt(&root)
        }
        Mode::Check => check_generated(&root, &generate_vendored(&root)?),
        Mode::Update(upstream) => update(&root, &upstream),
    }
}

fn parse_arguments(arguments: impl IntoIterator<Item = OsString>) -> Result<Mode> {
    let arguments: Vec<OsString> = arguments.into_iter().collect();
    match arguments.as_slice() {
        [command] if command == OsStr::new("sync") => Ok(Mode::Sync),
        [command] if command == OsStr::new("check") => Ok(Mode::Check),
        [command, upstream] if command == OsStr::new("update") => {
            Ok(Mode::Update(PathBuf::from(upstream)))
        }
        [command, ..]
            if command == OsStr::new("sync")
                || command == OsStr::new("check")
                || command == OsStr::new("update") =>
        {
            Err(Box::new(usage()))
        }
        [command, ..] => Err(Box::new(CodegenError::Arguments(format!(
            "unknown codegen command `{}`",
            command.to_string_lossy()
        )))),
        [] => Err(Box::new(usage())),
    }
}

fn usage() -> CodegenError {
    CodegenError::Arguments(String::from(
        "usage: cargo run -p codegen -- <sync|check|update <creator-docs>>",
    ))
}

fn generate_vendored(root: &Path) -> Result<Generated> {
    let api_bytes = read(&root.join(SPEC_PATH))?;
    let source = read_source(&root.join(SOURCE_PATH))?;
    generate(&api_bytes, &source)
}

fn read_source(path: &Path) -> Result<Source> {
    serde_json::from_slice(&read(path)?).map_err(|source| {
        Box::new(CodegenError::Json {
            path: path.to_path_buf(),
            source,
        })
    })
}

fn update(root: &Path, upstream: &Path) -> Result<()> {
    let api_bytes = read(&upstream.join(UPSTREAM_DOCUMENT))?;
    let source = Source::creator_docs(
        git(
            upstream,
            &["log", "-1", "--format=%H", "--", UPSTREAM_DOCUMENT],
        )?,
        git(
            upstream,
            &["log", "-1", "--format=%cs", "--", UPSTREAM_DOCUMENT],
        )?,
    );
    let generated = generate(&api_bytes, &source)?;
    let mut source_json = serde_json::to_string_pretty(&source).map_err(|source| {
        Box::new(CodegenError::Json {
            path: root.join(SOURCE_PATH),
            source,
        })
    })?;
    source_json.push('\n');

    write(&root.join(SPEC_PATH), &api_bytes)?;
    write(&root.join(SOURCE_PATH), source_json.as_bytes())?;
    write_generated(root, &generated)?;
    cargo_fmt(root)
}

fn workspace_root() -> Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            Box::new(CodegenError::Arguments(String::from(
                "the codegen crate has no workspace parent",
            )))
        })
}

fn read(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).map_err(|source| {
        Box::new(CodegenError::Io {
            action: "read",
            path: path.to_path_buf(),
            source,
        })
    })
}

fn write(path: &Path, contents: &[u8]) -> Result<()> {
    fs::write(path, contents).map_err(|source| {
        Box::new(CodegenError::Io {
            action: "write",
            path: path.to_path_buf(),
            source,
        })
    })
}

fn create_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path).map_err(|source| {
        Box::new(CodegenError::Io {
            action: "create directory",
            path: path.to_path_buf(),
            source,
        })
    })
}

fn git(repository: &Path, arguments: &[&str]) -> Result<String> {
    let command_name = format!("git -C {} {}", repository.display(), arguments.join(" "));
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(arguments)
        .output()
        .map_err(|source| {
            Box::new(CodegenError::CommandIo {
                command: command_name.clone(),
                source,
            })
        })?;
    if !output.status.success() {
        return Err(Box::new(CodegenError::Command {
            command: command_name,
            stderr: String::from_utf8_lossy(&output.stderr).trim().into(),
        }));
    }
    String::from_utf8(output.stdout)
        .map(|value| String::from(value.trim()))
        .map_err(|source| {
            Box::new(CodegenError::CommandUtf8 {
                command: command_name,
                source,
            })
        })
}

fn write_generated(root: &Path, generated: &Generated) -> Result<()> {
    let spec_dir = root.join("spec");
    let generated_dir = root.join("src/generated");
    create_dir(&spec_dir)?;
    create_dir(&generated_dir)?;
    write(
        &spec_dir.join("coverage.json"),
        generated.coverage_json().as_bytes(),
    )?;
    write(
        &generated_dir.join("coverage.rs"),
        &rustfmt(generated.coverage_rust())?,
    )?;
    write(
        &generated_dir.join("types.rs"),
        &rustfmt(generated.types())?,
    )?;
    for (domain, source) in generated.domains() {
        write(
            &generated_dir.join(format!("{domain}.rs")),
            &rustfmt(source)?,
        )?;
    }
    Ok(())
}

fn check_generated(root: &Path, generated: &Generated) -> Result<()> {
    let mut drift = Vec::new();
    compare(
        &root.join("spec/coverage.json"),
        generated.coverage_json().as_bytes(),
        &mut drift,
    )?;
    compare(
        &root.join("src/generated/coverage.rs"),
        &rustfmt(generated.coverage_rust())?,
        &mut drift,
    )?;
    compare(
        &root.join("src/generated/types.rs"),
        &rustfmt(generated.types())?,
        &mut drift,
    )?;
    for (domain, source) in generated.domains() {
        compare(
            &root.join(format!("src/generated/{domain}.rs")),
            &rustfmt(source)?,
            &mut drift,
        )?;
    }
    if drift.is_empty() {
        Ok(())
    } else {
        Err(Box::new(CodegenError::Drift(
            drift
                .iter()
                .map(|path: &PathBuf| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", "),
        )))
    }
}

fn compare(path: &Path, expected: &[u8], drift: &mut Vec<PathBuf>) -> Result<()> {
    match fs::read(path) {
        Ok(actual) => {
            if actual != expected {
                drift.push(path.to_path_buf());
            }
            Ok(())
        }
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            drift.push(path.to_path_buf());
            Ok(())
        }
        Err(source) => Err(Box::new(CodegenError::Io {
            action: "read",
            path: path.to_path_buf(),
            source,
        })),
    }
}

fn rustfmt(rust_source: &str) -> Result<Vec<u8>> {
    let command_name = String::from("rustfmt --edition 2024 --emit stdout");
    let mut child = Command::new("rustfmt")
        .args(["--edition", "2024", "--emit", "stdout"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| {
            Box::new(CodegenError::CommandIo {
                command: command_name.clone(),
                source,
            })
        })?;
    let mut stdin = child.stdin.take().ok_or_else(|| {
        Box::new(CodegenError::CommandIo {
            command: command_name.clone(),
            source: std::io::Error::other("rustfmt stdin was not piped"),
        })
    })?;
    stdin.write_all(rust_source.as_bytes()).map_err(|source| {
        Box::new(CodegenError::CommandIo {
            command: command_name.clone(),
            source,
        })
    })?;
    drop(stdin);

    let output = child.wait_with_output().map_err(|source| {
        Box::new(CodegenError::CommandIo {
            command: command_name.clone(),
            source,
        })
    })?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(Box::new(CodegenError::Command {
            command: command_name,
            stderr: String::from_utf8_lossy(&output.stderr).trim().into(),
        }))
    }
}

fn cargo_fmt(root: &Path) -> Result<()> {
    let command_name = String::from("cargo fmt --all");
    let output = Command::new("cargo")
        .current_dir(root)
        .args(["fmt", "--all"])
        .output()
        .map_err(|source| {
            Box::new(CodegenError::CommandIo {
                command: command_name.clone(),
                source,
            })
        })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(Box::new(CodegenError::Command {
            command: command_name,
            stderr: String::from_utf8_lossy(&output.stderr).trim().into(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use super::{Mode, parse_arguments};
    use crate::Result;

    #[test]
    fn parses_generation_modes() -> Result<()> {
        assert_eq!(
            parse_arguments([OsString::from("sync")])?,
            Mode::Sync,
            "sync should use the vendored specification"
        );
        assert_eq!(
            parse_arguments([OsString::from("check")])?,
            Mode::Check,
            "check should use the vendored specification"
        );
        assert_eq!(
            parse_arguments([OsString::from("update"), OsString::from("creator-docs"),])?,
            Mode::Update(PathBuf::from("creator-docs")),
            "update should accept an explicit Creator Docs checkout"
        );
        Ok(())
    }
}
