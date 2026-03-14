#!/usr/bin/env python3
"""
PipeWire filter-chain configuration generator.

Generates dynamic PipeWire filter-chain configurations based on user settings.
Supports:
- GTCRN AI noise reduction
- Gate filter
- Stereo enhancement modes
- Voice pitch shifting
"""

from __future__ import annotations

import logging
from dataclasses import dataclass, field
from pathlib import Path

from biglinux_microphone.config import (
    NoiseModel,
    StereoMode,
    get_model_control_value,
)

logger = logging.getLogger(__name__)

# ============================================================================
# LADSPA Plugin Constants - VERIFIED ON SYSTEM
# ============================================================================

# GTCRN AI Noise Reduction
GTCRN_LIBRARY = "/usr/lib/ladspa/libgtcrn_ladspa.so"
GTCRN_LABEL = "gtcrn_mono"


# Gate Filter (Steve Harris gate_1410)
GATE_LIBRARY = "/usr/lib/ladspa/gate_1410.so"
GATE_LABEL = "gate"

# Mono to Stereo Split (split_1406)
SPLIT_LIBRARY = "/usr/lib/ladspa/split_1406.so"
SPLIT_LABEL = "split"

# Lowpass IIR Filter (lowpass_iir_1891) - for natural sound on delayed channel
LOWPASS_LIBRARY = "/usr/lib/ladspa/lowpass_iir_1891.so"
LOWPASS_LABEL = "lowpass_iir"

# GVerb Reverb (gverb_1216) - mono to stereo reverb
GVERB_LIBRARY = "/usr/lib/ladspa/gverb_1216.so"
GVERB_LABEL = "gverb"

# Matrix Spatialiser (matrix_spatialiser_1422) - stereo width control
SPATIALISER_LIBRARY = "/usr/lib/ladspa/matrix_spatialiser_1422.so"
SPATIALISER_LABEL = "matrixSpatialiser"

# Amplifier (amp_1181) - for gain compensation
AMP_LIBRARY = "/usr/lib/ladspa/amp_1181.so"
AMP_LABEL = "amp"

# SC4 Mono Compressor (sc4m_1916) - professional compressor for volume normalization
SC4_LIBRARY = "/usr/lib/ladspa/sc4m_1916.so"
SC4_LABEL = "sc4m"


# Pitch Scale (pitch_scale_1193) - high quality pitch shifting
PITCH_SCALE_LIBRARY = "/usr/lib/ladspa/pitch_scale_1193.so"
PITCH_SCALE_LABEL = "pitchScale"


# Multiband EQ (mbeq_1197) - 15-band graphic equalizer
MBEQ_LIBRARY = "/usr/lib/ladspa/mbeq_1197.so"
MBEQ_LABEL = "mbeq"

# Hard Limiter (hard_limiter_1413) - safety limiter to prevent clipping
LIMITER_LIBRARY = "/usr/lib/ladspa/hard_limiter_1413.so"
LIMITER_LABEL = "hardLimiter"

# MBEQ frequency bands (Hz) - plugin has 15 bands
MBEQ_BANDS = [
    50,
    100,
    156,
    220,
    311,
    440,
    622,
    880,
    1250,
    1750,
    2500,
    3500,
    5000,
    10000,
    20000,
]

# Our UI uses 10 bands - map to closest MBEQ bands
# UI bands: [31, 63, 125, 250, 500, 1000, 2000, 4000, 8000, 16000] Hz
# MBEQ map: [50, 100, 156, 311, 622, 1250, 2500, 5000, 10000, 20000] indices: [0, 1, 2, 4, 6, 8, 10, 12, 13, 14]
EQ_BAND_TO_MBEQ_INDEX = [0, 1, 2, 4, 6, 8, 10, 12, 13, 14]

# MBEQ parameter names for PipeWire set-param (must match LADSPA port names exactly)
MBEQ_PARAM_NAMES = [
    "50Hz gain (low shelving)",
    "100Hz gain",
    "156Hz gain",
    "220Hz gain",
    "311Hz gain",
    "440Hz gain",
    "622Hz gain",
    "880Hz gain",
    "1250Hz gain",
    "1750Hz gain",
    "2500Hz gain",
    "3500Hz gain",
    "5000Hz gain",
    "10000Hz gain",
    "20000Hz gain",
]

