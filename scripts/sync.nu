const HTTP_METHODS = [
    delete
    get
    head
    options
    patch
    post
    put
    trace
]

const TYPED_DOMAINS = [
    datastore
    memory_store
    messaging
    ordered_datastore
    universe
]

const RUST_KEYWORDS = [
    as
    async
    await
    break
    const
    continue
    crate
    dyn
    else
    enum
    extern
    false
    fn
    for
    if
    impl
    in
    let
    loop
    match
    mod
    move
    mut
    pub
    ref
    return
    self
    Self
    static
    struct
    super
    trait
    true
    type
    unsafe
    use
    where
    while
    yield
]

def rust-identifier [value: string]: nothing -> string {
    let identifier = (
        $value
        | str snake-case
        | str replace --all --regex '[^a-zA-Z0-9]+' _
        | str trim --char _
    )
    let identifier = if $identifier =~ '^[0-9]' {
        $'operation_($identifier)'
    } else if $identifier == '' {
        'operation'
    } else {
        $identifier
    }

    if $identifier in $RUST_KEYWORDS {
        $'($identifier)_value'
    } else {
        $identifier
    }
}

# nu-lint-ignore: single_call_command
def domain-for [tag: string, path: string]: string -> string {
    let summary = $in
    let lower_path = $path | str downcase
    let lower_summary = $summary | str downcase

    if $lower_path =~ :publishmessage {
        'messaging'
    } else if $lower_path =~ /secrets {
        'secrets'
    } else if (
        $lower_path =~ ordered-data-store
        or $lower_summary =~ 'ordered data store'
    ) {
        'ordered_datastore'
    } else if (
        $lower_path =~ memory-store
        or $lower_summary =~ 'memory store'
    ) {
        'memory_store'
    } else if (
        $lower_path =~ data-store
        or $lower_path =~ datastore
    ) {
        'datastore'
    } else if $lower_path =~ user-restriction {
        'restrictions'
    } else if $lower_path =~ luau-execution {
        'luau'
    } else if $lower_path =~ /subscriptions/ {
        'subscriptions'
    } else {
        match $tag {
            'Analytics' => 'analytics'
            'Assets' => 'assets'
            'Avatars' => 'avatars'
            'Badges' => 'badges'
            'Bans and blocks' => 'restrictions'
            'Configs' => 'configs'
            'Creator Store' => 'creator_store'
            'Data and memory stores' => 'datastore'
            'Developer products' => 'developer_products'
            'Game passes' => 'game_passes'
            'Generative AI' => 'generative_ai'
            'Groups' => 'groups'
            'Interactions' => 'interactions'
            'Inventories' => 'inventory'
            'Localization' => 'localization'
            'Luau Execution' => 'luau'
            'Matchmaking' => 'matchmaking'
            'Notifications' => 'notifications'
            'Places' => 'places'
            'Team Create' => 'team_create'
            'Thumbnails' => 'thumbnails'
            'Universes' => 'universe'
            'Users' => 'users'
            _ => (error make $'No Rust domain mapping for OpenAPI tag ($tag)')
        }
    }
}

def rust-string []: any -> string {
    to json --raw
}

def route-identifier [operation: record]: nothing -> string {
    let parts = (
        $operation.path
        | split row /
        | where $it != '' and $it !~ '^v[0-9]+$'
        | each {|part|
            $part
            | str replace --all --regex '[{}]' ''
        }
    )
    let tail = $parts | last 3
    let path_name = $tail | str join ' '
    let name = rust-identifier $'($operation.http_method) ($path_name)'
    if ($name | str length) <= 72 {
        $name
    } else {
        let shorter_path = $tail | last 2 | str join ' '
        rust-identifier $'($operation.http_method) ($shorter_path)'
    }
}

