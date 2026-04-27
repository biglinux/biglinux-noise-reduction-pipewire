//! Declarative descriptions of every node we emit into a PipeWire filter
//! graph, plus a tiny formatter that renders each node into its canonical
//! `nodes = [ ... ]` entry.
//!
//! Two flavours of node exist:
//!
//! 1. **Builtin** — compiled into libpipewire's filter-chain module
//!    (`dcblock`, `bq_highpass`, `param_eq`, `mixer`, `copy`,
//!    `mult`, `linear`, …). Identified by a plugin value of `"builtin"`.
//! 2. **LADSPA** — external shared objects located under
//!    `/usr/lib/ladspa/`. The filter-chain module accepts either the
//!    bare library stem or its full path.
//!
//! Every node carries its canonical in/out port names so the graph-level
//! linker does not have to special-case per filter.

use std::fmt::Write as _;

/// Source of a filter node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    /// PipeWire `filter-chain` built-in. Rendered with `plugin = "builtin"`.
    Builtin,
    /// External LADSPA plugin loaded from `/usr/lib/ladspa/<stem>.so`.
    Ladspa { plugin_stem: &'static str },
}

/// A single node emitted inside `filter.graph.nodes`.
#[derive(Debug, Clone)]
pub struct Node {
    pub name: String,
    pub kind: NodeKind,
    pub label: &'static str,
    pub input_port: &'static str,
    pub output_port: &'static str,
    pub controls: Vec<(&'static str, f64)>,
    /// Optional `config = { … }` block (used by `param_eq`).
    pub config: Option<String>,
}

impl Node {
    pub fn builtin(name: impl Into<String>, label: &'static str) -> Self {
        Self {
            name: name.into(),
            kind: NodeKind::Builtin,
            label,
            input_port: "In",
            output_port: "Out",
            controls: Vec::new(),
            config: None,
        }
    }

    pub fn ladspa(name: impl Into<String>, plugin_stem: &'static str, label: &'static str) -> Self {
        Self {
            name: name.into(),
            kind: NodeKind::Ladspa { plugin_stem },
            label,
            input_port: "Input",
            output_port: "Output",
            controls: Vec::new(),
            config: None,
        }
    }

    pub fn with_ports(mut self, input: &'static str, output: &'static str) -> Self {
        self.input_port = input;
        self.output_port = output;
        self
    }

    pub fn with_controls(
        mut self,
        controls: impl IntoIterator<Item = (&'static str, f64)>,
    ) -> Self {
        self.controls = controls.into_iter().collect();
        self
    }

    pub fn with_config(mut self, block: String) -> Self {
        self.config = Some(block);
        self
    }

    /// Render this node as a single entry inside the `nodes = [ … ]` array,
    /// prefixed by `indent` spaces on each line so the parent graph can
    /// control overall formatting.
    #[must_use]
    pub fn render(&self, indent: usize) -> String {
        let pad = " ".repeat(indent);
        let mut out = String::new();
        let _ = writeln!(out, "{pad}{{");
        let _ = writeln!(out, "{pad}    type = {}", self.kind.type_str());
        let _ = writeln!(out, "{pad}    name = \"{}\"", self.name);
        let _ = writeln!(out, "{pad}    plugin = \"{}\"", self.kind.plugin_str());
        let _ = writeln!(out, "{pad}    label = \"{}\"", self.label);
        if !self.controls.is_empty() {
            let _ = writeln!(out, "{pad}    control = {{");
            for (k, v) in &self.controls {
                let _ = writeln!(out, "{pad}        \"{k}\" = {}", format_f64(*v));
            }
            let _ = writeln!(out, "{pad}    }}");
        }
        if let Some(cfg) = &self.config {
            // `cfg` is already rendered; indent each line.
            for line in cfg.lines() {
                let _ = writeln!(out, "{pad}    {line}");
            }
        }
        let _ = writeln!(out, "{pad}}}");
        out
    }
}

impl NodeKind {
    fn type_str(self) -> &'static str {
        match self {
            Self::Builtin => "builtin",
            Self::Ladspa { .. } => "ladspa",
        }
    }

    fn plugin_str(self) -> &'static str {
        match self {
            Self::Builtin => "builtin",
            Self::Ladspa { plugin_stem } => plugin_stem,
        }
    }
}

/// Format a float without trailing zeros while keeping enough precision to
/// survive a round trip through PipeWire's config parser.
#[must_use]
pub fn format_f64(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{v:.1}")
    } else {
        format!("{v}")
    }
}

// ── Canonical LADSPA plugin stems ────────────────────────────────────

