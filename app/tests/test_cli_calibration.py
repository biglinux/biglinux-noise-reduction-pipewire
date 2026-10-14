"""Backend exit status must take precedence over an optimistic JSON report."""

import json

import pytest
from biglinux_microphone import cli


def outcome(**changes):
    return {"heard_speech": True, "volume": 1.25, "device": "mic", **changes}


def invoke(result, code=0, stderr=""):
    return cli._parse_calibration_result(json.dumps(result), stderr, code)


def test_measurement_with_failed_save_is_not_success():
    with pytest.raises(cli.BackendError, match="read-only"):
        invoke(outcome(), 1, "settings are read-only")


def test_no_speech_is_a_valid_nonzero_outcome():
    assert invoke(outcome(heard_speech=False), 1)["heard_speech"] is False


def test_saved_measurement_is_accepted():
    assert invoke(outcome())["volume"] == 1.25


def test_automatic_volume_rejects_a_changed_microphone(monkeypatch):
    monkeypatch.setattr(cli, "get_default_source", lambda: "other")
    with pytest.raises(cli.BackendError, match="microphone changed"):
        cli.set_level_range("mic", (80, 120))


@pytest.mark.parametrize(
    "bounds", [(100, 100), (120, 80), (-1, 10), (0, 301), (False, 100)]
)
def test_automatic_volume_rejects_invalid_limits(bounds):
    with pytest.raises(cli.BackendError, match="Invalid automatic"):
        cli.set_level_range("mic", bounds)


@pytest.mark.parametrize(
    "changes",
    [
        {"heard_speech": 1},
        {"volume": True},
        {"volume": -1},
        {"volume": 3.1},
        {"device": ""},
        {"device": None},
        {"volume": float("nan")},
    ],
)
def test_invalid_report_is_rejected(changes):
    with pytest.raises(cli.BackendError):
        invoke(outcome(**changes))