# Config output path - Use filter-chain.conf.d for separate filter-chain process
# This is loaded by running: pipewire -c filter-chain.conf
CONFIG_DIR = Path.home() / ".config" / "pipewire" / "filter-chain.conf.d"
CONFIG_FILE = "source-gtcrn-smart.conf"

# Device name shown to user in audio settings
DEVICE_NAME = "Noise Canceling Microphone"
FILTER_SMART_NAME = "big.filter-microphone"


# ============================================================================
# Configuration Data Class
# ============================================================================


@dataclass
class FilterChainConfig:
    """
    Complete filter chain configuration.

    All parameters needed to generate a PipeWire filter-chain config.
    """

    # High-pass filter (rumble removal, before AI)
    hpf_enabled: bool = True
    hpf_frequency: float = 80.0

    # Noise reduction
    noise_reduction_enabled: bool = True
    noise_reduction_model: NoiseModel = NoiseModel.GTCRN_DNS3
    noise_reduction_strength: float = 1.0
    noise_reduction_speech_strength: float = 1.0
    noise_reduction_lookahead_ms: int = 100
    noise_reduction_voice_enhance: float = 0.50
    noise_reduction_model_blending: float = 0.0
    noise_reduction_noise_gate: float = 0.5
    # Compressor (after AI, before gate)
    compressor_enabled: bool = False
    compressor_threshold_db: float = -20.0
    compressor_ratio: float = 3.0
    compressor_attack_ms: float = 10.0
    compressor_release_ms: float = 100.0
    compressor_makeup_gain_db: float = 6.0
    compressor_knee_db: float = 3.0
    compressor_rms_peak: float = 0.2

    # Gate filter
    gate_enabled: bool = True
    gate_threshold_db: int = -30  # Level below which gate closes
    gate_range_db: int = (
        -60
    )  # Reduction amount when gate closes (more negative = quieter)
    gate_attack_ms: float = 10.0  # Fast attack to catch speech
    gate_hold_ms: float = 400.0  # Hold time to avoid choppy speech
    gate_release_ms: float = 200.0  # Release time for smooth transitions

    # Stereo enhancement
    stereo_mode: StereoMode = StereoMode.MONO
    stereo_width: float = 0.7

    crossfeed_enabled: bool = False
    crossfeed_level: float = 0.3

    # Equalizer
    eq_enabled: bool = False
    eq_bands: list[float] = field(default_factory=lambda: [0.0] * 10)

    # Echo cancellation (WebRTC AEC)
    echo_cancel_enabled: bool = False
    source_node_name: str = ""  # Hardware mic node.name for AEC routing
    echo_cancel_gain_control: bool = True
    echo_cancel_noise_suppression: bool = False
    echo_cancel_voice_detection: bool = True


# ============================================================================
# Filter Chain Generator
# ============================================================================


