"""
Tests for profile_service module.

Tests profile management, CRUD operations, and import/export.
"""

from __future__ import annotations

import json
import tempfile
from pathlib import Path

from biglinux_microphone.services.profile_service import (
    Profile,
    ProfileService,
)


class TestProfile:
    """Tests for Profile dataclass."""

    def test_default_values(self) -> None:
        """Test default profile values."""
        profile = Profile()
        assert profile.name == "New Profile"
        assert profile.description == ""
        assert profile.noise_reduction_enabled is True
        assert profile.noise_strength == 0.7
        assert profile.gate_enabled is True
        assert profile.stereo_mode == "mono"
        assert profile.eq_enabled is False
        assert len(profile.eq_bands) == 10

    def test_custom_values(self) -> None:
        """Test custom profile values."""
        profile = Profile(
            name="Test Profile",
            description="A test profile",
            noise_strength=0.5,
            stereo_mode="dual_mono",
        )
        assert profile.name == "Test Profile"
        assert profile.description == "A test profile"
        assert profile.noise_strength == 0.5
        assert profile.stereo_mode == "dual_mono"

    def test_from_dict(self) -> None:
        """Test creating profile from dictionary."""
        data = {
            "id": "test123",
            "name": "Test Profile",
            "noise_strength": 0.8,
            "stereo_mode": "spatial",
            "unknown_field": "ignored",  # Should be ignored
        }
        profile = Profile.from_dict(data)
        assert profile.id == "test123"
        assert profile.name == "Test Profile"
        assert profile.noise_strength == 0.8
        assert profile.stereo_mode == "spatial"

    def test_to_dict(self) -> None:
        """Test converting profile to dictionary."""
        profile = Profile(
            id="test123",
            name="Test Profile",
            noise_strength=0.9,
        )
        data = profile.to_dict()
        assert data["id"] == "test123"
        assert data["name"] == "Test Profile"
        assert data["noise_strength"] == 0.9
        assert "created_at" in data
        assert "modified_at" in data

    def test_update_modified(self) -> None:
        """Test updating modification timestamp."""
        profile = Profile()
        original_modified = profile.modified_at

        import time

        time.sleep(0.01)  # Ensure time difference

        profile.update_modified()
        assert profile.modified_at != original_modified


