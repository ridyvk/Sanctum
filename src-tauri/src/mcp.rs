use sanctum_core::{
    AttachmentRelation, BlockKind, BlockStatus, CreateBlockInput, SaveBlockInput, Vault,
};
use serde_json::{json, Value};
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub(crate) const MCP_ADDRESS: &str = "127.0.0.1:43991";
const MAX_HEADER_BYTES: usize = 64 * 1024;
const MAX_BODY_BYTES: usize = 1024 * 1024;
const LATEST_PROTOCOL_VERSION: &str = "2025-06-18";

pub(crate) type SharedVault = Arc<Mutex<Option<Arc<Vault>>>>;

pub(crate) fn start(vault: SharedVault) -> io::Result<()> {
    let listener = TcpListener::bind(MCP_ADDRESS)?;
    thread::Builder::new()
        .name("sanctum-mcp".into())
        .spawn(move || {
            for connection in listener.incoming() {
                match connection {
                    Ok(stream) => {
                        if let Err(error) = handle_connection(stream, &vault) {
                            eprintln!("Sanctum MCP request failed: {error}");
                        }
                    }
                    Err(error) => eprintln!("Sanctum MCP listener failed: {error}"),
                }
            }
        })?;
    Ok(())
}

fn handle_connection(mut stream: TcpStream, vault: &SharedVault) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;

    let request = match read_request(&mut stream) {
        Ok(request) => request,
        Err(error) => {
            write_response(
                &mut stream,
                "400 Bad Request",
                "application/json",
                json!({"error": error.to_string()}).to_string().as_bytes(),
            )?;
            return Ok(());
        }
    };

    if request.method == "OPTIONS" || request.origin.is_some() {
        return write_response(
            &mut stream,
            "403 Forbidden",
            "application/json",
            br#"{"error":"browser-origin requests are not allowed"}"#,
        );
    }
    if request.method == "GET" && request.path == "/health" {
        let body = json!({"service": "sanctum-mcp", "status": "ok"}).to_string();
        return write_response(&mut stream, "200 OK", "application/json", body.as_bytes());
    }
    if request.method != "POST" || request.path != "/mcp" {
        return write_response(
            &mut stream,
            "404 Not Found",
            "application/json",
            br#"{"error":"not found"}"#,
        );
    }
    if request
        .content_type
        .as_deref()
        .is_none_or(|value| !value.eq_ignore_ascii_case("application/json"))
    {
        return write_response(
            &mut stream,
            "415 Unsupported Media Type",
            "application/json",
            br#"{"error":"content-type must be application/json"}"#,
        );
    }

    let message: Value = match serde_json::from_slice(&request.body) {
        Ok(message) => message,
        Err(error) => {
            let body = rpc_error(Value::Null, -32700, format!("Invalid JSON: {error}"));
            return write_response(
                &mut stream,
                "200 OK",
                "application/json",
                body.to_string().as_bytes(),
            );
        }
    };

    match dispatch(&message, vault) {
        Some(response) => write_response(
            &mut stream,
            "200 OK",
            "application/json",
            response.to_string().as_bytes(),
        ),
        None => write_response(&mut stream, "202 Accepted", "text/plain", &[]),
    }
}

struct HttpRequest {
    method: String,
    path: String,
    content_type: Option<String>,
    origin: Option<String>,
    body: Vec<u8>,
}

fn read_request(stream: &mut TcpStream) -> io::Result<HttpRequest> {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 4096];
    let header_end = loop {
        let count = stream.read(&mut chunk)?;
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "request ended before headers",
            ));
        }
        bytes.extend_from_slice(&chunk[..count]);
        if bytes.len() > MAX_HEADER_BYTES + MAX_BODY_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "request is too large",
            ));
        }
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
        if bytes.len() > MAX_HEADER_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "request headers are too large",
            ));
        }
    };

    let headers = std::str::from_utf8(&bytes[..header_end]).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidData, "request headers are not UTF-8")
    })?;
    let mut lines = headers.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing request line"))?;
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing method"))?
        .to_owned();
    let path = request_parts
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing path"))?
        .to_owned();

    let mut content_length = 0;
    let mut content_type = None;
    let mut origin = None;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            content_length = value.parse::<usize>().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "invalid content length")
            })?;
        } else if name.eq_ignore_ascii_case("content-type") {
            content_type = Some(
                value
                    .split(';')
                    .next()
                    .unwrap_or(value)
                    .trim()
                    .to_owned(),
            );
        } else if name.eq_ignore_ascii_case("origin") {
            origin = Some(value.to_owned());
        }
    }
    if content_length > MAX_BODY_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "request body is too large",
        ));
    }

    while bytes.len() < header_end + content_length {
        let count = stream.read(&mut chunk)?;
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "request body is incomplete",
            ));
        }
        bytes.extend_from_slice(&chunk[..count]);
    }

    Ok(HttpRequest {
        method,
        path: path.split('?').next().unwrap_or(&path).to_owned(),
        content_type,
        origin,
        body: bytes[header_end..header_end + content_length].to_vec(),
    })
}

