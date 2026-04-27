//! Assemble [`super::nodes::Node`] instances into a complete filter graph
//! and render them inside the `args = { … }` block of a
//! `libpipewire-module-filter-chain` module declaration.
//!
//! The module keeps layout concerns (indentation, quoting, canonical
//! inputs/outputs binding) out of the per-chain builders so `mic.rs` and
//! `output.rs` can focus on which nodes belong to each pipeline.

use std::fmt::Write as _;

use super::nodes::Node;

/// How the emitted `.conf` is going to be consumed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderMode {
    /// Drop-in file under `~/.config/pipewire/filter-chain.conf.d/`,
    /// loaded by the user-level `filter-chain.service` alongside every
    /// other drop-in.
    DropIn,
    /// Standalone `pipewire -c` invocation. Emits its own
    /// `context.properties`, SPA lib table, and the minimum set of
    /// PipeWire modules needed to host a filter chain. Required for
    /// pipelines that can't share a process with the main filter-chain
    /// daemon (for example, each one needs its own GTCRN singleton).
    Standalone,
}

/// One link inside `filter.graph.links`.
#[derive(Debug, Clone)]
pub struct Link {
    pub output: String,
    pub input: String,
}

impl Link {
    pub fn new(output: impl Into<String>, input: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            input: input.into(),
        }
    }
}

/// A fully-formed filter graph together with its module-level properties.
#[derive(Debug, Clone)]
pub struct Graph {
    pub description: String,
    pub media_name: String,
    pub nodes: Vec<Node>,
    pub links: Vec<Link>,
    /// Entries rendered inside `inputs = [ … ]`.
    pub inputs: Vec<String>,
    /// Entries rendered inside `outputs = [ … ]`.
    pub outputs: Vec<String>,
    /// Raw body of `capture.props = { … }`, rendered as-is.
    pub capture_props: String,
    /// Raw body of `playback.props = { … }`, rendered as-is.
    pub playback_props: String,
}

impl Graph {
    /// Render the graph as a complete `.conf` file.
    ///
    /// * [`RenderMode::DropIn`] emits only the `context.modules`
    ///   wrapper that `filter-chain.service` expects from every drop-in
    ///   file in `filter-chain.conf.d/`.
    /// * [`RenderMode::Standalone`] additionally prepends the
    ///   `context.properties`, `context.spa-libs`, and protocol/adapter
    ///   modules required by a bare `pipewire -c` invocation.
    #[must_use]
    pub fn render(&self, mode: RenderMode) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "# BigLinux Microphone — auto-generated config");
        let _ = writeln!(
            out,
            "# DO NOT EDIT: this file is rebuilt every time settings change."
        );
        let _ = writeln!(out);

        if mode == RenderMode::Standalone {
            // Minimum context a dedicated `pipewire` instance needs to
            // host a filter-chain. The clock block locks the standalone
            // graph at 48 kHz / 1024-frame quantum so neural inference
            // sees stable buffer sizes; min/max bracket what the host
            // can negotiate without re-allocating GTCRN buffers.
            out.push_str("context.properties = {\n");
            out.push_str("    log.level = 0\n");
            out.push_str("    default.clock.rate          = 48000\n");
            out.push_str("    default.clock.quantum       = 1024\n");
            out.push_str("    default.clock.min-quantum   = 256\n");
            out.push_str("    default.clock.max-quantum   = 8192\n");
            out.push_str("}\n\n");
            out.push_str("context.spa-libs = {\n");
            out.push_str("    audio.convert.* = audioconvert/libspa-audioconvert\n");
            out.push_str("    support.*       = support/libspa-support\n");
            out.push_str("}\n\n");
        }

        let _ = writeln!(out, "context.modules = [");
        if mode == RenderMode::Standalone {
            // Realtime scheduling for the standalone PipeWire instance.
            // nice.level/rt.prio mirror /usr/share/pipewire/pipewire.conf
            // so our filter chain runs at the same priority class as the
            // system PipeWire daemon — neither pre-empts the other under
            // load. rt.time is intentionally left unset so the PAM
            // RLIMIT_RTTIME (200 ms by default on rtkit/realtime-privileges
            // distros) takes effect; hard-coding a cap here would override
            // a sysadmin-tuned limit.
            out.push_str("    { name = libpipewire-module-rt\n");
            out.push_str("        args = {\n");
            out.push_str("            nice.level    = -11\n");
            out.push_str("            rt.prio       = 88\n");
            out.push_str("        }\n");
            out.push_str("        flags = [ ifexists nofail ]\n");
            out.push_str("    }\n");
            out.push_str("    { name = libpipewire-module-protocol-native }\n");
            out.push_str("    { name = libpipewire-module-client-node }\n");
            out.push_str("    { name = libpipewire-module-adapter }\n");
        }
        let _ = writeln!(out, "    {{");
        let _ = writeln!(out, "        name = libpipewire-module-filter-chain");
        let _ = writeln!(out, "        args = {{");
        let _ = writeln!(
            out,
            "            node.description = \"{}\"",
            self.description
        );
        let _ = writeln!(out, "            media.name = \"{}\"", self.media_name);
        let _ = writeln!(out, "            filter.graph = {{");

        // ── nodes ────────────────────────────────────────────────
        let _ = writeln!(out, "                nodes = [");
        for node in &self.nodes {
            out.push_str(&node.render(20));
        }
        let _ = writeln!(out, "                ]");

        // ── links ────────────────────────────────────────────────
        let _ = writeln!(out, "                links = [");
        for link in &self.links {
            let _ = writeln!(
                out,
                "                    {{ output = \"{}\" input = \"{}\" }}",
                link.output, link.input,
            );
        }
        let _ = writeln!(out, "                ]");

        // ── graph i/o ────────────────────────────────────────────
        let _ = write!(out, "                inputs = [");
        for i in &self.inputs {
            let _ = write!(out, " \"{i}\"");
        }
        let _ = writeln!(out, " ]");

        let _ = write!(out, "                outputs = [");
        for o in &self.outputs {
            let _ = write!(out, " \"{o}\"");
        }
        let _ = writeln!(out, " ]");

        let _ = writeln!(out, "            }}");

        // ── props ────────────────────────────────────────────────
        let _ = writeln!(out, "            capture.props = {{");
        for line in self.capture_props.lines() {
            let _ = writeln!(out, "                {line}");
        }
        let _ = writeln!(out, "            }}");

        let _ = writeln!(out, "            playback.props = {{");
        for line in self.playback_props.lines() {
            let _ = writeln!(out, "                {line}");
        }
        let _ = writeln!(out, "            }}");

        let _ = writeln!(out, "        }}");
        let _ = writeln!(out, "    }}");
        let _ = writeln!(out, "]");
        out
    }
}