class TestProfileService:
    """Tests for ProfileService."""

    def test_init_creates_config_dir(self) -> None:
        """Test service creates config directory."""
        with tempfile.TemporaryDirectory() as tmpdir:
            config_dir = Path(tmpdir) / "test_config"
            ProfileService(config_dir=config_dir)
            assert config_dir.exists()

    def test_default_profiles_loaded(self) -> None:
        """Test default profiles are loaded."""
        with tempfile.TemporaryDirectory() as tmpdir:
            config_dir = Path(tmpdir) / "test_config"
            service = ProfileService(config_dir=config_dir)

            profiles = service.list_profiles()
            profile_ids = [p.id for p in profiles]

            assert "voice_call" in profile_ids
            assert "music_recording" in profile_ids
            assert "podcast" in profile_ids
            assert "gaming" in profile_ids

    def test_create_profile(self) -> None:
        """Test creating a new profile."""
        with tempfile.TemporaryDirectory() as tmpdir:
            config_dir = Path(tmpdir) / "test_config"
            service = ProfileService(config_dir=config_dir)

            profile = service.create_profile(
                name="My Profile",
                description="My description",
            )

            assert profile.name == "My Profile"
            assert profile.description == "My description"
            assert profile.id is not None

            # Should be retrievable
            retrieved = service.get_profile(profile.id)
            assert retrieved is not None
            assert retrieved.name == "My Profile"

    def test_create_profile_from_base(self) -> None:
        """Test creating profile based on existing profile."""
        with tempfile.TemporaryDirectory() as tmpdir:
            config_dir = Path(tmpdir) / "test_config"
            service = ProfileService(config_dir=config_dir)

            profile = service.create_profile(
                name="Based on Voice Call",
                base_profile_id="voice_call",
            )

            base = service.get_profile("voice_call")
            assert profile.noise_strength == base.noise_strength
            assert profile.gate_threshold == base.gate_threshold

    def test_get_profile(self) -> None:
        """Test getting a profile by ID."""
        with tempfile.TemporaryDirectory() as tmpdir:
            config_dir = Path(tmpdir) / "test_config"
            service = ProfileService(config_dir=config_dir)

            profile = service.get_profile("voice_call")
            assert profile is not None
            assert profile.name == "Voice Call"

            # Non-existent profile
            nonexistent = service.get_profile("nonexistent")
            assert nonexistent is None

    def test_update_profile(self) -> None:
        """Test updating a profile."""
        with tempfile.TemporaryDirectory() as tmpdir:
            config_dir = Path(tmpdir) / "test_config"
            service = ProfileService(config_dir=config_dir)

            # Create a user profile
            profile = service.create_profile(name="Test")
            original_modified = profile.modified_at

            # Update it
            profile.name = "Updated Name"
            profile.noise_strength = 0.5

            import time

            time.sleep(0.01)
            service.update_profile(profile)

            # Verify update
            updated = service.get_profile(profile.id)
            assert updated.name == "Updated Name"
            assert updated.noise_strength == 0.5
            assert updated.modified_at != original_modified

    def test_cannot_update_default_profile(self) -> None:
        """Test that default profiles cannot be modified."""
        with tempfile.TemporaryDirectory() as tmpdir:
            config_dir = Path(tmpdir) / "test_config"
            service = ProfileService(config_dir=config_dir)

            # Get a copy of the default profile data
            original = service.get_profile("voice_call")

            # Create a new Profile object with modified name (simulating modification)
            modified = Profile.from_dict(original.to_dict())
            modified.name = "Modified Name"
            service.update_profile(modified)

            # Voice call should remain unchanged because it's a default profile
            # The update_profile should have rejected the modification
            # Re-create service to verify default wasn't persisted
            service2 = ProfileService(config_dir=config_dir)
            retrieved = service2.get_profile("voice_call")
            assert retrieved.name == "Voice Call"

    def test_delete_profile(self) -> None:
        """Test deleting a profile."""
        with tempfile.TemporaryDirectory() as tmpdir:
            config_dir = Path(tmpdir) / "test_config"
            service = ProfileService(config_dir=config_dir)

            # Create and delete
            profile = service.create_profile(name="To Delete")
            result = service.delete_profile(profile.id)

            assert result is True
            assert service.get_profile(profile.id) is None

    def test_cannot_delete_default_profile(self) -> None:
        """Test that default profiles cannot be deleted."""
        with tempfile.TemporaryDirectory() as tmpdir:
            config_dir = Path(tmpdir) / "test_config"
            service = ProfileService(config_dir=config_dir)

            result = service.delete_profile("voice_call")
            assert result is False
            assert service.get_profile("voice_call") is not None

    def test_active_profile(self) -> None:
        """Test setting and getting active profile."""
        with tempfile.TemporaryDirectory() as tmpdir:
            config_dir = Path(tmpdir) / "test_config"
            service = ProfileService(config_dir=config_dir)

            # Initially no active profile
            assert service.get_active_profile() is None

            # Set active profile
            service.set_active_profile("voice_call")
            active = service.get_active_profile()
            assert active is not None
            assert active.id == "voice_call"

            # Clear active profile
            service.set_active_profile(None)
            assert service.get_active_profile() is None

    def test_delete_active_profile_clears_active(self) -> None:
        """Test that deleting active profile clears the active state."""
        with tempfile.TemporaryDirectory() as tmpdir:
            config_dir = Path(tmpdir) / "test_config"
            service = ProfileService(config_dir=config_dir)

            profile = service.create_profile(name="Test")
            service.set_active_profile(profile.id)
            assert service.get_active_profile() is not None

            service.delete_profile(profile.id)
            assert service.get_active_profile() is None

    def test_duplicate_profile(self) -> None:
        """Test duplicating a profile."""
        with tempfile.TemporaryDirectory() as tmpdir:
            config_dir = Path(tmpdir) / "test_config"
            service = ProfileService(config_dir=config_dir)

            duplicate = service.duplicate_profile("voice_call", "My Voice Call")
            assert duplicate is not None
            assert duplicate.name == "My Voice Call"
            assert duplicate.id != "voice_call"

            original = service.get_profile("voice_call")
            assert duplicate.noise_strength == original.noise_strength

    def test_duplicate_with_default_name(self) -> None:
        """Test duplicating with auto-generated name."""
        with tempfile.TemporaryDirectory() as tmpdir:
            config_dir = Path(tmpdir) / "test_config"
            service = ProfileService(config_dir=config_dir)

            duplicate = service.duplicate_profile("voice_call")
            assert duplicate is not None
            assert "(Copy)" in duplicate.name

    def test_export_profile(self) -> None:
        """Test exporting profile to file."""
        with tempfile.TemporaryDirectory() as tmpdir:
            config_dir = Path(tmpdir) / "test_config"
            service = ProfileService(config_dir=config_dir)

            # Create a user profile to export (not default)
            profile = service.create_profile(
                name="Export Test",
                description="Test export",
            )

            export_path = Path(tmpdir) / "exported.json"
            result = service.export_profile(profile.id, export_path)

            assert result is True
            assert export_path.exists()

            with open(export_path) as f:
                data = json.load(f)
                assert data["name"] == "Export Test"

    def test_import_profile(self) -> None:
        """Test importing profile from file."""
        with tempfile.TemporaryDirectory() as tmpdir:
            config_dir = Path(tmpdir) / "test_config"
            service = ProfileService(config_dir=config_dir)

            # Create a profile file
            import_path = Path(tmpdir) / "import.json"
            data = {
                "name": "Imported Profile",
                "noise_strength": 0.6,
                "stereo_mode": "dual_mono",
            }
            with open(import_path, "w") as f:
                json.dump(data, f)

            profile = service.import_profile(import_path)
            assert profile is not None
            assert "(Imported)" in profile.name
            assert profile.noise_strength == 0.6
            assert profile.stereo_mode == "dual_mono"

            # Should be in service
            assert service.get_profile(profile.id) is not None

    def test_list_profiles_order(self) -> None:
        """Test profiles are listed in correct order."""
        with tempfile.TemporaryDirectory() as tmpdir:
            config_dir = Path(tmpdir) / "test_config"
            service = ProfileService(config_dir=config_dir)

            # Create user profiles
            service.create_profile(name="Zebra")
            service.create_profile(name="Alpha")

            profiles = service.list_profiles()

            # Defaults should come first
            default_profiles = [
                p for p in profiles if p.id in ProfileService.DEFAULT_PROFILES
            ]
            user_profiles = [
                p for p in profiles if p.id not in ProfileService.DEFAULT_PROFILES
            ]

            assert profiles[: len(default_profiles)] == default_profiles

            # User profiles should be sorted by name
            user_names = [p.name for p in user_profiles]
            assert user_names == sorted(user_names, key=str.lower)

    def test_persistence(self) -> None:
        """Test profiles are persisted across service instances."""
        with tempfile.TemporaryDirectory() as tmpdir:
            config_dir = Path(tmpdir) / "test_config"

            # Create profile in first instance
            service1 = ProfileService(config_dir=config_dir)
            profile = service1.create_profile(name="Persistent")
            profile_id = profile.id
            service1.set_active_profile(profile_id)

            # Load in second instance
            service2 = ProfileService(config_dir=config_dir)
            loaded = service2.get_profile(profile_id)

            assert loaded is not None
            assert loaded.name == "Persistent"
            assert service2.get_active_profile().id == profile_id


