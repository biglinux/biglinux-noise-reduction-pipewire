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
    /// Bare module-args body — just the `{ … }` block that
    /// `biglinux-microphone-pwloader` reads and passes verbatim to
    /// `pw_context_load_module()` as its `args` parameter. No comment
    /// header, no `context.modules` wrapper. Each filter-chain (mic,
    /// output, AEC) lives in its own pwloader process that connects as
    /// a client of the main PipeWire daemon, so all three filter graphs
    /// share the daemon's clock — no cross-process drift, no
    /// `spa.alsa: front:1p ... resync` events.
    ModuleArgs,
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
    /// Render the graph as the `args = { … }` body of a
    /// `libpipewire-module-filter-chain` declaration.
    ///
    /// `biglinux-microphone-pwloader` reads this file verbatim and
    /// passes it as the `args` C-string to `pw_context_load_module()`.
    /// No `context.modules` wrapper, no `# DO NOT EDIT` header — the
    /// loader is the only consumer.
    #[must_use]
    pub fn render(&self, _mode: RenderMode) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "{{");
        let _ = writeln!(out, "    node.description = \"{}\"", self.description);
        let _ = writeln!(out, "    media.name = \"{}\"", self.media_name);
        let _ = writeln!(out, "    filter.graph = {{");

        // ── nodes ────────────────────────────────────────────────
        let _ = writeln!(out, "        nodes = [");
        for node in &self.nodes {
            out.push_str(&node.render(12));
        }
        let _ = writeln!(out, "        ]");

        // ── links ────────────────────────────────────────────────
        let _ = writeln!(out, "        links = [");
        for link in &self.links {
            let _ = writeln!(
                out,
                "            {{ output = \"{}\" input = \"{}\" }}",
                link.output, link.input,
            );
        }
        let _ = writeln!(out, "        ]");

        // ── graph i/o ────────────────────────────────────────────
        let _ = write!(out, "        inputs = [");
        for i in &self.inputs {
            let _ = write!(out, " \"{i}\"");
        }
        let _ = writeln!(out, " ]");

        let _ = write!(out, "        outputs = [");
        for o in &self.outputs {
            let _ = write!(out, " \"{o}\"");
        }
        let _ = writeln!(out, " ]");

        let _ = writeln!(out, "    }}");

        // ── props ────────────────────────────────────────────────
        let _ = writeln!(out, "    capture.props = {{");
        for line in self.capture_props.lines() {
            let _ = writeln!(out, "        {line}");
        }
        let _ = writeln!(out, "    }}");

        let _ = writeln!(out, "    playback.props = {{");
        for line in self.playback_props.lines() {
            let _ = writeln!(out, "        {line}");
        }
        let _ = writeln!(out, "    }}");

        let _ = writeln!(out, "}}");
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
    fn render_emits_module_args_body_only() {
        // The pwloader passes the file contents verbatim as the module
        // `args` C-string, so the rendered form must be just `{ … }` —
        // no `context.modules` wrapper, no `# DO NOT EDIT` header, no
        // bootstrap modules. The whole output is a single SPA-JSON
        // object the filter-chain module knows how to parse.
        let g = sample_graph();
        let s = g.render(RenderMode::ModuleArgs);
        assert!(s.starts_with('{'));
        assert!(!s.contains("context.modules"));
        assert!(!s.contains("# BigLinux"));
        assert!(!s.contains("libpipewire-module-filter-chain"));
        assert!(s.contains("filter.graph = {"));
        assert!(s.contains("nodes = ["));
        assert!(s.contains("links = ["));
        assert!(s.trim_end().ends_with('}'));
    }

    #[test]
    fn render_contains_declared_links() {
        let g = sample_graph();
        let s = g.render(RenderMode::ModuleArgs);
        assert!(s.contains(r#"{ output = "m:Out" input = "c:In" }"#));
    }

    #[test]
    fn render_contains_inputs_and_outputs() {
        let g = sample_graph();
        let s = g.render(RenderMode::ModuleArgs);
        assert!(s.contains(r#"inputs = [ "m:In 1" ]"#));
        assert!(s.contains(r#"outputs = [ "c:Out" ]"#));
    }

    #[test]
    fn render_indents_custom_capture_props() {
        let g = sample_graph();
        let s = g.render(RenderMode::ModuleArgs);
        assert!(s.contains("capture.props = {"));
        assert!(s.contains("        node.name = \"cap\""));
        assert!(s.contains("    playback.props = {"));
    }

    #[test]
    fn render_omits_bootstrap_modules() {
        // Bootstrap modules (rt, protocol-native, adapter, client-node)
        // come from the daemon `client.conf` we connect to — never from
        // our args. Including them here would either no-op or
        // double-load.
        let g = sample_graph();
        let s = g.render(RenderMode::ModuleArgs);
        assert!(!s.contains("context.properties"));
        assert!(!s.contains("context.spa-libs"));
        assert!(!s.contains("protocol-native"));
        assert!(!s.contains("libpipewire-module-rt"));
        assert!(!s.contains("libpipewire-module-adapter"));
    }
}