def render-endpoint [operation: record]: nothing -> string {
    let scopes = (
        $operation.scopes
        | each {|scope| $scope | rust-string }
        | str join ', '
    )
    let method = match $operation.http_method {
        DELETE => 'Delete'
        GET => 'Get'
        HEAD => 'Head'
        OPTIONS => 'Options'
        PATCH => 'Patch'
        POST => 'Post'
        PUT => 'Put'
        TRACE => 'Trace'
        _ => (error make $'Unsupported HTTP method ($operation.http_method)')
    }
    let stability = match $operation.stability {
        stable => 'Stable'
        beta => 'Beta'
        legacy-beta => 'LegacyBeta'
        _ => (error make $'Unsupported stability ($operation.stability)')
    }
    let path = $operation.path | rust-string
    let summary = $operation.summary | rust-string
    let summary_doc = (
        $operation.summary
        | str replace --all --regex '\r|\n' ' '
    )

    [
        $'/// ($summary_doc)'
        $"pub const ($operation.constant): crate::Endpoint = crate::Endpoint::new\("
        $'    crate::HttpMethod::($method),'
        $'    ($path),'
        $'    ($summary),'
        $'    crate::Stability::($stability),'
        $"    crate::AuthenticationSupport::new\(($operation.api_key), ($operation.oauth), ($operation.unauthenticated)),"
        $'    &[($scopes)],'
        ');'
    ] | str join (char newline)
}

def render-method [operation: record]: nothing -> string {
    let summary_doc = (
        $operation.summary
        | str replace --all --regex '\r|\n' ' '
    )
    let arguments = (
        $operation.path_parameters
        | each {|parameter|
            let rust_name = rust-identifier $parameter
            $'        ($rust_name): impl std::fmt::Display,'
        }
    )

    if ($arguments | is-empty) {
        [
            $'    /// ($summary_doc)'
            '    #[must_use]'
            $"    pub fn ($operation.rust_method)\(&self) -> crate::OperationRequest<'_> {"
            $"        self.client.operation\(($operation.constant))"
            '    }'
        ] | str join (char newline)
    } else {
        let substitutions = (
            $operation.path_parameters
            | each {|parameter|
                let rust_name = rust-identifier $parameter
                let parameter_name = $parameter | rust-string
                $"        let request = request.path\(($parameter_name), ($rust_name))?;"
            }
        )
        [
            $'    /// ($summary_doc)'
            '    ///'
            '    /// # Errors'
            '    ///'
            '    /// Returns an error if a generated path substitution fails.'
            $"    pub fn ($operation.rust_method)\("
            '        &self,'
            ...$arguments
            "    ) -> crate::Result<crate::OperationRequest<'_>> {"
            $"        let request = self.client.operation\(($operation.constant));"
            ...$substitutions
            '        Ok(request)'
            '    }'
        ] | str join (char newline)
    }
}

# nu-lint-ignore: single_call_command
def render-domain [domain: string, ...existing_domains: string]: list<record> -> string {
    let operations = $in
    let endpoints = (
        $operations
        | each {|operation| render-endpoint $operation }
        | str join $'(char newline)(char newline)'
    )
    let endpoint_names = (
        $operations
        | each {|operation| $'    ($operation.constant),' }
    )
    let methods = (
        $operations
        | each {|operation| render-method $operation }
        | str join $'(char newline)(char newline)'
    )
    let service = match $domain {
        analytics => 'Analytics'
        assets => 'Assets'
        avatars => 'Avatars'
        badges => 'Badges'
        configs => 'Configs'
        creator_store => 'CreatorStore'
        datastore => 'DataStores'
        developer_products => 'DeveloperProducts'
        game_passes => 'GamePasses'
        generative_ai => 'GenerativeAi'
        groups => 'Groups'
        interactions => 'Interactions'
        inventory => 'Inventory'
        localization => 'Localization'
        luau => 'Luau'
        matchmaking => 'Matchmaking'
        memory_store => 'MemoryStore'
        messaging => 'Messaging'
        notifications => 'Notifications'
        ordered_datastore => 'OrderedDataStores'
        places => 'Places'
        restrictions => 'Restrictions'
        secrets => 'Secrets'
        subscriptions => 'Subscriptions'
        team_create => 'TeamCreate'
        thumbnails => 'Thumbnails'
        universe => 'Universes'
        users => 'Users'
        _ => (error make $'No service type for ($domain)')
    }

    let common = [
        '// @generated by scripts/sync.nu; do not edit by hand.'
        ''
        $endpoints
        ''
        '/// Every supported endpoint in this domain.'
        'pub const ENDPOINTS: &[crate::Endpoint] = &['
        ...$endpoint_names
        '];'
        ''
    ]

    if $domain in $existing_domains {
        [
            ...$common
            $"impl ($service)<'_> {"
            $methods
            '}'
            ''
        ] | str join (char newline)
    } else {
        let accessor = match $domain {
            universe => 'universes'
            _ => $domain
        }
        [
            ...$common
            '/// Operations in this Roblox Open Cloud domain.'
            '#[derive(Debug, Clone, Copy)]'
            $"pub struct ($service)<'client> {"
            "    client: &'client crate::Client,"
            '}'
            ''
            'impl crate::Client {'
            '    /// Returns operations in this Roblox Open Cloud domain.'
            '    #[must_use]'
            $"    pub const fn ($accessor)\(&self) -> ($service)<'_> {"
            $'        ($service) { client: self }'
            '    }'
            '}'
            ''
            $"impl ($service)<'_> {"
            $methods
            '}'
            ''
        ] | str join (char newline)
    }
}