class FilterChainGenerator:
    """
    Generates PipeWire filter-chain configuration files.

    Creates a complete audio processing pipeline based on FilterChainConfig.
    """

    def __init__(self, config: FilterChainConfig) -> None:
        """
        Initialize the generator.

        Args:
            config: Filter chain configuration
        """
        self._config = config

    def _generate_echo_cancel_module(self) -> str:
        """Generate the echo-cancel PipeWire module block.

        Uses WebRTC AEC with monitor.mode to capture the default sink
        output as reference signal for echo cancellation.
        The AEC capture must target the hardware mic explicitly via
        target.object to prevent WirePlumber from routing it to the
        filter chain output (which would create a feedback loop).
        Latency is 960/48000 (20ms = 2x WebRTC 480-sample frames).
        """
        capture_target = ""
        if self._config.source_node_name:
            safe_name = self._config.source_node_name.replace('"', '')
            capture_target = f'\n                target.object = "{safe_name}"'
        gc = "true" if self._config.echo_cancel_gain_control else "false"
        ns = "true" if self._config.echo_cancel_noise_suppression else "false"
        vd = "true" if self._config.echo_cancel_voice_detection else "false"
        return f"""    {{
        name = libpipewire-module-echo-cancel
        args = {{
            library.name = aec/libspa-aec-webrtc
            node.latency = 960/48000
            monitor.mode = true
            audio.channels = 1
            audio.rate = 48000
            buffer.max_size = 250
            aec.args = {{
                webrtc.gain_control = {gc}
                webrtc.noise_suppression = {ns}
                webrtc.voice_detection = {vd}
            }}
            capture.props = {{
                node.name = "big-aec-capture"{capture_target}
            }}
            source.props = {{
                node.name = "big-aec-source"
                node.description = "BigLinux AEC"
                media.class = "Audio/Source"
            }}
        }}
    }},"""

    def _get_capture_props(self) -> str:
        """Get capture.props block, targeting AEC source when echo cancel is active.

        When AEC is enabled, node.passive is omitted so WirePlumber
        respects target.object and routes the filter chain input
        to big-aec-source instead of the hardware mic.
        """
        if self._config.echo_cancel_enabled:
            return """            capture.props = {
                target.object = "big-aec-source"
            }"""
        return """            capture.props = {
                node.passive = true
            }"""

    def _get_playback_props(self, extra_props: str = "") -> str:
        """Get playback.props block.

        When AEC is enabled, filter.smart is removed because WirePlumber's
        smart filter routing overrides capture target.object. Without
        filter.smart, the node is a regular Audio/Source and PipeWire
        respects target.object for capture routing.
        """
        if self._config.echo_cancel_enabled:
            return f"""            playback.props = {{
                node.pause-on-idle = false
                node.latency = 960/48000
                audio.rate = 48000{extra_props}
                media.class = "Audio/Source"
                node.name = "big-noise-canceling-output"
                node.description = "{DEVICE_NAME}"
            }}"""
        return f"""            playback.props = {{
                node.pause-on-idle = false
                node.latency = 1024/48000
                audio.rate = 48000{extra_props}
                filter.smart = true
                media.class = "Audio/Source"
                filter.smart.name = "{FILTER_SMART_NAME}"
            }}"""

    def generate(self) -> str:
        """
        Generate complete filter-chain configuration.

        Returns:
            str: PipeWire filter-chain config content
        """
        is_stereo = self._config.stereo_mode != StereoMode.MONO

        if is_stereo:
            return self._generate_stereo_config()
        else:
            return self._generate_mono_config()

    def _generate_gate_node(self) -> str:
        """Generate gate filter node."""
        c = self._config

        return f'''                    {{
                        type = ladspa
                        name = "gate"
                        plugin = "{GATE_LIBRARY}"
                        label = "{GATE_LABEL}"
                        control = {{
                            "Threshold (dB)" = {c.gate_threshold_db}
                            "Attack (ms)" = {c.gate_attack_ms}
                            "Hold (ms)" = {c.gate_hold_ms}
                            "Decay (ms)" = {c.gate_release_ms}
                            "Range (dB)" = {c.gate_range_db}
                            "LF key filter (Hz)" = 20.0
                            "HF key filter (Hz)" = 20000.0
                            "Output select (-1 = key listen, 0 = gate, 1 = bypass)" = 0
                        }}
                    }}'''

    def _generate_ai_node(self) -> str:
        """Generate AI noise reduction node (GTCRN)."""
        c = self._config

        library = GTCRN_LIBRARY
        label = GTCRN_LABEL

        # Get the control value within the plugin's model list
        model_control = get_model_control_value(c.noise_reduction_model)

        return f'''                    {{
                        type = ladspa
                        name = "ai"
                        plugin = "{library}"
                        label = "{label}"
                        control = {{
                            "Enable" = 1.0
                            "Strength" = {c.noise_reduction_strength}
                            "Model" = {model_control}
                            "SpeechStrength" = {c.noise_reduction_speech_strength}
                            "LookaheadMs" = {c.noise_reduction_lookahead_ms}
                            "VoiceEnhance" = {c.noise_reduction_voice_enhance}
                            "ModelBlend" = {c.noise_reduction_model_blending}
                            "NoiseGate" = {c.noise_reduction_noise_gate}
                        }}
                    }}'''

    def _generate_hpf_node(self, bypass: bool = False) -> str:
        """Generate high-pass filter node (PipeWire builtin bq_highpass).

        Args:
            bypass: If True, set frequency to 5 Hz (inaudible passthrough).
        """
        c = self._config
        freq = 5.0 if bypass else c.hpf_frequency
        return f"""                    {{
                        type = builtin
                        name = "hpf"
                        label = bq_highpass
                        control = {{ "Freq" = {freq:.1f} "Q" = 0.707 }}
                    }}"""

    def _generate_compressor_node(self) -> str:
        """Generate SC4 mono compressor node."""
        c = self._config
        return f'''                    {{
                        type = ladspa
                        name = "compressor"
                        plugin = "{SC4_LIBRARY}"
                        label = "{SC4_LABEL}"
                        control = {{
                            "RMS/peak" = {c.compressor_rms_peak:.2f}
                            "Attack time (ms)" = {c.compressor_attack_ms:.1f}
                            "Release time (ms)" = {c.compressor_release_ms:.1f}
                            "Threshold level (dB)" = {c.compressor_threshold_db:.1f}
                            "Ratio (1:n)" = {c.compressor_ratio:.1f}
                            "Knee radius (dB)" = {c.compressor_knee_db:.1f}
                            "Makeup gain (dB)" = {c.compressor_makeup_gain_db:.1f}
                        }}
                    }}'''

    def _generate_limiter_node(self) -> str:
        """Generate hard limiter node to prevent output clipping.

        Always active when the compressor is enabled.
        Limit at -1 dB prevents digital clipping while remaining
        transparent for signals that are already within range.
        """
        return f'''                    {{
                        type = ladspa
                        name = "limiter"
                        plugin = "{LIMITER_LIBRARY}"
                        label = "{LIMITER_LABEL}"
                        control = {{
                            "dB limit" = -1.0
                            "Wet level" = 1.0
                            "Residue level" = 0.0
                        }}
                    }}'''

    def _generate_passthrough_config(self) -> str:
        """Generate passthrough config when no filters are enabled."""
        aec_module = self._generate_echo_cancel_module() if self._config.echo_cancel_enabled else ""
        capture_props = self._get_capture_props()
        playback_props = self._get_playback_props()
        return f'''# BigLinux Microphone Enhanced Filter Chain
# Auto-generated configuration - Passthrough (no filters)

context.modules = [
{aec_module}
    {{
        name = libpipewire-module-filter-chain
        args = {{
            node.description = "{DEVICE_NAME}"
            media.name = "{DEVICE_NAME}"
            filter.graph = {{
                nodes = [
                    {{
                        type = builtin
                        name = "passthrough"
                        label = copy
                    }}
                ]
                links = []
                inputs = [ "passthrough:In" ]
                outputs = [ "passthrough:Out" ]
            }}
            audio.channels = 1
{capture_props}
{playback_props}
        }}
    }}
]
'''

    def _generate_mono_config(self) -> str:
        """Generate mono (non-stereo) configuration."""
        c = self._config

        # Build list of active filters in order:
        # hpf -> compressor -> ai -> eq -> limiter -> gate
        # HPF removes rumble first, compressor evens volume before AI, AI cleans noise,
        # EQ shapes tone, gate mutes residual noise in silence last.
        # NOTE: EQ must come before gate to work around a PipeWire 1.6.1 crash
        # when gate_1410.so is loaded before mbeq_1197.so in the filter graph.
        # Each entry: (name, input_port, output_port)
        active_filters: list[tuple[str, str, str]] = []
        nodes: list[str] = []

        # Always include HPF so frequency can be changed live via pw-cli.
        # When disabled, frequency is set to 5 Hz (inaudible passthrough).
        active_filters.append(("hpf", "In", "Out"))
        nodes.append(self._generate_hpf_node(bypass=not c.hpf_enabled))

        if c.compressor_enabled:
            active_filters.append(("compressor", "Input", "Output"))
            nodes.append(self._generate_compressor_node())

        if c.noise_reduction_enabled:
            active_filters.append(("ai", "Input", "Output"))
            nodes.append(self._generate_ai_node())

        # Always include EQ node (before gate) to avoid PipeWire 1.6.1 crash.
        # When disabled, all bands are 0 dB (transparent passthrough).
        active_filters.append(("eq", "Input", "Output"))
        nodes.append(self._generate_eq_node(bypass=not c.eq_enabled))

        # Hard limiter prevents output clipping when compressor
        # pushes a hot signal over 0 dBFS.
        if c.compressor_enabled:
            active_filters.append(("limiter", "Input", "Output"))
            nodes.append(self._generate_limiter_node())

        if c.gate_enabled:
            active_filters.append(("gate", "Input", "Output"))
            nodes.append(self._generate_gate_node())

        # If no filters enabled, return minimal passthrough config
        if not active_filters:
            return self._generate_passthrough_config()

        # Build links between active filters (respecting port name conventions)
        links = []
        for i in range(len(active_filters) - 1):
            src_name, _, src_out = active_filters[i]
            dst_name, dst_in, _ = active_filters[i + 1]
            links.append(
                f'{{ output = "{src_name}:{src_out}" input = "{dst_name}:{dst_in}" }}'
            )

        # Determine input and output nodes
        first_name, first_in, _ = active_filters[0]
        last_name, _, last_out = active_filters[-1]

        # Format nodes and links for config
        nodes_str = "\n".join(nodes)
        links_str = "\n                    ".join(links) if links else ""

        aec_module = self._generate_echo_cancel_module() if c.echo_cancel_enabled else ""
        capture_props = self._get_capture_props()
        playback_props = self._get_playback_props(
            extra_props='\n                audio.position = [ MONO ]'
        )

        return f'''# BigLinux Microphone Enhanced Filter Chain
# Auto-generated configuration - Mono output

context.modules = [
{aec_module}
    {{
        name = libpipewire-module-filter-chain
        args = {{
            node.description = "{DEVICE_NAME}"
            media.name = "{DEVICE_NAME}"
            filter.graph = {{
                nodes = [
{nodes_str}
                ]
                links = [
                    {links_str}
                ]
                inputs = [ "{first_name}:{first_in}" ]
                outputs = [ "{last_name}:{last_out}" ]
            }}
            audio.channels = 1
{capture_props}
{playback_props}

        }}
    }}
]
'''

    def _generate_eq_node(self, bypass: bool = False) -> str:
        """Generate the EQ LADSPA node configuration.

        Args:
            bypass: If True, all bands are set to 0 dB (passthrough).
                    Used to keep the node in the graph when EQ is disabled,
                    avoiding a PipeWire 1.6.1 crash on certain node counts.
        """
        c = self._config

        # Map our 10-band EQ to MBEQ's 15 bands
        # Create all 15 bands, using 0 for unmapped bands
        mbeq_values = [0.0] * 15
        if not bypass:
            for i, band_val in enumerate(c.eq_bands):
                if i < len(EQ_BAND_TO_MBEQ_INDEX):
                    mbeq_values[EQ_BAND_TO_MBEQ_INDEX[i]] = band_val

        return f'''
                    # Multiband EQ (15 bands)
                    {{
                        type = ladspa
                        name = "eq"
                        plugin = "{MBEQ_LIBRARY}"
                        label = "{MBEQ_LABEL}"
                        control = {{
                            "50Hz gain (low shelving)" = {mbeq_values[0]}
                            "100Hz gain" = {mbeq_values[1]}
                            "156Hz gain" = {mbeq_values[2]}
                            "220Hz gain" = {mbeq_values[3]}
                            "311Hz gain" = {mbeq_values[4]}
                            "440Hz gain" = {mbeq_values[5]}
                            "622Hz gain" = {mbeq_values[6]}
                            "880Hz gain" = {mbeq_values[7]}
                            "1250Hz gain" = {mbeq_values[8]}
                            "1750Hz gain" = {mbeq_values[9]}
                            "2500Hz gain" = {mbeq_values[10]}
                            "3500Hz gain" = {mbeq_values[11]}
                            "5000Hz gain" = {mbeq_values[12]}
                            "10000Hz gain" = {mbeq_values[13]}
                            "20000Hz gain" = {mbeq_values[14]}
                        }}
                    }}'''

    def _generate_stereo_config(self) -> str:
        """Generate stereo configuration based on selected mode.

        Modes:
        - DUAL_MONO: Simple channel duplication (mono to both channels)
        - VOICE_CHANGER: Pitch shifting effect
        """
        c = self._config

        # Build list of active filters in order:
        # hpf -> compressor -> ai -> eq -> limiter -> gate
        # NOTE: EQ must come before gate — see mono config comment.
        # Each entry: (name, input_port, output_port)
        active_filters: list[tuple[str, str, str]] = []
        nodes: list[str] = []

        # Always include HPF so frequency can be changed live via pw-cli.
        # When disabled, frequency is set to 5 Hz (inaudible passthrough).
        active_filters.append(("hpf", "In", "Out"))
        nodes.append(self._generate_hpf_node(bypass=not c.hpf_enabled))

        if c.compressor_enabled:
            active_filters.append(("compressor", "Input", "Output"))
            nodes.append(self._generate_compressor_node())

        if c.noise_reduction_enabled:
            active_filters.append(("ai", "Input", "Output"))
            nodes.append(self._generate_ai_node())

        # Always include EQ node (before gate) to avoid PipeWire 1.6.1 crash.
        # When disabled, all bands are 0 dB (transparent passthrough).
        active_filters.append(("eq", "Input", "Output"))
        nodes.append(self._generate_eq_node(bypass=not c.eq_enabled))

        # Hard limiter prevents output clipping.
        has_gain_stage = c.compressor_enabled
        if has_gain_stage:
            active_filters.append(("limiter", "Input", "Output"))
            nodes.append(self._generate_limiter_node())

        if c.gate_enabled:
            active_filters.append(("gate", "Input", "Output"))
            nodes.append(self._generate_gate_node())

        # If no filters enabled, return minimal passthrough config
        if not active_filters:
            return self._generate_passthrough_config()

        # Build links between active filters (mono chain)
        links = []
        for i in range(len(active_filters) - 1):
            src_name, _, src_out = active_filters[i]
            dst_name, dst_in, _ = active_filters[i + 1]
            links.append(
                f'{{ output = "{src_name}:{src_out}" input = "{dst_name}:{dst_in}" }}'
            )

        # Determine input node and the output of mono chain
        first_name, first_in, _ = active_filters[0]
        last_name, _, last_out = active_filters[-1]

        # Dispatch to specific mode generators
        if c.stereo_mode == StereoMode.VOICE_CHANGER:
            return self._generate_pitch_mode(
                nodes, links, first_name, first_in, last_name, last_out
            )
        else:
            # DUAL_MONO - simple stereo duplication
            return self._generate_dual_mono_stereo(
                nodes, links, first_name, first_in, last_name, last_out
            )

    def _generate_dual_mono_stereo(
        self,
        nodes: list[str],
        links: list[str],
        first_name: str,
        first_in: str,
        last_name: str,
        last_out: str,
    ) -> str:
        """Generate stereo config using simple channel duplication (DUAL_MONO mode)."""
        # Add stereo split nodes
        nodes.append("""                    # Copy node to tee the signal
                    {
                        type = builtin
                        name = "copy_tee"
                        label = copy
                    }""")

        nodes.append("""                    # Left channel output (direct copy)
                    {
                        type = builtin
                        name = "copy_left"
                        label = copy
                    }""")

        # DUAL_MONO - simple copy for right channel
        nodes.append("""                    # Right channel output (copy)
                    {
                        type = builtin
                        name = "copy_right"
                        label = copy
                    }""")

        # Add link from last mono filter to stereo tee
        links.append(f'{{ output = "{last_name}:{last_out}" input = "copy_tee:In" }}')

        # Add stereo split links
        links.append('{ output = "copy_tee:Out" input = "copy_left:In" }')
        links.append('{ output = "copy_tee:Out" input = "copy_right:In" }')
        right_output = "copy_right:Out"

        # Format nodes and links for config
        nodes_str = "\n".join(nodes)
        links_str = "\n                    ".join(links)

        aec_module = self._generate_echo_cancel_module() if self._config.echo_cancel_enabled else ""
        if self._config.echo_cancel_enabled:
            capture_target = '\n                target.object = "big-aec-source"'
            capture_passive = ""
        else:
            capture_target = ""
            capture_passive = "\n                node.passive = true"
        playback_props = self._get_playback_props(
            extra_props='\n                audio.position = [ FL FR ]'
        )

        return f'''# BigLinux Microphone Enhanced Filter Chain
# Auto-generated configuration - Stereo output with Dual Mono

context.modules = [
{aec_module}
    {{
        name = libpipewire-module-filter-chain
        args = {{
            node.description = "{DEVICE_NAME}"
            media.name = "{DEVICE_NAME}"
            filter.graph = {{
                nodes = [
{nodes_str}
                ]
                links = [
                    {links_str}
                ]
                inputs = [ "{first_name}:{first_in}" ]
                outputs = [ "copy_left:Out" "{right_output}" ]
            }}
            capture.props = {{
                audio.position = [ MONO ]{capture_passive}{capture_target}
            }}
{playback_props}
        }}
    }}
]
'''

    def _generate_pitch_mode(
        self,
        nodes: list[str],
        links: list[str],
        first_name: str,
        first_in: str,
        last_name: str,
        last_out: str,
    ) -> str:
        """Generate VOICE_CHANGER mode config.

        Unified Voice Changer:
        Width 0.0 = 0.5x Pitch (Deep/Anonymous)
        Width 0.5 = 1.0x Pitch (Normal)
        Width 1.0 = 2.0x Pitch (High/Squirrel)
        """
        c = self._config
        width = c.stereo_width

        # Pitch mapping:
        # We want to map [0.0, 1.0] -> [0.5, 2.0]
        # Exponential mapping is usually better for pitch: P = 0.5 * 4^width
        # 0.0 -> 0.5
        # 0.5 -> 0.5 * 2 = 1.0
        # 1.0 -> 0.5 * 4 = 2.0
        pitch_coeff = 0.5 * (4.0**width)

        # Safety clamp
        pitch_coeff = max(0.5, min(2.0, pitch_coeff))
        # Define mode name for comments
        mode_name = "Voice Changer"

        # Add Pitch Scale Node
        nodes.append(f"""                    # Pitch Scale for {mode_name}
                    {{
                        type = ladspa
                        name = "pitch"
                        plugin = "{PITCH_SCALE_LIBRARY}"
                        label = "{PITCH_SCALE_LABEL}"
                        control = {{
                            "Pitch co-efficient" = {pitch_coeff:.2f}
                        }}
                    }}""")

        # Automatic Gain Compensation for Deep Voice
        # Deep voice (0.5x) often loses energy. Boost gain when pitch < 1.0.
        # Base gain: +5.0dB (always applied for punchiness)
        # Deep boost: Up to +10.0dB extra at 0.5x pitch
        gain_db = 5.0

        if pitch_coeff < 1.0:
            # Linear interp: 0.5 -> 10.0, 1.0 -> 0.0
            # formula: (1.0 - pitch) * 20.0
            gain_db += (1.0 - pitch_coeff) * 20.0

        # Add Gain Node (LADSPA Amplifier)
        nodes.append(f"""                    # Gain Compensation
                    {{
                        type = ladspa
                        name = "pitch_gain"
                        plugin = "{AMP_LIBRARY}"
                        label = "{AMP_LABEL}"
                        control = {{
                            "Amps gain (dB)" = {gain_db:.2f}
                        }}
                    }}""")

        # Add stereo split nodes (duplicate shifted signal)
        nodes.append("""                    # Copy node to tee the shifted signal
                    {
                        type = builtin
                        name = "copy_tee"
                        label = copy
                    }""")

        nodes.append("""                    # Left channel output
                    {
                        type = builtin
                        name = "copy_left"
                        label = copy
                    }""")

        nodes.append("""                    # Right channel output
                    {
                        type = builtin
                        name = "copy_right"
                        label = copy
                    }""")

        # Links: mono chain -> pitch -> stereo split
        links.append(f'{{ output = "{last_name}:{last_out}" input = "pitch:Input" }}')
        links.append('{ output = "pitch:Output" input = "pitch_gain:Input" }')
        links.append('{ output = "pitch_gain:Output" input = "copy_tee:In" }')
        links.append('{ output = "copy_tee:Out" input = "copy_left:In" }')
        links.append('{ output = "copy_tee:Out" input = "copy_right:In" }')

        # Format nodes and links for config
        nodes_str = "\n".join(nodes)
        links_str = "\n                    ".join(links)

        aec_module = self._generate_echo_cancel_module() if self._config.echo_cancel_enabled else ""
        if self._config.echo_cancel_enabled:
            capture_target = '\n                target.object = "big-aec-source"'
            capture_passive = ""
        else:
            capture_target = ""
            capture_passive = "\n                node.passive = true"
        playback_props = self._get_playback_props(
            extra_props='\n                audio.position = [ FL FR ]'
        )

        return f'''# BigLinux Microphone Enhanced Filter Chain
# Auto-generated configuration - {mode_name}

context.modules = [
{aec_module}
    {{
        name = libpipewire-module-filter-chain
        args = {{
            node.description = "{DEVICE_NAME}"
            media.name = "{DEVICE_NAME}"
            filter.graph = {{
                nodes = [
{nodes_str}
                ]
                links = [
                    {links_str}
                ]
                inputs = [ "{first_name}:{first_in}" ]
                outputs = [ "copy_left:Out" "copy_right:Out" ]
            }}
            capture.props = {{
                audio.position = [ MONO ]{capture_passive}{capture_target}
            }}
{playback_props}
        }}
    }}
]
'''

    def save(self, path: Path | None = None) -> Path:
        """
        Save configuration to file.

        Args:
            path: Optional custom path, defaults to standard location

        Returns:
            Path: Path where config was saved
        """
        if path is None:
            path = CONFIG_DIR / CONFIG_FILE

        # Ensure directory exists
        path.parent.mkdir(parents=True, exist_ok=True)

        # Generate and save
        content = self.generate()
        path.write_text(content)

        logger.info("Filter chain config saved to: %s", path)
        return path

    def delete(self, path: Path | None = None) -> bool:
        """
        Delete configuration file.

        Args:
            path: Optional custom path, defaults to standard location

        Returns:
            bool: True if deleted, False if not found
        """
        if path is None:
            path = CONFIG_DIR / CONFIG_FILE

        if path.exists():
            path.unlink()
            logger.info("Filter chain config deleted: %s", path)
            return True
        return False


