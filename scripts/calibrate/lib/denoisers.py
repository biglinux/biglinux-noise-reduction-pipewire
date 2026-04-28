"""Streaming denoiser harness — runs different ONNX speech-enhancement
models against a common signal interface.

Models share a per-frame STFT contract but differ in tensor layout and
cache shapes. Each entry in `REGISTRY` declares the framing + how to
pack/unpack the model's tensors. `run` does the OLA loop generically.

Currently supports:
- GTCRN family (`gtcrn_*.onnx`): 16 kHz, 512 NFFT / 256 hop, 257 bins,
  3 named caches with multi-dim shapes.
- UL-UNAS streaming (`ulunas_stream*.onnx`): same framing as GTCRN but
  flat 1D caches, different names (`tfa_cache` vs `tra_cache`).
- DPDFNet (`dpdfnet*.onnx`): 16 kHz, 320 NFFT / 160 hop, 161 bins,
  single packed `state_in` vector.
"""

from __future__ import annotations

import time
from dataclasses import dataclass, field
from pathlib import Path

import numpy as np


@dataclass
class DenoiserSpec:
    """Static description of one ONNX denoiser model."""

    name: str
    onnx_path: Path
    sample_rate: int
    nfft: int
    hop: int
    family: str  # "gtcrn", "ulunas", "dpdfnet"


def _spec_for(name: str, onnx_path: Path) -> DenoiserSpec:
    """Resolve framing + family from a friendly name."""
    if name.startswith("gtcrn"):
        return DenoiserSpec(name, onnx_path, 16000, 512, 256, "gtcrn")
    if name.startswith("ulunas"):
        return DenoiserSpec(name, onnx_path, 16000, 512, 256, "ulunas")
    if name.startswith("dpdfnet") or name == "baseline":
        return DenoiserSpec(name, onnx_path, 16000, 320, 160, "dpdfnet")
    raise ValueError(f"unknown denoiser family for name={name!r}")


class _OrtSession:
    """Wraps `onnxruntime.InferenceSession` with the run/get_inputs
    interface the OLA loop expects. Default backend."""

    def __init__(self, path: Path):
        import onnxruntime as ort

        self._sess = ort.InferenceSession(str(path), providers=["CPUExecutionProvider"])

    def get_inputs(self):
        return self._sess.get_inputs()

    def run(self, _, feeds: dict):
        return self._sess.run(None, feeds)


class _OvSession:
    """OpenVINO 2026.x backend exposing the same get_inputs/run shape as
    `_OrtSession`. Lets us A/B onnxruntime vs OpenVINO without changing
    the OLA loop. Inputs come back as a list of objects with `name` +
    `shape`, matching the ORT proxy shape."""

    def __init__(self, path: Path, device: str = "CPU"):
        from openvino import Core

        core = Core()
        # OpenVINO 2026.x crashes inside the Snippets pass when lowering
        # certain GRU patterns (e.g. DPDFNet's `df_dec/df_gru`). Disabling
        # Snippets falls back to the regular CPU kernels with no
        # measurable speed loss on these tiny streaming models — the
        # Snippets pipeline is tuned for batch transformer workloads.
        core.set_property(device, {"SNIPPETS_MODE": "DISABLE"})
        model = core.read_model(str(path))
        self._compiled = core.compile_model(model, device)
        self._req = self._compiled.create_infer_request()
        # Compiled-model outputs/inputs — these are the ConstOutput
        # objects the result dict is keyed by; the source `model`
        # objects are different references and won't index the dict.
        self._outputs = list(self._compiled.outputs)
        self._inputs = [self._wrap_port(p) for p in self._compiled.inputs]

    @staticmethod
    def _wrap_port(port):
        from dataclasses import dataclass

        @dataclass
        class _Port:
            name: str
            shape: tuple

        # `any_name` returns the friendly name; partial_shape -> tuple
        # of ints (every shape we feed is fully static).
        return _Port(name=port.any_name, shape=tuple(int(d) for d in port.shape))

    def get_inputs(self):
        return self._inputs

    def run(self, _, feeds: dict):
        result = self._req.infer(feeds)
        return [np.asarray(result[o]) for o in self._outputs]


