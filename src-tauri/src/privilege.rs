use anyhow::Result;

/// 检测当前进程是否以管理员/root 权限运行
pub fn is_elevated() -> bool {
    #[cfg(target_os = "windows")]
    return windows_priv::is_elevated();

    #[cfg(target_os = "linux")]
    return linux_priv::is_elevated();

    #[cfg(target_os = "macos")]
    return macos_priv::is_elevated();

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    false
}

/// 触发系统原生提权，以管理员/root 权限执行指定的 Sidecar 程序
pub fn elevate_and_execute(sidecar_path: &str, args: &[&str]) -> Result<()> {
    #[cfg(target_os = "windows")]
    return windows_priv::elevate_and_execute(sidecar_path, args);

    #[cfg(target_os = "linux")]
    return linux_priv::elevate_and_execute(sidecar_path, args);

    #[cfg(target_os = "macos")]
    return macos_priv::elevate_and_execute(sidecar_path, args);

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    Err(anyhow!("不支持的操作系统"))
}

// ===== Windows 平台实现 =====
#[cfg(target_os = "windows")]
pub mod windows_priv {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use anyhow::{Result, anyhow};
    use std::process::Command;

    /// 检测当前进程是否以管理员身份运行
    pub fn is_elevated() -> bool {
        unsafe {
            let mut token: HANDLE = HANDLE(std::ptr::null_mut());
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
                return false;
            }

            let mut elevation = TOKEN_ELEVATION::default();
            let mut ret_len = 0;

            let success = GetTokenInformation(
                token,
                TokenElevation,
                Some(&mut elevation as *mut _ as *mut _),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut ret_len,
            ).is_ok();

            success && elevation.TokenIsElevated != 0
        }
    }

    /// 触发 UAC 提权，启动 Sidecar 进程
    pub fn elevate_and_execute(sidecar_path: &str, args: &[&str]) -> Result<()> {
        // Use Windows built-in runas via ShellExecute
        let status = Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "Start-Process -FilePath '{}' -ArgumentList '{}' -Verb RunAs -Wait",
                    sidecar_path,
                    args.join(" ")
                ),
            ])
            .status();

        match status {
            Ok(s) if s.success() => Ok(()),
            Ok(_) => Err(anyhow!("UAC elevation failed or was denied by user")),
            Err(e) => Err(anyhow!("Failed to launch elevated process: {}", e)),
        }
    }
}

// ===== Linux 平台实现 =====
#[cfg(target_os = "linux")]
pub mod linux_priv {
    use nix::unistd::geteuid;
    use std::process::Command;
    use anyhow::Result;

    pub fn is_elevated() -> bool {
        geteuid().is_root()
    }

    pub fn elevate_and_execute(sidecar_path: &str, args: &[&str]) -> Result<()> {
        // 优先尝试 pkexec（图形化 Polkit），失败则回退到 sudo
        let status = Command::new("pkexec")
            .arg(sidecar_path)
            .args(args)
            .status();

        if status.is_err() {
            Command::new("sudo")
                .arg(sidecar_path)
                .args(args)
                .status()?;
        }
        Ok(())
    }
}

// ===== macOS 平台实现 =====
#[cfg(target_os = "macos")]
pub mod macos_priv {
    use nix::unistd::geteuid;
    use std::process::Command;
    use anyhow::Result;

    pub fn is_elevated() -> bool {
        geteuid().is_root()
    }

    pub fn elevate_and_execute(sidecar_path: &str, args: &[&str]) -> Result<()> {
        // 使用 AppleScript 触发 macOS 原生的密码输入框
        let cmd = format!(
            "do shell script \"'{}' {}\" with administrator privileges",
            sidecar_path,
            args.join(" ")
        );

        Command::new("osascript")
            .arg("-e")
            .arg(cmd)
            .status()?;
        Ok(())
    }
}