class TestProfileServiceErrors:
    """Tests for error handling in ProfileService."""

    def test_get_nonexistent_profile(self) -> None:
        """Test getting non-existent profile returns None."""
        with tempfile.TemporaryDirectory() as tmpdir:
            config_dir = Path(tmpdir) / "test_config"
            service = ProfileService(config_dir=config_dir)

            result = service.get_profile("nonexistent")
            assert result is None

    def test_delete_nonexistent_profile(self) -> None:
        """Test deleting non-existent profile returns False."""
        with tempfile.TemporaryDirectory() as tmpdir:
            config_dir = Path(tmpdir) / "test_config"
            service = ProfileService(config_dir=config_dir)

            result = service.delete_profile("nonexistent")
            assert result is False

    def test_duplicate_nonexistent_profile(self) -> None:
        """Test duplicating non-existent profile returns None."""
        with tempfile.TemporaryDirectory() as tmpdir:
            config_dir = Path(tmpdir) / "test_config"
            service = ProfileService(config_dir=config_dir)

            result = service.duplicate_profile("nonexistent")
            assert result is None

    def test_export_nonexistent_profile(self) -> None:
        """Test exporting non-existent profile returns False."""
        with tempfile.TemporaryDirectory() as tmpdir:
            config_dir = Path(tmpdir) / "test_config"
            service = ProfileService(config_dir=config_dir)

            export_path = Path(tmpdir) / "export.json"
            result = service.export_profile("nonexistent", export_path)
            assert result is False

    def test_set_nonexistent_active_profile(self) -> None:
        """Test setting non-existent profile as active is ignored."""
        with tempfile.TemporaryDirectory() as tmpdir:
            config_dir = Path(tmpdir) / "test_config"
            service = ProfileService(config_dir=config_dir)

            service.set_active_profile("voice_call")  # Valid
            service.set_active_profile("nonexistent")  # Invalid

            # Should still be voice_call
            assert service.get_active_profile().id == "voice_call"

    def test_import_invalid_json(self) -> None:
        """Test importing invalid JSON returns None."""
        with tempfile.TemporaryDirectory() as tmpdir:
            config_dir = Path(tmpdir) / "test_config"
            service = ProfileService(config_dir=config_dir)

            import_path = Path(tmpdir) / "invalid.json"
            import_path.write_text("not valid json {{{")

            result = service.import_profile(import_path)
            assert result is None
