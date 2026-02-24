use serde_json::{Value, json};
use std::io::Write;
use std::sync::{Arc, Mutex};

/// Progress notification emitter for MCP index operations.
///
/// Two implementations:
/// - `McpProgressNotifier`: emits JSON-RPC `notifications/progress` to the client
/// - `NullProgressNotifier`: no-op for clients without notification support
pub trait ProgressNotifier: Send + Sync {
    /// Emit the start of a progress operation.
    fn emit_begin(&self, token: &str, title: &str, message: &str);

    /// Emit a progress update.
    fn emit_progress(&self, token: &str, title: &str, message: &str, percentage: Option<u32>);

    /// Emit a completion notification.
    fn emit_end(&self, token: &str, title: &str, message: &str);
}

/// Sends JSON-RPC `notifications/progress` to the MCP client via stdout.
pub struct McpProgressNotifier {
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
}

impl McpProgressNotifier {
    pub fn new(writer: Arc<Mutex<Box<dyn Write + Send>>>) -> Self {
        Self { writer }
    }

    fn send_notification(&self, params: Value) {
        let notification = json!({
            "jsonrpc": "2.0",
            "method": "notifications/progress",
            "params": params,
        });
        if let Ok(mut w) = self.writer.lock() {
            let _ = writeln!(
                w,
                "{}",
                serde_json::to_string(&notification).unwrap_or_default()
            );
            let _ = w.flush();
        }
    }
}

impl ProgressNotifier for McpProgressNotifier {
    fn emit_begin(&self, token: &str, title: &str, message: &str) {
        self.send_notification(json!({
            "progressToken": token,
            "value": {
                "kind": "begin",
                "title": title,
                "message": message,
            },
        }));
    }

    fn emit_progress(&self, token: &str, title: &str, message: &str, percentage: Option<u32>) {
        let mut value = json!({
            "kind": "report",
            "title": title,
            "message": message,
        });
        if let Some(pct) = percentage {
            value["percentage"] = json!(pct.min(100));
        }

        self.send_notification(json!({
            "progressToken": token,
            "value": value,
        }));
    }

    fn emit_end(&self, token: &str, title: &str, message: &str) {
        self.send_notification(json!({
            "progressToken": token,
            "value": {
                "kind": "end",
                "title": title,
                "message": message,
            },
        }));
    }
}

/// No-op progress notifier for clients that don't support notifications.
pub struct NullProgressNotifier;

impl ProgressNotifier for NullProgressNotifier {
    fn emit_begin(&self, _token: &str, _title: &str, _message: &str) {
        // no-op
    }

    fn emit_progress(&self, _token: &str, _title: &str, _message: &str, _percentage: Option<u32>) {
        // no-op
    }

    fn emit_end(&self, _token: &str, _title: &str, _message: &str) {
        // no-op
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// T221: Verify JSON-RPC notification format matches MCP spec
    #[test]
    fn mcp_notifier_emits_correct_jsonrpc_format() {
        let buffer: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let writer: Arc<Mutex<Box<dyn Write + Send>>> =
            Arc::new(Mutex::new(Box::new(CaptureWriter(buffer.clone()))));

        let notifier = McpProgressNotifier::new(writer);
        notifier.emit_progress("tok-123", "Indexing", "Scanning files", Some(25));

        let output = buffer.lock().unwrap();
        let line = String::from_utf8_lossy(&output);
        let parsed: Value = serde_json::from_str(line.trim()).unwrap();

        assert_eq!(parsed["jsonrpc"], "2.0");
        assert_eq!(parsed["method"], "notifications/progress");
        assert_eq!(parsed["params"]["progressToken"], "tok-123");
        assert_eq!(parsed["params"]["value"]["kind"], "report");
        assert_eq!(parsed["params"]["value"]["title"], "Indexing");
        assert_eq!(parsed["params"]["value"]["message"], "Scanning files");
        assert_eq!(parsed["params"]["value"]["percentage"], 25);
    }

    #[test]
    fn mcp_notifier_emits_end_notification() {
        let buffer: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let writer: Arc<Mutex<Box<dyn Write + Send>>> =
            Arc::new(Mutex::new(Box::new(CaptureWriter(buffer.clone()))));

        let notifier = McpProgressNotifier::new(writer);
        notifier.emit_end("tok-456", "Indexing", "Complete");

        let output = buffer.lock().unwrap();
        let line = String::from_utf8_lossy(&output);
        let parsed: Value = serde_json::from_str(line.trim()).unwrap();

        assert_eq!(parsed["params"]["value"]["kind"], "end");
        assert_eq!(parsed["params"]["value"]["title"], "Indexing");
        assert_eq!(parsed["params"]["value"]["message"], "Complete");
    }

    #[test]
    fn mcp_notifier_caps_percentage_at_100() {
        let buffer: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let writer: Arc<Mutex<Box<dyn Write + Send>>> =
            Arc::new(Mutex::new(Box::new(CaptureWriter(buffer.clone()))));

        let notifier = McpProgressNotifier::new(writer);
        notifier.emit_progress("tok", "Test", "msg", Some(150));

        let output = buffer.lock().unwrap();
        let line = String::from_utf8_lossy(&output);
        let parsed: Value = serde_json::from_str(line.trim()).unwrap();

        assert_eq!(parsed["params"]["value"]["percentage"], 100);
    }

    #[test]
    fn mcp_notifier_emits_begin_notification() {
        let buffer: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let writer: Arc<Mutex<Box<dyn Write + Send>>> =
            Arc::new(Mutex::new(Box::new(CaptureWriter(buffer.clone()))));

        let notifier = McpProgressNotifier::new(writer);
        notifier.emit_begin("tok-begin", "Indexing", "Starting");

        let output = buffer.lock().unwrap();
        let line = String::from_utf8_lossy(&output);
        let parsed: Value = serde_json::from_str(line.trim()).unwrap();

        assert_eq!(parsed["params"]["progressToken"], "tok-begin");
        assert_eq!(parsed["params"]["value"]["kind"], "begin");
        assert_eq!(parsed["params"]["value"]["title"], "Indexing");
        assert_eq!(parsed["params"]["value"]["message"], "Starting");
    }

    #[test]
    fn null_notifier_does_not_panic() {
        let notifier = NullProgressNotifier;
        notifier.emit_begin("tok", "title", "begin");
        notifier.emit_progress("tok", "title", "msg", Some(50));
        notifier.emit_end("tok", "title", "done");
        // Just verifying no panics
    }

    /// Helper writer that captures output to a shared buffer.
    struct CaptureWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for CaptureWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
}