#[cfg(test)]
mod tests {
    use super::super::nodes::{Node, LABEL_COPY, LABEL_MIXER};
    use super::*;

    fn sample_graph() -> Graph {
        Graph {
            description: "Test Graph".into(),
            media_name: "Test Graph".into(),
            nodes: vec![
                Node::builtin("m", LABEL_MIXER).with_ports("In 1", "Out"),
                Node::builtin("c", LABEL_COPY),
            ],
            links: vec![Link::new("m:Out", "c:In")],
            inputs: vec!["m:In 1".into()],
            outputs: vec!["c:Out".into()],
            capture_props: "node.name = \"cap\"\nmedia.class = Audio/Source".into(),
            playback_props: "node.name = \"pb\"\nnode.passive = true".into(),
        }
    }

    #[test]
    fn render_wraps_in_context_modules_block() {
        let g = sample_graph();
        let s = g.render(RenderMode::DropIn);
        assert!(s.starts_with("# BigLinux"));
        assert!(s.contains("context.modules = ["));
        assert!(s.contains("libpipewire-module-filter-chain"));
        assert!(s.contains("filter.graph = {"));
        assert!(s.contains("nodes = ["));
        assert!(s.contains("links = ["));
        assert!(s.trim_end().ends_with(']'));
    }

    #[test]
    fn render_contains_declared_links() {
        let g = sample_graph();
        let s = g.render(RenderMode::DropIn);
        assert!(s.contains(r#"{ output = "m:Out" input = "c:In" }"#));
    }

    #[test]
    fn render_contains_inputs_and_outputs() {
        let g = sample_graph();
        let s = g.render(RenderMode::DropIn);
        assert!(s.contains(r#"inputs = [ "m:In 1" ]"#));
        assert!(s.contains(r#"outputs = [ "c:Out" ]"#));
    }

    #[test]
    fn render_indents_custom_capture_props() {
        let g = sample_graph();
        let s = g.render(RenderMode::DropIn);
        assert!(s.contains("capture.props = {"));
        assert!(s.contains("                node.name = \"cap\""));
        assert!(s.contains("            playback.props = {"));
    }

    #[test]
    fn standalone_mode_prepends_context_and_protocol_modules() {
        let g = sample_graph();
        let s = g.render(RenderMode::Standalone);
        assert!(s.contains("context.properties = {"));
        assert!(s.contains("context.spa-libs = {"));
        assert!(s.contains("libpipewire-module-protocol-native"));
        assert!(s.contains("libpipewire-module-client-node"));
        assert!(s.contains("libpipewire-module-adapter"));
        assert!(s.contains("libpipewire-module-filter-chain"));
    }

    #[test]
    fn dropin_mode_omits_standalone_preamble() {
        let g = sample_graph();
        let s = g.render(RenderMode::DropIn);
        assert!(!s.contains("context.properties"));
        assert!(!s.contains("context.spa-libs"));
        assert!(!s.contains("protocol-native"));
    }
}