fn write_response(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
) -> io::Result<()> {
    let headers = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nMCP-Protocol-Version: {LATEST_PROTOCOL_VERSION}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(headers.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

fn dispatch(request: &Value, vault: &SharedVault) -> Option<Value> {
    let id = request.get("id").cloned();
    let method = match request.get("method").and_then(Value::as_str) {
        Some(method) => method,
        None => return Some(rpc_error(id.unwrap_or(Value::Null), -32600, "Invalid request")),
    };

    let id = id?;

    let result = match method {
        "initialize" => {
            let requested = request
                .pointer("/params/protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or(LATEST_PROTOCOL_VERSION);
            let protocol_version = match requested {
                "2024-11-05" | "2025-03-26" | "2025-06-18" => requested,
                _ => LATEST_PROTOCOL_VERSION,
            };
            json!({
                "protocolVersion": protocol_version,
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "sanctum", "version": "0.5.1"},
                "instructions": "Sanctumデスクトップで開いているVaultだけを扱います。編集前に最新版を取得し、rowVersionによる競合検知を必ず使ってください。削除と復元は提供しません。"
            })
        }
        "ping" => json!({}),
        "tools/list" => json!({"tools": tool_definitions()}),
        "tools/call" => {
            let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
            let name = params.get("name").and_then(Value::as_str);
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            match name {
                Some(name) => match execute_tool(name, &arguments, vault) {
                    Ok((message, structured)) => json!({
                        "content": [{"type": "text", "text": message}],
                        "structuredContent": structured,
                        "isError": false
                    }),
                    Err(error) => json!({
                        "content": [{"type": "text", "text": error}],
                        "isError": true
                    }),
                },
                None => {
                    return Some(rpc_error(id, -32602, "Tool name is required"));
                }
            }
        }
        _ => return Some(rpc_error(id, -32601, format!("Unknown method: {method}"))),
    };

    Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

fn rpc_error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {"code": code, "message": message.into()}
    })
}

