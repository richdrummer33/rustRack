use std::sync::Mutex;

use enigo::{Enigo, Key, KeyboardControllable, MouseButton, MouseControllable};
use serde_json::Value;

use crate::injector::Injector;
use crate::scene::SceneCache;

pub struct EnigoInjector {
    enigo: Mutex<Enigo>,
}

impl EnigoInjector {
    pub fn new() -> Self {
        Self { enigo: Mutex::new(Enigo::new()) }
    }
}

impl Injector for EnigoInjector {
    fn handle(&self, kind: &str, intent: &Value, scene: &SceneCache) -> Result<(), String> {
        let mut e = self.enigo.lock().map_err(|_| "enigo lock poisoned".to_string())?;
        match kind {
            "tap" => {
                let (x, y) = resolve_target(intent, scene)?;
                e.mouse_move_to(x, y);
                e.mouse_down(MouseButton::Left).map_err(stringify)?;
                e.mouse_up(MouseButton::Left);
            }
            "knob-drag" => {
                let m = i64_field(intent, "moduleId")?;
                let p = i64_field(intent, "paramId")?;
                let delta = f64_field(intent, "deltaPx").unwrap_or(0.0);
                let fine = intent.get("fine").and_then(|x| x.as_bool()).unwrap_or(false);
                let (x, y) = scene
                    .modules
                    .get(&m)
                    .and_then(|me| me.params.get(&p))
                    .copied()
                    .ok_or_else(|| format!("unknown param {m}/{p}"))?;
                if fine {
                    e.key_down(Key::Control).map_err(stringify)?;
                }
                e.mouse_move_to(x as i32, y as i32);
                e.mouse_down(MouseButton::Left).map_err(stringify)?;
                e.mouse_move_relative(0, -(delta as i32));
                e.mouse_up(MouseButton::Left);
                if fine {
                    e.key_up(Key::Control);
                }
            }
            "cable" => {
                let from = intent.get("from").ok_or_else(|| "missing from".to_string())?;
                let to = intent.get("to").ok_or_else(|| "missing to".to_string())?;
                let (x1, y1) = scene
                    .resolve_port_ref(from)
                    .ok_or_else(|| "from port not in scene".to_string())?;
                let (x2, y2) = scene
                    .resolve_port_ref(to)
                    .ok_or_else(|| "to port not in scene".to_string())?;
                e.mouse_move_to(x1 as i32, y1 as i32);
                e.mouse_down(MouseButton::Left).map_err(stringify)?;
                e.mouse_move_to(x2 as i32, y2 as i32);
                e.mouse_up(MouseButton::Left);
            }
            "pan" => {
                let dx = f64_field(intent, "dx").unwrap_or(0.0);
                let dy = f64_field(intent, "dy").unwrap_or(0.0);
                let ticks_y = (dy / 40.0) as i32;
                let ticks_x = (dx / 40.0) as i32;
                if ticks_y != 0 {
                    e.mouse_scroll_y(-ticks_y);
                }
                if ticks_x != 0 {
                    e.mouse_scroll_x(-ticks_x);
                }
            }
            "zoom" => {
                let factor = f64_field(intent, "factor").unwrap_or(1.0);
                let anchor = intent.get("anchor").ok_or_else(|| "missing anchor".to_string())?;
                let ax = f64_field(anchor, "x")?;
                let ay = f64_field(anchor, "y")?;
                let ticks = (factor.ln() / 1.1_f64.ln()).round() as i32;
                if ticks != 0 {
                    e.mouse_move_to(ax as i32, ay as i32);
                    e.key_down(Key::Control).map_err(stringify)?;
                    e.mouse_scroll_y(ticks);
                    e.key_up(Key::Control);
                }
            }
            "context" => {
                let (x, y) = resolve_target(intent, scene)?;
                e.mouse_move_to(x, y);
                e.mouse_down(MouseButton::Right).map_err(stringify)?;
                e.mouse_up(MouseButton::Right);
            }
            "set-param" => return Err("set-param not yet supported (needs Rack-side hook)".into()),
            other => return Err(format!("unknown intent kind: {other}")),
        }
        Ok(())
    }
}

fn resolve_target(intent: &Value, scene: &SceneCache) -> Result<(i32, i32), String> {
    let target = intent.get("target").ok_or_else(|| "missing target".to_string())?;
    let (x, y) = scene
        .resolve_target(target)
        .ok_or_else(|| "target not in scene".to_string())?;
    Ok((x as i32, y as i32))
}

fn i64_field(v: &Value, key: &str) -> Result<i64, String> {
    v.get(key).and_then(|x| x.as_i64()).ok_or_else(|| format!("missing {key}"))
}

fn f64_field(v: &Value, key: &str) -> Result<f64, String> {
    v.get(key).and_then(|x| x.as_f64()).ok_or_else(|| format!("missing {key}"))
}

fn stringify<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}
