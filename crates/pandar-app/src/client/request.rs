use anyhow::{Context, Result, bail};
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use std::{
    fs,
    io::{self, Read},
    path::Path,
};

use crate::cli_auth::secret::SecretString;

const FORBIDDEN_REQUEST_HEADERS: &[&str] = &["authorization", "host", "cookie"];
const REDACTED_RESPONSE_HEADERS: &[&str] = &[
    "set-cookie",
    "authorization",
    "www-authenticate",
    "proxy-authenticate",
];

pub(crate) fn validate_and_resolve_path(raw: &str, tenant_id: Option<&str>) -> Result<String> {
    if raw.is_empty() {
        bail!("API path must not be empty");
    }
    if raw
        .bytes()
        .any(|byte| byte < 0x20 || byte == 0x7f || byte.is_ascii_whitespace())
    {
        bail!("API path must not contain whitespace or control characters");
    }
    if raw.contains(':') {
        bail!("API path must be a relative path, not an absolute URL with a scheme");
    }
    if raw.contains('\\') {
        bail!("API path must not contain backslashes");
    }
    if raw.contains('?') {
        bail!("API path must not contain `?`; pass query parameters with `-q KEY=VALUE`");
    }
    if raw.contains('#') {
        bail!("API path must not contain `#` fragments");
    }

    let trimmed = raw
        .strip_prefix('/')
        .unwrap_or(raw)
        .strip_suffix('/')
        .unwrap_or_else(|| raw.strip_prefix('/').unwrap_or(raw));
    if trimmed.is_empty() {
        bail!("API path must not be empty or bare `/`");
    }

    for segment in trimmed.split('/') {
        if segment.is_empty() {
            bail!("API path must not contain empty or consecutive slash segments");
        }
        let decoded = strict_percent_decode(segment)?;
        if decoded == "." || decoded == ".." {
            bail!("API path must not contain `.` or `..` traversal segments");
        }
        if decoded.contains('/') || decoded.contains('\\') {
            bail!("API path segment must not decode to path separators");
        }
        if decoded.bytes().any(|byte| byte < 0x20 || byte == 0x7f) {
            bail!("API path segment must not decode to control characters");
        }
    }

    if trimmed.starts_with("api/") {
        return Ok(format!("/{trimmed}"));
    }
    let Some(tenant_id) = tenant_id else {
        bail!("tenant_id is required for tenant-scoped path `{raw}`");
    };
    validate_path_segment(tenant_id, "tenant_id")?;
    Ok(format!("/api/v1/tenants/{tenant_id}/{trimmed}"))
}

pub(crate) fn validate_path_segment(segment: &str, label: &str) -> Result<()> {
    if segment.is_empty()
        || segment.contains('/')
        || segment.contains('\\')
        || segment.contains('?')
        || segment.contains('#')
        || segment.contains(':')
        || segment
            .bytes()
            .any(|byte| byte < 0x20 || byte == 0x7f || byte.is_ascii_whitespace())
    {
        bail!("{label} must be a valid single path segment");
    }
    let decoded = strict_percent_decode(segment)?;
    if decoded == "."
        || decoded == ".."
        || decoded.contains('/')
        || decoded.contains('\\')
        || decoded.bytes().any(|byte| byte < 0x20 || byte == 0x7f)
    {
        bail!("{label} must be a valid single path segment");
    }
    Ok(())
}

fn strict_percent_decode(input: &str) -> Result<String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                bail!("API path contains an incomplete percent-escape");
            }
            let hi = hex_nibble(bytes[i + 1])?;
            let lo = hex_nibble(bytes[i + 2])?;
            out.push((hi << 4) | lo);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).context("API path contains invalid UTF-8 after percent-decoding")
}

fn hex_nibble(byte: u8) -> Result<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => bail!("API path contains an invalid percent-escape"),
    }
}

