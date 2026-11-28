use axum::{
    Json, Router,
    http::{HeaderValue, StatusCode, header::LOCATION},
    response::IntoResponse,
    routing::get,
};
use pandar_core::PrinterFamily;
use pandar_hub::{
    AppState,
    artifacts::{DEFAULT_MAX_ARTIFACT_BYTES, FilesystemArtifactStorage},
    db::{Database, DatabaseConfig},
    repositories::{AuditActor, PrinterSnapshotUpsert, TenantTokenScope, UserRole},
    router,
};
use serde::Serialize;
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::net::TcpListener;

const PANDAR_BIN: &str = env!("CARGO_BIN_EXE_pandar");

#[derive(Serialize)]
struct E2eArtifact {
    schema: &'static str,
    steps: Vec<E2eStep>,
}

#[derive(Serialize)]
struct E2eStep {
    name: &'static str,
    passed: bool,
    detail: String,
}

fn run_pandar(args: &[&str], stdin_data: Option<&str>, envs: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(PANDAR_BIN);
    cmd.args(args)
        .env_remove("PANDAR_SERVER")
        .env_remove("PANDAR_HUB_URL")
        .env_remove("PANDAR_API_KEY")
        .env_remove("PANDAR_TENANT_ID")
        .env_remove("PANDAR_PROFILE")
        .env_remove("PANDAR_CLI_CONFIG")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in envs {
        cmd.env(k, v);
    }
    if stdin_data.is_some() {
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null());
    }
    let mut child = cmd.spawn().expect("spawn pandar binary");
    if let Some(input) = stdin_data {
        child
            .stdin
            .as_mut()
            .expect("stdin pipe")
            .write_all(input.as_bytes())
            .expect("write stdin");
    }
    child.wait_with_output().expect("wait pandar output")
}

async fn start_server(app: Router) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