fn tool_definitions() -> Vec<Value> {
    vec![
        tool(
            "sanctum_status",
            "Sanctum接続状態",
            "Sanctumデスクトップとの接続と、現在開いているVaultを確認します。他のSanctumツールより先に使ってください。",
            json!({"type": "object", "properties": {}, "additionalProperties": false}),
            true,
        ),
        tool(
            "sanctum_list_blocks",
            "仮説ブロック一覧",
            "開いているVaultの仮説ブロックを本文なしの要約一覧で返します。種類・状態で絞り込み、ページングできます。",
            json!({
                "type": "object",
                "properties": {
                    "kind": {"type": "string", "enum": ["Hypothesis", "Assumption", "Method", "Evidence"]},
                    "status": {"type": "string", "enum": ["Idea", "Developing", "Testing", "Supported", "Weakly Supported", "Rejected", "Archived"]},
                    "offset": {"type": "integer", "minimum": 0, "default": 0},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 100, "default": 50}
                },
                "additionalProperties": false
            }),
            true,
        ),
        tool(
            "sanctum_search",
            "Sanctumを検索",
            "タイトル、本文、研究ノート、タグ、添付ファイル名を全文検索します。",
            json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "minLength": 1},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 100, "default": 25}
                },
                "required": ["query"],
                "additionalProperties": false
            }),
            true,
        ),
        tool(
            "sanctum_get_block",
            "仮説ブロックを取得",
            "一つのブロックについて、現在の本文、履歴、添付、引用を取得します。編集前に必ず使ってください。",
            json!({
                "type": "object",
                "properties": {"blockId": {"type": "string", "minLength": 1}},
                "required": ["blockId"],
                "additionalProperties": false
            }),
            true,
        ),
        tool(
            "sanctum_get_graph",
            "研究グラフを取得",
            "ブロックと、それらを結ぶ研究上の関係を取得します。",
            json!({"type": "object", "properties": {}, "additionalProperties": false}),
            true,
        ),
        tool(
            "sanctum_create_block",
            "仮説ブロックを作成",
            "ユーザーが保存を求めた内容を、新しい履歴付きブロックとして作成します。",
            json!({
                "type": "object",
                "properties": {
                    "title": {"type": "string", "minLength": 1},
                    "bodyMarkdown": {"type": "string", "default": ""},
                    "researchNotesMarkdown": {"type": "string", "default": ""},
                    "kind": {"type": "string", "enum": ["Hypothesis", "Assumption", "Method", "Evidence"], "default": "Hypothesis"},
                    "status": {"type": "string", "enum": ["Idea", "Developing", "Testing", "Supported", "Weakly Supported", "Rejected", "Archived"], "default": "Idea"},
                    "tags": {"type": "array", "items": {"type": "string"}, "default": []},
                    "changeReason": {"type": "string", "minLength": 1}
                },
                "required": ["title", "changeReason"],
                "additionalProperties": false
            }),
            false,
        ),
        tool(
            "sanctum_update_block",
            "仮説ブロックを編集",
            "既存ブロックの指定フィールドだけを更新し、新しい不変履歴を追加します。直前に取得したrowVersionが必要です。競合時は上書きしません。",
            json!({
                "type": "object",
                "properties": {
                    "blockId": {"type": "string", "minLength": 1},
                    "expectedRowVersion": {"type": "integer", "minimum": 1},
                    "title": {"type": "string", "minLength": 1},
                    "bodyMarkdown": {"type": "string"},
                    "researchNotesMarkdown": {"type": "string"},
                    "kind": {"type": "string", "enum": ["Hypothesis", "Assumption", "Method", "Evidence"]},
                    "status": {"type": "string", "enum": ["Idea", "Developing", "Testing", "Supported", "Weakly Supported", "Rejected", "Archived"]},
                    "tags": {"type": "array", "items": {"type": "string"}},
                    "changeReason": {"type": "string", "minLength": 1}
                },
                "required": ["blockId", "expectedRowVersion", "changeReason"],
                "additionalProperties": false
            }),
            false,
        ),
        tool(
            "sanctum_attach_file",
            "ファイルを添付",
            "ユーザーが明示したローカルファイルを、指定ブロックのCASへコピーして添付します。パスを推測して使わないでください。",
            json!({
                "type": "object",
                "properties": {
                    "blockId": {"type": "string", "minLength": 1},
                    "sourcePath": {"type": "string", "minLength": 1},
                    "relation": {"type": "string", "enum": ["Supports", "Contradicts", "Background", "Method", "Dataset", "Reference", "Other"], "default": "Reference"},
                    "locator": {"type": "object", "default": {}}
                },
                "required": ["blockId", "sourcePath"],
                "additionalProperties": false
            }),
            false,
        ),
        tool(
            "sanctum_list_attachments",
            "添付一覧",
            "指定ブロックに保存されている添付ファイルのメタデータを取得します。",
            json!({
                "type": "object",
                "properties": {"blockId": {"type": "string", "minLength": 1}},
                "required": ["blockId"],
                "additionalProperties": false
            }),
            true,
        ),
        tool(
            "sanctum_read_text_attachment",
            "テキスト添付を読む",
            "Markdown、テキスト、CSV、JSON、BibTeX、LaTeXなどのテキスト添付を読みます。PDFや画像などのbinary fileは読みません。",
            json!({
                "type": "object",
                "properties": {
                    "attachmentId": {"type": "string", "minLength": 1},
                    "maxCharacters": {"type": "integer", "minimum": 1, "maximum": 200000, "default": 50000}
                },
                "required": ["attachmentId"],
                "additionalProperties": false
            }),
            true,
        ),
        tool(
            "sanctum_integrity_check",
            "整合性を確認",
            "開いているVaultのDB、履歴、ジャーナル、添付参照の整合性を検査します。状態は変更しません。",
            json!({"type": "object", "properties": {}, "additionalProperties": false}),
            true,
        ),
    ]
}

fn tool(name: &str, title: &str, description: &str, input_schema: Value, read_only: bool) -> Value {
    json!({
        "name": name,
        "title": title,
        "description": description,
        "inputSchema": input_schema,
        "annotations": {
            "readOnlyHint": read_only,
            "destructiveHint": false,
            "openWorldHint": false
        }
    })
}