pub(crate) fn parse_header_arg(raw: &str) -> Result<(HeaderName, HeaderValue)> {
    let Some((name, value)) = raw.split_once(':') else {
        bail!("invalid header `{raw}`; expected `NAME: VALUE`");
    };
    let name_trimmed = name.trim();
    if name_trimmed.is_empty() {
        bail!("header name must not be empty");
    }
    let lower = name_trimmed.to_ascii_lowercase();
    if FORBIDDEN_REQUEST_HEADERS.contains(&lower.as_str()) {
        bail!(
            "header `{name_trimmed}` cannot be set via -H; use CLI auth options or --no-auth instead"
        );
    }
    let header_name = HeaderName::from_bytes(name_trimmed.as_bytes())
        .with_context(|| format!("invalid HTTP header name `{name_trimmed}`"))?;
    let header_value = HeaderValue::from_str(value.trim())
        .with_context(|| format!("invalid HTTP header value for `{name_trimmed}`"))?;
    Ok((header_name, header_value))
}

pub(crate) fn parse_query_arg(raw: &str) -> Result<(String, String)> {
    let Some((key, value)) = raw.split_once('=') else {
        bail!("invalid query parameter `{raw}`; expected `KEY=VALUE`");
    };
    if key.is_empty() {
        bail!("query parameter key must not be empty");
    }
    Ok((key.to_owned(), value.to_owned()))
}

pub(crate) fn read_body_input(
    data: Option<&str>,
    data_file: Option<&Path>,
) -> Result<Option<Vec<u8>>> {
    match (data, data_file) {
        (Some(_), Some(_)) => bail!("--data and --data-file cannot be used together"),
        (Some(inline), None) => Ok(Some(inline.as_bytes().to_vec())),
        (None, Some(path)) if path == Path::new("-") => {
            let mut buf = Vec::new();
            io::stdin()
                .read_to_end(&mut buf)
                .context("failed to read request body from stdin")?;
            Ok(Some(buf))
        }
        (None, Some(path)) => {
            let bytes = fs::read(path)
                .with_context(|| format!("failed to read request body file {}", path.display()))?;
            Ok(Some(bytes))
        }
        (None, None) => Ok(None),
    }
}

pub(crate) struct RawRequestParams<'a> {
    pub http: &'a reqwest::Client,
    pub method: reqwest::Method,
    pub url: &'a str,
    pub token: Option<&'a SecretString>,
    pub headers: &'a [(HeaderName, HeaderValue)],
    pub query: &'a [(String, String)],
    pub body: Option<Vec<u8>>,
    pub include_headers: bool,
}

pub(crate) async fn execute_and_print(params: RawRequestParams<'_>) -> Result<()> {
    let mut header_map = HeaderMap::new();
    for (name, value) in params.headers {
        header_map.append(name.clone(), value.clone());
    }
    if params.body.is_some() && !header_map.contains_key(CONTENT_TYPE) {
        header_map.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    }

    let mut target_url =
        reqwest::Url::parse(params.url).with_context(|| format!("invalid URL `{}`", params.url))?;
    if !params.query.is_empty() {
        let mut pairs = target_url.query_pairs_mut();
        for (key, value) in params.query {
            pairs.append_pair(key, value);
        }
    }

    let mut request = params
        .http
        .request(params.method.clone(), target_url)
        .headers(header_map);
    if let Some(token) = params.token {
        request = request.bearer_auth(token.expose());
    }
    if let Some(body) = params.body {
        request = request.body(body);
    }

    let response = request
        .send()
        .await
        .with_context(|| format!("HTTP {} {} failed", params.method, params.url))?;
    render_response(response, params.include_headers).await
}

pub(crate) async fn render_response(
    response: reqwest::Response,
    include_headers: bool,
) -> Result<()> {
    let status = response.status();
    if include_headers {
        eprintln!("HTTP {}", status);
        for (name, value) in response.headers() {
            let shown = redact_response_header(name.as_str(), value.to_str().unwrap_or("<binary>"));
            eprintln!("{}: {}", name.as_str(), shown);
        }
        eprintln!();
    }
    let body = response
        .text()
        .await
        .context("failed to read HTTP response body")?;
    if !body.is_empty() {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&body) {
            println!("{}", serde_json::to_string_pretty(&parsed)?);
        } else {
            println!("{body}");
        }
    }
    if !status.is_success() {
        bail!("request failed with HTTP {status}");
    }
    Ok(())
}

pub(crate) fn redact_response_header<'a>(name: &str, value: &'a str) -> &'a str {
    let lower = name.to_ascii_lowercase();
    if REDACTED_RESPONSE_HEADERS.contains(&lower.as_str()) {
        "[redacted]"
    } else {
        value
    }
}
