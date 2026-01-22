#!/usr/bin/env python3
"""
Profile management service for BigLinux Microphone Settings.

Handles saving, loading, and managing user audio profiles.
"""

from __future__ import annotations

import json
import logging
import shutil
import uuid
from dataclasses import asdict, dataclass, field
from datetime import datetime
from pathlib import Path
from typing import TYPE_CHECKING

from gi.repository import GLib

if TYPE_CHECKING:
    from ..config import EqualizerConfig, NoiseReductionConfig, StereoConfig

logger = logging.getLogger(__name__)


@dataclass
class Profile:
    """Audio configuration profile."""

    id: str = field(default_factory=lambda: str(uuid.uuid4())[:8])
    name: str = "New Profile"
    description: str = ""
    created_at: str = field(default_factory=lambda: datetime.now().isoformat())
    modified_at: str = field(default_factory=lambda: datetime.now().isoformat())

    # Audio settings
    noise_reduction_enabled: bool = True
    noise_model: str = "gtcrn"
    noise_strength: float = 0.7

    # Gate settings
    gate_enabled: bool = True
    gate_threshold: float = -50.0
    gate_attack: float = 5.0
    gate_release: float = 50.0

    # Stereo settings
    stereo_mode: str = "mono"
    stereo_width: float = 0.5

    crossfeed: float = 0.3

    # Equalizer settings
    eq_enabled: bool = False
    eq_preset: str = "flat"
    eq_bands: list[float] = field(default_factory=lambda: [0.0] * 10)

    @classmethod
    def from_dict(cls, data: dict) -> Profile:
        """Create profile from dictionary."""
        return cls(**{k: v for k, v in data.items() if k in cls.__dataclass_fields__})

    def to_dict(self) -> dict:
        """Convert profile to dictionary."""
        return asdict(self)

    def update_modified(self) -> None:
        """Update modification timestamp."""
        self.modified_at = datetime.now().isoformat()


