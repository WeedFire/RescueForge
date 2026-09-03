use std::process::{Command, Stdio, Child};
use std::io::{BufRead, BufReader, Write};
use anyhow::Result;

/// Sidecar 进程管理器
pub struct SidecarManager {
    process: Option<Child>,
    sidecar_path: String,
}

impl SidecarManager {
    pub fn new(sidecar_path: String) -> Self {
        Self {
            process: None,
            sidecar_path,
        }
    }

    /// 启动 Sidecar 进程
    pub fn launch(&mut self, args: &[&str]) -> Result<()> {
        let child = Command::new(&self.sidecar_path)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        self.process = Some(child);
        Ok(())
    }

    /// 向 Sidecar 发送 JSON 命令
    pub fn send_command(&mut self, command: &str) -> Result<()> {
        if let Some(ref mut child) = self.process {
            if let Some(ref mut stdin) = child.stdin {
                writeln!(stdin, "{}", command)?;
                stdin.flush()?;
            }
        }
        Ok(())
    }

    /// 读取 Sidecar 的输出行
    pub fn read_line(&mut self) -> Option<String> {
        if let Some(ref mut child) = self.process {
            if let Some(ref mut stdout) = child.stdout {
                let reader = BufReader::new(stdout);
                let mut lines = reader.lines();
                match lines.next() {
                    Some(Ok(line)) => return Some(line),
                    _ => return None,
                }
            }
        }
        None
    }

    /// 终止 Sidecar 进程
    pub fn kill(&mut self) -> Result<()> {
        if let Some(ref mut child) = self.process {
            child.kill()?;
            child.wait()?;
        }
        self.process = None;
        Ok(())
    }

    /// 检查 Sidecar 是否在运行
    pub fn is_running(&mut self) -> bool {
        if let Some(ref mut child) = self.process {
            match child.try_wait() {
                Ok(Some(_)) => false,
                Ok(None) => true,
                Err(_) => false,
            }
        } else {
            false
        }
    }
}