def generate_config_for_settings(
    noise_reduction_enabled: bool = True,
    noise_reduction_model: NoiseModel = NoiseModel.GTCRN_DNS3,
    noise_reduction_strength: float = 1.0,
    gate_enabled: bool = True,
    gate_threshold_db: int = -30,
    gate_range_db: int = -60,
    stereo_mode: StereoMode = StereoMode.MONO,
) -> str:
    """
    Convenience function to generate config from individual parameters.

    Args:
        noise_reduction_enabled: Enable AI noise reduction
        noise_reduction_model: GTCRN model to use
        noise_reduction_strength: Reduction strength (0-1)
        gate_enabled: Enable gate filter
        gate_threshold_db: Gate threshold in dB
        gate_range_db: Gate range in dB
        stereo_mode: Stereo enhancement mode


    Returns:
        str: Generated configuration content
    """
    config = FilterChainConfig(
        noise_reduction_enabled=noise_reduction_enabled,
        noise_reduction_model=noise_reduction_model,
        noise_reduction_strength=noise_reduction_strength,
        gate_enabled=gate_enabled,
        gate_threshold_db=gate_threshold_db,
        gate_range_db=gate_range_db,
        stereo_mode=stereo_mode,
    )
    generator = FilterChainGenerator(config)
    return generator.generate()


