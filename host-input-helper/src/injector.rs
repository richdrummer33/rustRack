use serde_json::Value;

pub trait Injector: Send + Sync {
    fn handle(&self, kind: &str, intent: &Value) -> Result<(), String>;
}

pub struct LogInjector;

impl Injector for LogInjector {
    fn handle(&self, kind: &str, intent: &Value) -> Result<(), String> {
        match kind {
            "tap" | "knob-drag" | "cable" | "pan" | "zoom" | "context" | "set-param" => {
                eprintln!("intent {kind}: {intent}");
                Ok(())
            }
            other => Err(format!("unknown intent kind: {other}")),
        }
    }
}
