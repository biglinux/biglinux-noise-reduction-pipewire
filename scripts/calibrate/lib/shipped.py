"""Run the denoiser we actually ship, not the ONNX file it came from.

The rest of this harness scores models by driving their `.onnx` through
onnxruntime with a Python overlap-add loop. That measures the network. It
does not measure the product: the shipped artifact is a LADSPA shared
object with its own STFT, its own window, its own attenuation blend and,
for DPDFNet, an OpenVINO graph converted from that same ONNX. It is also
the only way to score DeepFilterNet3 and RNNoise at all, since neither
ships as a single streaming ONNX.

So this module drives the `.so` through ffmpeg's `ladspa` filter, and
takes the plugin paths, labels and control strings from
`biglinux-microphone-cli models` rather than repeating them. A second copy
would disagree with the first the day someone renames a plugin, and the
disagreement would show up as a quality regression nobody could explain.
"""

from __future__ import annotations

import json
import subprocess
from dataclasses import dataclass
from functools import lru_cache
from pathlib import Path

import numpy as np

#: Where the catalogue comes from. Overridable for a build tree.
CLI = "biglinux-microphone-cli"


@dataclass(frozen=True)
class ShippedModel:
    """One entry of the catalogue the application exposes."""

    id: int
    label: str
    plugin: Path
    sample_rate: int
    realtime: bool
    loadable: bool
    ffmpeg_filter: str

    @property
    def name(self) -> str:
        """Short name for report columns."""
        return self.label.removesuffix("_mono")


@lru_cache(maxsize=1)
def catalogue(cli: str = CLI) -> tuple[ShippedModel, ...]:
    """Every model the application can select, as it describes itself."""
    out = subprocess.run(
        [cli, "models"], capture_output=True, text=True, check=True, timeout=30
    )
    return tuple(
        ShippedModel(
            id=row["id"],
            label=row["label"],
            plugin=Path(row["plugin"]),
            sample_rate=row["sample_rate"],
            realtime=row["realtime"],
            loadable=row["loadable"],
            ffmpeg_filter=row["ffmpeg_filter"],
        )
        for row in json.loads(out.stdout)
    )


def process(
    audio: np.ndarray, sr: int, model: ShippedModel, ffmpeg: str = "ffmpeg"
) -> np.ndarray:
    """Push `audio` through the shipped plugin and return what comes back.

    Resamples to the model's own rate and back, because a 16 kHz model in
    a 48 kHz chain pays for that conversion too and a comparison that
    hides it flatters the narrow-band models.

    Raises `RuntimeError` when the plugin refuses to load, rather than
    returning the input and quietly scoring a passthrough as a denoiser.
    """
    if not model.ffmpeg_filter:
        raise RuntimeError(f"{model.name}: no ffmpeg filter; is the plugin installed?")

    chain = f"aresample={model.sample_rate},{model.ffmpeg_filter},aresample={sr}"
    command = [
        ffmpeg,
        "-hide_banner",
        "-loglevel",
        "error",
        "-f",
        "f32le",
        "-ar",
        str(sr),
        "-ac",
        "1",
        "-i",
        "pipe:0",
        "-af",
        chain,
        "-f",
        "f32le",
        "-ar",
        str(sr),
        "-ac",
        "1",
        "pipe:1",
    ]
    done = subprocess.run(
        command,
        input=audio.astype(np.float32).tobytes(),
        capture_output=True,
        check=False,
        timeout=600,
    )
    if done.returncode != 0:
        message = done.stderr.decode("utf-8", "replace").strip().splitlines()
        raise RuntimeError(f"{model.name}: {message[-1] if message else 'ffmpeg failed'}")
    return np.frombuffer(done.stdout, dtype=np.float32)
