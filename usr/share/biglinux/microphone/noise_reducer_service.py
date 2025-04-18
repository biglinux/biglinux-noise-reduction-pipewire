import asyncio
import os
import pathlib


class NoiseReducerService:
    """
    Service manager for the noise reduction functionality.
    Uses actions.sh script to interact with the system instead of direct systemctl calls.
    """

    def __init__(self):
        self.service_name = "noise-reduction-pipewire"
        self._is_updating = False
        # Use the absolute path to the system command
        self.command_path = "/usr/sbin/pipewire-noise-remove"

        # Path to the actions.sh script in the same directory
        current_dir = pathlib.Path(__file__).parent.absolute()
        self.actions_script = os.path.join(current_dir, "actions.sh")

    async def get_noise_reduction_status(self):
        """Check if noise reduction service is active using actions.sh."""
        try:
            process = await asyncio.create_subprocess_exec(
                "/bin/bash",
                self.actions_script,
                "status",  # Changed from "check-noise-reduction" to "status"
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
            )
            stdout, _ = await process.communicate()
            status = stdout.decode().strip()

            if "enabled" in status.lower() or "active" in status.lower():
                return "enabled"
            return "disabled"
        except Exception as e:
            print(f"Error checking noise reduction status: {e}")
            return "disabled"

    async def start_noise_reduction(self):
        """Start the noise reduction service using actions.sh."""
        self._is_updating = True
        try:
            process = await asyncio.create_subprocess_exec(
                "/bin/bash",
                self.actions_script,
                "start",  # Changed from "start-noise-reduction" to "start"
            )
            await process.wait()
            # Give service time to start
            await asyncio.sleep(1)
        except Exception as e:
            print(f"Error starting noise reduction: {e}")
        finally:
            self._is_updating = False

    async def stop_noise_reduction(self):
        """Stop the noise reduction service using actions.sh."""
        self._is_updating = True
        try:
            process = await asyncio.create_subprocess_exec(
                "/bin/bash",
                self.actions_script,
                "stop",  # Changed from "stop-noise-reduction" to "stop"
            )
            await process.wait()
            # Give service time to stop
            await asyncio.sleep(1)
        except Exception as e:
            print(f"Error stopping noise reduction: {e}")
        finally:
            self._is_updating = False

    async def get_bluetooth_status(self):
        """Get bluetooth autoswitch status using actions.sh."""
        try:
            process = await asyncio.create_subprocess_exec(
                "/bin/bash",
                self.actions_script,
                "check-bluetooth-status",
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
            )
            stdout, _ = await process.communicate()
            status = stdout.decode().strip()

            # Properly interpret the status output
            if "enabled" in status.lower():
                return "enabled"
            return "disabled"
        except Exception as e:
            print(f"Error checking bluetooth status: {e}")
            return "disabled"

    async def enable_bluetooth_autoswitch(self):
        """Enable bluetooth autoswitch using actions.sh."""
        try:
            process = await asyncio.create_subprocess_exec(
                "/bin/bash", self.actions_script, "enable-bluetooth-autoswitch"
            )
            await process.wait()
            # Give time for operation to complete
            await asyncio.sleep(1)
        except Exception as e:
            print(f"Error enabling bluetooth autoswitch: {e}")

    async def disable_bluetooth_autoswitch(self):
        """Disable bluetooth autoswitch using actions.sh."""
        try:
            process = await asyncio.create_subprocess_exec(
                "/bin/bash", self.actions_script, "disable-bluetooth-autoswitch"
            )
            await process.wait()
            # Give time for operation to complete
            await asyncio.sleep(1)
        except Exception as e:
            print(f"Error disabling bluetooth autoswitch: {e}")