pub const LADSPA_GTCRN: &str = "/usr/lib/ladspa/libgtcrn_ladspa.so";
pub const LADSPA_SWH_GATE: &str = "/usr/lib/ladspa/gate_1410.so";
pub const LADSPA_SC4_MONO: &str = "/usr/lib/ladspa/sc4m_1916.so";
/// Steve Harris pitch shifter (`pitch_scale_1193`). Phase-vocoder
/// based: changes pitch without retiming. Mono in / mono out, control
/// `Pitch co-efficient` ranges 0.5..=2.0 (1.0 = passthrough).
pub const LADSPA_PITCH_SCALE: &str = "/usr/lib/ladspa/pitch_scale_1193.so";
/// Steve Harris simple amplifier (`amp_1181`). Used after the pitch
/// shifter to compensate for the perceived loudness shift (deep voice
/// loses energy, high voice clips). Control `Amps gain (dB)` is the
/// only knob.
pub const LADSPA_AMP: &str = "/usr/lib/ladspa/amp_1181.so";

// ── Canonical LADSPA labels ──────────────────────────────────────────

/// GTCRN mono speech-enhancement plugin. Exposes every processing control
/// (strength, speech strength, lookahead, model, blend, voice recovery)
/// plus an integrated gate section that we use on the mic chain.
pub const LABEL_GTCRN_MONO: &str = "gtcrn_mono";

/// Steve Harris gate (`gate_1410`). Used on the output chain because it
/// does not require the GTCRN plugin's initialisation and is cheaper when
/// noise reduction is disabled.
pub const LABEL_SWH_GATE: &str = "gate";

/// SC4 mono compressor.
pub const LABEL_SC4_MONO: &str = "sc4m";

/// Pitch-scaler LADSPA label (`pitch_scale_1193`).
pub const LABEL_PITCH_SCALE: &str = "pitchScale";

/// Simple amplifier LADSPA label (`amp_1181`).
pub const LABEL_AMP: &str = "amp";

// ── Shared builtin labels ────────────────────────────────────────────

pub const LABEL_BQ_HIGHPASS: &str = "bq_highpass";
pub const LABEL_PARAM_EQ: &str = "param_eq";
pub const LABEL_MIXER: &str = "mixer";
pub const LABEL_COPY: &str = "copy";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_render_contains_type_and_plugin() {
        let n = Node::builtin("m", LABEL_MIXER);
        let s = n.render(0);
        assert!(s.contains("type = builtin"));
        assert!(s.contains("plugin = \"builtin\""));
        assert!(s.contains("label = \"mixer\""));
        assert!(s.contains("name = \"m\""));
    }

    #[test]
    fn ladspa_render_uses_plugin_stem() {
        let n = Node::ladspa("ai", LADSPA_GTCRN, LABEL_GTCRN_MONO);
        let s = n.render(0);
        assert!(s.contains("type = ladspa"));
        assert!(s.contains(&format!("plugin = \"{LADSPA_GTCRN}\"")));
        assert!(s.contains("label = \"gtcrn_mono\""));
    }

    #[test]
    fn render_includes_controls_in_order() {
        let n =
            Node::builtin("hp", LABEL_BQ_HIGHPASS).with_controls([("Freq", 40.0), ("Q", 0.707)]);
        let s = n.render(0);
        let freq_idx = s.find("\"Freq\"").unwrap();
        let q_idx = s.find("\"Q\"").unwrap();
        assert!(freq_idx < q_idx);
        assert!(s.contains("\"Freq\" = 40.0"));
        assert!(s.contains("\"Q\" = 0.707"));
    }

    #[test]
    fn render_omits_control_block_when_empty() {
        let n = Node::builtin("cp", LABEL_COPY);
        let s = n.render(0);
        assert!(!s.contains("control"));
    }

    #[test]
    fn render_respects_custom_ports() {
        let n = Node::ladspa("g", LADSPA_SWH_GATE, LABEL_SWH_GATE);
        assert_eq!(n.input_port, "Input");
        assert_eq!(n.output_port, "Output");
        let n = Node::builtin("eq", LABEL_PARAM_EQ).with_ports("In 1", "Out 1");
        assert_eq!(n.input_port, "In 1");
        assert_eq!(n.output_port, "Out 1");
    }

    #[test]
    fn render_with_indent_prefix() {
        let n = Node::builtin("c", LABEL_COPY);
        let s = n.render(4);
        for line in s.lines().filter(|l| !l.is_empty()) {
            assert!(line.starts_with("    "));
        }
    }

    #[test]
    fn format_f64_drops_trailing_zero() {
        assert_eq!(format_f64(40.0), "40.0");
        assert_eq!(format_f64(0.707), "0.707");
        assert_eq!(format_f64(-30.0), "-30.0");
    }
}
