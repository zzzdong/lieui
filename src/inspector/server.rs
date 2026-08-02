//! Inspector WebSocket server —— Chrome DevTools Protocol (CDP) JSON-RPC 子集
//!
//! 使用 `tungstenite`（同步）在独立线程上监听 127.0.0.1，
//! 实现 CDP 的 Runtime / DOM 域最小子集，使 Chrome 的 Elements 面板能直接
//! 浏览 LieUI 的 **Widget 树**（由 BuildContext 收集）。
//!
//! 不引入任何 async 运行时依赖（与主页 winit 单线程架构保持兼容）。

use crate::inspector::{InspectorShared, WidgetDesc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tungstenite::Message;
use tungstenite::accept;

// ============================================================================
// CDP DOM 节点
// ============================================================================

#[derive(Serialize)]
struct CdpNode {
    #[serde(rename = "nodeId")]
    node_id: u32,
    #[serde(rename = "backendNodeId")]
    backend_node_id: u32,
    #[serde(rename = "nodeType")]
    node_type: u32,
    #[serde(rename = "nodeName")]
    node_name: String,
    #[serde(rename = "localName")]
    local_name: String,
    #[serde(rename = "nodeValue")]
    node_value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    attributes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    children: Option<Vec<CdpNode>>,
    #[serde(rename = "childNodeCount")]
    child_node_count: u32,
    #[serde(rename = "parentId", skip_serializing_if = "Option::is_none")]
    parent_id: Option<u32>,
}

/// 每个会话维护 path -> nodeId 的稳定映射
struct IdMap {
    path_to_id: HashMap<String, u32>,
    next: u32,
}

impl IdMap {
    fn new() -> Self {
        Self {
            path_to_id: HashMap::new(),
            next: 1,
        }
    }
    fn id_for(&mut self, path: &str) -> u32 {
        if let Some(id) = self.path_to_id.get(path) {
            return *id;
        }
        let id = self.next;
        self.next += 1;
        self.path_to_id.insert(path.to_string(), id);
        id
    }
}

impl CdpNode {
    fn from_desc(n: &WidgetDesc, parent_id: Option<u32>, ids: &mut IdMap) -> Self {
        let node_id = ids.id_for(&n.path);
        let child_count = n.children.len() as u32;
        let children = if child_count > 0 {
            Some(
                n.children
                    .iter()
                    .map(|c| CdpNode::from_desc(c, Some(node_id), ids))
                    .collect(),
            )
        } else {
            None
        };
        let mut attrs: Vec<String> = vec!["widget".into(), n.name.clone()];
        if let Some(t) = &n.text {
            attrs.push("text".into());
            attrs.push(t.clone());
        }
        CdpNode {
            node_id,
            backend_node_id: node_id,
            node_type: 1, // ELEMENT_NODE
            node_name: n.name.clone(),
            local_name: n.name.clone(),
            node_value: String::new(),
            attributes: Some(attrs),
            children,
            child_node_count: child_count,
            parent_id,
        }
    }
}

// ============================================================================
// 请求 / 响应
// ============================================================================

#[derive(Deserialize)]
struct CdpRequest {
    #[serde(default)]
    id: Option<u64>,
    method: String,
    params: Option<Value>,
}

#[derive(Serialize)]
struct CdpResponse {
    id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<CdpError>,
}

#[derive(Debug, Serialize)]
struct CdpError {
    code: i32,
    message: String,
}

#[derive(Serialize)]
struct CdpEvent {
    method: String,
    params: Value,
}

// ============================================================================
// Server
// ============================================================================

/// 启动 server 线程，绑定 127.0.0.1:0（自动分配端口）。
///
/// 返回 `(port, thread, stop)`：`stop` 置位后 server 线程会在下一次轮询时退出。
/// listener 设为非阻塞，accept 采用短轮询，因此无需关闭 socket 即可被打断，
/// 避免 `Inspector::drop` 的 `join` 因阻塞的 `accept` 而永久挂起（否则主线程卡死、
/// 窗口「无响应」、进程无法退出）。
pub(crate) fn spawn(
    shared: Arc<Mutex<InspectorShared>>,
) -> (u16, std::thread::JoinHandle<()>, Arc<AtomicBool>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("inspector: bind 127.0.0.1:0 failed");
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
    // 非阻塞：accept 不会无限挂起，配合 stop 标志实现可中断退出。
    let _ = listener.set_nonblocking(true);
    let stop = Arc::new(AtomicBool::new(false));
    let thread = {
        let stop = stop.clone();
        std::thread::spawn(move || run(listener, stop, shared))
    };
    (port, thread, stop)
}

fn run(listener: TcpListener, stop: Arc<AtomicBool>, shared: Arc<Mutex<InspectorShared>>) {
    if let Ok(mut g) = shared.lock() {
        g.ready = true;
    }
    loop {
        // 停止信号优先：确保 `Inspector::drop` 的 join 能快速返回。
        if stop.load(Ordering::SeqCst) {
            break;
        }
        match listener.accept() {
            Ok((stream, _)) => {
                // 连接到达：升级为非阻塞无关的同步 WebSocket 处理。
                let ws = match accept(stream) {
                    Ok(w) => w,
                    Err(_) => continue,
                };
                if handle_session(ws, shared.clone(), stop.clone()).is_err() {
                    // 连接断开，继续等待下一个
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // 当前无连接：短暂休眠后重试，避免空转吃 CPU。
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(_) => {
                // 其他 accept 错误：若已停止则退出，否则继续轮询。
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }
}

fn handle_session(
    mut ws: tungstenite::WebSocket<std::net::TcpStream>,
    shared: Arc<Mutex<InspectorShared>>,
    stop: Arc<AtomicBool>,
) -> Result<(), ()> {
    let mut ids = IdMap::new();
    eprintln!("[inspector-srv] session started, sending init events");

    // 读超时：让 `ws.read()` 周期性返回，从而能检查 stop 标志并退出。
    let _ = ws
        .get_mut()
        .set_read_timeout(Some(std::time::Duration::from_millis(500)));

    match send_event(
        &mut ws,
        "Runtime.executionContextCreated",
        serde_json::json!({ "context": {
            "id": 1,
            "name": "LieUI",
            "origin": "lieui://inspector",
            "auxData": { "isDefault": true, "type": "window" }
        }}),
    ) {
        Ok(_) => {}
        Err(e) => eprintln!("[lieui-inspector] init event failed: {:?}", e),
    }
    // 部分 DevTools 版本需要 Target 信息
    let _ = send_event(
        &mut ws,
        "Target.targetCreated",
        serde_json::json!({ "targetInfo": {
            "targetId": "lieui-inspector",
            "type": "page",
            "title": "LieUI Inspector",
            "url": "lieui://inspector"
        }}),
    );

    loop {
        // stop 优先：即便正在等待读，也尽快退出。
        if stop.load(Ordering::SeqCst) {
            return Ok(());
        }
        let msg = match ws.read() {
            Ok(Message::Text(t)) => t,
            Ok(Message::Binary(_)) => continue,
            Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => continue,
            Ok(Message::Close(_)) => return Ok(()),
            Ok(Message::Frame(_)) => continue,
            // 读超时或中断：若收到 stop 则退出，否则继续等待（不视为断开）。
            Err(e) => {
                let is_timeout = match e {
                    tungstenite::Error::Io(ref io) => {
                        io.kind() == std::io::ErrorKind::WouldBlock
                            || io.kind() == std::io::ErrorKind::TimedOut
                    }
                    _ => false,
                };
                if is_timeout {
                    continue;
                }
                return Ok(());
            }
        };

        let req: CdpRequest = match serde_json::from_str(&msg) {
            Ok(r) => r,
            Err(_) => continue,
        };

        // 无 id 的通知类消息（如 Runtime.runIfWaitingForDebugger）直接忽略
        let id = match req.id {
            Some(i) => i,
            None => continue,
        };

        let result = handle_method(
            id,
            &req.method,
            req.params.as_ref(),
            &shared,
            &mut ws,
            &mut ids,
        );
        let resp = match result {
            Ok(val) => CdpResponse {
                id,
                result: Some(val),
                error: None,
            },
            Err(err) => CdpResponse {
                id,
                result: None,
                error: Some(err),
            },
        };
        let payload = serde_json::to_string(&resp).unwrap_or_default();
        if ws.send(Message::Text(payload)).is_err() {
            return Err(());
        }
    }
}

fn handle_method(
    _id: u64,
    method: &str,
    params: Option<&Value>,
    shared: &Arc<Mutex<InspectorShared>>,
    ws: &mut tungstenite::WebSocket<std::net::TcpStream>,
    ids: &mut IdMap,
) -> Result<Value, CdpError> {
    match method {
        // ---- Runtime ----
        "Runtime.enable" => Ok(Value::Object(Map::new())),
        "Runtime.getIsolateId" => Ok(serde_json::json!({ "id": "lieui" })),

        // ---- Target / Browser（兼容性空应答）----
        "Target.enable"
        | "Target.getTargets"
        | "Target.setDiscoverTargets"
        | "Target.attachToTarget"
        | "Target.setAttachToCustomTarget"
        | "Browser.getVersion"
        | "Page.enable"
        | "DOM.enable"
        | "CSS.enable"
        | "Overlay.enable"
        | "Page.getResourceTree"
        | "Emulation.setDeviceMetricsOverride"
        | "Emulation.clearDeviceMetricsOverride"
        | "Inspector.enable"
        | "Log.enable"
        | "Network.enable"
        | "Page.getFrameTree" => Ok(Value::Object(Map::new())),

        // ---- DOM: 根文档 ----
        "DOM.getDocument" => {
            let g = shared.lock().map_err(|_| cdp_err(-1, "lock poisoned"))?;
            let root = g
                .snapshot
                .as_ref()
                .ok_or_else(|| cdp_err(-1, "no snapshot yet"))?;
            let node = CdpNode::from_desc(root, None, ids);
            Ok(serde_json::json!({
                "root": node,
                "documentURL": "lieui://inspector",
                "baseURL": "lieui://inspector",
            }))
        }

        // ---- DOM: 子节点（懒加载）----
        "DOM.requestChildNodes" => {
            let node_id = params
                .and_then(|p| p.get("nodeId"))
                .and_then(|v| v.as_u64())
                .ok_or_else(|| cdp_err(-32602, "missing nodeId"))?;
            // 反向查找：nodeId -> path
            let path = ids
                .path_to_id
                .iter()
                .find(|(_, v)| **v == node_id as u32)
                .map(|(k, _)| k.clone());
            let path = path.ok_or_else(|| cdp_err(-1, "node not found in id map"))?;

            let g = shared.lock().map_err(|_| cdp_err(-1, "lock poisoned"))?;
            let root = g
                .snapshot
                .as_ref()
                .ok_or_else(|| cdp_err(-1, "no snapshot"))?;
            let node = root
                .find_by_path(&path)
                .ok_or_else(|| cdp_err(-1, "node not found"))?;
            let children: Vec<CdpNode> = node
                .children
                .iter()
                .map(|c| CdpNode::from_desc(c, Some(node_id as u32), ids))
                .collect();
            drop(g);
            send_event(
                ws,
                "DOM.setChildNodes",
                serde_json::json!({
                    "parentId": node_id,
                    "nodes": children,
                }),
            )
            .map_err(|_| cdp_err(-1, "send failed"))?;
            Ok(Value::Object(Map::new()))
        }

        // ---- CSS: 计算样式（一期 widget 树无样式，返回空）----
        "CSS.getComputedStyleForNode"
        | "CSS.getInlineStylesForNode"
        | "CSS.getMatchedStylesForNode" => Ok(serde_json::json!({
            "computedStyle": [],
            "inlineStyle": null,
            "attributesStyle": null,
            "matchedCSSRules": [],
        })),

        // 未知方法：返回空结果（避免 DevTools 报错卡死）
        _ => Ok(Value::Object(Map::new())),
    }
}

fn cdp_err(code: i32, message: &str) -> CdpError {
    CdpError {
        code,
        message: message.to_string(),
    }
}

fn send_event(
    ws: &mut tungstenite::WebSocket<std::net::TcpStream>,
    method: &str,
    params: Value,
) -> Result<(), ()> {
    let ev = CdpEvent {
        method: method.to_string(),
        params,
    };
    let payload = serde_json::to_string(&ev).map_err(|_| ())?;
    ws.send(Message::Text(payload)).map_err(|_| ())
}
