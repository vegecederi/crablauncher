use chrono::Local;
use std::collections::VecDeque;

pub const MAX_CONSOLE_LINES: usize = 4000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
    Debug,
}

impl LogLevel {
    pub fn code(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
            Self::Debug => "debug",
        }
    }
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: LogLevel,
    pub message: String,
}

/// Buffer circular limitado para impedir consumo excessivo de memória com logs extensos.
#[derive(Debug, Clone)]
pub struct ConsoleBuffer {
    entries: VecDeque<LogEntry>,
    capacity: usize,
    stop_on_overflow: bool,
    overflowed: bool,
}

impl ConsoleBuffer {
    pub fn new(capacity: usize) -> Self {
        Self::with_policy(capacity, false)
    }

    pub fn with_policy(capacity: usize, stop_on_overflow: bool) -> Self {
        Self {
            entries: VecDeque::with_capacity(capacity.min(MAX_CONSOLE_LINES)),
            capacity: capacity.max(1),
            stop_on_overflow,
            overflowed: false,
        }
    }

    pub fn set_policy(&mut self, capacity: usize, stop_on_overflow: bool) {
        self.capacity = capacity.max(1);
        self.stop_on_overflow = stop_on_overflow;
        if !stop_on_overflow {
            self.overflowed = false;
            while self.entries.len() > self.capacity {
                self.entries.pop_front();
            }
        }
    }

    pub fn is_overflowed(&self) -> bool {
        self.overflowed
    }

    pub fn push(&mut self, level: LogLevel, message: String) -> LogEntry {
        let entry = LogEntry {
            timestamp: Local::now().format("%H:%M:%S").to_string(),
            level,
            message,
        };
        if self.entries.len() >= self.capacity {
            if self.stop_on_overflow {
                self.overflowed = true;
                return entry;
            }
            self.entries.pop_front();
        }
        self.entries.push_back(entry.clone());
        entry
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.overflowed = false;
    }

    pub fn entries(&self) -> &VecDeque<LogEntry> {
        &self.entries
    }

    pub fn to_plain_text(&self) -> String {
        self.entries
            .iter()
            .map(|e| format!("[{}] {}", e.timestamp, e.message))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Detecta o nível de log aproximado a partir de uma linha de saída do Minecraft/JVM.
pub fn infer_log_level(line: &str, is_stderr: bool) -> LogLevel {
    if is_stderr || line.contains("/ERROR]") || line.contains("Exception") || line.contains("FATAL")
    {
        LogLevel::Error
    } else if line.contains("/WARN]") || line.contains("WARNING") {
        LogLevel::Warn
    } else if line.contains("/DEBUG]") {
        LogLevel::Debug
    } else {
        LogLevel::Info
    }
}

/// Remove tokens e credenciais sensíveis antes de exibir, copiar ou persistir logs.
pub fn redact_sensitive_text(text: &str, secrets: &[String]) -> String {
    let mut sanitized = text.to_string();
    for secret in secrets {
        if !secret.is_empty() && sanitized.contains(secret) {
            sanitized = sanitized.replace(secret, "[REDACTED]");
        }
    }
    sanitized
}