fn execute_tool(
    name: &str,
    arguments: &Value,
    shared: &SharedVault,
) -> Result<(String, Value), String> {
    if name == "sanctum_status" {
        let vault = shared
            .lock()
            .map_err(|_| "Sanctumの接続状態を取得できない".to_owned())?
            .clone();
        return match vault {
            Some(vault) => {
                let summary = vault.summary().map_err(|error| error.to_string())?;
                Ok((
                    format!("「{}」に接続している", summary.name),
                    json!({"connected": true, "vault": summary}),
                ))
            }
            None => Ok((
                "Sanctumは起動しているが、Vaultが開かれていない".into(),
                json!({"connected": true, "vault": null}),
            )),
        };
    }

    let vault = active_vault(shared)?;
    match name {
        "sanctum_list_blocks" => {
            let kind = optional_string(arguments, "kind")?;
            let status = optional_string(arguments, "status")?;
            let offset = optional_usize(arguments, "offset")?.unwrap_or(0);
            let limit = optional_usize(arguments, "limit")?.unwrap_or(50).clamp(1, 100);
            let mut blocks = vault.list_blocks().map_err(|error| error.to_string())?;
            blocks.retain(|block| {
                kind.as_deref()
                    .is_none_or(|value| block.snapshot.kind.as_str() == value)
                    && status
                        .as_deref()
                        .is_none_or(|value| block.snapshot.status.as_str() == value)
            });
            blocks.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
            let total = blocks.len();
            let items = blocks
                .iter()
                .skip(offset)
                .take(limit)
                .map(|block| {
                    json!({
                        "id": block.snapshot.id,
                        "title": block.snapshot.title,
                        "kind": block.snapshot.kind,
                        "status": block.snapshot.status,
                        "tags": block.snapshot.tags,
                        "rowVersion": block.row_version,
                        "updatedAt": block.updated_at
                    })
                })
                .collect::<Vec<_>>();
            Ok((
                format!("{total}件中{}件のブロックを取得した", items.len()),
                json!({"items": items, "offset": offset, "limit": limit, "total": total}),
            ))
        }
        "sanctum_search" => {
            let query = required_string(arguments, "query")?;
            let limit = optional_usize(arguments, "limit")?.unwrap_or(25).clamp(1, 100);
            let hits = vault
                .search(&query, limit)
                .map_err(|error| error.to_string())?;
            Ok((
                format!("{}件見つかった", hits.len()),
                json!({"query": query, "hits": hits}),
            ))
        }
        "sanctum_get_block" => {
            let block_id = required_string(arguments, "blockId")?;
            let block = vault
                .get_block(&block_id)
                .map_err(|error| error.to_string())?;
            let versions = vault
                .versions(&block_id)
                .map_err(|error| error.to_string())?;
            let attachments = vault
                .attachments_for_block(&block_id)
                .map_err(|error| error.to_string())?;
            let citations = vault
                .citations_for_block(&block_id)
                .map_err(|error| error.to_string())?;
            Ok((
                format!("「{}」を取得した", block.snapshot.title),
                json!({
                    "block": block,
                    "versions": versions,
                    "attachments": attachments,
                    "citations": citations
                }),
            ))
        }
        "sanctum_get_graph" => {
            let graph = vault.graph().map_err(|error| error.to_string())?;
            Ok((
                format!(
                    "{}件のブロックと{}件の関係を取得した",
                    graph.blocks.len(),
                    graph.edges.len()
                ),
                json!({"graph": graph}),
            ))
        }
        "sanctum_create_block" => {
            let title = required_string(arguments, "title")?;
            let change_reason = required_string(arguments, "changeReason")?;
            let block = vault
                .create_block(CreateBlockInput {
                    title,
                    body_markdown: optional_string(arguments, "bodyMarkdown")?
                        .unwrap_or_default(),
                    research_notes_markdown: optional_string(
                        arguments,
                        "researchNotesMarkdown",
                    )?
                    .unwrap_or_default(),
                    kind: parse_kind(optional_string(arguments, "kind")?.as_deref())?,
                    status: parse_status(optional_string(arguments, "status")?.as_deref())?,
                    tags: optional_strings(arguments, "tags")?.unwrap_or_default(),
                    change_reason,
                })
                .map_err(|error| error.to_string())?;
            Ok((
                format!("「{}」を作成した", block.snapshot.title),
                json!({"block": block}),
            ))
        }
        "sanctum_update_block" => {
            let block_id = required_string(arguments, "blockId")?;
            let expected_row_version = required_i64(arguments, "expectedRowVersion")?;
            let change_reason = required_string(arguments, "changeReason")?;
            let current = vault
                .get_block(&block_id)
                .map_err(|error| error.to_string())?;
            let kind = match optional_string(arguments, "kind")? {
                Some(value) => BlockKind::try_from(value.as_str()).map_err(|error| error.to_string())?,
                None => current.snapshot.kind,
            };
            let status = match optional_string(arguments, "status")? {
                Some(value) => {
                    BlockStatus::try_from(value.as_str()).map_err(|error| error.to_string())?
                }
                None => current.snapshot.status,
            };
            let updated = vault
                .save_block(SaveBlockInput {
                    block_id,
                    expected_row_version,
                    title: optional_string(arguments, "title")?
                        .unwrap_or(current.snapshot.title),
                    body_markdown: optional_string(arguments, "bodyMarkdown")?
                        .unwrap_or(current.snapshot.body_markdown),
                    research_notes_markdown: optional_string(
                        arguments,
                        "researchNotesMarkdown",
                    )?
                    .unwrap_or(current.snapshot.research_notes_markdown),
                    kind,
                    status,
                    tags: optional_strings(arguments, "tags")?
                        .unwrap_or(current.snapshot.tags),
                    change_reason,
                })
                .map_err(|error| error.to_string())?;
            Ok((
                format!("「{}」を更新した", updated.snapshot.title),
                json!({"block": updated}),
            ))
        }
        "sanctum_attach_file" => {
            let block_id = required_string(arguments, "blockId")?;
            let source_path = PathBuf::from(required_string(arguments, "sourcePath")?);
            let metadata = std::fs::metadata(&source_path)
                .map_err(|error| format!("添付元ファイルを確認できない: {error}"))?;
            if !metadata.is_file() {
                return Err("添付元は通常ファイルを指定して".into());
            }
            let source_path = std::fs::canonicalize(source_path)
                .map_err(|error| format!("添付元ファイルを解決できない: {error}"))?;
            let relation = optional_string(arguments, "relation")?
                .unwrap_or_else(|| "Reference".into());
            let relation = AttachmentRelation::try_from(relation.as_str())
                .map_err(|error| error.to_string())?;
            let locator = arguments
                .get("locator")
                .cloned()
                .unwrap_or_else(|| json!({}));
            if !locator.is_object() {
                return Err("locatorはJSON objectで指定して".into());
            }
            let attachment = vault
                .attach_file(&block_id, source_path, relation, locator)
                .map_err(|error| error.to_string())?;
            Ok((
                format!("「{}」を添付した", attachment.display_name),
                json!({"attachment": attachment}),
            ))
        }
        "sanctum_list_attachments" => {
            let block_id = required_string(arguments, "blockId")?;
            let attachments = vault
                .attachments_for_block(&block_id)
                .map_err(|error| error.to_string())?;
            Ok((
                format!("{}件の添付を取得した", attachments.len()),
                json!({"attachments": attachments}),
            ))
        }
        "sanctum_read_text_attachment" => {
            let attachment_id = required_string(arguments, "attachmentId")?;
            let max_characters = optional_usize(arguments, "maxCharacters")?
                .unwrap_or(50_000)
                .clamp(1, 200_000);
            let attachment = vault
                .attachment(&attachment_id)
                .map_err(|error| error.to_string())?;
            if !is_text_attachment(&attachment.display_name, attachment.media_type.as_deref()) {
                return Err(
                    "この添付はテキスト形式ではない。PDFと画像の本文抽出はSanctumデスクトップで確認して"
                        .into(),
                );
            }
            let path = vault
                .attachment_object_path(&attachment_id)
                .map_err(|error| error.to_string())?;
            let byte_limit = max_characters.saturating_mul(4).saturating_add(4);
            let mut bytes = Vec::new();
            std::fs::File::open(path)
                .map_err(|error| format!("添付を開けない: {error}"))?
                .take(u64::try_from(byte_limit).unwrap_or(u64::MAX))
                .read_to_end(&mut bytes)
                .map_err(|error| format!("添付を読めない: {error}"))?;
            let bytes_truncated = attachment.byte_size > i64::try_from(bytes.len()).unwrap_or(i64::MAX);
            let text = match String::from_utf8(bytes) {
                Ok(text) => text,
                Err(error) if error.utf8_error().error_len().is_none() => {
                    let valid_up_to = error.utf8_error().valid_up_to();
                    String::from_utf8(error.into_bytes()[..valid_up_to].to_vec())
                        .map_err(|_| "添付はUTF-8テキストとして読めない".to_owned())?
                }
                Err(_) => return Err("添付はUTF-8テキストとして読めない".into()),
            };
            let characters_read = text.chars().count();
            let truncated = bytes_truncated || characters_read > max_characters;
            let text = if truncated {
                text.chars().take(max_characters).collect::<String>()
            } else {
                text
            };
            let characters_returned = text.chars().count();
            Ok((
                format!(
                    "「{}」から{}文字を取得した{}",
                    attachment.display_name,
                    characters_returned,
                    if truncated { "（省略あり）" } else { "" }
                ),
                json!({
                    "attachment": attachment,
                    "text": text,
                    "truncated": truncated,
                    "charactersReturned": characters_returned
                }),
            ))
        }
        "sanctum_integrity_check" => {
            let report = vault
                .integrity_check()
                .map_err(|error| error.to_string())?;
            let fatal = report
                .findings
                .iter()
                .filter(|finding| {
                    matches!(finding.severity, sanctum_core::IntegritySeverity::Fatal)
                })
                .count();
            Ok((
                if fatal == 0 {
                    "致命的な整合性問題は見つからなかった".into()
                } else {
                    format!("{fatal}件の致命的な整合性問題が見つかった")
                },
                json!({"report": report}),
            ))
        }
        _ => Err(format!("不明なSanctumツール: {name}")),
    }
}

