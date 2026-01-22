#!/usr/bin/env python3
"""
Settings service for managing application settings.

Handles loading, saving, and synchronizing settings.
"""

from __future__ import annotations

import logging

from biglinux_microphone.config import (
    CONFIG_DIR,
    PROFILES_DIR,
    AppSettings,
    load_settings,
    save_settings,
)

logger = logging.getLogger(__name__)


class SettingsService:
    """
    Service for managing application settings.

    Provides methods for:
    - Loading and saving settings
    - Applying quick presets
    - Managing user profiles
    """

    def __init__(self) -> None:
        """Initialize the settings service."""
        self._settings: AppSettings | None = None
        self._ensure_directories()
        logger.debug("Settings service initialized")

    def _ensure_directories(self) -> None:
        """Ensure configuration directories exist."""
        CONFIG_DIR.mkdir(parents=True, exist_ok=True)
        PROFILES_DIR.mkdir(parents=True, exist_ok=True)

    def load(self) -> AppSettings:
        """
        Load settings from disk.

        Returns:
            AppSettings: Loaded or default settings
        """
        self._settings = load_settings()
        logger.debug("Settings loaded")
        return self._settings

    def save(self, settings: AppSettings) -> bool:
        """
        Save settings to disk.

        Args:
            settings: Settings to save

        Returns:
            bool: True if save successful
        """
        self._settings = settings
        result = save_settings(settings)
        if result:
            logger.debug("Settings saved")
        return result

    def get(self) -> AppSettings:
        """
        Get current settings (loads if not cached).

        Returns:
            AppSettings: Current settings
        """
        if self._settings is None:
            return self.load()
        return self._settings

    def list_profiles(self) -> list[str]:
        """
        List available user profiles.

        Returns:
            list[str]: List of profile names
        """
        profiles = []
        for file in PROFILES_DIR.glob("*.json"):
            profiles.append(file.stem)
        return sorted(profiles)

    def save_profile(self, name: str, settings: AppSettings) -> bool:
        """
        Save settings as a named profile.

        Args:
            name: Profile name
            settings: Settings to save

        Returns:
            bool: True if save successful
        """
        import json

        profile_file = PROFILES_DIR / f"{name}.json"

        try:
            with open(profile_file, "w", encoding="utf-8") as f:
                json.dump(settings.to_dict(), f, indent=4)
            logger.info("Profile saved: %s", name)
            return True
        except OSError:
            logger.exception("Error saving profile: %s", name)
            return False

    def load_profile(self, name: str) -> AppSettings | None:
        """
        Load a named profile.

        Args:
            name: Profile name

        Returns:
            AppSettings or None: Loaded settings or None if not found
        """
        import json

        profile_file = PROFILES_DIR / f"{name}.json"

        if not profile_file.exists():
            logger.warning("Profile not found: %s", name)
            return None

        try:
            with open(profile_file, encoding="utf-8") as f:
                data = json.load(f)
                return AppSettings.from_dict(data)
        except (json.JSONDecodeError, OSError):
            logger.exception("Error loading profile: %s", name)
            return None

    def delete_profile(self, name: str) -> bool:
        """
        Delete a named profile.

        Args:
            name: Profile name

        Returns:
            bool: True if delete successful
        """
        profile_file = PROFILES_DIR / f"{name}.json"

        if not profile_file.exists():
            return True  # Already deleted

        try:
            profile_file.unlink()
            logger.info("Profile deleted: %s", name)
            return True
        except OSError:
            logger.exception("Error deleting profile: %s", name)
            return False
