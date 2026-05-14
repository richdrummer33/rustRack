use std::collections::HashMap;

use serde_json::Value;

#[derive(Debug, Default, Clone)]
pub struct SceneCache {
    pub modules: HashMap<i64, ModuleEntry>,
}

#[derive(Debug, Clone, Default)]
pub struct ModuleEntry {
    pub box_: [f64; 4],
    pub params: HashMap<i64, (f64, f64)>,
    pub inputs: HashMap<i64, (f64, f64)>,
    pub outputs: HashMap<i64, (f64, f64)>,
}

impl SceneCache {
    pub fn from_envelope(v: &Value) -> Self {
        let mut cache = Self::default();
        let Some(mods) = v.get("modules").and_then(|x| x.as_array()) else {
            return cache;
        };
        for m in mods {
            let Some(id) = m.get("id").and_then(|x| x.as_i64()) else { continue; };
            let mut entry = ModuleEntry::default();
            if let Some(b) = m.get("screenBox").and_then(|x| x.as_array()) {
                for (i, e) in b.iter().enumerate().take(4) {
                    entry.box_[i] = e.as_f64().unwrap_or(0.0);
                }
            }
            collect(&mut entry.params,  m.get("params"),  "id");
            collect(&mut entry.inputs,  m.get("inputs"),  "id");
            collect(&mut entry.outputs, m.get("outputs"), "id");
            cache.modules.insert(id, entry);
        }
        cache
    }

    #[allow(dead_code)]
    pub fn resolve_target(&self, target: &Value) -> Option<(f64, f64)> {
        let kind = target.get("kind").and_then(|x| x.as_str())?;
        let module_id = || target.get("moduleId").and_then(|x| x.as_i64());
        match kind {
            "param" => {
                let m = self.modules.get(&module_id()?)?;
                let pid = target.get("paramId").and_then(|x| x.as_i64())?;
                m.params.get(&pid).copied()
            }
            "input" => {
                let m = self.modules.get(&module_id()?)?;
                let pid = target.get("portId").and_then(|x| x.as_i64())?;
                m.inputs.get(&pid).copied()
            }
            "output" => {
                let m = self.modules.get(&module_id()?)?;
                let pid = target.get("portId").and_then(|x| x.as_i64())?;
                m.outputs.get(&pid).copied()
            }
            "module" => {
                let m = self.modules.get(&module_id()?)?;
                Some((m.box_[0] + m.box_[2] * 0.5, m.box_[1] + m.box_[3] * 0.5))
            }
            "point" => Some((
                target.get("x").and_then(|x| x.as_f64())?,
                target.get("y").and_then(|x| x.as_f64())?,
            )),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn resolve_port_ref(&self, port_ref: &Value) -> Option<(f64, f64)> {
        let m = self.modules.get(&port_ref.get("moduleId").and_then(|x| x.as_i64())?)?;
        let pid = port_ref.get("portId").and_then(|x| x.as_i64())?;
        match port_ref.get("port").and_then(|x| x.as_str())? {
            "input" => m.inputs.get(&pid).copied(),
            "output" => m.outputs.get(&pid).copied(),
            _ => None,
        }
    }
}

fn collect(dst: &mut HashMap<i64, (f64, f64)>, src: Option<&Value>, id_key: &str) {
    let Some(arr) = src.and_then(|x| x.as_array()) else { return; };
    for it in arr {
        let Some(id) = it.get(id_key).and_then(|x| x.as_i64()) else { continue; };
        let x = it.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let y = it.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0);
        dst.insert(id, (x, y));
    }
}