fn active_vault(shared: &SharedVault) -> Result<Arc<Vault>, String> {
    shared
        .lock()
        .map_err(|_| "SanctumのVault状態を取得できない".to_owned())?
        .clone()
        .ok_or_else(|| "Vaultが開かれていない。Sanctumデスクトップで対象Vaultを開いて".into())
}

fn required_string(arguments: &Value, key: &str) -> Result<String, String> {
    let value = arguments
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{key}を指定して"))?;
    Ok(value.to_owned())
}

fn optional_string(arguments: &Value, key: &str) -> Result<Option<String>, String> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(format!("{key}は文字列で指定して")),
    }
}

fn required_i64(arguments: &Value, key: &str) -> Result<i64, String> {
    arguments
        .get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("{key}を整数で指定して"))
}

fn optional_usize(arguments: &Value, key: &str) -> Result<Option<usize>, String> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .map(Some)
            .ok_or_else(|| format!("{key}は0以上の整数で指定して")),
    }
}

fn optional_strings(arguments: &Value, key: &str) -> Result<Option<Vec<String>>, String> {
    let Some(value) = arguments.get(key) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let values = value
        .as_array()
        .ok_or_else(|| format!("{key}は文字列の配列で指定して"))?;
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("{key}は文字列の配列で指定して"))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