fn write_artifact(artifact: &E2eArtifact) {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let dir = workspace_root.join("target").join("e2e-artifacts");
    fs::create_dir_all(&dir).expect("create e2e artifacts dir");
    let path = dir.join("cli-api-key-flow.json");
    let json = serde_json::to_string_pretty(artifact).expect("serialize artifact");
    fs::write(&path, format!("{json}\n")).expect("write e2e artifact");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pandar_cli_api_key_and_browser_login_e2e_flow() {
    // SAFETY: cli_e2e runs a single test binary process before spawning any Hub tasks.
    unsafe {
        std::env::set_var(
            "PANDAR_PRINTER_ACCESS_CODE_KEY",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        );
    }
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("hub.db");
    let spool_dir = temp.path().join("spool");
    fs::create_dir_all(&spool_dir).unwrap();

    let database = Database::connect(
        &DatabaseConfig::from_url(format!("sqlite://{}?mode=rwc", db_path.display())).unwrap(),
    )
    .await
    .unwrap();
    database.migrate().await.unwrap();
    let storage = FilesystemArtifactStorage::new(&spool_dir, DEFAULT_MAX_ARTIFACT_BYTES).unwrap();
    let state = AppState::from_database(database, storage).await.unwrap();

    // Seed tenant, user, agent, printer, and initial admin + operator API keys.
    let tenant = state.tenants().create("acme", "Acme Labs").await.unwrap();
    let user = state
        .auth()
        .create_user(
            tenant.id,
            "alice@acme.test",
            "Alice Maker",
            UserRole::Operator,
        )
        .await
        .unwrap();
    let agent = state
        .agents()
        .create(tenant.id, "shop-agent")
        .await
        .unwrap();
    state
        .agents()
        .claim_online_session(
            tenant.id,
            agent.id,
            "sess-1",
            "0.2.2",
            &pandar_core::created_at_now(),
        )
        .await
        .unwrap();
    let printer = state
        .printers()
        .upsert_snapshot_if_current(
            tenant.id,
            agent.id,
            "sess-1",
            PrinterSnapshotUpsert {
                serial_number: "01P00A000000001".to_owned(),
                printer_family: PrinterFamily::BambuLab,
                host: Some("192.0.2.10".to_owned()),
                access_code: Some("12345678".to_owned()),
                name: "Lab X1C".to_owned(),
                model: Some("X1C".to_owned()),
                status: Some("IDLE".to_owned()),
                observed_at: pandar_core::created_at_now(),
                nozzle_temperatures: vec![],
                active_nozzle: None,
                bed_temperature_celsius: None,
                bed_target_temperature_celsius: None,
                chamber_temperature_celsius: None,
                chamber_target_temperature_celsius: None,
                chamber_light_on: Some(false),
                cooling_system: None,
                nozzle_system: None,
                connection_authoritative: true,
                telemetry_authoritative: false,
            },
        )
        .await
        .unwrap();

    let admin_key = state
        .auth()
        .create_tenant_token_with_audit(
            tenant.id,
            "Bootstrap Admin Key",
            vec![TenantTokenScope::All],
            None,
            AuditActor::no_auth(),
        )
        .await
        .unwrap()
        .plaintext_token;
    let operator_key = state
        .auth()
        .create_tenant_token_with_audit(
            tenant.id,
            "CI Operator Key",
            vec![TenantTokenScope::Operator],
            None,
            AuditActor::no_auth(),
        )
        .await
        .unwrap()
        .plaintext_token;

    let hub_url = start_server(router(state.clone())).await;
    let mut steps = Vec::new();

    // 1. Malicious /.well-known/pandar pointing cross-origin must NEVER receive API key.
    let trap_hits = Arc::new(AtomicUsize::new(0));
    let trap_hits_clone = Arc::clone(&trap_hits);
    let trap_url = start_server(Router::new().fallback(move || {
        let hits = Arc::clone(&trap_hits_clone);
        async move {
            hits.fetch_add(1, Ordering::SeqCst);
            StatusCode::OK
        }
    }))
    .await;
    let trap_url_for_wk = trap_url.clone();
    let malicious_web_url = start_server(Router::new().route(
        "/.well-known/pandar",
        get(move || {
            let target = trap_url_for_wk.clone();
            async move { Json(serde_json::json!({ "hubUrl": target })) }
        }),
    ))
    .await;
    let config_path = temp.path().join("cfg").join("cli.json");
    let config_str = config_path.to_str().unwrap();

    let out = run_pandar(
        &[
            "login",
            "--server",
            &malicious_web_url,
            "--api-key-stdin",
            "--config",
            config_str,
        ],
        Some(&format!("{admin_key}\n")),
        &[],
    );
    assert!(!out.status.success(), "cross-origin discovery must fail");
    assert_eq!(trap_hits.load(Ordering::SeqCst), 0);
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !combined.contains(&admin_key),
        "plaintext API key must never appear in CLI output"
    );
    steps.push(E2eStep {
        name: "reject_cross_origin_well_known_discovery",
        passed: true,
        detail: "trap received 0 requests; API key never leaked".to_owned(),
    });

    // 2. 200 OK invalid JSON /.well-known/pandar must fail closed without falling back.
    let bad_wk_whoami_hits = Arc::new(AtomicUsize::new(0));
    let bad_wk_whoami_hits_clone = Arc::clone(&bad_wk_whoami_hits);
    let bad_wk_url = start_server(
        Router::new()
            .route(
                "/.well-known/pandar",
                get(|| async { (StatusCode::OK, "not-json") }),
            )
            .route(
                "/api/v1/auth/whoami",
                get(move || {
                    let hits = Arc::clone(&bad_wk_whoami_hits_clone);
                    async move {
                        hits.fetch_add(1, Ordering::SeqCst);
                        StatusCode::OK
                    }
                }),
            ),
    )
    .await;
    let out = run_pandar(
        &[
            "login",
            "--server",
            &bad_wk_url,
            "--api-key-stdin",
            "--config",
            config_str,
        ],
        Some(&admin_key),
        &[],
    );
    assert!(
        !out.status.success(),
        "200 invalid JSON discovery must fail"
    );
    assert_eq!(
        bad_wk_whoami_hits.load(Ordering::SeqCst),
        0,
        "200 invalid JSON discovery must not fall back to /api/v1/auth/whoami"
    );
    steps.push(E2eStep {
        name: "reject_malformed_200_well_known",
        passed: true,
        detail: "failed closed on HTTP 200 non-JSON discovery without calling whoami".to_owned(),
    });

    // 3. Discovery non-whitelisted statuses (302, 500, 204), connection error, and timeout must
    //    fail closed without falling back to /api/v1/auth/whoami or following redirects.
    let redirect_whoami_hits = Arc::new(AtomicUsize::new(0));
    let redirect_whoami_hits_clone = Arc::clone(&redirect_whoami_hits);
    let redirect_target = trap_url.clone();
    let redirect_url = start_server(
        Router::new()
            .route(
                "/.well-known/pandar",
                get(move || {
                    let loc =
                        HeaderValue::from_str(&format!("{redirect_target}/api/v1/auth/whoami"))
                            .unwrap();
                    async move { (StatusCode::FOUND, [(LOCATION, loc)]).into_response() }
                }),
            )
            .route(
                "/api/v1/auth/whoami",
                get(move || {
                    let hits = Arc::clone(&redirect_whoami_hits_clone);
                    async move {
                        hits.fetch_add(1, Ordering::SeqCst);
                        StatusCode::OK
                    }
                }),
            ),
    )
    .await;
    let out = run_pandar(
        &[
            "login",
            "--server",
            &redirect_url,
            "--api-key-stdin",
            "--config",
            config_str,
        ],
        Some(&admin_key),
        &[],
    );
    assert!(!out.status.success(), "302 discovery must fail closed");
    assert_eq!(trap_hits.load(Ordering::SeqCst), 0);
    assert_eq!(
        redirect_whoami_hits.load(Ordering::SeqCst),
        0,
        "302 discovery must not fall back to /api/v1/auth/whoami"
    );
    let redirect_stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        redirect_stderr.contains("302") && !redirect_stderr.contains(&admin_key),
        "302 discovery error should mention HTTP 302 without leaking secret: {redirect_stderr}"
    );

    for status in [StatusCode::INTERNAL_SERVER_ERROR, StatusCode::NO_CONTENT] {
        let whoami_hits = Arc::new(AtomicUsize::new(0));
        let whoami_hits_clone = Arc::clone(&whoami_hits);
        let status_url = start_server(
            Router::new()
                .route("/.well-known/pandar", get(move || async move { status }))
                .route(
                    "/api/v1/auth/whoami",
                    get(move || {
                        let hits = Arc::clone(&whoami_hits_clone);
                        async move {
                            hits.fetch_add(1, Ordering::SeqCst);
                            StatusCode::OK
                        }
                    }),
                ),
        )
        .await;
        let out = run_pandar(
            &[
                "login",
                "--server",
                &status_url,
                "--api-key-stdin",
                "--config",
                config_str,
            ],
            Some(&admin_key),
            &[],
        );
        assert!(
            !out.status.success(),
            "HTTP {status} discovery must fail closed"
        );
        assert_eq!(
            whoami_hits.load(Ordering::SeqCst),
            0,
            "HTTP {status} discovery must not fall back to /api/v1/auth/whoami"
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains(status.as_str()) && !stderr.contains(&admin_key),
            "HTTP {status} discovery stderr should report status without secret: {stderr}"
        );
    }

    // Connection refused must fail at discovery with preserved underlying error cause.
    let closed_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let closed_addr = closed_listener.local_addr().unwrap();
    drop(closed_listener);
    let closed_url = format!("http://{closed_addr}");
    let out = run_pandar(
        &[
            "login",
            "--server",
            &closed_url,
            "--api-key-stdin",
            "--config",
            config_str,
        ],
        Some(&admin_key),
        &[],
    );
    assert!(!out.status.success(), "unreachable server must fail");
    let closed_stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        closed_stderr.contains("/.well-known/pandar")
            && closed_stderr.contains("Caused by:")
            && !closed_stderr.contains("/api/v1/auth/whoami")
            && !closed_stderr.contains(&admin_key),
        "connection error must preserve cause at discovery without fallback: {closed_stderr}"
    );

    // Discovery timeout must fail closed without falling back to /api/v1/auth/whoami.
    let timeout_whoami_hits = Arc::new(AtomicUsize::new(0));
    let timeout_whoami_hits_clone = Arc::clone(&timeout_whoami_hits);
    let slow_url = start_server(
        Router::new()
            .route(
                "/.well-known/pandar",
                get(|| async {
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    StatusCode::OK
                }),
            )
            .route(
                "/api/v1/auth/whoami",
                get(move || {
                    let hits = Arc::clone(&timeout_whoami_hits_clone);
                    async move {
                        hits.fetch_add(1, Ordering::SeqCst);
                        StatusCode::OK
                    }
                }),
            ),
    )
    .await;
    let out = run_pandar(
        &[
            "login",
            "--server",
            &slow_url,
            "--api-key-stdin",
            "--config",
            config_str,
        ],
        Some(&admin_key),
        &[],
    );
    assert!(!out.status.success(), "discovery timeout must fail closed");
    assert_eq!(
        timeout_whoami_hits.load(Ordering::SeqCst),
        0,
        "discovery timeout must not fall back to /api/v1/auth/whoami"
    );
    let timeout_stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        timeout_stderr.contains("/.well-known/pandar")
            && timeout_stderr.contains("Caused by:")
            && !timeout_stderr.contains(&admin_key),
        "discovery timeout must preserve error chain without leaking secret: {timeout_stderr}"
    );
    steps.push(E2eStep {
        name: "reject_discovery_redirects_errors_and_timeouts",
        passed: true,
        detail:
            "verified 302/500/204/connection-refused/timeout fail closed with 0 whoami/trap hits"
                .to_owned(),
    });

    // 4. Stdin API key login + file permission check + stdout redaction.
    let out = run_pandar(
        &[
            "login",
            "--server",
            &hub_url,
            "--api-key-stdin",
            "--config",
            config_str,
        ],
        Some(&format!("{admin_key}\n")),
        &[],
    );
    assert!(
        out.status.success(),
        "login failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains(&admin_key), "login stdout leaked secret");
    let login_json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(login_json["tenant_slug"], "acme");
    assert_eq!(login_json["effective_role"], "tenant_admin");
    assert_eq!(login_json["auth_kind"], "api_key");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&config_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "config file must be 0600");
    }
    steps.push(E2eStep {
        name: "login_api_key_stdin_and_permissions",
        passed: true,
        detail: "saved 0600 profile without printing secret".to_owned(),
    });

    // 5. Reject insecure permissions and symlinks on Unix, then restore 0600.
    #[cfg(unix)]
    {
        use std::os::unix::fs::{PermissionsExt, symlink};
        fs::set_permissions(&config_path, fs::Permissions::from_mode(0o644)).unwrap();
        let bad_perm = run_pandar(&["client", "--config", config_str, "whoami"], None, &[]);
        assert!(!bad_perm.status.success(), "0644 config must be rejected");
        assert!(
            String::from_utf8_lossy(&bad_perm.stderr).contains("chmod 600"),
            "error should include chmod 600 hint"
        );
        fs::set_permissions(&config_path, fs::Permissions::from_mode(0o600)).unwrap();

        let symlink_cfg = temp.path().join("symlink-cli.json");
        symlink(&config_path, &symlink_cfg).unwrap();
        let sym_out = run_pandar(
            &[
                "client",
                "--config",
                symlink_cfg.to_str().unwrap(),
                "whoami",
            ],
            None,
            &[],
        );
        assert!(!sym_out.status.success(), "symlink config must be rejected");
    }
    steps.push(E2eStep {
        name: "config_symlink_and_permission_guards",
        passed: true,
        detail: "rejected 0644 config and symlink config".to_owned(),
    });

    // 6. Concurrent logins to two distinct profiles must preserve both profiles.
    let (h1, h2) = (
        std::thread::spawn({
            let hub_url = hub_url.clone();
            let config_str = config_str.to_owned();
            let key = admin_key.clone();
            move || {
                run_pandar(
                    &[
                        "login",
                        "--server",
                        &hub_url,
                        "--api-key-stdin",
                        "--profile",
                        "concurrent-a",
                        "--config",
                        &config_str,
                    ],
                    Some(&key),
                    &[],
                )
            }
        }),
        std::thread::spawn({
            let hub_url = hub_url.clone();
            let config_str = config_str.to_owned();
            let key = operator_key.clone();
            move || {
                run_pandar(
                    &[
                        "login",
                        "--server",
                        &hub_url,
                        "--api-key-stdin",
                        "--profile",
                        "concurrent-b",
                        "--config",
                        &config_str,
                    ],
                    Some(&key),
                    &[],
                )
            }
        }),
    );
    assert!(h1.join().unwrap().status.success());
    assert!(h2.join().unwrap().status.success());
    let saved: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    assert!(saved["profiles"]["concurrent-a"].is_object());
    assert!(saved["profiles"]["concurrent-b"].is_object());
    steps.push(E2eStep {
        name: "concurrent_profile_writes_locked",
        passed: true,
        detail: "both concurrent profiles persisted cleanly".to_owned(),
    });

    // 7. Structured client subcommands + real multipart jobs create + request + security rejections.
    let whoami_out = run_pandar(
        &[
            "client",
            "--config",
            config_str,
            "--profile",
            "default",
            "whoami",
        ],
        None,
        &[],
    );
    assert!(whoami_out.status.success());
    let printers_out = run_pandar(
        &[
            "client",
            "--config",
            config_str,
            "--profile",
            "default",
            "printers",
            "list",
        ],
        None,
        &[],
    );
    assert!(printers_out.status.success());
    let printers_json: serde_json::Value = serde_json::from_slice(&printers_out.stdout).unwrap();
    assert_eq!(printers_json["printers"][0]["id"], printer.id);

    let artifact_file = temp.path().join("benchy.3mf");
    fs::write(&artifact_file, b"PK\x03\x04fake-3mf-payload").unwrap();
    let create_job_out = run_pandar(
        &[
            "client",
            "--config",
            config_str,
            "--profile",
            "concurrent-b",
            "jobs",
            "create",
            &printer.id,
            "--file",
            artifact_file.to_str().unwrap(),
            "--plate",
            "1",
        ],
        None,
        &[],
    );
    assert!(
        create_job_out.status.success(),
        "operator jobs create failed: {}",
        String::from_utf8_lossy(&create_job_out.stderr)
    );
    let created_job: serde_json::Value = serde_json::from_slice(&create_job_out.stdout).unwrap();
    let job_id = created_job["id"].as_str().unwrap().to_owned();

    // Operator profile cannot create tokens (403 -> non-zero exit).
    let forbidden_tok = run_pandar(
        &[
            "client",
            "--config",
            config_str,
            "--profile",
            "concurrent-b",
            "tokens",
            "create",
            "--name",
            "nope",
            "--scopes",
            "viewer",
        ],
        None,
        &[],
    );
    assert!(!forbidden_tok.status.success());

    // Admin profile can create, list, rotate, and revoke tokens via CLI.
    let create_tok = run_pandar(
        &[
            "client",
            "--config",
            config_str,
            "--profile",
            "default",
            "tokens",
            "create",
            "--name",
            "cli-minted",
            "--scopes",
            "viewer,operator,viewer",
        ],
        None,
        &[],
    );
    assert!(create_tok.status.success());
    let created_tok: serde_json::Value = serde_json::from_slice(&create_tok.stdout).unwrap();
    assert_eq!(
        created_tok["tenant_token"]["scopes"],
        serde_json::json!(["operator", "viewer"])
    );
    let minted_id = created_tok["tenant_token"]["id"].as_str().unwrap();
    let revoke_tok = run_pandar(
        &[
            "client",
            "--config",
            config_str,
            "--profile",
            "default",
            "tokens",
            "revoke",
            minted_id,
        ],
        None,
        &[],
    );
    assert!(revoke_tok.status.success());

    // Generic request + public --no-auth + path traversal & forbidden header rejection.
    let raw_job = run_pandar(
        &[
            "client",
            "--config",
            config_str,
            "--profile",
            "default",
            "request",
            "GET",
            &format!("/jobs/{job_id}"),
        ],
        None,
        &[],
    );
    assert!(raw_job.status.success());
    let public_status = run_pandar(
        &[
            "client",
            "--server",
            &hub_url,
            "request",
            "GET",
            "/api/v1/auth/status",
            "--no-auth",
            "--include",
        ],
        None,
        &[],
    );
    assert!(public_status.status.success());
    for bad_path in [
        "/",
        "//evil.example/printers",
        "/printers//123",
        "../tenants",
        "/printers/%2e./jobs",
        "/printers/a%2fb",
        "https://evil.example/x",
        "/printers?foo=1",
    ] {
        let res = run_pandar(
            &[
                "client",
                "--config",
                config_str,
                "--profile",
                "default",
                "request",
                "GET",
                bad_path,
            ],
            None,
            &[],
        );
        assert!(!res.status.success(), "bad path `{bad_path}` must fail");
    }
    let bad_hdr = run_pandar(
        &[
            "client",
            "--config",
            config_str,
            "--profile",
            "default",
            "request",
            "GET",
            "/printers",
            "-H",
            "Authorization: Bearer override",
        ],
        None,
        &[],
    );
    assert!(
        !bad_hdr.status.success(),
        "Authorization header override must fail"
    );
    steps.push(E2eStep {
        name: "client_commands_multipart_and_request_guards",
        passed: true,
        detail: "printers/jobs create/tokens/request and all path+header guards verified"
            .to_owned(),
    });

    // 8. Stateless CI env var mode (no config file, no PANDAR_TENANT_ID -> auto whoami).
    let ci_out = run_pandar(
        &["client", "agents", "list"],
        None,
        &[
            ("PANDAR_SERVER", &hub_url),
            ("PANDAR_API_KEY", &operator_key),
        ],
    );
    assert!(
        ci_out.status.success(),
        "stateless CI mode failed: {}",
        String::from_utf8_lossy(&ci_out.stderr)
    );
    let ci_agents: serde_json::Value = serde_json::from_slice(&ci_out.stdout).unwrap();
    assert_eq!(ci_agents["agents"][0]["id"], agent.id.to_string());
    steps.push(E2eStep {
        name: "stateless_ci_env_auto_tenant_discovery",
        passed: true,
        detail: "resolved tenant_id via whoami with zero disk config".to_owned(),
    });

    // 9. Browser PKCE login E2E + callback hardening + logout remote session revocation.
    let mut browser_child = Command::new(PANDAR_BIN)
        .args([
            "login",
            "--server",
            "http://127.0.0.1:3000",
            "--hub-url",
            &hub_url,
            "--no-browser",
            "--profile",
            "browser",
            "--config",
            config_str,
        ])
        .env_remove("PANDAR_SERVER")
        .env_remove("PANDAR_HUB_URL")
        .env_remove("PANDAR_API_KEY")
        .env_remove("PANDAR_TENANT_ID")
        .env_remove("PANDAR_PROFILE")
        .env_remove("PANDAR_CLI_CONFIG")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn browser login");

    // Read stderr until the authorize URL line appears.
    use std::io::{BufRead, BufReader};
    let stderr = browser_child.stderr.take().unwrap();
    let mut reader = BufReader::new(stderr);
    let mut authorize_url = String::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap() == 0 {
            break;
        }
        if line
            .trim()
            .starts_with("http://127.0.0.1:3000/cli-sign-in?")
        {
            authorize_url = line.trim().to_owned();
            break;
        }
    }
    assert!(
        !authorize_url.is_empty(),
        "did not receive browser authorize URL on stderr"
    );
    let parsed_auth = reqwest::Url::parse(&authorize_url).unwrap();
    let mut callback_url = None;
    let mut code_challenge = None;
    let mut expected_state = None;
    for (k, v) in parsed_auth.query_pairs() {
        match k.as_ref() {
            "redirect_url" => callback_url = Some(v.into_owned()),
            "code_challenge" => code_challenge = Some(v.into_owned()),
            "state" => expected_state = Some(v.into_owned()),
            _ => {}
        }
    }
    let callback_url = callback_url.unwrap();
    let code_challenge = code_challenge.unwrap();
    let expected_state = expected_state.unwrap();

    let http = reqwest::Client::new();
    // Exercise callback server edge cases before delivering the real ticket.
    assert_eq!(
        http.post(&callback_url).send().await.unwrap().status(),
        reqwest::StatusCode::METHOD_NOT_ALLOWED
    );
    let wrong_path = callback_url.replace("/callback", "/other");
    assert_eq!(
        http.get(&wrong_path).send().await.unwrap().status(),
        reqwest::StatusCode::NOT_FOUND
    );
    assert_eq!(
        http.get(format!("{callback_url}?ticket=fake&state=wrong"))
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::BAD_REQUEST
    );

    // Mint a real CLI login ticket on Hub and complete the browser callback.
    let ticket = state
        .auth()
        .create_cli_login_ticket_with_audit(
            tenant.id,
            Some(user.id.clone()),
            &callback_url,
            code_challenge,
            "2099-01-01T00:00:00Z".to_owned(),
            AuditActor::user(user.id.clone()),
        )
        .await
        .unwrap()
        .plaintext_ticket;
    let cb_ok = http
        .get(format!(
            "{callback_url}?ticket={ticket}&state={expected_state}"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(cb_ok.status(), reqwest::StatusCode::OK);

    let browser_out = browser_child.wait_with_output().unwrap();
    assert!(
        browser_out.status.success(),
        "browser login did not exit cleanly"
    );
    let browser_json: serde_json::Value = serde_json::from_slice(&browser_out.stdout).unwrap();
    assert_eq!(browser_json["auth_kind"], "browser_session");
    assert_eq!(browser_json["user_name"], "Alice Maker");
    assert_eq!(browser_json["effective_role"], "operator");

    let saved_after: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    let browser_token = saved_after["profiles"]["browser"]["token"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(browser_token.starts_with("pandar_cli_"));

    // Logout browser profile -> revokes remote session on Hub.
    let logout_browser = run_pandar(
        &["logout", "--profile", "browser", "--config", config_str],
        None,
        &[],
    );
    assert!(logout_browser.status.success());
    let logout_browser_json: serde_json::Value =
        serde_json::from_slice(&logout_browser.stdout).unwrap();
    assert_eq!(logout_browser_json["revoked_remote_session"], true);
    let post_logout_whoami = http
        .get(format!("{hub_url}/api/v1/auth/whoami"))
        .bearer_auth(&browser_token)
        .send()
        .await
        .unwrap();
    assert_eq!(
        post_logout_whoami.status(),
        reqwest::StatusCode::UNAUTHORIZED
    );

    // Logout API key profile -> removes locally, does NOT revoke shared API key on Hub.
    let logout_apikey = run_pandar(
        &["logout", "--profile", "default", "--config", config_str],
        None,
        &[],
    );
    assert!(logout_apikey.status.success());
    let logout_apikey_json: serde_json::Value =
        serde_json::from_slice(&logout_apikey.stdout).unwrap();
    assert_eq!(logout_apikey_json["revoked_remote_session"], false);
    let still_valid = http
        .get(format!("{hub_url}/api/v1/auth/whoami"))
        .bearer_auth(&admin_key)
        .send()
        .await
        .unwrap();
    assert_eq!(still_valid.status(), reqwest::StatusCode::OK);
    steps.push(E2eStep {
        name: "browser_pkce_login_and_logout_semantics",
        passed: true,
        detail: "verified 405/404/400/200 callback, PKCE exchange, remote CLI revoke, and API-key preservation".to_owned(),
    });

    write_artifact(&E2eArtifact {
        schema: "pandar.e2e.cli-api-key-flow.v1",
        steps,
    });
}
