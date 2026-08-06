use std/assert

def checked [program: string, ...arguments: string]: nothing -> nothing {
    run-external $program ...$arguments
    assert ($env.LAST_EXIT_CODE == 0) $"($program) failed"
}

# Verify generated output and the complete Rust workspace.
def main [
    --allow-dirty # Permit generated changes while verifying an automated update.
]: nothing -> nothing {
    checked cargo run "--locked" "-p" codegen "--" check
    checked cargo fmt "--all" "--" "--check"
    (checked
        cargo
        clippy
        "--workspace"
        "--all-targets"
        "--locked"
        "--"
        "-D"
        warnings
    )
    checked cargo test "--workspace" "--locked"
    with-env {RUSTDOCFLAGS: "-D warnings"} {
        checked cargo doc "--workspace" "--locked" "--no-deps"
    }

    if $allow_dirty {
        checked cargo package "--locked" "--allow-dirty"
    } else {
        checked cargo package "--locked"
    }
}
