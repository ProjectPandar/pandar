#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NativeTarget {
    LinuxAmd64,
    MacosAmd64,
    MacosArm64,
    WindowsAmd64,
}

impl NativeTarget {
    pub fn label(self) -> &'static str {
        match self {
            Self::LinuxAmd64 => "linux-amd64",
            Self::MacosAmd64 => "macos-amd64",
            Self::MacosArm64 => "macos-arm64",
            Self::WindowsAmd64 => "windows-amd64",
        }
    }
}

pub(crate) fn validate_current_host(label: &str) -> Result<NativeTarget, String> {
    validate_host(label, std::env::consts::OS, std::env::consts::ARCH)
}

fn validate_host(label: &str, os: &str, arch: &str) -> Result<NativeTarget, String> {
    let target = match label {
        "linux-amd64" => NativeTarget::LinuxAmd64,
        "macos-amd64" => NativeTarget::MacosAmd64,
        "macos-arm64" => NativeTarget::MacosArm64,
        "windows-amd64" => NativeTarget::WindowsAmd64,
        _ => return Err(format!("unsupported native release label {label}")),
    };
    let expected = match target {
        NativeTarget::LinuxAmd64 => ("linux", "x86_64"),
        NativeTarget::MacosAmd64 => ("macos", "x86_64"),
        NativeTarget::MacosArm64 => ("macos", "aarch64"),
        NativeTarget::WindowsAmd64 => ("windows", "x86_64"),
    };
    if (os, arch) != expected {
        return Err(format!(
            "release label {label} requires native host {}/{}, got {os}/{arch}",
            expected.0, expected.1
        ));
    }
    Ok(target)
}
