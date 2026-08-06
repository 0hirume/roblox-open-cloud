use std/assert

const CHECKOUT = ".upstream/creator-docs"
const DOCUMENT = "content/en-us/reference/cloud/openapi.json"
const GENERATED = [spec src/generated]

def checked [program: string, ...arguments: string]: nothing -> nothing {
    run-external $program ...$arguments
    assert ($env.LAST_EXIT_CODE == 0) $"($program) failed"
}

def capture [program: string, ...arguments: string]: nothing -> string {
    let result = run-external $program ...$arguments | complete

    assert ($result.exit_code == 0) $"($program) failed: ($result.stderr | str trim)"
    $result.stdout | str trim
}

# Import the current Creator Docs OpenAPI document and report whether it changed.
def main []: nothing -> nothing {
    (checked
        gh
        repo
        clone
        $env.DOCS_REPOSITORY
        $CHECKOUT
        "--"
        "--filter=blob:none"
        "--single-branch"
        "--sparse"
    )
    checked git "-C" $CHECKOUT sparse-checkout set "--no-cone" $DOCUMENT
    checked cargo run "--locked" "-p" codegen "--" update $CHECKOUT

    let changes = capture git status "--short" "--" ...$GENERATED
    try {
        $"changed=($changes | is-not-empty)(char newline)" | save --append $env.GITHUB_OUTPUT
    } catch {|error| assert false $error.msg }
}
