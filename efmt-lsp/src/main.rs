//! A minimal LSP server for efmt.
//!
//! Listens on stdin/stdout, handles `textDocument/formatting` only.

use efmt_core::items::ModuleOrConfig;
use serde::Deserialize;
use std::io::{BufRead, BufReader, Write};

fn main() {
    let mut stdin = BufReader::new(std::io::stdin());
    let mut stdout = std::io::stdout();

    loop {
        let content = match read_message(&mut stdin) {
            Ok(Some(c)) => c,
            Ok(None) => break,
            Err(e) => {
                eprintln!("efmt-lsp: read error: {e}");
                break;
            }
        };

        let mut request: RequestMessage = match serde_json::from_str(&content) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("efmt-lsp: parse error: {e}");
                continue;
            }
        };

        match request.method.as_str() {
            "initialize" => {
                let result = serde_json::json!({
                    "capabilities": {
                        "documentFormattingProvider": true,
                        "textDocumentSync": { "openClose": true, "change": 1 }
                    },
                    "serverInfo": {
                        "name": "efmt-lsp",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                });
                write_response(&mut stdout, &request, result);
            }
            "initialized" | "$/setTrace" => {}
            "shutdown" => {
                write_response(&mut stdout, &request, serde_json::Value::Null);
            }
            "exit" => break,
            "textDocument/formatting" => {
                let params: FormattingParams =
                    serde_json::from_value(request.params.take().unwrap_or_default()).unwrap_or_default();
                let result = match efmt_core::format_text::<ModuleOrConfig>(&params.text) {
                    Ok(formatted) => {
                        let line_count = params.text.lines().count();
                        let last_line_len = params
                            .text
                            .lines()
                            .last()
                            .map(|l| l.len())
                            .unwrap_or(0);
                        serde_json::json!([{
                            "range": {
                                "start": { "line": 0, "character": 0 },
                                "end": { "line": line_count.saturating_sub(1) as u32, "character": last_line_len as u32 }
                            },
                            "newText": formatted
                        }])
                    }
                    Err(e) => {
                        eprintln!("efmt-lsp: format error: {e}");
                        serde_json::Value::Null
                    }
                };
                write_response(&mut stdout, &request, result);
            }
            _ => {}
        }
    }
}

#[derive(Debug, Deserialize)]
struct RequestMessage {
    #[allow(dead_code)]
    jsonrpc: Option<String>,
    id: Option<serde_json::Value>,
    method: String,
    #[serde(default)]
    params: Option<serde_json::Value>,
}

#[derive(Debug, Default, Deserialize)]
struct FormattingParams {
    #[serde(rename = "textDocument")]
    #[allow(dead_code)]
    text_document: serde_json::Value,
    text: String,
}

fn read_message<R: BufRead>(reader: &mut R) -> std::io::Result<Option<String>> {
    let mut content_length: Option<usize> = None;

    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let line = line.trim().to_string();
        if line.is_empty() {
            break;
        }
        if let Some(len) = line.strip_prefix("Content-Length: ") {
            content_length = Some(len.trim().parse().map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, e)
            })?);
        }
    }

    let len = content_length.ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "missing Content-Length")
    })?;

    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;
    Ok(Some(String::from_utf8_lossy(&buf).into_owned()))
}

fn write_response<W: Write>(writer: &mut W, request: &RequestMessage, result: serde_json::Value) {
    if let Some(id) = &request.id {
        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result
        });
        let body = serde_json::to_string(&response).unwrap_or_default();
        write!(writer, "Content-Length: {}\r\n\r\n{}", body.len(), body).ok();
        writer.flush().ok();
    }
}