@dataclass
class DenoiserRunner:
    """Lazy backend session + framing context for one model.
    `backend` is "onnxruntime" (default) or "openvino"."""

    spec: DenoiserSpec
    backend: str = "onnxruntime"
    _session: object | None = field(default=None, init=False, repr=False)

    def session(self):
        if self._session is None:
            if self.backend == "openvino":
                self._session = _OvSession(self.spec.onnx_path)
            elif self.backend == "onnxruntime":
                self._session = _OrtSession(self.spec.onnx_path)
            else:
                raise ValueError(f"unknown backend: {self.backend!r}")
        return self._session

    def run(self, x: np.ndarray, sr: int) -> tuple[np.ndarray, float]:
        """Returns (enhanced_pcm, processing_seconds). Resamples in/out
        to the model's sample_rate when `sr` differs."""
        from scipy.signal import resample_poly

        sess = self.session()
        target_sr = self.spec.sample_rate
        if sr != target_sr:
            x_in = resample_poly(x, target_sr, sr).astype(np.float32)
        else:
            x_in = x.astype(np.float32)

        nfft = self.spec.nfft
        hop = self.spec.hop
        n_frames = 1 + max(0, (len(x_in) - nfft) // hop)
        if n_frames <= 0:
            return x.astype(np.float32), 0.0

        win = np.hanning(nfft).astype(np.float32)
        enhanced = np.zeros_like(x_in)
        norm = np.zeros_like(x_in) + 1e-9

        # Framing depends on model family.
        runner = _FRAMERS[self.spec.family]
        in_names = [i.name for i in sess.get_inputs()]
        cache_state = runner.init_caches(sess)

        t0 = time.perf_counter()
        for f in range(n_frames):
            seg = x_in[f * hop : f * hop + nfft] * win
            S = np.fft.rfft(seg, n=nfft).astype(np.complex64)
            mix_in = runner.pack_mix(S)
            feeds = {in_names[0]: mix_in}
            feeds.update(cache_state)
            outputs = sess.run(None, feeds)
            S_enh = runner.unpack_enh(outputs[0])
            cache_state = runner.next_caches(sess, outputs)
            time_seg = np.fft.irfft(S_enh, n=nfft).astype(np.float32) * win
            enhanced[f * hop : f * hop + nfft] += time_seg
            norm[f * hop : f * hop + nfft] += win * win
        dt = time.perf_counter() - t0

        enhanced = enhanced / norm
        if sr != target_sr:
            enhanced = resample_poly(enhanced, sr, target_sr).astype(np.float32)
        n = min(len(enhanced), len(x))
        return enhanced[:n].astype(np.float32), dt


# ── Per-family pack/unpack adapters ──────────────────────────────────


class _GtcrnFramer:
    """GTCRN/UL-UNAS layout: mix is [1, bins, 1, 2]; caches are named
    tensors with whatever shape the ONNX file declared."""

    @staticmethod
    def pack_mix(S: np.ndarray) -> np.ndarray:
        bins = S.shape[0]
        out = np.zeros((1, bins, 1, 2), dtype=np.float32)
        out[0, :, 0, 0] = S.real
        out[0, :, 0, 1] = S.imag
        return out

    @staticmethod
    def unpack_enh(enh: np.ndarray) -> np.ndarray:
        return enh[0, :, 0, 0] + 1j * enh[0, :, 0, 1]

    @staticmethod
    def init_caches(sess) -> dict[str, np.ndarray]:
        out = {}
        for inp in sess.get_inputs()[1:]:
            shape = tuple(int(d) for d in inp.shape)
            out[inp.name] = np.zeros(shape, dtype=np.float32)
        return out

    @staticmethod
    def next_caches(sess, outputs) -> dict[str, np.ndarray]:
        # Outputs declared in the same positional order as inputs[1:].
        names = [i.name for i in sess.get_inputs()[1:]]
        return {name: outputs[1 + idx] for idx, name in enumerate(names)}


class _DpdfnetFramer:
    """DPDFNet layout: spec is [1, 1, bins, 2]; one flat state_in."""

    @staticmethod
    def pack_mix(S: np.ndarray) -> np.ndarray:
        bins = S.shape[0]
        out = np.zeros((1, 1, bins, 2), dtype=np.float32)
        out[0, 0, :, 0] = S.real
        out[0, 0, :, 1] = S.imag
        return out

    @staticmethod
    def unpack_enh(enh: np.ndarray) -> np.ndarray:
        return enh[0, 0, :, 0] + 1j * enh[0, 0, :, 1]

    @staticmethod
    def init_caches(sess) -> dict[str, np.ndarray]:
        state = sess.get_inputs()[1]
        shape = tuple(int(d) for d in state.shape)
        return {state.name: np.zeros(shape, dtype=np.float32)}

    @staticmethod
    def next_caches(sess, outputs) -> dict[str, np.ndarray]:
        state = sess.get_inputs()[1]
        return {state.name: outputs[1]}


_FRAMERS = {
    "gtcrn": _GtcrnFramer,
    "ulunas": _GtcrnFramer,  # same layout, different cache names
    "dpdfnet": _DpdfnetFramer,
}


# ── Convenience constructor ──────────────────────────────────────────


def load(name: str, onnx_path: Path, backend: str = "onnxruntime") -> DenoiserRunner:
    """Build a runner from a friendly name + path. The name's prefix
    decides framing; pass any path that ONNXRuntime/OpenVINO can open.
    `backend` toggles between `onnxruntime` and `openvino`."""
    return DenoiserRunner(_spec_for(name, onnx_path), backend=backend)