# nu-lint-ignore: single_call_command
def render-coverage []: list<record> -> string {
    let operations = $in
    let entries = (
        $operations
        | each {|operation|
            $'    crate::($operation.domain)::($operation.constant),'
        }
    )
    [
        '// @generated by scripts/sync.nu; do not edit by hand.'
        ''
        '/// Every recommended resource operation covered by this crate.'
        'pub const ENDPOINTS: &[crate::Endpoint] = &['
        ...$entries
        '];'
        ''
    ] | str join (char newline)
}

def collect-operations [api: record]: nothing -> list<record> {
    let operations = (
        $api.paths
        | items {|path, item|
            $item
            | transpose method operation
            | where method in $HTTP_METHODS
            | each {|entry|
                let operation = $entry.operation
                let security = $operation | get --optional security | default []
                let schemes = (
                    $security
                    | each {|requirement| $requirement | columns }
                    | flatten
                    | uniq
                    | sort
                )
                let tags = $operation | get --optional tags | default [Uncategorized]
                let tag = $tags | first
                let summary = (
                    $operation
                    | get --optional summary
                    | default $'($entry.method | str upcase) ($path)'
                )
                let documented_stability = (
                    $operation
                    | get --optional 'x-roblox-stability'
                    | default UNSPECIFIED
                )
                let deprecated = (
                    ($operation | get --optional deprecated | default false)
                    or (($operation | get --optional 'x-roblox-deprecated') != null)
                )
                let api_key = 'roblox-api-key' in $schemes
                let oauth = 'roblox-oauth2' in $schemes
                let modern = $api_key or $oauth
                let unauthenticated = $schemes | is-empty
                let recommended = (
                    (not $deprecated)
                    and (
                        ($modern and $documented_stability != EXPERIMENTAL)
                        or (
                            $unauthenticated
                            and ($documented_stability in [STABLE BETA])
                        )
                    )
                )

                if $recommended {
                    let scopes = (
                        $operation
                        | get --optional 'x-roblox-scopes'
                        | default []
                        | get name
                        | uniq
                        | sort
                    )
                    let stability = match $documented_stability {
                        STABLE => 'stable'
                        BETA => 'beta'
                        _ => 'legacy-beta'
                    }
                    let path_parameters = (
                        $path
                        | parse --regex '\x7b(?<name>[^\x7d]+)\x7d'
                        | get name
                    )
                    let operation_id = $operation | get --optional operationId
                    let domain = $summary | domain-for $tag $path
                    {
                        domain: $domain
                        http_method: ($entry.method | str upcase)
                        operation_id: (match $operation_id {
                            null => ''
                            _ => $operation_id
                        })
                        path: $path
                        path_parameters: $path_parameters
                        summary: $summary
                        tag: $tag
                        stability: $stability
                        api_key: $api_key
                        oauth: $oauth
                        unauthenticated: $unauthenticated
                        scopes: $scopes
                    }
                }
            }
        }
        | flatten
    )

    let operations = (
        $operations
        | each {|operation|
            $operation | merge {
                base_method: (rust-identifier $operation.summary)
            }
        }
    )
    let duplicate_keys = (
        $operations
        | group-by {|operation|
            $'($operation.domain)::($operation.base_method)'
        }
        | transpose key operations
        | where ($it.operations | length) > 1
        | get key
    )

    $operations
    | each {|operation|
        let key = $'($operation.domain)::($operation.base_method)'
        let rust_method = if (
            $key not-in $duplicate_keys
            and ($operation.base_method | str length) <= 72
        ) {
            $operation.base_method
        } else {
            route-identifier $operation
        }
        let rust_method = if $operation.domain in $TYPED_DOMAINS {
            $'request_($rust_method)'
        } else {
            $rust_method
        }
        let constant = $rust_method | str upcase
        $operation | reject base_method | merge {
            id: $'($operation.domain)::($rust_method)'
            rust_method: $rust_method
            constant: $constant
        }
    }
    | sort-by domain rust_method
}

