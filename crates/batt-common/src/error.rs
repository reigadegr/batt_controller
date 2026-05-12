use std::fmt;

/// 充电控制工具统一错误类型
#[derive(Debug, thiserror::Error)]
pub enum BattError {
    /// sysfs/proc 文件 I/O 操作失败
    #[error("sysfs I/O error: {0}")]
    Sysfs(String),

    /// 配置文件解析失败
    #[error("config parse error: {0}")]
    Config(String),

    /// bcc_parms 解析失败
    #[error("bcc_parms parse error")]
    BccParmsParse,

    /// sysfs 节点打开失败（路径）
    #[error("failed to open sysfs node: {path}")]
    SysfsOpen { path: String },

    /// 参数无效
    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    /// 线程启动失败
    #[error("thread spawn failed: {0}")]
    ThreadSpawn(String),

    /// 通用错误
    #[error("{0}")]
    Other(String),
}

/// 从 `io::Error` 转换
impl From<std::io::Error> for BattError {
    fn from(e: std::io::Error) -> Self {
        Self::Sysfs(e.to_string())
}
}

/// 从 `String` 转换（用于 CLI 错误消息等）
impl From<String> for BattError {
    fn from(s: String) -> Self {
        Self::Other(s)
    }
}

/// 从 `fmt::Error` 转换
impl From<fmt::Error> for BattError {
    fn from(e: fmt::Error) -> Self {
        Self::Other(e.to_string())
    }
}

/// 从 `batt_sysfs` 的 `i32` 错误码转换
impl From<i32> for BattError {
    fn from(code: i32) -> Self {
        Self::Sysfs(format!("sysfs error code: {code}"))
    }
}