fn parse_kind(value: Option<&str>) -> Result<BlockKind, String> {
    BlockKind::try_from(value.unwrap_or("Hypothesis")).map_err(|error| error.to_string())
}

fn parse_status(value: Option<&str>) -> Result<BlockStatus, String> {
    BlockStatus::try_from(value.unwrap_or("Idea")).map_err(|error| error.to_string())
}

fn is_text_attachment(display_name: &str, media_type: Option<&str>) -> bool {
    if media_type.is_some_and(|media_type| {
        media_type.starts_with("text/")
            || matches!(
                media_type,
                "application/json" | "application/xml" | "application/x-bibtex"
            )
    }) {
        return true;
    }
    let extension = std::path::Path::new(display_name)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(
        extension.as_str(),
        "txt" | "md" | "markdown" | "csv" | "tsv" | "json" | "jsonl" | "bib" | "tex"
            | "yaml" | "yml" | "xml" | "toml"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn state_with_vault() -> (TempDir, SharedVault) {
        let directory = tempfile::tempdir().expect("temporary directory");
        let vault = Arc::new(
            Vault::create(directory.path().join("test.sanctum"), "MCP Test")
                .expect("create vault"),
        );
        let state = Arc::new(Mutex::new(Some(vault)));
        (directory, state)
    }

    fn call(name: &str, arguments: Value, state: &SharedVault) -> Value {
        dispatch(
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": {"name": name, "arguments": arguments}
            }),
            state,
        )
        .expect("tool response")
    }

    #[test]
    fn lists_safe_tool_annotations() {
        let response = dispatch(
            &json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
            &Arc::new(Mutex::new(None)),
        )
        .expect("tools response");
        let tools = response
            .pointer("/result/tools")
            .and_then(Value::as_array)
            .expect("tool list");
        assert!(tools.iter().any(|tool| {
            tool.get("name") == Some(&Value::String("sanctum_update_block".into()))
                && tool.pointer("/annotations/destructiveHint") == Some(&Value::Bool(false))
        }));
        assert!(!tools.iter().any(|tool| {
            tool.get("name")
                .and_then(Value::as_str)
                .is_some_and(|name| name.contains("delete") || name.contains("restore"))
        }));
    }

    #[test]
    fn creates_and_updates_through_core_history() {
        let (_directory, state) = state_with_vault();
        let created = call(
            "sanctum_create_block",
            json!({
                "title": "接続仮説",
                "bodyMarkdown": "最初の本文",
                "changeReason": "ChatGPT接続テスト"
            }),
            &state,
        );
        assert_eq!(created.pointer("/result/isError"), Some(&Value::Bool(false)));
        let block = created
            .pointer("/result/structuredContent/block")
            .expect("created block");
        let block_id = block.get("id").and_then(Value::as_str).expect("block id");
        let row_version = block
            .get("rowVersion")
            .and_then(Value::as_i64)
            .expect("row version");

        let updated = call(
            "sanctum_update_block",
            json!({
                "blockId": block_id,
                "expectedRowVersion": row_version,
                "bodyMarkdown": "更新した本文",
                "changeReason": "本文を更新"
            }),
            &state,
        );
        assert_eq!(updated.pointer("/result/isError"), Some(&Value::Bool(false)));
        assert_eq!(
            updated.pointer("/result/structuredContent/block/bodyMarkdown"),
            Some(&Value::String("更新した本文".into()))
        );

        let vault = active_vault(&state).expect("active vault");
        assert_eq!(vault.versions(block_id).expect("versions").len(), 2);
    }

    #[test]
    fn refuses_stale_updates_without_overwrite() {
        let (_directory, state) = state_with_vault();
        let created = call(
            "sanctum_create_block",
            json!({"title": "競合", "changeReason": "作成"}),
            &state,
        );
        let block_id = created
            .pointer("/result/structuredContent/block/id")
            .and_then(Value::as_str)
            .expect("block id");

        let first = call(
            "sanctum_update_block",
            json!({
                "blockId": block_id,
                "expectedRowVersion": 1,
                "bodyMarkdown": "先の変更",
                "changeReason": "先に保存"
            }),
            &state,
        );
        assert_eq!(first.pointer("/result/isError"), Some(&Value::Bool(false)));

        let stale = call(
            "sanctum_update_block",
            json!({
                "blockId": block_id,
                "expectedRowVersion": 1,
                "bodyMarkdown": "古い変更",
                "changeReason": "競合させる"
            }),
            &state,
        );
        assert_eq!(stale.pointer("/result/isError"), Some(&Value::Bool(true)));
        let vault = active_vault(&state).expect("active vault");
        assert_eq!(
            vault.get_block(block_id).expect("block").snapshot.body_markdown,
            "先の変更"
        );
    }

    #[test]
    fn attaches_and_reads_utf8_text_through_the_object_store() {
        let (directory, state) = state_with_vault();
        let created = call(
            "sanctum_create_block",
            json!({"title": "資料", "changeReason": "作成"}),
            &state,
        );
        let block_id = created
            .pointer("/result/structuredContent/block/id")
            .and_then(Value::as_str)
            .expect("block id");
        let source = directory.path().join("notes.md");
        std::fs::write(&source, "研究メモ\n第二行").expect("write source");
        let attached = call(
            "sanctum_attach_file",
            json!({"blockId": block_id, "sourcePath": source}),
            &state,
        );
        let attachment_id = attached
            .pointer("/result/structuredContent/attachment/id")
            .and_then(Value::as_str)
            .expect("attachment id");

        let read = call(
            "sanctum_read_text_attachment",
            json!({"attachmentId": attachment_id}),
            &state,
        );
        assert_eq!(
            read.pointer("/result/structuredContent/text"),
            Some(&Value::String("研究メモ\n第二行".into()))
        );
    }
}