# Synchronizes recommended Roblox Open Cloud operations from Creator Docs.
export def main [--docs: path = 'C:/Users/ohirume/src/creator-docs']: nothing -> nothing {
    let spec_path = (
        $docs
        | path join content en-us reference cloud openapi.json
    )
    let api = try {
        open $spec_path
    } catch {|error|
        $error | error make
    }
    let operations = collect-operations $api
    let duplicate_ids = (
        $operations
        | group-by id
        | transpose id operations
        | where ($it.operations | length) > 1
    )
    if ($duplicate_ids | is-not-empty) {
        error make {
            msg: 'Generated Rust endpoint identifiers are not unique'
            help: ($duplicate_ids | table --expand)
            labels: [
                {
                    text: 'duplicate endpoint identifiers'
                    span: (metadata $duplicate_ids).span
                }
            ]
        }
    }

    let commit = (
        git -C $docs rev-parse HEAD
        | complete
    )
    if $commit.exit_code != 0 {
        error make $commit.stderr
    }
    let commit_date = (
        git -C $docs show -s '--format=%cs' HEAD
        | complete
    )
    if $commit_date.exit_code != 0 {
        error make $commit_date.stderr
    }

    try {
        mkdir spec
        mkdir src/generated
    } catch {|error|
        $error | error make
    }

    try {
        {
            source: {
                repository: Roblox/creator-docs
                commit: ($commit.stdout | str trim)
                commit_date: ($commit_date.stdout | str trim)
                document: content/en-us/reference/cloud/openapi.json
            }
            policy: {
                include: [
                    'non-deprecated, non-experimental operations supporting API keys or OAuth'
                    'non-deprecated unauthenticated operations marked stable or beta'
                ]
                exclude: [
                    'cookie-only operations'
                    'deprecated operations'
                    'experimental operations'
                    'unauthenticated operations without a stability marker'
                ]
            }
            count: ($operations | length)
            operations: $operations
        }
        | to json --indent 2
        | save --force spec/coverage.json
    } catch {|error|
        $error | error make
    }

    let existing_domains = [
        datastore
        memory_store
        messaging
        ordered_datastore
        universe
    ]
    $operations
    | group-by domain
    | items {|domain, operations|
        let source = $operations | render-domain $domain ...$existing_domains
        try {
            $source | save --force $'src/generated/($domain).rs'
        } catch {|error|
            $error | error make
        }
    }
    try {
        $operations
        | render-coverage
        | save --force src/generated/coverage.rs
    } catch {|error|
        $error | error make
    }
    let format = cargo fmt --all | complete
    if $format.exit_code != 0 {
        error make {
            msg: 'rustfmt failed after endpoint generation'
            help: $format.stderr
            labels: [
                {
                    text: 'rustfmt failed'
                    span: (metadata $format).span
                }
            ]
        }
    }

    (
        $operations
        | group-by domain
        | items {|domain, operations|
            {
                domain: $domain
                operations: ($operations | length)
            }
        }
        | sort-by domain
    )
}