def ensure_daemon_config() -> None:
    """
    Ensure ~/.config/pipewire/filter-chain.conf exists with correct realtime priority.

    This main config file is required by the separate 'pipewire -c filter-chain.conf'
    process to ensure it runs with correct RT priority (83), preventing choppy audio.
    """
    config_path = Path.home() / ".config" / "pipewire" / "filter-chain.conf"

    # Check if file exists and has correct priority
    valid = False
    if config_path.exists():
        try:
            content = config_path.read_text()
            if "rt.prio" in content and "83" in content:
                valid = True
        except OSError:
            pass

    if not valid:
        logger.info("Updating %s with correct RT priority", config_path)
        content = """# BigLinux Microphone Filter Chain RT Config
# Required for correct process priority to avoid audio stuttering

context.properties = {
    ## Properties for the PipeWire daemon.
    #module.x11.bell = false
}

context.modules = [
    { name = libpipewire-module-rt
        args = {
            rt.prio      = 83
            nice.level   = -11
            rt.time.soft = -1
            rt.time.hard = -1
        }
        flags = [ ifexists nofail ]
    },
    { name = libpipewire-module-protocol-native },
    { name = libpipewire-module-client-node },
    { name = libpipewire-module-adapter }
]
"""
        try:
            config_path.parent.mkdir(parents=True, exist_ok=True)
            config_path.write_text(content)
        except OSError as e:
            logger.error("Failed to write RT config to %s: %s", config_path, e)
