import asyncio
import os
import pathlib
import logging

logger = logging.getLogger(__name__)


class NoiseReducerService:
    """
    Service manager for the noise reduction functionality.
    Uses actions.sh script to interact with the system instead of direct systemctl calls.
    """

    def __init__(self) -> None:
        self.service_name = "noise-reduction-pipewire"
        self._is_updating = False
        # Use the absolute path to the system command
        self.command_path = "/usr/sbin/pipewire-noise-remove"

        # Path to the actions.sh script in the same directory
        current_dir = pathlib.Path(__file__).parent.absolute()
        self.actions_script = os.path.join(current_dir, "actions.sh")

    async def _run_action_script(
        self, action: str, capture_output: bool = True
    ) -> str | None:
        """
        Base method to run the actions script with a given action.

        Args:
            action (str): The action to execute via the script
            capture_output (bool): Whether to capture and return output

        Returns:
            str: Script output if capture_output is True, else None
        """
        try:
            cmd = ["/bin/bash", self.actions_script, action]

            if capture_output:
                process = await asyncio.create_subprocess_exec(
                    *cmd,
                    stdout=asyncio.subprocess.PIPE,
                    stderr=asyncio.subprocess.PIPE,
                )
                stdout, _ = await process.communicate()
                return stdout.decode().strip()
            else:
                process = await asyncio.create_subprocess_exec(*cmd)
                await process.wait()
                # Give service time to complete operation
                await asyncio.sleep(1)
                return None

        except Exception:
            logger.exception("Error running action %s", action)
            return None

    async def get_noise_reduction_status(self) -> str:
        """
        Check if noise reduction service is active.

        Returns:
            str: 'enabled' if the service is active, 'disabled' otherwise
        """
        try:
            status = await self._run_action_script("status")

            if status and ("enabled" in status.lower() or "active" in status.lower()):
                return "enabled"
            return "disabled"
        except Exception:
            logger.exception("Error checking noise reduction status")
            return "disabled"

    async def start_noise_reduction(self) -> None:
        """Start the noise reduction service."""
        self._is_updating = True
        try:
            await self._run_action_script("start", capture_output=False)
        finally:
            self._is_updating = False

    async def stop_noise_reduction(self) -> None:
        """Stop the noise reduction service."""
        self._is_updating = True
        try:
            await self._run_action_script("stop", capture_output=False)
        finally:
            self._is_updating = False

    async def get_bluetooth_status(self) -> str:
        """
        Get bluetooth autoswitch status.

        Returns:
            str: 'enabled' if bluetooth autoswitch is active, 'disabled' otherwise
        """
        try:
            status = await self._run_action_script("check-bluetooth-status")

            # Properly interpret the status output
            if status and "enabled" in status.lower():
                return "enabled"
            return "disabled"
        except Exception:
            logger.exception("Error checking bluetooth status")
            return "disabled"

    async def enable_bluetooth_autoswitch(self) -> None:
        """Enable bluetooth autoswitch."""
        await self._run_action_script(
            "enable-bluetooth-autoswitch", capture_output=False
        )

    async def disable_bluetooth_autoswitch(self) -> None:
        """Disable bluetooth autoswitch."""
        await self._run_action_script(
            "disable-bluetooth-autoswitch", capture_output=False
        )