class ProfileService:
    """
    Manages user audio profiles.

    Provides CRUD operations for profiles with automatic persistence
    to disk in the user's config directory.
    """

    DEFAULT_PROFILES = {
        "voice_call": Profile(
            id="voice_call",
            name="Voice Call",
            description="Optimized for video calls and meetings",
            noise_model="gtcrn",
            noise_strength=0.8,
            gate_enabled=True,
            gate_threshold=-45.0,
            stereo_mode="mono",
            eq_enabled=True,
            eq_preset="voice",
            eq_bands=[2.0, 1.0, 0.0, 0.0, 3.0, 4.0, 3.0, 0.0, -1.0, -2.0],
        ),
        "music_recording": Profile(
            id="music_recording",
            name="Music Recording",
            description="High quality for music and singing",
            noise_model="low_latency",
            noise_strength=0.4,
            gate_enabled=False,
            stereo_mode="dual_mono",
            stereo_width=0.7,
            eq_enabled=True,
            eq_preset="warmth",
            eq_bands=[3.0, 2.0, 1.0, 0.0, -1.0, 0.0, 1.0, 2.0, 3.0, 2.0],
        ),
        "podcast": Profile(
            id="podcast",
            name="Podcast",
            description="Clean voice for podcasting",
            noise_model="gtcrn",
            noise_strength=0.7,
            gate_enabled=True,
            gate_threshold=-50.0,
            gate_attack=3.0,
            gate_release=100.0,
            stereo_mode="mono",
            eq_enabled=True,
            eq_preset="podcast",
            eq_bands=[0.0, -1.0, 0.0, 1.0, 2.0, 3.0, 3.0, 2.0, 0.0, -1.0],
        ),
        "gaming": Profile(
            id="gaming",
            name="Gaming",
            description="Low latency for gaming communication",
            noise_model="low_latency",
            noise_strength=0.6,
            gate_enabled=True,
            gate_threshold=-55.0,
            gate_attack=2.0,
            gate_release=30.0,
            stereo_mode="mono",
            eq_enabled=False,
        ),
    }

    def __init__(self, config_dir: Path | None = None) -> None:
        """
        Initialize the profile service.

        Args:
            config_dir: Configuration directory (defaults to XDG config)
        """
        if config_dir is None:
            xdg_config = Path(GLib.get_user_config_dir())
            config_dir = xdg_config / "biglinux-microphone"

        self._config_dir = config_dir
        self._profiles_file = config_dir / "profiles.json"
        self._profiles: dict[str, Profile] = {}
        self._active_profile_id: str | None = None

        self._ensure_config_dir()
        self._load_profiles()

        logger.debug("Profile service initialized at %s", config_dir)

    def _ensure_config_dir(self) -> None:
        """Ensure config directory exists."""
        self._config_dir.mkdir(parents=True, exist_ok=True)

    def _load_profiles(self) -> None:
        """Load profiles from disk."""
        # Add default profiles
        for profile_id, profile in self.DEFAULT_PROFILES.items():
            self._profiles[profile_id] = profile

        # Load user profiles
        if self._profiles_file.exists():
            try:
                with open(self._profiles_file, encoding="utf-8") as f:
                    data = json.load(f)

                if "active_profile" in data:
                    self._active_profile_id = data["active_profile"]

                for profile_data in data.get("profiles", []):
                    profile = Profile.from_dict(profile_data)
                    self._profiles[profile.id] = profile

                logger.info("Loaded %d user profiles", len(data.get("profiles", [])))
            except Exception:
                logger.exception("Error loading profiles")

    def _save_profiles(self) -> None:
        """Save profiles to disk."""
        try:
            # Only save user profiles (not defaults)
            user_profiles = [
                profile.to_dict()
                for profile_id, profile in self._profiles.items()
                if profile_id not in self.DEFAULT_PROFILES
            ]

            data = {
                "version": 1,
                "active_profile": self._active_profile_id,
                "profiles": user_profiles,
            }

            # Write atomically
            tmp_file = self._profiles_file.with_suffix(".tmp")
            with open(tmp_file, "w", encoding="utf-8") as f:
                json.dump(data, f, indent=2)

            shutil.move(str(tmp_file), str(self._profiles_file))
            logger.debug("Saved %d user profiles", len(user_profiles))
        except Exception:
            logger.exception("Error saving profiles")

    def list_profiles(self) -> list[Profile]:
        """
        List all available profiles.

        Returns:
            list[Profile]: All profiles (defaults + user)
        """
        # Return defaults first, then user profiles sorted by name
        defaults = [
            p for pid, p in self._profiles.items() if pid in self.DEFAULT_PROFILES
        ]
        user = sorted(
            [
                p
                for pid, p in self._profiles.items()
                if pid not in self.DEFAULT_PROFILES
            ],
            key=lambda p: p.name.lower(),
        )
        return defaults + user

    def get_profile(self, profile_id: str) -> Profile | None:
        """
        Get a profile by ID.

        Args:
            profile_id: Profile ID

        Returns:
            Profile or None if not found
        """
        return self._profiles.get(profile_id)

    def get_active_profile(self) -> Profile | None:
        """
        Get the currently active profile.

        Returns:
            Profile or None if no active profile
        """
        if self._active_profile_id:
            return self._profiles.get(self._active_profile_id)
        return None

    def set_active_profile(self, profile_id: str | None) -> None:
        """
        Set the active profile.

        Args:
            profile_id: Profile ID or None to clear
        """
        if profile_id is not None and profile_id not in self._profiles:
            logger.warning("Profile not found: %s", profile_id)
            return

        self._active_profile_id = profile_id
        self._save_profiles()
        logger.info("Active profile set to: %s", profile_id)

    def create_profile(
        self,
        name: str,
        description: str = "",
        base_profile_id: str | None = None,
    ) -> Profile:
        """
        Create a new profile.

        Args:
            name: Profile name
            description: Profile description
            base_profile_id: ID of profile to copy settings from

        Returns:
            Profile: The new profile
        """
        if base_profile_id and base_profile_id in self._profiles:
            # Copy from existing profile
            base = self._profiles[base_profile_id]
            profile = Profile(
                name=name,
                description=description,
                noise_reduction_enabled=base.noise_reduction_enabled,
                noise_model=base.noise_model,
                noise_strength=base.noise_strength,
                gate_enabled=base.gate_enabled,
                gate_threshold=base.gate_threshold,
                gate_attack=base.gate_attack,
                gate_release=base.gate_release,
                stereo_mode=base.stereo_mode,
                stereo_width=base.stereo_width,
                crossfeed=base.crossfeed,
                eq_enabled=base.eq_enabled,
                eq_preset=base.eq_preset,
                eq_bands=base.eq_bands.copy(),
            )
        else:
            profile = Profile(name=name, description=description)

        self._profiles[profile.id] = profile
        self._save_profiles()
        logger.info("Created profile: %s (%s)", profile.name, profile.id)

        return profile

    def update_profile(self, profile: Profile) -> None:
        """
        Update an existing profile.

        Args:
            profile: Profile to update
        """
        if profile.id not in self._profiles:
            logger.warning("Profile not found: %s", profile.id)
            return

        if profile.id in self.DEFAULT_PROFILES:
            logger.warning("Cannot modify default profile: %s", profile.id)
            return

        profile.update_modified()
        self._profiles[profile.id] = profile
        self._save_profiles()
        logger.debug("Updated profile: %s", profile.name)

    def delete_profile(self, profile_id: str) -> bool:
        """
        Delete a profile.

        Args:
            profile_id: Profile ID

        Returns:
            bool: True if deleted, False if not found or is default
        """
        if profile_id in self.DEFAULT_PROFILES:
            logger.warning("Cannot delete default profile: %s", profile_id)
            return False

        if profile_id not in self._profiles:
            logger.warning("Profile not found: %s", profile_id)
            return False

        # Clear active if deleting active profile
        if self._active_profile_id == profile_id:
            self._active_profile_id = None

        del self._profiles[profile_id]
        self._save_profiles()
        logger.info("Deleted profile: %s", profile_id)

        return True

    def duplicate_profile(
        self, profile_id: str, new_name: str | None = None
    ) -> Profile | None:
        """
        Duplicate an existing profile.

        Args:
            profile_id: Profile ID to duplicate
            new_name: Name for the new profile

        Returns:
            Profile or None if original not found
        """
        original = self._profiles.get(profile_id)
        if original is None:
            return None

        name = new_name or f"{original.name} (Copy)"
        return self.create_profile(
            name=name,
            description=original.description,
            base_profile_id=profile_id,
        )

    def export_profile(self, profile_id: str, path: Path) -> bool:
        """
        Export a profile to a file.

        Args:
            profile_id: Profile ID
            path: Output file path

        Returns:
            bool: True if exported successfully
        """
        profile = self._profiles.get(profile_id)
        if profile is None:
            return False

        try:
            with open(path, "w", encoding="utf-8") as f:
                json.dump(profile.to_dict(), f, indent=2)
            logger.info("Exported profile to %s", path)
            return True
        except Exception:
            logger.exception("Error exporting profile")
            return False

    def import_profile(self, path: Path) -> Profile | None:
        """
        Import a profile from a file.

        Args:
            path: Input file path

        Returns:
            Profile or None if import failed
        """
        try:
            with open(path, encoding="utf-8") as f:
                data = json.load(f)

            # Generate new ID to avoid conflicts
            data["id"] = str(uuid.uuid4())[:8]
            data["name"] = f"{data.get('name', 'Imported')} (Imported)"
            data["created_at"] = datetime.now().isoformat()
            data["modified_at"] = datetime.now().isoformat()

            profile = Profile.from_dict(data)
            self._profiles[profile.id] = profile
            self._save_profiles()

            logger.info("Imported profile: %s", profile.name)
            return profile
        except Exception:
            logger.exception("Error importing profile")
            return None

    def apply_profile(
        self,
        profile_id: str,
        noise_config: NoiseReductionConfig | None = None,
        stereo_config: StereoConfig | None = None,
        eq_config: EqualizerConfig | None = None,
    ) -> bool:
        """
        Apply a profile's settings to configuration objects.

        Args:
            profile_id: Profile ID
            noise_config: NoiseReductionConfig to update
            stereo_config: StereoConfig to update
            eq_config: EqualizerConfig to update

        Returns:
            bool: True if applied successfully
        """
        profile = self._profiles.get(profile_id)
        if profile is None:
            return False

        if noise_config is not None:
            noise_config.enabled = profile.noise_reduction_enabled
            noise_config.strength = profile.noise_strength

        if stereo_config is not None:
            from ..config import StereoMode

            try:
                stereo_config.mode = StereoMode(profile.stereo_mode)
            except ValueError:
                stereo_config.mode = StereoMode.MONO
            stereo_config.width = profile.stereo_width

            stereo_config.crossfeed = profile.crossfeed

        if eq_config is not None:
            eq_config.enabled = profile.eq_enabled
            eq_config.bands = profile.eq_bands.copy()

        self.set_active_profile(profile_id)
        return True
